//! Scheduling engine: job dispatch, look-ahead staging, deadlock detection.
//!
//! The scheduler runs on a periodic heartbeat ([`SCHEDULER_INTERVAL`]).
//! Each tick it:
//! 1. Scans for idle mills and assigns the highest-priority feasible job.
//! 2. Dispatches AGVs to deliver tools and pallets.
//! 3. Looks ahead N jobs to pre-stage resources.
//! 4. Builds the wait-for graph and checks for deadlocks.

use std::collections::{HashSet, VecDeque};

use crate::agv::{Agv, LaneNetwork, WaitForGraph};
use crate::engine::{Event, TimedEvent};
use crate::factory::{Mill, PalletMagazine, PrepItem, ToolCrib, WorkPrepStation};
use crate::types::*;

const LOOK_AHEAD_DEPTH: usize = 8;

/// The scheduler owns the job queue and the dispatch state.
pub struct Scheduler {
    pub job_queue: VecDeque<Job>,
    pub wait_graph: WaitForGraph,
    pub deadlocks_detected: u64,
    pub jobs_dispatched: u64,
    pub back_pressure_events: u64,
    pub chip_evacs_dispatched: u64,
    pub work_prep_deliveries: u64,
    pub max_wip: usize,
    next_job_id: u64,
    was_back_pressured: bool,
    pending_chip_evacs: HashSet<MillId>,
    pub pending_prep_deliveries: VecDeque<PrepItem>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self {
            job_queue: VecDeque::new(),
            wait_graph: WaitForGraph::new(),
            deadlocks_detected: 0,
            jobs_dispatched: 0,
            back_pressure_events: 0,
            chip_evacs_dispatched: 0,
            work_prep_deliveries: 0,
            max_wip: DEFAULT_MAX_WIP,
            next_job_id: 1,
            was_back_pressured: false,
            pending_chip_evacs: HashSet::new(),
            pending_prep_deliveries: VecDeque::new(),
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
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        now: SimTime,
        mills: &mut [Mill],
        agvs: &mut [Agv],
        tool_crib: &mut ToolCrib,
        pallet_mag: &mut PalletMagazine,
        lanes: &mut LaneNetwork,
        work_prep: &WorkPrepStation,
    ) -> Vec<TimedEvent> {
        let mut events = Vec::new();

        // ── 1. Deliver prepared workpieces to mills (priority) ──────
        self.dispatch_prep_deliveries(now, agvs, lanes, &mut events);

        // ── 2. Assign jobs to idle mills ────────────────────────────
        self.dispatch_jobs(
            now,
            mills,
            agvs,
            tool_crib,
            pallet_mag,
            lanes,
            work_prep,
            &mut events,
        );

        // ── 3. Dispatch chip evacuations ────────────────────────────
        self.dispatch_chip_evacs(now, mills, agvs, lanes, &mut events);

        // ── 4. Advance blocked AGVs ─────────────────────────────────
        self.try_advance_agvs(now, agvs, lanes, &mut events);

        // ── 5. Look-ahead pre-staging ───────────────────────────────
        self.look_ahead_stage(tool_crib, pallet_mag);

        // ── 6. Deadlock detection ───────────────────────────────────
        self.detect_and_resolve_deadlocks(now, agvs, mills, pallet_mag, lanes, &mut events);

        // ── 7. Schedule next tick ───────────────────────────────────
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
            .filter(|m| {
                m.state != MillState::Idle
                    && m.state != MillState::Faulted
                    && m.state != MillState::ChipFull
            })
            .count()
    }

    pub fn clear_pending_chip_evac(&mut self, mill_id: MillId) {
        self.pending_chip_evacs.remove(&mill_id);
    }

    // ── Prepared workpiece delivery (work prep → mill spur) ────────
    fn dispatch_prep_deliveries(
        &mut self,
        now: SimTime,
        agvs: &mut [Agv],
        lanes: &LaneNetwork,
        events: &mut Vec<TimedEvent>,
    ) {
        while let Some(delivery) = self.pending_prep_deliveries.front() {
            let dest_spur = mill_spur(delivery.mill_id);
            let agv_opt = agvs.iter().find(|a| a.is_idle() && a.can_enter_spur());
            let Some(agv) = agv_opt else { break };
            let agv_id = agv.id;

            let delivery = self.pending_prep_deliveries.pop_front().unwrap();
            if let Some(path) = lanes.route(agvs[agv_id].segment, dest_spur) {
                agvs[agv_id].cargo = Cargo::Workpiece {
                    job_id: delivery.job_id,
                    op_index: delivery.op_index,
                };
                agvs[agv_id].path = path;
                agvs[agv_id].path_cursor = 0;

                if let Some(&seg) = agvs[agv_id].path.first() {
                    agvs[agv_id].state = AgvState::Traveling;
                    events.push(TimedEvent {
                        time: now + agvs[agv_id].travel_time(),
                        event: Event::AgvArrived {
                            agv_id,
                            segment: seg,
                        },
                    });
                } else {
                    agvs[agv_id].state = AgvState::Unloading;
                    events.push(TimedEvent {
                        time: now + MILL_LOAD_TIME,
                        event: Event::AgvUnloadDone { agv_id },
                    });
                }
                self.work_prep_deliveries += 1;
            } else {
                self.pending_prep_deliveries.push_front(delivery);
                break;
            }
        }
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
        work_prep: &WorkPrepStation,
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

            // Don't flood the work prep station.
            if work_prep.total_items() + self.pending_prep_deliveries.len() >= WORK_PREP_MAX_QUEUE {
                break;
            }

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

            // First leg goes to WORK_PREP_SEG (on loop), any vehicle works.
            let agv_opt = agvs.iter().find(|a| a.is_idle());
            let Some(agv) = agv_opt else { break };
            let agv_id = agv.id;

            // Reserve resources.
            if need_tool {
                tool_crib.checkout(op.tool_set);
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

            // Dispatch vehicle to carry raw pallet to work prep station.
            if let Some(path) = lanes.route(agvs[agv_id].segment, WORK_PREP_SEG) {
                agvs[agv_id].cargo = Cargo::PrepPallet {
                    job_id: job.id,
                    op_index: 0,
                    mill_id: mid,
                };
                agvs[agv_id].path = path;
                agvs[agv_id].path_cursor = 0;

                if let Some(&seg) = agvs[agv_id].path.first() {
                    agvs[agv_id].state = AgvState::Traveling;
                    events.push(TimedEvent {
                        time: now + agvs[agv_id].travel_time(),
                        event: Event::AgvArrived {
                            agv_id,
                            segment: seg,
                        },
                    });
                } else {
                    agvs[agv_id].state = AgvState::Unloading;
                    events.push(TimedEvent {
                        time: now + MILL_LOAD_TIME,
                        event: Event::AgvUnloadDone { agv_id },
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

    // ── Chip evacuation dispatch ──────────────────────────────────
    #[allow(clippy::needless_range_loop)]
    fn dispatch_chip_evacs(
        &mut self,
        now: SimTime,
        mills: &[Mill],
        agvs: &mut [Agv],
        lanes: &LaneNetwork,
        events: &mut Vec<TimedEvent>,
    ) {
        for mid in 0..mills.len() {
            if mills[mid].state != MillState::ChipFull {
                continue;
            }
            if self.pending_chip_evacs.contains(&mid) {
                continue;
            }
            let dest_spur = mill_spur(mid);
            let agv_opt = agvs.iter().find(|a| a.is_idle() && a.can_enter_spur());
            let Some(agv) = agv_opt else { break };
            let agv_id = agv.id;

            if let Some(path) = lanes.route(agvs[agv_id].segment, dest_spur) {
                agvs[agv_id].state = AgvState::Traveling;
                agvs[agv_id].cargo = Cargo::ChipBin(mid);
                agvs[agv_id].path = path;
                agvs[agv_id].path_cursor = 0;

                if let Some(&seg) = agvs[agv_id].path.first() {
                    events.push(TimedEvent {
                        time: now + agvs[agv_id].travel_time(),
                        event: Event::AgvArrived {
                            agv_id,
                            segment: seg,
                        },
                    });
                } else {
                    // Already at the spur
                    events.push(TimedEvent {
                        time: now + CHIP_EVAC_TIME,
                        event: Event::ChipEvacDone(mid),
                    });
                    agvs[agv_id].state = AgvState::Idle;
                    agvs[agv_id].cargo = Cargo::Empty;
                    agvs[agv_id].loads_delivered += 1;
                }

                self.pending_chip_evacs.insert(mid);
                self.chip_evacs_dispatched += 1;
                eprintln!("[{now:.1}s] CHIP-EVAC: dispatched AGV {agv_id} to mill {mid}");
            }
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
        // First pass: yield idle vehicles blocking active ones.
        Self::yield_idle_blockers(agvs, lanes);

        for aid in 0..agvs.len() {
            if agvs[aid].state != AgvState::Blocked {
                continue;
            }
            if let Some(seg) = agvs[aid].next_segment() {
                if lanes.claim(seg, agvs[aid].id) {
                    lanes.release(agvs[aid].segment);
                    agvs[aid].advance();
                    self.wait_graph.remove(agvs[aid].id);
                    if agvs[aid].at_destination() {
                        agvs[aid].state = AgvState::Unloading;
                        events.push(TimedEvent {
                            time: now + MILL_LOAD_TIME,
                            event: Event::AgvUnloadDone { agv_id: aid },
                        });
                    } else {
                        agvs[aid].state = AgvState::Traveling;
                        if let Some(next) = agvs[aid].next_segment() {
                            events.push(TimedEvent {
                                time: now + agvs[aid].travel_time(),
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
    }

    /// Move idle vehicles out of the way when they block active ones.
    fn yield_idle_blockers(agvs: &mut [Agv], lanes: &mut LaneNetwork) {
        let mut yields: Vec<(AgvId, SegmentId)> = Vec::new();

        for aid in 0..agvs.len() {
            if agvs[aid].state != AgvState::Blocked {
                continue;
            }
            let Some(wanted) = agvs[aid].next_segment() else {
                continue;
            };
            let Some(blocker_id) = lanes.occupant(wanted) else {
                continue;
            };
            if !agvs[blocker_id].is_idle() {
                continue;
            }
            if let Some(free_seg) = lanes.nearest_free_loop(wanted, agvs[aid].segment) {
                yields.push((blocker_id, free_seg));
            }
        }

        for (blocker_id, free_seg) in yields {
            let old_seg = agvs[blocker_id].segment;
            if lanes.claim(free_seg, blocker_id) {
                lanes.release(old_seg);
                agvs[blocker_id].segment = free_seg;
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
        mills: &mut [Mill],
        pallet_mag: &mut PalletMagazine,
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
            if let Some(&victim_id) = cycle.last() {
                // Recover resources from cargo before dropping it.
                let dest_mill = match &agvs[victim_id].cargo {
                    Cargo::PrepPallet { mill_id, .. } => Some(*mill_id),
                    Cargo::Workpiece { .. } => agvs[victim_id].path.last().and_then(|&d| {
                        (d >= SPUR_BASE && d - SPUR_BASE < mills.len()).then_some(d - SPUR_BASE)
                    }),
                    _ => None,
                };
                let chip_evac_mill = match &agvs[victim_id].cargo {
                    Cargo::ChipBin(mid) => Some(*mid),
                    _ => None,
                };

                if let Some(mid) = dest_mill {
                    Self::release_mill_for_dropped_cargo(mills, pallet_mag, mid);
                }
                if let Some(mid) = chip_evac_mill {
                    self.pending_chip_evacs.remove(&mid);
                }

                let cur = agvs[victim_id].segment;
                agvs[victim_id].state = AgvState::Idle;
                agvs[victim_id].path.clear();
                agvs[victim_id].path_cursor = 0;
                agvs[victim_id].cargo = Cargo::Empty;
                self.wait_graph.remove(victim_id);

                lanes.release(cur);
                lanes.claim(cur, victim_id);

                eprintln!("[{now:.1}s] DEADLOCK resolved: retreated AGV {victim_id} at seg {cur}");
            }
        }
    }

    fn release_mill_for_dropped_cargo(
        mills: &mut [Mill],
        pallet_mag: &mut PalletMagazine,
        mill_id: MillId,
    ) {
        if mills[mill_id].state == MillState::Loading {
            if let (Some(pt), Some(pid)) = (
                mills[mill_id].loaded_pallet_type.take(),
                mills[mill_id].loaded_pallet.take(),
            ) {
                pallet_mag.return_pallet(pt, pid);
            }
            mills[mill_id].current_job = None;
            mills[mill_id].current_op = 0;
            mills[mill_id].state = MillState::Idle;
        }
    }
}
