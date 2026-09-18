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

use std::env;

use factory_sim::engine::SimEngine;
use factory_sim::fault::FaultConfig;
use factory_sim::ipc::{IpcConfig, IpcRunner};
use factory_sim::types::*;
use factory_sim::world::{Notification, World, WorldConfig};

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
    let mut num_amrs: usize = DEFAULT_NUM_AMRS;

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
            "--num-amrs" => {
                i += 1;
                num_amrs = args[i].parse().expect("invalid num-amrs");
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
                       --num-amrs N             Number of AMRs in fleet (default: 2)\n  \
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

    let fault_cfg = FaultConfig {
        mill_mtbf,
        agv_mtbf,
        num_amrs,
        enabled: faults_enabled,
        ..FaultConfig::default()
    };

    // ── IPC dashboard mode ──────────────────────────────────────────
    if ipc_mode {
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
            num_amrs,
        };
        let mut runner = IpcRunner::new(config);
        runner.run();
        return;
    }

    // ── Batch mode ──────────────────────────────────────────────────
    let world_cfg = WorldConfig {
        fault_cfg,
        num_amrs,
        seed,
        tool_types: 8,
        tool_copies: 4,
        pallet_types: 4,
        pallet_copies: 8,
        snapshot_interval: 60.0,
        max_wip,
    };
    let mut world = World::new(world_cfg);
    let mut engine = SimEngine::new();

    engine.schedule_many(world.seed_events());

    eprintln!(
        "factory-sim: running {duration:.0}s simulation ({} mills, {} AGVs, {} AMRs)",
        NUM_MILLS, NUM_AGVS, num_amrs
    );

    while let Some(te) = engine.step() {
        if te.time > duration {
            break;
        }
        let now = te.time;
        let (new_events, notes) = world.handle(&te);
        engine.schedule_many(new_events);

        for note in notes {
            match note {
                Notification::FaultOccurred { ref target } => {
                    eprintln!("[{now:.1}s] FAULT: {} down", world.fault_label(target));
                }
                Notification::Repaired { ref target } => {
                    eprintln!(
                        "[{now:.1}s] REPAIR: {} back online",
                        world.fault_label(target)
                    );
                }
                Notification::ChipEvacDone { mill_id } => {
                    eprintln!("[{now:.1}s] CHIP-EVAC: mill {mill_id} chips cleared");
                }
                Notification::WorkPrepReady { job_id, mill_id } => {
                    eprintln!("[{now:.1}s] WORK-PREP: job {job_id} ready for mill {mill_id}");
                }
                Notification::Reconciliation { pass, ref drifts } => {
                    eprintln!(
                        "[{now:.1}s] RECONCILIATION: pass {pass} found {} drift(s)",
                        drifts.len()
                    );
                    for d in drifts {
                        eprintln!("  {:?}: {}", d.category, d.description);
                    }
                }
            }
        }
    }

    let summary = world.metrics.summarize(
        duration,
        engine.events_processed(),
        &world.mills,
        &world.scheduler,
        world.fault_inj.total_faults,
        &world.reconciler,
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
        eprintln!("Chip evacuations:   {}", summary.chip_evacuations);
        eprintln!("Work prep jobs:     {}", summary.work_prep_jobs);
        eprintln!(
            "Reconciliation:     {} passes, {} drifts (max {} per pass)",
            summary.reconciliation_passes,
            summary.reconciliation_drifts,
            summary.max_drifts_in_pass
        );
        eprintln!("Equipment faults:   {}", summary.total_faults);
        eprintln!(
            "Throughput:         {:.1} parts/hr",
            summary.total_throughput as f64 / (duration / 3600.0)
        );
    }
}
