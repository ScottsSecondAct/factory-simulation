//! factory-sim: discrete-event factory orchestration simulator.
//!
//! Simulates a flexible manufacturing system with 25 CNC mills, 6 AGVs
//! on a shared lane network, a tool crib, pallet magazine, and a
//! priority job queue. The scheduling engine includes look-ahead
//! staging, deadlock detection (wait-for graph), and stochastic fault
//! injection.
//!
//! Usage:
//!   factory-sim [--duration SECS] [--no-faults] [--json] [--snapshots] [--ipc]
//!
//! Modes:
//!   (default)    Human-readable stderr output
//!   --json       Final summary as JSON to stdout
//!   --snapshots  Full snapshot array + summary as JSON to stdout
//!   --ipc        Dashboard mode: JSON-lines protocol on stdio

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::env;

use factory_sim::agv::{Agv, LaneNetwork};
use factory_sim::engine::{Event, SimEngine, TimedEvent};
use factory_sim::factory::{Mill, PalletMagazine, ToolCrib};
use factory_sim::fault::{FaultConfig, FaultInjector};
use factory_sim::ipc::{IpcConfig, IpcRunner};
use factory_sim::metrics::Metrics;
use factory_sim::scheduler::Scheduler;
use factory_sim::types::*;

// ── World: aggregate root of all simulation state ───────────────────
struct World {
    mills: Vec<Mill>,
    agvs: Vec<Agv>,
    lanes: LaneNetwork,
    tool_crib: ToolCrib,
    pallet_mag: PalletMagazine,
    scheduler: Scheduler,
    fault_inj: FaultInjector,
    metrics: Metrics,
    rng: StdRng,
}

impl World {
    fn new(fault_cfg: FaultConfig) -> Self {
        let mills: Vec<Mill> = (0..NUM_MILLS).map(Mill::new).collect();

        let agvs: Vec<Agv> = (0..NUM_AGVS)
            .map(|i| Agv::new(i, (i * 3) % LOOP_SEGMENTS))
            .collect();

        let mut lanes = LaneNetwork::new();
        for agv in &agvs {
            lanes.claim(agv.segment, agv.id);
        }

        Self {
            mills,
            agvs,
            lanes,
            tool_crib: ToolCrib::new(8, 4),
            pallet_mag: PalletMagazine::new(4, 8),
            scheduler: Scheduler::new(),
            fault_inj: FaultInjector::new(fault_cfg),
            metrics: Metrics::new(60.0),
            rng: StdRng::seed_from_u64(42),
        }
    }

    fn handle(&mut self, te: &TimedEvent) -> Vec<TimedEvent> {
        let now = te.time;
        let mut out = Vec::new();

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
                        },
                    });
                }
            }
            Event::MillMachiningDone { mill_id, .. } => {
                self.mills[*mill_id].finish_machining(300.0);
                self.mills[*mill_id].state = MillState::Unloading;
                out.push(TimedEvent {
                    time: now + MILL_UNLOAD_TIME,
                    event: Event::MillUnloadDone(*mill_id),
                });
            }
            Event::MillUnloadDone(mid) => {
                if let Some(pid) = self.mills[*mid].loaded_pallet.take() {
                    let pt = self.mills[*mid].loaded_pallet_type.take().unwrap_or(0);
                    self.pallet_mag.return_pallet(pt, pid);
                }
                self.mills[*mid].finish_unloading();
            }

            Event::AgvArrived { agv_id, segment } => {
                let aid = *agv_id;
                let seg = *segment;
                if self.lanes.claim(seg, aid) {
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
                            time: now + AGV_SEGMENT_TRAVEL,
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
                if let Cargo::Workpiece { job_id, op_index } = &self.agvs[*agv_id].cargo {
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
                self.agvs[*agv_id].cargo = Cargo::Empty;
                self.agvs[*agv_id].state = AgvState::Idle;
                self.agvs[*agv_id].loads_delivered += 1;
            }

            Event::ToolIssued { .. } | Event::PalletIssued { .. } => {}

            Event::FaultOccur(target) => {
                match target {
                    FaultTarget::Mill(mid) => {
                        self.mills[*mid].fault(now);
                        eprintln!("[{now:.1}s] FAULT: Mill {mid} down");
                    }
                    FaultTarget::Agv(aid) => {
                        self.agvs[*aid].fault();
                        eprintln!("[{now:.1}s] FAULT: AGV {aid} down");
                    }
                }
                out.push(self.fault_inj.schedule_repair(now, target));
            }
            Event::FaultRepair(target) => {
                match target {
                    FaultTarget::Mill(mid) => {
                        self.mills[*mid].repair(now);
                        eprintln!("[{now:.1}s] REPAIR: Mill {mid} back online");
                    }
                    FaultTarget::Agv(aid) => {
                        self.agvs[*aid].repair();
                        eprintln!("[{now:.1}s] REPAIR: AGV {aid} back online");
                    }
                }
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
                );
                out.extend(sched_events);
            }
        }

        out
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
        let ops = (0..num_ops)
            .map(|_| Operation {
                tool_set: self.rng.gen_range(0..8),
                duration: 180.0 + self.rng.gen::<f64>() * 720.0,
                pallet_type: self.rng.gen_range(0..4),
            })
            .collect();
        Job {
            id,
            priority,
            operations: ops,
            arrived_at: arrival,
        }
    }
}

// ── CLI & main loop ─────────────────────────────────────────────────
fn main() {
    let args: Vec<String> = env::args().collect();
    let mut duration = 28_800.0;
    let mut faults_enabled = true;
    let mut json_output = false;
    let mut snapshots_output = false;
    let mut ipc_mode = false;
    let mut snapshot_interval = 1.0;
    let mut mill_mtbf = 28_800.0;
    let mut agv_mtbf = 43_200.0;
    let mut seed: u64 = 42;
    let mut max_wip: usize = DEFAULT_MAX_WIP;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--duration" => {
                i += 1;
                duration = args[i].parse().expect("invalid duration");
            }
            "--snapshot-interval" => {
                i += 1;
                snapshot_interval = args[i].parse().expect("invalid snapshot-interval");
            }
            "--mill-mtbf" => {
                i += 1;
                mill_mtbf = args[i].parse().expect("invalid mill-mtbf");
            }
            "--agv-mtbf" => {
                i += 1;
                agv_mtbf = args[i].parse().expect("invalid agv-mtbf");
            }
            "--seed" => {
                i += 1;
                seed = args[i].parse().expect("invalid seed");
            }
            "--max-wip" => {
                i += 1;
                max_wip = args[i].parse().expect("invalid max-wip");
            }
            "--no-faults" => faults_enabled = false,
            "--json" => json_output = true,
            "--snapshots" => snapshots_output = true,
            "--ipc" => ipc_mode = true,
            "--help" | "-h" => {
                println!(
                    "factory-sim [--duration SECS] [--no-faults] [--json] [--snapshots] [--ipc]\n\
                     \n\
                     Options:\n  \
                       --duration SECS          Simulation duration (default: 28800)\n  \
                       --snapshot-interval SECS  IPC snapshot interval (default: 1.0)\n  \
                       --mill-mtbf SECS         Mean time between mill failures\n  \
                       --agv-mtbf SECS          Mean time between AGV failures\n  \
                       --seed N                 RNG seed (default: 42)\n  \
                       --max-wip N              WIP limit for back-pressure (default: 20)\n  \
                       --no-faults              Disable fault injection\n  \
                       --json                   Output summary as JSON\n  \
                       --snapshots              Output snapshots + summary as JSON\n  \
                       --ipc                    Dashboard mode (JSON-lines on stdio)"
                );
                return;
            }
            _ => eprintln!("unknown arg: {}", args[i]),
        }
        i += 1;
    }

    // ── IPC dashboard mode ──────────────────────────────────────────
    if ipc_mode {
        let fault_cfg = FaultConfig {
            mill_mtbf,
            agv_mtbf,
            enabled: faults_enabled,
            ..FaultConfig::default()
        };
        let config = IpcConfig {
            duration,
            snapshot_interval,
            fault_cfg,
            seed,
            tool_types: 8,
            tool_copies: 4,
            pallet_types: 4,
            pallet_copies: 8,
            max_wip,
        };
        let mut runner = IpcRunner::new(config);
        runner.run();
        return;
    }

    // ── Batch mode (original behavior) ──────────────────────────────
    let fault_cfg = FaultConfig {
        mill_mtbf,
        agv_mtbf,
        enabled: faults_enabled,
        ..FaultConfig::default()
    };
    let mut world = World::new(fault_cfg);
    world.scheduler.max_wip = max_wip;
    let mut engine = SimEngine::new();

    let first_job = world.generate_job(0.0);
    engine.schedule(0.0, Event::JobArrival(first_job));
    engine.schedule(0.0, Event::SchedulerTick);
    let fault_events = world.fault_inj.seed_faults(&mut world.rng);
    engine.schedule_many(fault_events);

    eprintln!(
        "factory-sim: running {duration:.0}s simulation ({} mills, {} AGVs)",
        NUM_MILLS, NUM_AGVS
    );

    while let Some(te) = engine.step() {
        if te.time > duration {
            break;
        }
        let new_events = world.handle(&te);
        engine.schedule_many(new_events);
    }

    let summary = world.metrics.summarize(
        duration,
        engine.events_processed(),
        &world.mills,
        &world.scheduler,
        world.fault_inj.total_faults,
    );

    if snapshots_output {
        #[derive(serde::Serialize)]
        struct FullOutput<'a> {
            summary: &'a factory_sim::metrics::Summary,
            snapshots: &'a [factory_sim::metrics::Snapshot],
        }
        let out = FullOutput {
            summary: &summary,
            snapshots: world.metrics.snapshots(),
        };
        println!("{}", serde_json::to_string(&out).unwrap());
    } else if json_output {
        println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    } else {
        eprintln!("\n═══ Simulation Summary ═══");
        eprintln!(
            "Duration:           {:.0}s ({:.1} hours)",
            duration,
            duration / 3600.0
        );
        eprintln!("Events processed:   {}", summary.events_processed);
        eprintln!("Jobs dispatched:    {}", summary.jobs_dispatched);
        eprintln!("Parts completed:    {}", summary.jobs_completed);
        eprintln!(
            "Avg utilization:    {:.1}%",
            summary.avg_utilization * 100.0
        );
        eprintln!("Avg queue depth:    {:.1}", summary.avg_queue_depth);
        eprintln!("Deadlocks detected: {}", summary.deadlocks_detected);
        eprintln!("Back-pressure:      {}", summary.back_pressure_events);
        eprintln!("Equipment faults:   {}", summary.total_faults);
        eprintln!(
            "Throughput:         {:.1} parts/hr",
            summary.total_throughput as f64 / (duration / 3600.0)
        );
    }
}
