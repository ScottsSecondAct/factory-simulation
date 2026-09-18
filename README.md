# Factory Orchestration Simulator

Discrete-event factory orchestration simulator modeling a flexible manufacturing system (FMS) with 25 CNC mills, 6 AGVs on a shared-lane network, tool crib, pallet magazine, and priority job queue -- plus an Electron dashboard for real-time visualization.

[![Rust](https://img.shields.io/badge/Rust-2021_edition-orange)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-Source_Available-lightgrey.svg)](LICENSE)
[![CI](https://github.com/ScottsSecondAct/factory-simulation/actions/workflows/ci.yml/badge.svg)](https://github.com/ScottsSecondAct/factory-simulation/actions/workflows/ci.yml)
[![Claude Assisted](https://img.shields.io/badge/Claude-Assisted-blueviolet?logo=anthropic&logoColor=white)](https://claude.ai)

## Architecture

The simulator is a **discrete-event engine** built on a min-heap of `TimedEvent`s. Pop the next event, advance the simulation clock, dispatch to the appropriate handler, collect any new events the handler returns, and enqueue them. No real clock -- an 8-hour shift completes in under a second.

```
SimEngine (min-heap event queue)
    |
    v
World::handle(event) --> dispatches to subsystems --> returns new events
    |
    +-- factory.rs    Mill state machines, ToolCrib, PalletMagazine
    +-- agv.rs        AGV fleet, LaneNetwork (segment locking, BFS routing)
    +-- scheduler.rs  Job dispatch, look-ahead, deadlock detection
    +-- fault.rs      Stochastic failure model (MTBF/repair)
    +-- metrics.rs    Snapshots, utilization, throughput, JSON output
    +-- ipc.rs        Dashboard IPC protocol (JSON-lines on stdio)
```

The **Electron dashboard** (`dashboard/`) is a standalone application built with React, TypeScript, Zustand, and electron-vite. It spawns the Rust sim as a child process with `--ipc` and communicates over stdio using a JSON-lines protocol. The dashboard renders the factory floor as an SVG, displays live metrics, and accepts commands (speed control, fault injection, pause/resume) that are sent back to the sim on stdin.

## Quick Start

### Prerequisites

- **Rust toolchain** (stable, 2021 edition) -- install via [rustup](https://rustup.rs/)
- **Node.js 18+** and npm -- required for the Electron dashboard

### Build and run the simulation (standalone)

```bash
cargo build --release

# Default: 8-hour shift (28800s) with stochastic faults
cargo run -- --duration 28800

# JSON summary to stdout
cargo run -- --duration 28800 --json

# Full snapshot array + summary
cargo run -- --duration 28800 --snapshots
```

### Run the Electron dashboard

```bash
cd dashboard
npm install
npm run dev
```

The dashboard launches the sim binary automatically in IPC mode. It expects the Rust binary to be built first (`cargo build --release`).

## CLI Flags

| Flag | Description | Default |
|---|---|---|
| `--duration SECS` | Simulation duration in seconds | `28800` (8 hours) |
| `--no-faults` | Disable stochastic fault injection | faults enabled |
| `--json` | Output final summary as JSON to stdout | human-readable stderr |
| `--snapshots` | Output full snapshot array + summary as JSON | off |
| `--ipc` | Dashboard mode: JSON-lines protocol on stdio | off |
| `--snapshot-interval SECS` | Interval between IPC snapshots | `1.0` |
| `--mill-mtbf SECS` | Mean time between mill failures | `28800` |
| `--agv-mtbf SECS` | Mean time between AGV failures | `43200` |
| `--seed N` | RNG seed for reproducible runs | `42` |
| `--max-wip N` | WIP limit for back-pressure | `20` |
| `--num-amrs N` | Number of AMRs in the fleet | `2` |

## IPC Protocol

When launched with `--ipc`, the sim communicates over stdio using JSON-lines (one JSON object per `\n`). This is the protocol the Electron dashboard uses.

**Outgoing (sim -> dashboard on stdout):**

- `{"type":"ready", ...}` -- sent once at startup with configuration, layout geometry, and initial state
- `{"type":"snapshot", ...}` -- periodic state snapshot (mill states, AGV positions, resources, metrics)
- `{"type":"event", ...}` -- discrete events (faults, repairs, deadlocks, job completions)
- `{"type":"summary", ...}` -- final summary at simulation end

**Incoming (dashboard -> sim on stdin):**

- `{"cmd":"set_speed", "factor": N}` -- adjust simulation speed
- `{"cmd":"pause"}` / `{"cmd":"resume"}` -- pause/resume simulation
- `{"cmd":"inject_fault", "target": ...}` -- manually trigger a fault
- `{"cmd":"reset", ...}` -- reset simulation with new parameters

Diagnostic messages go to stderr with severity prefixes, separate from protocol traffic.

## Dashboard Features

- **Factory floor SVG** -- real-time visualization of the 5x5 mill grid, lane network, and AGV positions with state-based coloring
- **Metrics panel** -- live utilization, throughput, events processed, jobs dispatched/completed
- **Resource tracking** -- tool crib inventory and pallet magazine availability
- **Job queue panel** -- current queue depth with priority breakdown
- **Trend charts** -- time-series plots of utilization and throughput
- **Event log** -- timestamped simulation events (faults, repairs, deadlocks)
- **Control bar** -- play/pause, speed control, fault injection, simulation reset

## Project Structure

```
factory-simulation/
+-- src/                    Rust simulation
|   +-- main.rs             CLI entry point, batch-mode event loop
|   +-- world.rs            World aggregate, shared event dispatch
|   +-- engine.rs           Event queue, clock, TimedEvent
|   +-- factory.rs          Mill FSM, ToolCrib, PalletMagazine, WorkPrepStation
|   +-- agv.rs              AGV state, LaneNetwork, WaitForGraph
|   +-- scheduler.rs        Job dispatch, look-ahead, deadlock detection
|   +-- fault.rs            Stochastic failure model
|   +-- reconcile.rs        Periodic state reconciliation pass
|   +-- metrics.rs          Snapshots, summaries, JSON serialization
|   +-- ipc.rs              IPC protocol for dashboard communication
|   +-- types.rs            IDs, enums, constants, layout geometry
|   +-- lib.rs              Library crate root
+-- dashboard/              Electron dashboard (React + TypeScript)
|   +-- src/
|   |   +-- main/           Electron main process (spawns sim, IPC bridge)
|   |   +-- preload/        Context bridge for renderer
|   |   +-- renderer/src/   React UI components
|   |       +-- App.tsx
|   |       +-- store.ts            Zustand state management
|   |       +-- components/
|   |           +-- FactoryFloor/   SVG factory visualization
|   |           +-- MetricsPanel.tsx
|   |           +-- ResourcePanel.tsx
|   |           +-- JobQueuePanel.tsx
|   |           +-- TrendChart.tsx
|   |           +-- EventLog.tsx
|   |           +-- ControlBar.tsx
|   +-- package.json
|   +-- electron.vite.config.ts
+-- Cargo.toml
+-- CLAUDE.md               Architecture and conventions reference
```

## Lane Network Topology

```
              Tool Crib (seg 0)
                   |
         +--- 19 < 0 > 1 ---+
         |    |              |
        18    2--[Row 0: Mills 0-4, spurs 20-24]
         |    |
        17    3
         |    |
        16    4
         |    |              Chip Station (seg 5)
Work     |    |                   |
Prep  > 15    5 <-----------------+
(seg 15) |    |
        14    6--[Row 1: Mills 5-9, spurs 25-29]
         |    |
        13    7
         |    |
        12    8
         |    |
        11    9
         |    |              |
         +-- 10 < - - - - ---+
              |
         Pallet Magazine (seg 10)
```

Each mill connects to the loop via a dedicated spur segment. The fleet includes both AGVs (can enter spurs) and AMRs (main loop only). Vehicles compete for segment access with mutual exclusion at the segment level. The scheduler maintains a wait-for graph to detect and resolve circular waits (deadlocks) through victim retreat.

## AI Assistance

This project was developed with the assistance of [Claude](https://claude.ai) (Anthropic). Claude contributed to code implementation, debugging, documentation, and code review throughout the development process. All architecture decisions, domain modeling, and engineering direction are by the author.

## License

Copyright (c) 2026 Scott Davis. All rights reserved. This source code is available for viewing and reference only. See [LICENSE](LICENSE) for details.
