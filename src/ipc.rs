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

use serde::{Deserialize, Serialize};

use crate::engine::{Event, SimEngine, TimedEvent};
use crate::fault::FaultConfig;
use crate::scheduler::Scheduler;
use crate::types::*;
use crate::world::{Notification, World, WorldConfig};

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
    pub num_amrs: usize,
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
    pub max_wip: usize,
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
    pub chip_station: SegmentId,
    pub work_prep: SegmentId,
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
    pub chip_level: f64,
    pub chip_capacity: f64,
}

#[derive(Serialize)]
pub struct AgvSnap<'a> {
    pub id: AgvId,
    pub vehicle_type: &'a VehicleType,
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
    pub wip: usize,
    pub max_wip: usize,
    pub back_pressure_events: u64,
    pub chip_evacuations: u64,
    pub work_prep_jobs: u64,
    pub work_prep_queue: usize,
    pub work_prep_state: WorkPrepState,
    pub reconciliation_passes: u64,
    pub reconciliation_drifts: u64,
    pub max_drifts_in_pass: u64,
}

#[derive(Serialize)]
pub struct SummaryMsg {
    pub sim_duration: SimTime,
    pub events_processed: u64,
    pub jobs_completed: u64,
    pub jobs_dispatched: u64,
    pub deadlocks_detected: u64,
    pub total_faults: u64,
    pub back_pressure_events: u64,
    pub chip_evacuations: u64,
    pub work_prep_jobs: u64,
    pub reconciliation_passes: u64,
    pub reconciliation_drifts: u64,
    pub max_drifts_in_pass: u64,
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
    pub max_wip: usize,
    pub num_amrs: usize,
}

// ── IPC runner ─────────────────────────────────────────────────────

pub struct IpcRunner {
    world: World,
    engine: SimEngine,
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
        let world_cfg = WorldConfig {
            fault_cfg: config.fault_cfg.clone(),
            num_amrs: config.num_amrs,
            seed: config.seed,
            tool_types: config.tool_types,
            tool_copies: config.tool_copies,
            pallet_types: config.pallet_types,
            pallet_copies: config.pallet_copies,
            snapshot_interval: config.snapshot_interval,
            max_wip: config.max_wip,
        };
        let world = World::new(world_cfg);
        let engine = SimEngine::new();

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
            world,
            engine,
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
        let seed_events = self.world.seed_events();
        self.engine.schedule_many(seed_events);
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
                "mill_mtbf" => self.world.fault_inj.config.mill_mtbf = value,
                "agv_mtbf" => self.world.fault_inj.config.agv_mtbf = value,
                "snapshot_interval" => self.config.snapshot_interval = value,
                "max_wip" => self.world.scheduler.max_wip = value as usize,
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
        let (new_events, notes) = self.world.handle(&te);
        self.engine.schedule_many(new_events);
        self.handle_notifications(now, notes);
        self.maybe_emit_snapshot(now);
        true
    }

    fn handle_notifications(&self, now: SimTime, notes: Vec<Notification>) {
        for note in notes {
            match note {
                Notification::WorkPrepReady { job_id, mill_id } => {
                    eprintln!(
                        "[INFO] [{now:.1}s] WORK-PREP: job {job_id} ready for mill {mill_id}"
                    );
                }
                Notification::Reconciliation { pass, ref drifts } => {
                    for d in drifts {
                        emit_json(&OutMessage::Event {
                            time: now,
                            kind: "drift",
                            detail: serde_json::json!({
                                "category": format!("{:?}", d.category),
                                "description": d.description,
                                "pass": pass,
                            }),
                        });
                    }
                    eprintln!(
                        "[INFO] [{now:.1}s] RECONCILIATION: pass {pass} found {} drift(s)",
                        drifts.len()
                    );
                }
                // FaultOccurred / Repaired / ChipEvacDone are already
                // emitted by emit_event_if_notable before handling.
                _ => {}
            }
        }
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
                num_amrs: self.config.num_amrs,
                duration: self.config.duration,
                snapshot_interval: self.config.snapshot_interval,
                faults_enabled: self.world.fault_inj.config.enabled,
                mill_mtbf: self.world.fault_inj.config.mill_mtbf,
                agv_mtbf: self.world.fault_inj.config.agv_mtbf,
                seed: self.config.seed,
                tool_types: self.config.tool_types,
                pallet_types: self.config.pallet_types,
                loop_segments: LOOP_SEGMENTS,
                total_segments: TOTAL_SEGMENTS,
                max_wip: self.world.scheduler.max_wip,
            },
            layout: LayoutInfo {
                mills: mill_layouts,
                stations: StationLayout {
                    tool_crib: TOOL_CRIB_SEG,
                    pallet_magazine: PALLET_MAG_SEG,
                    chip_station: CHIP_STATION_SEG,
                    work_prep: WORK_PREP_SEG,
                },
            },
        };
        emit_json(&msg);
    }

    fn emit_snapshot(&mut self, now: SimTime) {
        let w = &self.world;
        let total_parts: u64 = w.mills.iter().map(|m| m.parts_completed).sum();
        self.throughput_history.push((now, total_parts));

        let rate = if now > 0.0 {
            total_parts as f64 / (now / 3600.0)
        } else {
            0.0
        };

        let total_busy: SimTime = w.mills.iter().map(|m| m.busy_time).sum();
        let avg_util = if now > 0.0 {
            total_busy / (now * NUM_MILLS as f64)
        } else {
            0.0
        };

        let mill_snaps: Vec<MillSnap> = w
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
                chip_level: m.chip_level,
                chip_capacity: m.chip_capacity,
            })
            .collect();

        let agv_snaps: Vec<AgvSnap> = w
            .agvs
            .iter()
            .map(|a| AgvSnap {
                id: a.id,
                vehicle_type: &a.vehicle_type,
                state: a.state.clone(),
                segment: a.segment,
                cargo: &a.cargo,
                path: &a.path,
                path_cursor: a.path_cursor,
                delivered: a.loads_delivered,
            })
            .collect();

        let lane_occ: Vec<i32> = w
            .lanes
            .occupancy()
            .iter()
            .map(|o| o.map_or(-1, |id| id as i32))
            .collect();

        let next_8: Vec<JobPreview> = w
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
                inventory: w.tool_crib.inventory().clone(),
                total_issues: w.tool_crib.total_issues(),
            },
            pallet_magazine: PalletMagSnap {
                available: w.pallet_mag.available_counts(),
                total_issued: w.pallet_mag.total_issued(),
            },
            job_queue: JobQueueSnap {
                depth: w.scheduler.job_queue.len(),
                next_8,
            },
            metrics: MetricsSnap {
                throughput: total_parts,
                throughput_rate: rate,
                avg_utilization: avg_util,
                avg_queue_depth: 0.0,
                deadlocks: w.scheduler.deadlocks_detected,
                faults: w.fault_inj.total_faults,
                wip: Scheduler::wip_count(&w.mills),
                max_wip: w.scheduler.max_wip,
                back_pressure_events: w.scheduler.back_pressure_events,
                chip_evacuations: w.scheduler.chip_evacs_dispatched,
                work_prep_jobs: w.work_prep.jobs_completed,
                work_prep_queue: w.work_prep.total_items(),
                work_prep_state: w.work_prep.state.clone(),
                reconciliation_passes: w.reconciler.passes,
                reconciliation_drifts: w.reconciler.total_drifts,
                max_drifts_in_pass: w.reconciler.max_drifts_in_pass,
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
                    FaultTarget::Mill(_) => now + self.world.fault_inj.config.mill_repair,
                    FaultTarget::Agv(id) => {
                        if *id >= NUM_AGVS {
                            now + self.world.fault_inj.config.amr_repair
                        } else {
                            now + self.world.fault_inj.config.agv_repair
                        }
                    }
                    FaultTarget::WorkPrep => now + self.world.fault_inj.config.work_prep_repair,
                };
                emit_json(&OutMessage::Event {
                    time: now,
                    kind: "fault",
                    detail: serde_json::json!({
                        "target": target,
                        "expected_repair": repair_time,
                    }),
                });
                eprintln!(
                    "[INFO] [{now:.1}s] FAULT: {} down",
                    self.world.fault_label(target)
                );
            }
            Event::FaultRepair(target) => {
                emit_json(&OutMessage::Event {
                    time: now,
                    kind: "repair",
                    detail: serde_json::json!({ "target": target }),
                });
                eprintln!(
                    "[INFO] [{now:.1}s] REPAIR: {} back online",
                    self.world.fault_label(target)
                );
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
            Event::ChipEvacDone(mid) => {
                emit_json(&OutMessage::Event {
                    time: now,
                    kind: "chip_evac",
                    detail: serde_json::json!({ "mill_id": mid }),
                });
            }
            Event::WorkPrepDone {
                job_id, mill_id, ..
            } => {
                emit_json(&OutMessage::Event {
                    time: now,
                    kind: "work_prep_done",
                    detail: serde_json::json!({
                        "job_id": job_id,
                        "mill_id": mill_id,
                    }),
                });
            }
            _ => {}
        }
    }

    fn emit_summary(&self) {
        let w = &self.world;
        let dur = self.config.duration;
        let total_parts: u64 = w.mills.iter().map(|m| m.parts_completed).sum();
        let utilizations: Vec<f64> = w
            .mills
            .iter()
            .map(|m| if dur > 0.0 { m.busy_time / dur } else { 0.0 })
            .collect();
        let avg_util = utilizations.iter().sum::<f64>() / utilizations.len().max(1) as f64;

        let msg = OutMessage::Summary(SummaryMsg {
            sim_duration: dur,
            events_processed: self.engine.events_processed(),
            jobs_completed: total_parts,
            jobs_dispatched: w.scheduler.jobs_dispatched,
            deadlocks_detected: w.scheduler.deadlocks_detected,
            total_faults: w.fault_inj.total_faults,
            back_pressure_events: w.scheduler.back_pressure_events,
            chip_evacuations: w.scheduler.chip_evacs_dispatched,
            work_prep_jobs: w.work_prep.jobs_completed,
            reconciliation_passes: w.reconciler.passes,
            reconciliation_drifts: w.reconciler.total_drifts,
            max_drifts_in_pass: w.reconciler.max_drifts_in_pass,
            mill_utilization: utilizations,
            avg_utilization: avg_util,
            avg_queue_depth: 0.0,
            throughput_rate: if dur > 0.0 {
                total_parts as f64 / (dur / 3600.0)
            } else {
                0.0
            },
            agv_distance: w.agvs.iter().map(|a| a.distance_traveled).collect(),
            agv_deliveries: w.agvs.iter().map(|a| a.loads_delivered).collect(),
        });
        emit_json(&msg);
    }
}

fn emit_json<T: Serialize>(msg: &T) {
    let line = serde_json::to_string(msg).unwrap();
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    let _ = writeln!(lock, "{line}");
    let _ = lock.flush();
}
