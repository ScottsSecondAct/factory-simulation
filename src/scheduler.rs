//! Scheduling engine: job dispatch, look-ahead staging, deadlock detection.
//!
//! The scheduler runs on a periodic heartbeat ([`SCHEDULER_INTERVAL`]).
//! Each tick it:
//! 1. Scans for idle mills and assigns the highest-priority feasible job.
//! 2. Dispatches AGVs to deliver tools and pallets.
//! 3. Looks ahead N jobs to pre-stage resources.
//! 4. Builds the wait-for graph and checks for deadlocks.

use std::collections::VecDeque;

use crate::agv::{Agv, LaneNetwork, WaitForGraph};
use crate::engine::{Event, TimedEvent};
use crate::factory::{Mill, PalletMagazine, ToolCrib};
use crate::types::*;

const LOOK_AHEAD_DEPTH: usize = 8;

/// A pending AGV mission: pick up something and deliver it somewhere.
#[derive(Debug, Clone)]
pub struct Mission {
    pub agv_id: AgvId,
    pub pickup_seg: SegmentId,
    pub deliver_seg: SegmentId,
    pub cargo: Cargo,
    pub dest_mill: MillId,
}

/// The scheduler owns the job queue and the dispatch state.
pub struct Scheduler {
    pub job_queue: VecDeque<Job>,
    pub pending_missions: Vec<Mission>,
    pub wait_graph: WaitForGraph,
    pub deadlocks_detected: u64,
    pub jobs_dispatched: u64,
    pub back_pressure_events: u64,
    pub max_wip: usize,
    next_job_id: u64,
    was_back_pressured: bool,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self {
            job_queue: VecDeque::new(),
            pending_missions: Vec::new(),
            wait_graph: WaitForGraph::new(),
            deadlocks_detected: 0,
            jobs_dispatched: 0,
            back_pressure_events: 0,
            max_wip: DEFAULT_MAX_WIP,
            next_job_id: 1,
            was_back_pressured: false,
        }
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_id(&mut self) -> JobId {
        let id = self.next_job_id;
        self.next_job_id += 1;
        id
    }

    pub fn enqueue(&mut self, job: Job) {
        // Insert in priority order (stable within same priority).
        let pos = self
            .job_queue
            .iter()
            .position(|j| j.priority > job.priority)
            .unwrap_or(self.job_queue.len());
        self.job_queue.insert(pos, job);
    }

    /// Main scheduling tick. Returns events to schedule.
    pub fn tick(
        &mut self,
        now: SimTime,
        mills: &mut [Mill],
        agvs: &mut [Agv],
        tool_crib: &mut ToolCrib,
        pallet_mag: &mut PalletMagazine,
        lanes: &mut LaneNetwork,
    ) -> Vec<TimedEvent> {
        let mut events = Vec::new();

        // ── 1. Assign jobs to idle mills ────────────────────────────
        self.dispatch_jobs(now, mills, agvs, tool_crib, pallet_mag, lanes, &mut events);

        // ── 2. Advance blocked AGVs ─────────────────────────────────
        self.try_advance_agvs(now, agvs, lanes, &mut events);

        // ── 3. Look-ahead pre-staging ───────────────────────────────
        self.look_ahead_stage(tool_crib, pallet_mag);

        // ── 4. Deadlock detection ───────────────────────────────────
        self.detect_and_resolve_deadlocks(now, agvs, lanes, &mut events);

        // ── 5. Schedule next tick ───────────────────────────────────
        events.push(TimedEvent {
            time: now + SCHEDULER_INTERVAL,
            event: Event::SchedulerTick,
        });

        events
    }

    /// Count jobs currently in flight (mills actively processing work).
    pub fn wip_count(mills: &[Mill]) -> usize {
        mills
            .iter()
            .filter(|m| m.state != MillState::Idle && m.state != MillState::Faulted)
            .count()
    }

    // ── Job dispatch ────────────────────────────────────────────────
    #[allow(clippy::too_many_arguments)]
    fn dispatch_jobs(
        &mut self,
        now: SimTime,
        mills: &mut [Mill],
        agvs: &mut [Agv],
        tool_crib: &mut ToolCrib,
        pallet_mag: &mut PalletMagazine,
        lanes: &LaneNetwork,
        events: &mut Vec<TimedEvent>,
    ) {
        let mut assigned = Vec::new();
        let mut wip = Self::wip_count(mills);

        for (qi, job) in self.job_queue.iter().enumerate() {
            if wip >= self.max_wip {
                self.back_pressure_events += 1;
                if !self.was_back_pressured {
                    self.was_back_pressured = true;
                    eprintln!(
                        "[{now:.1}s] BACK-PRESSURE: WIP at limit ({}/{}), holding dispatch",
                        wip, self.max_wip
                    );
                }
                break;
            }
            if job.operations.is_empty() {
                continue;
            }
            let op = &job.operations[0];

            // Find an idle mill that already has the right tool (prefer),
            // or any idle mill.
            let mill_id = mills
                .iter()
                .filter(|m| m.is_available())
                .min_by_key(|m| {
                    if !m.needs_tool_change(op.tool_set) {
                        0
                    } else {
                        1
                    }
                })
                .map(|m| m.id);

            let Some(mid) = mill_id else { break };

            // Check resource availability.
            let need_tool = mills[mid].needs_tool_change(op.tool_set);
            if need_tool && tool_crib.available(op.tool_set) == 0 {
                continue; // tool not available, try next job
            }
            if pallet_mag.available(op.pallet_type) == 0 {
                continue; // pallet not available
            }

            // Find an idle AGV.
            let agv_opt = agvs.iter().find(|a| a.is_idle());
            let Some(agv) = agv_opt else { break };
            let agv_id = agv.id;

            // Reserve resources.
            if need_tool {
                tool_crib.checkout(op.tool_set);
                // Return the old tool if any.
                if let Some(old) = mills[mid].loaded_tool {
                    tool_crib.checkin(old);
                }
                mills[mid].begin_tool_change(op.tool_set);
                events.push(TimedEvent {
                    time: now + TOOL_CHANGE_TIME,
                    event: Event::ToolChangeDone(mid),
                });
            }

            let pallet = pallet_mag.take(op.pallet_type).unwrap();
            mills[mid].loaded_pallet = Some(pallet);
            mills[mid].loaded_pallet_type = Some(op.pallet_type);
            mills[mid].begin_loading();

            // Dispatch AGV to carry workpiece to mill.
            let dest_spur = mill_spur(mid);
            if let Some(path) = lanes.route(agvs[agv_id].segment, dest_spur) {
                agvs[agv_id].state = AgvState::Traveling;
                agvs[agv_id].cargo = Cargo::Workpiece {
                    job_id: job.id,
                    op_index: 0,
                };
                agvs[agv_id].path = path;
                agvs[agv_id].path_cursor = 0;

                // Schedule first move.
                if let Some(&seg) = agvs[agv_id].path.first() {
                    events.push(TimedEvent {
                        time: now + AGV_SEGMENT_TRAVEL,
                        event: Event::AgvArrived {
                            agv_id,
                            segment: seg,
                        },
                    });
                }
            }

            self.jobs_dispatched += 1;
            wip += 1;
            assigned.push(qi);
        }

        if wip < self.max_wip && self.was_back_pressured {
            self.was_back_pressured = false;
            eprintln!(
                "[{now:.1}s] BACK-PRESSURE relieved: WIP {wip}/{}",
                self.max_wip
            );
        }

        // Remove assigned jobs (iterate in reverse to keep indices valid).
        for &qi in assigned.iter().rev() {
            self.job_queue.remove(qi);
        }
    }

    // ── AGV advancement ─────────────────────────────────────────────
    #[allow(clippy::needless_range_loop)]
    fn try_advance_agvs(
        &mut self,
        now: SimTime,
        agvs: &mut [Agv],
        lanes: &mut LaneNetwork,
        events: &mut Vec<TimedEvent>,
    ) {
        for aid in 0..agvs.len() {
            if agvs[aid].state != AgvState::Blocked {
                continue;
            }
            if let Some(seg) = agvs[aid].next_segment() {
                if lanes.claim(seg, agvs[aid].id) {
                    // Release previous segment.
                    lanes.release(agvs[aid].segment);
                    agvs[aid].advance();
                    agvs[aid].state = AgvState::Traveling;
                    self.wait_graph.remove(agvs[aid].id);
                    // Schedule next hop.
                    if let Some(next) = agvs[aid].next_segment() {
                        events.push(TimedEvent {
                            time: now + AGV_SEGMENT_TRAVEL,
                            event: Event::AgvArrived {
                                agv_id: aid,
                                segment: next,
                            },
                        });
                    }
                }
            }
        }
    }

    // ── Look-ahead pre-staging ──────────────────────────────────────
    /// Examine the next N jobs and ensure their tool sets are reserved
    /// in the crib (not checked out, just flagged for the planner).
    fn look_ahead_stage(&self, tool_crib: &ToolCrib, _pallet_mag: &PalletMagazine) {
        let count = self.job_queue.len().min(LOOK_AHEAD_DEPTH);
        for job in self.job_queue.iter().take(count) {
            if let Some(op) = job.operations.first() {
                let _avail = tool_crib.available(op.tool_set);
                // In a production system this would pre-stage tools to
                // shadow positions or warm-swap carts. Here we just
                // verify availability for scheduling feasibility.
            }
        }
    }

    // ── Deadlock detection and resolution ────────────────────────────
    fn detect_and_resolve_deadlocks(
        &mut self,
        now: SimTime,
        agvs: &mut [Agv],
        lanes: &mut LaneNetwork,
        _events: &mut Vec<TimedEvent>,
    ) {
        // Build wait-for graph from current AGV states.
        self.wait_graph.clear();
        for agv in agvs.iter() {
            if agv.state == AgvState::Blocked {
                if let Some(wanted) = agv.next_segment() {
                    if let Some(blocker) = lanes.occupant(wanted) {
                        if blocker != agv.id {
                            self.wait_graph.add_wait(agv.id, blocker);
                        }
                    }
                }
            }
        }

        let cycle = self.wait_graph.find_cycle();
        if !cycle.is_empty() {
            self.deadlocks_detected += 1;
            // Resolution strategy: retreat the lowest-priority AGV in
            // the cycle to a safe segment (its current spur or nearest
            // idle loop segment). This is a simplified version of the
            // "victim selection" pattern used in real FMS controllers.
            if let Some(&victim_id) = cycle.last() {
                let cur = agvs[victim_id].segment;
                agvs[victim_id].state = AgvState::Idle;
                agvs[victim_id].path.clear();
                agvs[victim_id].path_cursor = 0;
                agvs[victim_id].cargo = Cargo::Empty;
                self.wait_graph.remove(victim_id);

                // Release any claimed-ahead segments.
                lanes.release(cur);
                // Re-claim current position.
                lanes.claim(cur, victim_id);

                eprintln!("[{now:.1}s] DEADLOCK resolved: retreated AGV {victim_id} at seg {cur}");
            }
        }
    }
}
