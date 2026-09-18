//! Shared simulation world: aggregate root of all simulation state.
//!
//! Both the batch-mode CLI (`main.rs`) and the IPC dashboard runner
//! (`ipc.rs`) delegate to this single [`World`] for event dispatch,
//! eliminating duplicated handler logic.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::agv::{Agv, LaneNetwork};
use crate::engine::{Event, TimedEvent};
use crate::factory::{Mill, PalletMagazine, PrepItem, ToolCrib, WorkPrepStation};
use crate::fault::{FaultConfig, FaultInjector};
use crate::metrics::Metrics;
use crate::reconcile::{Drift, Reconciler};
use crate::scheduler::Scheduler;
use crate::types::*;

pub use crate::strategy;

// ── Configuration ──────────────────────────────────────────────────

pub struct WorldConfig {
    pub fault_cfg: FaultConfig,
    pub num_amrs: usize,
    pub seed: u64,
    pub tool_types: u16,
    pub tool_copies: u16,
    pub pallet_types: u8,
    pub pallet_copies: usize,
    pub snapshot_interval: SimTime,
    pub max_wip: usize,
    pub strategy: StrategyName,
}

// ── Notifications ──────────────────────────────────────────────────

/// Side effects produced during event handling that callers may want
/// to log, emit as IPC messages, or otherwise present.
pub enum Notification {
    FaultOccurred { target: FaultTarget },
    Repaired { target: FaultTarget },
    ChipEvacDone { mill_id: MillId },
    WorkPrepReady { job_id: JobId, mill_id: MillId },
    Reconciliation { pass: u64, drifts: Vec<Drift> },
}

// ── World ──────────────────────────────────────────────────────────

pub struct World {
    pub mills: Vec<Mill>,
    pub agvs: Vec<Agv>,
    pub lanes: LaneNetwork,
    pub tool_crib: ToolCrib,
    pub pallet_mag: PalletMagazine,
    pub scheduler: Scheduler,
    pub fault_inj: FaultInjector,
    pub work_prep: WorkPrepStation,
    pub reconciler: Reconciler,
    pub metrics: Metrics,
    pub rng: StdRng,
    tool_types: u16,
    pallet_types: u8,
}

impl World {
    pub fn new(config: WorldConfig) -> Self {
        let mills: Vec<Mill> = (0..NUM_MILLS).map(Mill::new).collect();

        let mut agvs: Vec<Agv> = (0..NUM_AGVS)
            .map(|i| Agv::new(i, (i * 3) % LOOP_SEGMENTS))
            .collect();

        for i in 0..config.num_amrs {
            let id = NUM_AGVS + i;
            let start = ((NUM_AGVS + i) * 3 + 1) % LOOP_SEGMENTS;
            agvs.push(Agv::new_amr(id, start));
        }

        let mut lanes = LaneNetwork::new();
        for agv in &agvs {
            lanes.claim(agv.segment, agv.id);
        }

        let mut scheduler = Scheduler::with_strategy(config.strategy);
        scheduler.max_wip = config.max_wip;

        Self {
            mills,
            agvs,
            lanes,
            tool_crib: ToolCrib::new(config.tool_types, config.tool_copies),
            pallet_mag: PalletMagazine::new(config.pallet_types, config.pallet_copies),
            scheduler,
            fault_inj: FaultInjector::new(config.fault_cfg),
            work_prep: WorkPrepStation::new(),
            reconciler: Reconciler::new(),
            metrics: Metrics::new(config.snapshot_interval),
            rng: StdRng::seed_from_u64(config.seed),
            tool_types: config.tool_types,
            pallet_types: config.pallet_types,
        }
    }

    /// Dispatch a single simulation event, returning new events to
    /// schedule and any notable side effects for the caller to present.
    pub fn handle(&mut self, te: &TimedEvent) -> (Vec<TimedEvent>, Vec<Notification>) {
        let now = te.time;
        let mut out = Vec::new();
        let mut notes = Vec::new();

        match &te.event {
            Event::JobArrival(job) => {
                self.scheduler.enqueue(job.clone());
                let gap = -120.0 * self.rng.gen::<f64>().ln();
                let next_job = self.generate_job(now + gap);
                out.push(TimedEvent {
                    time: now + gap,
                    event: Event::JobArrival(next_job),
                });
            }

            Event::ToolChangeDone(mid) => {
                if self.mills[*mid].state == MillState::ToolChange {
                    self.mills[*mid].finish_tool_change();
                }
            }
            Event::MillLoadDone(mid) => {
                if self.mills[*mid].state == MillState::Loading {
                    let m = &mut self.mills[*mid];
                    if let (Some(jid), op_idx) = (m.current_job, m.current_op) {
                        m.begin_machining(jid, op_idx);
                        let duration = 300.0 + self.rng.gen::<f64>() * 600.0;
                        out.push(TimedEvent {
                            time: now + duration,
                            event: Event::MillMachiningDone {
                                mill_id: *mid,
                                job_id: jid,
                                op_index: op_idx,
                                duration,
                            },
                        });
                    }
                }
            }
            Event::MillMachiningDone {
                mill_id, duration, ..
            } => {
                if self.mills[*mill_id].state == MillState::Machining {
                    self.mills[*mill_id].finish_machining(*duration);
                    out.push(TimedEvent {
                        time: now + MILL_UNLOAD_TIME,
                        event: Event::MillUnloadDone(*mill_id),
                    });
                }
            }
            Event::MillUnloadDone(mid) => {
                if self.mills[*mid].state == MillState::Unloading {
                    if let Some(pid) = self.mills[*mid].loaded_pallet.take() {
                        let pt = self.mills[*mid].loaded_pallet_type.take().unwrap_or(0);
                        self.pallet_mag.return_pallet(pt, pid);
                    }
                    self.mills[*mid].finish_unloading();
                }
            }

            Event::AgvArrived { agv_id, segment } => {
                let aid = *agv_id;
                let seg = *segment;
                if self.agvs[aid].state == AgvState::Faulted
                    || self.agvs[aid].state == AgvState::Idle
                {
                    // Stale — vehicle faulted or was reset since this was scheduled
                } else if self.lanes.claim(seg, aid) {
                    self.lanes.release(self.agvs[aid].segment);
                    self.agvs[aid].advance();
                    if self.agvs[aid].at_destination() {
                        self.agvs[aid].state = AgvState::Unloading;
                        out.push(TimedEvent {
                            time: now + MILL_LOAD_TIME,
                            event: Event::AgvUnloadDone { agv_id: aid },
                        });
                    } else if let Some(next) = self.agvs[aid].next_segment() {
                        out.push(TimedEvent {
                            time: now + self.agvs[aid].travel_time(),
                            event: Event::AgvArrived {
                                agv_id: aid,
                                segment: next,
                            },
                        });
                    }
                } else {
                    self.agvs[aid].state = AgvState::Blocked;
                    if let Some(blocker) = self.lanes.occupant(seg) {
                        self.scheduler.wait_graph.add_wait(aid, blocker);
                    }
                }
            }
            Event::AgvLoadDone { agv_id } => {
                self.agvs[*agv_id].state = AgvState::Traveling;
                self.agvs[*agv_id].loads_delivered += 1;
            }
            Event::AgvUnloadDone { agv_id } => {
                if self.agvs[*agv_id].state == AgvState::Faulted
                    || self.agvs[*agv_id].state == AgvState::Idle
                {
                    // Stale — vehicle faulted or was reset since this was scheduled
                } else {
                    match &self.agvs[*agv_id].cargo {
                        Cargo::Workpiece { job_id, op_index } => {
                            let seg = self.agvs[*agv_id].segment;
                            if seg >= SPUR_BASE {
                                let mid = seg - SPUR_BASE;
                                if mid < NUM_MILLS {
                                    self.mills[mid].current_job = Some(*job_id);
                                    self.mills[mid].current_op = *op_index;
                                    out.push(TimedEvent {
                                        time: now + MILL_LOAD_TIME,
                                        event: Event::MillLoadDone(mid),
                                    });
                                }
                            }
                        }
                        Cargo::ChipBin(mid) => {
                            out.push(TimedEvent {
                                time: now + CHIP_EVAC_TIME,
                                event: Event::ChipEvacDone(*mid),
                            });
                        }
                        Cargo::PrepPallet {
                            job_id,
                            op_index,
                            mill_id,
                        } => {
                            let item = PrepItem {
                                job_id: *job_id,
                                op_index: *op_index,
                                mill_id: *mill_id,
                            };
                            self.work_prep.enqueue(item);
                            if let Some((prep_item, gen)) = self.work_prep.try_start() {
                                let prep_time = WORK_PREP_TIME_MIN
                                    + self.rng.gen::<f64>()
                                        * (WORK_PREP_TIME_MAX - WORK_PREP_TIME_MIN);
                                out.push(TimedEvent {
                                    time: now + prep_time,
                                    event: Event::WorkPrepDone {
                                        job_id: prep_item.job_id,
                                        op_index: prep_item.op_index,
                                        mill_id: prep_item.mill_id,
                                        gen,
                                    },
                                });
                            }
                        }
                        _ => {}
                    }
                    self.agvs[*agv_id].cargo = Cargo::Empty;
                    self.agvs[*agv_id].state = AgvState::Idle;
                    self.agvs[*agv_id].loads_delivered += 1;
                }
            }

            Event::ChipEvacDone(mid) => {
                self.mills[*mid].finish_chip_evac();
                self.scheduler.clear_pending_chip_evac(*mid);
                notes.push(Notification::ChipEvacDone { mill_id: *mid });
            }

            Event::WorkPrepDone {
                job_id,
                op_index: _,
                mill_id,
                gen,
            } => {
                if *gen != self.work_prep.current_generation()
                    || self.work_prep.state != WorkPrepState::Processing
                {
                    // Stale event (station faulted and restarted)
                } else if let Some(item) = self.work_prep.finish_processing() {
                    self.scheduler.pending_prep_deliveries.push_back(PrepItem {
                        job_id: item.job_id,
                        op_index: item.op_index,
                        mill_id: item.mill_id,
                    });
                    notes.push(Notification::WorkPrepReady {
                        job_id: *job_id,
                        mill_id: *mill_id,
                    });
                    if let Some((next_item, next_gen)) = self.work_prep.try_start() {
                        let prep_time = WORK_PREP_TIME_MIN
                            + self.rng.gen::<f64>() * (WORK_PREP_TIME_MAX - WORK_PREP_TIME_MIN);
                        out.push(TimedEvent {
                            time: now + prep_time,
                            event: Event::WorkPrepDone {
                                job_id: next_item.job_id,
                                op_index: next_item.op_index,
                                mill_id: next_item.mill_id,
                                gen: next_gen,
                            },
                        });
                    }
                }
            }

            Event::FaultOccur(target) => {
                match target {
                    FaultTarget::Mill(mid) => self.mills[*mid].fault(now),
                    FaultTarget::Agv(aid) => self.agvs[*aid].fault(),
                    FaultTarget::WorkPrep => self.work_prep.fault(now),
                }
                notes.push(Notification::FaultOccurred {
                    target: target.clone(),
                });
                out.push(self.fault_inj.schedule_repair(now, target));
            }
            Event::FaultRepair(target) => {
                match target {
                    FaultTarget::Mill(mid) => {
                        if let Some((pt, pid)) = self.mills[*mid].repair(now) {
                            self.pallet_mag.return_pallet(pt, pid);
                        }
                    }
                    FaultTarget::Agv(aid) => {
                        let aid = *aid;
                        let dest_mill = match &self.agvs[aid].cargo {
                            Cargo::PrepPallet { mill_id, .. } => Some(*mill_id),
                            Cargo::Workpiece { .. } => self.agvs[aid].path.last().and_then(|&d| {
                                (d >= SPUR_BASE && d - SPUR_BASE < NUM_MILLS)
                                    .then_some(d - SPUR_BASE)
                            }),
                            _ => None,
                        };
                        let chip_evac_mill = match &self.agvs[aid].cargo {
                            Cargo::ChipBin(mid) => Some(*mid),
                            _ => None,
                        };
                        if let Some(mid) = dest_mill {
                            if self.mills[mid].state == MillState::Loading {
                                if let (Some(pt), Some(pid)) = (
                                    self.mills[mid].loaded_pallet_type.take(),
                                    self.mills[mid].loaded_pallet.take(),
                                ) {
                                    self.pallet_mag.return_pallet(pt, pid);
                                }
                                self.mills[mid].current_job = None;
                                self.mills[mid].current_op = 0;
                                self.mills[mid].state = MillState::Idle;
                            }
                        }
                        if let Some(mid) = chip_evac_mill {
                            self.scheduler.clear_pending_chip_evac(mid);
                        }
                        self.agvs[aid].repair();
                    }
                    FaultTarget::WorkPrep => {
                        self.work_prep.repair(now);
                        if let Some((item, gen)) = self.work_prep.try_start() {
                            let prep_time = WORK_PREP_TIME_MIN
                                + self.rng.gen::<f64>() * (WORK_PREP_TIME_MAX - WORK_PREP_TIME_MIN);
                            out.push(TimedEvent {
                                time: now + prep_time,
                                event: Event::WorkPrepDone {
                                    job_id: item.job_id,
                                    op_index: item.op_index,
                                    mill_id: item.mill_id,
                                    gen,
                                },
                            });
                        }
                    }
                }
                notes.push(Notification::Repaired {
                    target: target.clone(),
                });
                out.push(
                    self.fault_inj
                        .schedule_next_fault(&mut self.rng, now, target),
                );
            }

            Event::SchedulerTick => {
                self.metrics.sample_queue(self.scheduler.job_queue.len());
                self.metrics
                    .maybe_snapshot(now, &self.mills, &self.agvs, &self.scheduler);
                let sched_events = self.scheduler.tick(
                    now,
                    &mut self.mills,
                    &mut self.agvs,
                    &mut self.tool_crib,
                    &mut self.pallet_mag,
                    &mut self.lanes,
                    &self.work_prep,
                );
                out.extend(sched_events);
            }

            Event::ReconciliationTick => {
                let drifts = self.reconciler.reconcile(
                    &self.mills,
                    &self.agvs,
                    &self.tool_crib,
                    &self.pallet_mag,
                    &self.work_prep,
                );
                if !drifts.is_empty() {
                    notes.push(Notification::Reconciliation {
                        pass: self.reconciler.passes,
                        drifts,
                    });
                }
                out.push(TimedEvent {
                    time: now + RECONCILIATION_INTERVAL,
                    event: Event::ReconciliationTick,
                });
            }
        }

        (out, notes)
    }

    /// Seed initial events: first job, scheduler tick, faults, reconciliation.
    pub fn seed_events(&mut self) -> Vec<TimedEvent> {
        let first_job = self.generate_job(0.0);
        let mut events = vec![
            TimedEvent {
                time: 0.0,
                event: Event::JobArrival(first_job),
            },
            TimedEvent {
                time: 0.0,
                event: Event::SchedulerTick,
            },
            TimedEvent {
                time: 0.0,
                event: Event::ReconciliationTick,
            },
        ];
        events.extend(self.fault_inj.seed_faults(&mut self.rng));
        events
    }

    fn generate_job(&mut self, arrival: SimTime) -> Job {
        let id = self.scheduler.next_id();
        let priority = match self.rng.gen_range(0..100) {
            0..=4 => Priority::Critical,
            5..=19 => Priority::High,
            20..=79 => Priority::Normal,
            _ => Priority::Low,
        };
        let num_ops = self.rng.gen_range(1..=3);
        let ops: Vec<Operation> = (0..num_ops)
            .map(|_| Operation {
                tool_set: self.rng.gen_range(0..self.tool_types),
                duration: 180.0 + self.rng.gen::<f64>() * 720.0,
                pallet_type: self.rng.gen_range(0..self.pallet_types),
            })
            .collect();
        let total_processing: SimTime = ops.iter().map(|op| op.duration).sum();
        // 70% of jobs get a due date: arrival + (2..5)× processing time.
        let due_date = if self.rng.gen::<f64>() < 0.7 {
            let slack_factor = 2.0 + self.rng.gen::<f64>() * 3.0;
            Some(arrival + total_processing * slack_factor)
        } else {
            None
        };
        Job {
            id,
            priority,
            operations: ops,
            arrived_at: arrival,
            due_date,
        }
    }

    /// Format a fault target as a human-readable label.
    pub fn fault_label(&self, target: &FaultTarget) -> String {
        match target {
            FaultTarget::Mill(mid) => format!("Mill {mid}"),
            FaultTarget::Agv(aid) => {
                let vtype = if self.agvs[*aid].vehicle_type == VehicleType::Amr {
                    "AMR"
                } else {
                    "AGV"
                };
                format!("{vtype} {aid}")
            }
            FaultTarget::WorkPrep => "Work prep station".to_string(),
        }
    }
}
