//! IPC protocol for the Electron dashboard.
//!
//! When invoked with `--ipc`, the sim communicates over stdio using
//! JSON-lines (one JSON object per newline). Outgoing messages go to
//! stdout; incoming commands arrive on stdin. Diagnostic output goes
//! to stderr with severity prefixes.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::mpsc;
use std::thread;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::agv::{Agv, LaneNetwork};
use crate::engine::{Event, SimEngine, TimedEvent};
use crate::factory::{Mill, PalletMagazine, ToolCrib};
use crate::fault::{FaultConfig, FaultInjector};
use crate::metrics::Metrics;
use crate::scheduler::Scheduler;
use crate::types::*;

// ── Outgoing messages (sim → dashboard) ────────────────────────────

#[derive(Serialize)]
#[serde(tag = "type")]
pub enum OutMessage<'a> {
    #[serde(rename = "ready")]
    Ready {
        version: &'static str,
        config: ReadyConfig,
        layout: LayoutInfo,
    },
    #[serde(rename = "snapshot")]
    Snapshot(DashboardSnapshot<'a>),
    #[serde(rename = "event")]
    Event {
        time: SimTime,
        kind: &'static str,
        detail: serde_json::Value,
    },
    #[serde(rename = "summary")]
    Summary(SummaryMsg),
}

#[derive(Serialize)]
pub struct ReadyConfig {
    pub num_mills: usize,
    pub num_agvs: usize,
    pub duration: SimTime,
    pub snapshot_interval: SimTime,
    pub faults_enabled: bool,
    pub mill_mtbf: SimTime,
    pub agv_mtbf: SimTime,
    pub seed: u64,
    pub tool_types: u16,
    pub pallet_types: u8,
    pub loop_segments: usize,
    pub total_segments: usize,
}

#[derive(Serialize)]
pub struct LayoutInfo {
    pub mills: Vec<MillLayout>,
    pub stations: StationLayout,
}

#[derive(Serialize)]
pub struct MillLayout {
    pub id: MillId,
    pub row: usize,
    pub col: usize,
    pub loop_seg: SegmentId,
    pub spur_seg: SegmentId,
}

#[derive(Serialize)]
pub struct StationLayout {
    pub tool_crib: SegmentId,
    pub pallet_magazine: SegmentId,
}

#[derive(Serialize)]
pub struct DashboardSnapshot<'a> {
    pub time: SimTime,
    pub event_count: u64,
    pub mills: Vec<MillSnap>,
    pub agvs: Vec<AgvSnap<'a>>,
    pub lane_occupancy: Vec<i32>,
    pub tool_crib: ToolCribSnap,
    pub pallet_magazine: PalletMagSnap,
    pub job_queue: JobQueueSnap,
    pub metrics: MetricsSnap,
}

#[derive(Serialize)]
pub struct MillSnap {
    pub id: MillId,
    pub state: MillState,
    pub job_id: Option<JobId>,
    pub op_index: usize,
    pub loaded_tool: Option<ToolSetId>,
    pub loaded_pallet: Option<PalletId>,
    pub parts_completed: u64,
    pub busy_time: SimTime,
    pub fault_time: SimTime,
}

#[derive(Serialize)]
pub struct AgvSnap<'a> {
    pub id: AgvId,
    pub state: AgvState,
    pub segment: SegmentId,
    pub cargo: &'a Cargo,
    pub path: &'a Vec<SegmentId>,
    pub path_cursor: usize,
    pub delivered: u64,
}

#[derive(Serialize)]
pub struct ToolCribSnap {
    pub inventory: HashMap<ToolSetId, u16>,
    pub total_issues: u64,
}

#[derive(Serialize)]
pub struct PalletMagSnap {
    pub available: HashMap<u8, usize>,
    pub total_issued: u64,
}

#[derive(Serialize)]
pub struct JobQueueSnap {
    pub depth: usize,
    pub next_8: Vec<JobPreview>,
}

#[derive(Serialize)]
pub struct JobPreview {
    pub id: JobId,
    pub priority: Priority,
    pub ops: usize,
    pub wait_time: SimTime,
}

#[derive(Serialize)]
pub struct MetricsSnap {
    pub throughput: u64,
    pub throughput_rate: f64,
    pub avg_utilization: f64,
    pub avg_queue_depth: f64,
    pub deadlocks: u64,
    pub faults: u64,
}

#[derive(Serialize)]
pub struct SummaryMsg {
    pub sim_duration: SimTime,
    pub events_processed: u64,
    pub jobs_completed: u64,
    pub jobs_dispatched: u64,
    pub deadlocks_detected: u64,
    pub total_faults: u64,
    pub mill_utilization: Vec<f64>,
    pub avg_utilization: f64,
    pub avg_queue_depth: f64,
    pub throughput_rate: f64,
    pub agv_distance: Vec<u64>,
    pub agv_deliveries: Vec<u64>,
}

// ── Incoming commands (dashboard → sim) ────────────────────────────

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
pub enum InCommand {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "pause")]
    Pause,
    #[serde(rename = "resume")]
    Resume,
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "speed")]
    Speed { multiplier: usize },
    #[serde(rename = "inject_fault")]
    InjectFault { target: FaultTarget },
    #[serde(rename = "set_param")]
    SetParam { param: String, value: f64 },
    #[serde(rename = "step")]
    Step { count: usize },
}

// ── IPC configuration ──────────────────────────────────────────────

pub struct IpcConfig {
    pub duration: SimTime,
    pub snapshot_interval: SimTime,
    pub fault_cfg: FaultConfig,
    pub seed: u64,
    pub tool_types: u16,
    pub tool_copies: u16,
    pub pallet_types: u8,
    pub pallet_copies: usize,
}

// ── IPC runner ─────────────────────────────────────────────────────

pub struct IpcRunner {
    mills: Vec<Mill>,
    agvs: Vec<Agv>,
    lanes: LaneNetwork,
    tool_crib: ToolCrib,
    pallet_mag: PalletMagazine,
    scheduler: Scheduler,
    fault_inj: FaultInjector,
    metrics: Metrics,
    engine: SimEngine,
    rng: StdRng,

    config: IpcConfig,
    paused: bool,
    speed: usize,
    last_snapshot_time: SimTime,
    finished: bool,
    cmd_rx: mpsc::Receiver<InCommand>,
    throughput_history: Vec<(SimTime, u64)>,
}

impl IpcRunner {
    pub fn new(config: IpcConfig) -> Self {
        let mills: Vec<Mill> = (0..NUM_MILLS).map(Mill::new).collect();
        let agvs: Vec<Agv> = (0..NUM_AGVS)
            .map(|i| Agv::new(i, (i * 3) % LOOP_SEGMENTS))
            .collect();
        let mut lanes = LaneNetwork::new();
        for agv in &agvs {
            lanes.claim(agv.segment, agv.id);
        }

        let tool_crib = ToolCrib::new(config.tool_types, config.tool_copies);
        let pallet_mag = PalletMagazine::new(config.pallet_types, config.pallet_copies);
        let fault_inj = FaultInjector::new(config.fault_cfg.clone());
        let metrics = Metrics::new(config.snapshot_interval);
        let engine = SimEngine::new();
        let rng = StdRng::seed_from_u64(config.seed);

        let (cmd_tx, cmd_rx) = mpsc::channel();
        thread::spawn(move || {
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break };
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<InCommand>(&line) {
                    Ok(cmd) => {
                        if cmd_tx.send(cmd).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("[WARN] bad command: {e}");
                    }
                }
            }
        });

        Self {
            mills,
            agvs,
            lanes,
            tool_crib,
            pallet_mag,
            scheduler: Scheduler::new(),
            fault_inj,
            metrics,
            engine,
            rng,
            config,
            paused: true,
            speed: 1,
            last_snapshot_time: -1.0,
            finished: false,
            cmd_rx,
            throughput_history: Vec::new(),
        }
    }

    pub fn run(&mut self) {
        self.seed_events();
        self.emit_ready();

        loop {
            self.drain_commands();
            if self.finished {
                break;
            }
            if self.paused {
                match self.cmd_rx.recv() {
                    Ok(cmd) => self.handle_command(cmd),
                    Err(_) => break,
                }
                continue;
            }
            self.process_batch();
        }
    }

    fn seed_events(&mut self) {
        let first_job = self.generate_job(0.0);
        self.engine.schedule(0.0, Event::JobArrival(first_job));
        self.engine.schedule(0.0, Event::SchedulerTick);
        let fault_events = self.fault_inj.seed_faults(&mut self.rng);
        self.engine.schedule_many(fault_events);
    }

    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            self.handle_command(cmd);
        }
    }

    fn handle_command(&mut self, cmd: InCommand) {
        match cmd {
            InCommand::Start | InCommand::Resume => {
                self.paused = false;
            }
            InCommand::Pause => {
                self.paused = true;
            }
            InCommand::Stop => {
                self.emit_summary();
                self.finished = true;
            }
            InCommand::Speed { multiplier } => {
                self.speed = multiplier.max(1);
            }
            InCommand::InjectFault { target } => {
                let now = self.engine.now();
                self.engine.schedule(now, Event::FaultOccur(target));
            }
            InCommand::SetParam { param, value } => match param.as_str() {
                "mill_mtbf" => self.fault_inj.config.mill_mtbf = value,
                "agv_mtbf" => self.fault_inj.config.agv_mtbf = value,
                "snapshot_interval" => self.config.snapshot_interval = value,
                _ => eprintln!("[WARN] unknown param: {param}"),
            },
            InCommand::Step { count } => {
                for _ in 0..count {
                    if !self.step_one() {
                        break;
                    }
                }
                self.paused = true;
            }
        }
    }

    fn process_batch(&mut self) {
        for _ in 0..self.speed {
            if !self.step_one() {
                self.emit_summary();
                self.finished = true;
                return;
            }
        }
    }

    fn step_one(&mut self) -> bool {
        let Some(te) = self.engine.step() else {
            return false;
        };
        if te.time > self.config.duration {
            return false;
        }
        let now = te.time;
        self.emit_event_if_notable(&te);
        let new_events = self.handle_event(&te);
        self.engine.schedule_many(new_events);
        self.maybe_emit_snapshot(now);
        true
    }

    fn maybe_emit_snapshot(&mut self, now: SimTime) {
        if now - self.last_snapshot_time >= self.config.snapshot_interval {
            self.last_snapshot_time = now;
            self.emit_snapshot(now);
        }
    }

    fn emit_ready(&self) {
        let mill_layouts: Vec<MillLayout> = (0..NUM_MILLS)
            .map(|mid| MillLayout {
                id: mid,
                row: mid / MILLS_PER_ROW,
                col: mid % MILLS_PER_ROW,
                loop_seg: mill_loop_segment(mid),
                spur_seg: mill_spur(mid),
            })
            .collect();

        let msg = OutMessage::Ready {
            version: "0.1.0",
            config: ReadyConfig {
                num_mills: NUM_MILLS,
                num_agvs: NUM_AGVS,
                duration: self.config.duration,
                snapshot_interval: self.config.snapshot_interval,
                faults_enabled: self.fault_inj.config.enabled,
                mill_mtbf: self.fault_inj.config.mill_mtbf,
                agv_mtbf: self.fault_inj.config.agv_mtbf,
                seed: self.config.seed,
                tool_types: self.config.tool_types,
                pallet_types: self.config.pallet_types,
                loop_segments: LOOP_SEGMENTS,
                total_segments: TOTAL_SEGMENTS,
            },
            layout: LayoutInfo {
                mills: mill_layouts,
                stations: StationLayout {
                    tool_crib: TOOL_CRIB_SEG,
                    pallet_magazine: PALLET_MAG_SEG,
                },
            },
        };
        emit_json(&msg);
    }

    fn emit_snapshot(&mut self, now: SimTime) {
        let total_parts: u64 = self.mills.iter().map(|m| m.parts_completed).sum();
        self.throughput_history.push((now, total_parts));

        let rate = if now > 0.0 {
            total_parts as f64 / (now / 3600.0)
        } else {
            0.0
        };

        let total_busy: SimTime = self.mills.iter().map(|m| m.busy_time).sum();
        let avg_util = if now > 0.0 {
            total_busy / (now * NUM_MILLS as f64)
        } else {
            0.0
        };

        let mill_snaps: Vec<MillSnap> = self
            .mills
            .iter()
            .map(|m| MillSnap {
                id: m.id,
                state: m.state.clone(),
                job_id: m.current_job,
                op_index: m.current_op,
                loaded_tool: m.loaded_tool,
                loaded_pallet: m.loaded_pallet,
                parts_completed: m.parts_completed,
                busy_time: m.busy_time,
                fault_time: m.fault_time,
            })
            .collect();

        let agv_snaps: Vec<AgvSnap> = self
            .agvs
            .iter()
            .map(|a| AgvSnap {
                id: a.id,
                state: a.state.clone(),
                segment: a.segment,
                cargo: &a.cargo,
                path: &a.path,
                path_cursor: a.path_cursor,
                delivered: a.loads_delivered,
            })
            .collect();

        let lane_occ: Vec<i32> = self
            .lanes
            .occupancy()
            .iter()
            .map(|o| o.map_or(-1, |id| id as i32))
            .collect();

        let next_8: Vec<JobPreview> = self
            .scheduler
            .job_queue
            .iter()
            .take(8)
            .map(|j| JobPreview {
                id: j.id,
                priority: j.priority,
                ops: j.operations.len(),
                wait_time: now - j.arrived_at,
            })
            .collect();

        let snap = DashboardSnapshot {
            time: now,
            event_count: self.engine.events_processed(),
            mills: mill_snaps,
            agvs: agv_snaps,
            lane_occupancy: lane_occ,
            tool_crib: ToolCribSnap {
                inventory: self.tool_crib.inventory().clone(),
                total_issues: self.tool_crib.total_issues(),
            },
            pallet_magazine: PalletMagSnap {
                available: self.pallet_mag.available_counts(),
                total_issued: self.pallet_mag.total_issued(),
            },
            job_queue: JobQueueSnap {
                depth: self.scheduler.job_queue.len(),
                next_8,
            },
            metrics: MetricsSnap {
                throughput: total_parts,
                throughput_rate: rate,
                avg_utilization: avg_util,
                avg_queue_depth: 0.0,
                deadlocks: self.scheduler.deadlocks_detected,
                faults: self.fault_inj.total_faults,
            },
        };

        let msg = OutMessage::Snapshot(snap);
        emit_json(&msg);
    }

    fn emit_event_if_notable(&self, te: &TimedEvent) {
        let now = te.time;
        match &te.event {
            Event::FaultOccur(target) => {
                let repair_time = match target {
                    FaultTarget::Mill(_) => now + self.fault_inj.config.mill_repair,
                    FaultTarget::Agv(_) => now + self.fault_inj.config.agv_repair,
                };
                emit_json(&OutMessage::Event {
                    time: now,
                    kind: "fault",
                    detail: serde_json::json!({
                        "target": target,
                        "expected_repair": repair_time,
                    }),
                });
                let label = match target {
                    FaultTarget::Mill(id) => format!("Mill {id}"),
                    FaultTarget::Agv(id) => format!("AGV {id}"),
                };
                eprintln!("[INFO] [{now:.1}s] FAULT: {label} down");
            }
            Event::FaultRepair(target) => {
                emit_json(&OutMessage::Event {
                    time: now,
                    kind: "repair",
                    detail: serde_json::json!({ "target": target }),
                });
                let label = match target {
                    FaultTarget::Mill(id) => format!("Mill {id}"),
                    FaultTarget::Agv(id) => format!("AGV {id}"),
                };
                eprintln!("[INFO] [{now:.1}s] REPAIR: {label} back online");
            }
            Event::MillMachiningDone {
                mill_id, job_id, ..
            } => {
                emit_json(&OutMessage::Event {
                    time: now,
                    kind: "completion",
                    detail: serde_json::json!({
                        "mill_id": mill_id,
                        "job_id": job_id,
                    }),
                });
            }
            _ => {}
        }
    }

    fn emit_summary(&self) {
        let dur = self.config.duration;
        let total_parts: u64 = self.mills.iter().map(|m| m.parts_completed).sum();
        let utilizations: Vec<f64> = self
            .mills
            .iter()
            .map(|m| if dur > 0.0 { m.busy_time / dur } else { 0.0 })
            .collect();
        let avg_util = utilizations.iter().sum::<f64>() / utilizations.len().max(1) as f64;

        let msg = OutMessage::Summary(SummaryMsg {
            sim_duration: dur,
            events_processed: self.engine.events_processed(),
            jobs_completed: total_parts,
            jobs_dispatched: self.scheduler.jobs_dispatched,
            deadlocks_detected: self.scheduler.deadlocks_detected,
            total_faults: self.fault_inj.total_faults,
            mill_utilization: utilizations,
            avg_utilization: avg_util,
            avg_queue_depth: 0.0,
            throughput_rate: if dur > 0.0 {
                total_parts as f64 / (dur / 3600.0)
            } else {
                0.0
            },
            agv_distance: self.agvs.iter().map(|a| a.distance_traveled).collect(),
            agv_deliveries: self.agvs.iter().map(|a| a.loads_delivered).collect(),
        });
        emit_json(&msg);
    }

    // Reuse the same event handling logic as World in main.rs.
    fn handle_event(&mut self, te: &TimedEvent) -> Vec<TimedEvent> {
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
                    FaultTarget::Mill(mid) => self.mills[*mid].fault(now),
                    FaultTarget::Agv(aid) => self.agvs[*aid].fault(),
                }
                out.push(self.fault_inj.schedule_repair(now, target));
            }
            Event::FaultRepair(target) => {
                match target {
                    FaultTarget::Mill(mid) => self.mills[*mid].repair(now),
                    FaultTarget::Agv(aid) => self.agvs[*aid].repair(),
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
                tool_set: self.rng.gen_range(0..self.config.tool_types),
                duration: 180.0 + self.rng.gen::<f64>() * 720.0,
                pallet_type: self.rng.gen_range(0..self.config.pallet_types),
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

fn emit_json<T: Serialize>(msg: &T) {
    let line = serde_json::to_string(msg).unwrap();
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    let _ = writeln!(lock, "{line}");
    let _ = lock.flush();
}
