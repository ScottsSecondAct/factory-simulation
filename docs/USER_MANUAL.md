# Factory Orchestration Simulator — User Manual

## 1. Introduction

The Factory Orchestration Simulator is a discrete-event simulation of a flexible manufacturing system (FMS). It models the complete operational behavior of a factory floor: CNC milling machines processing jobs, a heterogeneous fleet of automated guided vehicles (AGVs) and autonomous mobile robots (AMRs) transporting materials, a centralized tool crib issuing cutting tool sets, a pallet magazine dispensing workholding fixtures, and a scheduling engine that coordinates all of these resources under realistic constraints including WIP admission control, equipment failures, and traffic deadlocks.

The simulator is written in Rust for performance and correctness, and includes an Electron-based dashboard for real-time visualization and interactive control. The two communicate over a bidirectional JSON-lines protocol on standard I/O.

### 1.1 What This Simulator Models

This is not an animation or a visualization layer on top of static data. It is a functioning factory controller prototype that makes real-time scheduling decisions, resolves resource contention, detects and breaks deadlocks, and reacts to stochastic equipment failures — all driven by an event queue with no wall-clock dependency. An 8-hour production shift simulates in under one second.

### 1.2 Intended Use Cases

- **Factory automation engineering**: Evaluate scheduling algorithms, fleet sizing, and resource pool configurations before committing to physical hardware.
- **Education**: Study discrete-event simulation, finite state machines, deadlock detection, and material handling system design in a realistic setting.
- **What-if analysis**: Compare fault tolerance under different MTBF profiles, test the impact of adding or removing machines, explore how queue arrival rates affect throughput, or tune WIP limits and fleet composition.
- **Algorithm development**: Use as a testbed for new scheduling heuristics, AGV/AMR routing strategies, or predictive maintenance policies.

---

## 2. Factory Floor Layout

### 2.1 CNC Mills

The factory contains **25 CNC milling machines** arranged in a **5×5 grid** (5 rows of 5 mills each). Each mill is an independent workstation capable of performing machining operations when equipped with the correct tool set and pallet fixture.

Mills are identified by index 0–24. Mill positions map to the grid as:

| | Col 0 | Col 1 | Col 2 | Col 3 | Col 4 |
|---|---|---|---|---|---|
| **Row 0** | Mill 0 | Mill 1 | Mill 2 | Mill 3 | Mill 4 |
| **Row 1** | Mill 5 | Mill 6 | Mill 7 | Mill 8 | Mill 9 |
| **Row 2** | Mill 10 | Mill 11 | Mill 12 | Mill 13 | Mill 14 |
| **Row 3** | Mill 15 | Mill 16 | Mill 17 | Mill 18 | Mill 19 |
| **Row 4** | Mill 20 | Mill 21 | Mill 22 | Mill 23 | Mill 24 |

### 2.2 Lane Network

AGVs travel on a **20-segment bidirectional loop** that encircles the factory floor. Each mill connects to the loop via a dedicated **spur segment** (segments 20–44), giving a total of **45 lane segments**.

Key fixed stations on the loop:
- **Segment 0**: Tool Crib — where tool sets are issued and returned.
- **Segment 5**: Chip Station — waste station where chip bins are emptied during chip evacuation.
- **Segment 10**: Pallet Magazine — where pallet fixtures are dispensed and returned.
- **Segment 15**: Work Prep Station — robotic billet loading station where raw stock is clamped onto pallet fixtures before delivery to a mill.

Mill rows attach to the loop at specific segments:
- Row 0 (Mills 0–4): loop segment 2
- Row 1 (Mills 5–9): loop segment 6
- Row 2 (Mills 10–14): loop segment 10
- Row 3 (Mills 15–19): loop segment 14
- Row 4 (Mills 20–24): loop segment 18

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

Routing uses **BFS shortest-path** on the bidirectional graph. AGVs can traverse the loop in either direction, choosing the shorter arc to reach their destination. AMRs use a restricted BFS that excludes spur segments.

### 2.3 Segment Mutual Exclusion

Each lane segment holds **at most one vehicle** at any time. This is the fundamental concurrency constraint: when a vehicle wants to enter a segment already occupied by another vehicle, it blocks and waits. This segment-level mutual exclusion is what makes the deadlock detection and idle-vehicle yielding systems necessary.

---

## 3. Simulated Objects

### 3.1 CNC Mill

Each mill is modeled as a **finite state machine** with 9 states:

| State | Description |
|---|---|
| **Idle** | Available for work. No job assigned. |
| **WaitingPallet** | Job assigned, waiting for a pallet fixture to be delivered. |
| **WaitingTool** | Job assigned, waiting for a tool set to be delivered. |
| **Loading** | Workpiece is being loaded onto the pallet fixture in the mill. Duration: **45 seconds**. |
| **Machining** | Actively cutting the workpiece. Duration: **300–900 seconds** (random, per operation). Chips accumulate in the chip bin during this state. |
| **Unloading** | Finished part is being removed from the mill. Duration: **45 seconds**. |
| **ToolChange** | Swapping the current tool set for a different one. Duration: **120 seconds**. |
| **Faulted** | Machine has experienced a breakdown. No work is processed until repair completes. |
| **ChipFull** | Chip bin has reached capacity. Mill is paused until an AGV evacuates the chips. |

**Tracked metrics per mill:**
- `parts_completed` — cumulative count of finished parts
- `busy_time` — total seconds spent in the Machining state
- `fault_time` — total seconds spent in the Faulted state
- `loaded_tool` — which tool set (if any) is currently installed
- `loaded_pallet` — which pallet fixture (if any) is currently mounted
- `loaded_pallet_type` — the fixture type of the loaded pallet (0–3)
- `chip_level` — current chip bin fill level (0.0–100.0)
- `chip_capacity` — chip bin capacity (default: 100.0)

**Mill state transitions:**

```
Idle → Loading → Machining → Unloading → Idle
  \                                  \     ↑
   → ToolChange → Idle                → ChipFull → Idle (after evacuation)
                                          |
Any state → Faulted → Idle (on repair) ───┘
```

When a part finishes unloading, the mill checks its chip bin level. If the bin is at or above capacity, the mill transitions to **ChipFull** instead of **Idle**. The scheduler dispatches an AGV to evacuate the chips. After a 60-second evacuation time, the mill's chip bin is emptied and it returns to **Idle**.

### 3.2 Vehicle Fleet (AGVs and AMRs)

The factory operates a **heterogeneous fleet** of vehicles: **6 AGVs** and **2 AMRs** (configurable via `--num-amrs`). Both vehicle types share the same finite state machine with 6 states, but differ in speed, routing capability, and fault characteristics.

| State | Description |
|---|---|
| **Idle** | Parked at a segment, available for dispatch. |
| **Traveling** | Moving along a path, one segment at a time. |
| **Loading** | Picking up cargo at a station. |
| **Unloading** | Delivering cargo at a mill spur. Duration: **45 seconds**. |
| **Blocked** | Wants to enter a segment that is occupied by another vehicle. Waiting for it to clear. |
| **Faulted** | Vehicle breakdown. Cannot move or carry cargo until repaired. |

**Vehicle type differences:**

| Property | AGV | AMR |
|---|---|---|
| Travel time per segment | 8 seconds | 6 seconds |
| Can enter spur segments | Yes | No (main loop only) |
| MTBF | 43,200s (12 hours) | 57,600s (16 hours) |
| Repair duration | 900s (15 minutes) | 600s (10 minutes) |
| ID range | 0–5 | 6+ (starting after AGVs) |

AMRs are faster and more reliable, but because they cannot enter spur segments, they cannot deliver workpieces directly to mills. AMRs can participate in the first leg of a two-leg mission (delivering raw pallets to the work prep station on the main loop). Only AGVs handle the second leg (delivering prepared workpieces from work prep to mill spurs).

**Vehicle cargo types:**
- `Empty` — no cargo
- `Pallet(id)` — carrying a specific pallet fixture
- `ToolSet(id)` — carrying a specific tool set
- `Workpiece { job_id, op_index }` — carrying a prepared workpiece for delivery to a mill spur
- `ChipBin(mill_id)` — carrying a chip bin from a specific mill for evacuation
- `PrepPallet { job_id, op_index, mill_id }` — carrying a raw pallet to the work prep station for billet loading

**Initial positions:** AGVs start distributed around the loop at segments 0, 3, 6, 9, 12, and 15 (computed as `(id × 3) % 20`). AMRs are placed at offset positions to avoid collisions (computed as `((NUM_AGVS + i) × 3 + 1) % 20`).

**Idle-vehicle yielding:** When an active vehicle is blocked by an idle vehicle (AGV or AMR), the scheduler moves the idle vehicle to the nearest free loop segment via BFS. This prevents permanently parked vehicles from creating impassable bottlenecks on the loop.

**Tracked metrics per vehicle:**
- `distance_traveled` — total segments traversed
- `loads_delivered` — total cargo deliveries completed

### 3.3 Tool Crib

The tool crib is a centralized inventory of **cutting tool sets** located at loop segment 0. It manages finite stock: **8 tool set types** with **4 copies of each** (32 total tool sets).

Each tool set represents a logical group of cutting tools (e.g., a roughing end mill, drill, and chamfer tool packaged together). Different machining operations require different tool set types.

**Behavior:**
- **Checkout**: The scheduler reserves a tool set copy when dispatching a job. If no copies are available, the job waits in the queue.
- **Checkin**: When a mill swaps to a different tool set, the old one is returned to the crib.
- **Tool change**: If a mill already has the required tool set loaded, no change is needed and the 120-second changeover is skipped.

The tool crib is a shared resource that creates scheduling contention: if all 4 copies of a tool set type are checked out to mills, any new job requiring that type must wait.

### 3.4 Pallet Magazine

The pallet magazine is located at loop segment 10 and dispenses **workholding fixtures** (pallets). It stocks **4 pallet types** with **8 pallets of each type** (32 total pallets).

Pallet types represent different fixture geometries for different part shapes. Each operation specifies which pallet type it needs.

**Behavior:**
- **Take**: A pallet is issued to a mill when a job is dispatched. If the required type is depleted, the job waits.
- **Return**: After a part completes machining and unloading, the pallet is returned to the magazine with its correct type preserved.

Like the tool crib, the pallet magazine creates finite-resource contention that the scheduler must manage.

### 3.5 Work Prep Station

The work prep station is a **robotic billet loading station** located at **loop segment 15**. It models the real-world process of clamping raw stock onto pallet fixtures before the workpiece can be delivered to a mill for machining. It is a **single-server queue**: one robot processes items one at a time from a FIFO queue with a maximum depth of **4 items**.

**State machine (3 states):**

| State | Description |
|---|---|
| **Idle** | Robot is available. If the queue is non-empty, it immediately dequeues and begins processing. |
| **Processing** | Robot is clamping a billet onto a pallet fixture. Duration: **60–120 seconds** (uniform random). |
| **Faulted** | Robot has experienced a breakdown. Processing resumes after repair. |

**Processing flow:**

1. A vehicle delivers a `PrepPallet` to segment 15 and unloads.
2. The pallet is enqueued at the work prep station.
3. If the station is Idle and the queue is non-empty, it dequeues an item and begins Processing.
4. After 60–120 seconds, the prepared workpiece is pushed to the scheduler's delivery queue.
5. The scheduler dispatches a spur-capable AGV to carry the prepared workpiece to the assigned mill.

**Fault behavior:** When the station faults during Processing, the item being worked on is returned to the front of the queue (preserving its priority). A **generation counter** ensures that any in-flight `WorkPrepDone` event from before the fault is ignored — the counter increments each time processing starts, and the event carries the generation it was created with. On repair, the station returns to Idle and immediately starts the next queued item if available.

**Fault parameters:**

| Parameter | Default | Description |
|---|---|---|
| Work prep MTBF | 57,600s (16 hours) | Mean time between station failures |
| Work prep repair | 600s (10 minutes) | Fixed repair duration |

**Capacity management:** The scheduler checks `queue_depth + pending_deliveries < 4` before dispatching new jobs through the work prep station. This prevents flooding the station's queue and stalling the pipeline.

### 3.6 Job Queue

Jobs arrive stochastically following a **Poisson process** with a mean inter-arrival time of **120 seconds** (approximately 30 jobs per hour). Each job is a work order consisting of:

- **Job ID**: Unique identifier (sequential, starting at 1)
- **Priority**: One of four levels with the following probability distribution:
  - Critical (5%): highest priority, scheduled first
  - High (15%): elevated priority
  - Normal (60%): standard priority
  - Low (20%): lowest priority, scheduled last
- **Operations**: 1–3 sequential machining steps (random), each specifying:
  - A required tool set type (0–7, random)
  - A machining duration (180–900 seconds, random)
  - A required pallet type (0–3, random)
- **Arrival time**: When the job entered the queue

The queue is maintained in **priority order** (stable within the same priority level — earlier arrivals go first among equal-priority jobs). The scheduler processes the highest-priority feasible job first.

---

## 4. Scheduling Engine

The scheduler runs on a **5-second periodic heartbeat** (`SchedulerTick`). Each tick executes six phases in order:

### 4.1 Phase 1: Prepared Workpiece Delivery

The scheduler delivers prepared workpieces from the work prep station to their assigned mills. This phase runs first (highest priority) because prepared workpieces are already clamped and ready — any delay wastes the robot's setup time.

For each pending delivery:
1. Find an idle **spur-capable AGV** (AMRs cannot enter mill spurs).
2. Route the AGV to the assigned mill's spur segment with `Workpiece` cargo.
3. Increment the work prep delivery counter.

### 4.2 Phase 2: Job Dispatch

The scheduler scans the job queue front-to-back and attempts to assign each job to a mill. Before each dispatch, the scheduler enforces **WIP admission control**: if the number of mills actively processing work (not Idle and not Faulted) has reached the configurable `max_wip` limit (default: 20), dispatch is suspended and a back-pressure event is recorded. Dispatch resumes automatically when WIP drops below the limit. Back-pressure state transitions are logged to stderr (only on the transition into and out of back-pressure, not every tick).

Jobs now follow a **two-leg mission** through the work prep station:

For each job:

1. **Check WIP limit.** If WIP ≥ `max_wip`, stop dispatching (back-pressure).
2. **Check work prep capacity.** If the station's queue plus pending deliveries ≥ 4, stop dispatching (prevents flooding the bottleneck).
3. **Find an idle mill.** Prefer a mill that already has the correct tool set loaded (avoids a 120-second tool change).
4. **Check resource availability.** Verify the required tool set has copies in the crib and the required pallet type is available in the magazine.
5. **Find an idle vehicle.** Any vehicle (AGV or AMR) can handle the first leg since the work prep station is on the main loop. If no vehicle is free, dispatch pauses.
6. **Reserve resources.** Check out the tool set, take the pallet, start a tool change if needed. Put the mill into Loading state.
7. **Dispatch vehicle (Leg 1).** Route the vehicle to the work prep station (segment 15) with `PrepPallet` cargo containing the job ID, operation index, and assigned mill ID.

After Leg 1 completes (vehicle arrives at work prep, unloads the raw pallet), the work prep station processes the billet (60–120s). When finished, the prepared workpiece is queued for Leg 2 delivery (Phase 1 on the next tick).

Multiple jobs can be dispatched in a single tick if resources and vehicles are available.

### 4.3 Phase 3: Chip Evacuation Dispatch

The scheduler scans all mills in the **ChipFull** state and dispatches an available AGV to each one. Only AGVs are dispatched for chip evacuation because the mission requires entering the mill's spur segment (AMRs are restricted to the main loop).

For each ChipFull mill without a pending evacuation:
1. Find an idle AGV that can enter spurs.
2. Compute the shortest path to the mill's spur segment.
3. Dispatch the AGV with `ChipBin(mill_id)` cargo.
4. Track the mill as having a pending evacuation (prevents duplicate dispatches).

When the AGV arrives at the spur and unloads, a **60-second chip evacuation timer** begins. On completion, the mill's chip bin is emptied (chip level reset to 0) and the mill returns to **Idle**.

Chip evacuation competes directly with production dispatch for AGV availability: an AGV servicing a chip-full mill is unavailable for workpiece delivery. This creates the scheduling tension that makes chip management a meaningful system concern.

### 4.4 Phase 4: Blocked Vehicle Advancement

The scheduler first runs **idle-vehicle yielding**: for each blocked vehicle, if the segment it needs is held by an idle vehicle, the idle vehicle is moved to the nearest free loop segment (found via BFS). This prevents permanently parked AMRs or idle AGVs from creating impassable bottlenecks.

After yielding, the scheduler checks all blocked vehicles and attempts to advance them. If the segment a vehicle is waiting for has become free, the vehicle claims it, releases its previous segment, and continues traveling.

### 4.5 Phase 5: Look-Ahead Pre-Staging

The scheduler examines the next 8 jobs in the queue and checks whether their required tool sets are available. In a production system, this would trigger pre-staging of tools to shadow positions. In the current implementation, it validates scheduling feasibility.

### 4.6 Phase 6: Deadlock Detection and Resolution

The scheduler builds a **wait-for graph** from all currently blocked AGVs:
- For each blocked AGV, determine which segment it wants.
- Determine which AGV occupies that segment.
- Add a directed edge: `waiting_agv → blocking_agv`.

If the graph contains a **cycle**, a deadlock has occurred (circular wait). The resolution strategy is **victim retreat**: the last AGV in the cycle is selected as the victim, its path is cleared, its cargo is dropped, and it returns to Idle state at its current position. This breaks the cycle and allows the other AGVs to proceed.

---

## 5. Fault Injection

The simulator models **stochastic equipment failures** for mills, AGVs, AMRs, and the work prep station using an exponential distribution for time-between-failures.

### 5.1 Fault Parameters

| Parameter | Default | Description |
|---|---|---|
| Mill MTBF | 28,800s (8 hours) | Mean time between mill failures |
| Mill repair duration | 1,800s (30 minutes) | Fixed time to repair a faulted mill |
| AGV MTBF | 43,200s (12 hours) | Mean time between AGV failures |
| AGV repair duration | 900s (15 minutes) | Fixed time to repair a faulted AGV |
| AMR MTBF | 57,600s (16 hours) | Mean time between AMR failures |
| AMR repair duration | 600s (10 minutes) | Fixed time to repair a faulted AMR |
| Work prep MTBF | 57,600s (16 hours) | Mean time between work prep station failures |
| Work prep repair | 600s (10 minutes) | Fixed time to repair the work prep robot |

AMRs are more reliable than AGVs (higher MTBF) and faster to repair, reflecting their simpler mechanical design (no guide-wire infrastructure, fewer wear components). The work prep station has the same fault profile as AMRs.

### 5.2 Fault Lifecycle

1. **Seed**: At simulation start, an initial fault time is drawn from `Exp(MTBF)` for every piece of equipment (25 mills + 6 AGVs + 2 AMRs + 1 work prep station = 34 initial fault events by default).
2. **Occur**: When the fault event fires, the equipment transitions to the `Faulted` state. It stops processing work immediately. A repair event is scheduled at `now + repair_duration` (using the vehicle-type-specific repair time for AGVs and AMRs).
3. **Repair**: The equipment returns to `Idle`. A new fault event is scheduled at `now + Exp(MTBF)`, continuing the failure cycle for the rest of the simulation.

### 5.3 Disabling Faults

Faults can be disabled entirely with the `--no-faults` CLI flag for deterministic analysis of scheduling behavior without failure noise.

---

## 6. Chip Evacuation

### 6.1 Overview

CNC machining generates metal chips (swarf) that accumulate in each mill's chip bin during the Machining state. When a bin reaches capacity, the mill cannot accept new work until the chips are evacuated by a vehicle. This creates a **competing resource flow**: chip evacuation missions contend with production workpiece deliveries for the same pool of AGVs.

### 6.2 Chip Accumulation

Each mill has a chip bin with a capacity of **100 units** (configurable via `chip_capacity`). Chips accumulate at a rate of **0.05 units per second** of machining time. A typical machining operation (300–900 seconds) adds 15–45 units of chips. The bin fills after approximately 3–4 consecutive operations, depending on duration.

The chip level check occurs after the unloading phase completes. If `chip_level ≥ chip_capacity`, the mill transitions to **ChipFull** instead of **Idle**. This means the current part is successfully completed and counted before the mill pauses.

### 6.3 Evacuation Process

1. The scheduler detects a mill in ChipFull state during Phase 2 (Chip Evacuation Dispatch).
2. An idle AGV is dispatched to the mill's spur with `ChipBin(mill_id)` cargo.
3. The AGV travels to the spur (standard segment-by-segment movement, subject to lane contention).
4. On arrival, a **60-second evacuation timer** begins (`CHIP_EVAC_TIME`).
5. On completion (`ChipEvacDone`), the mill's chip level resets to 0.0 and the mill returns to **Idle**.

### 6.4 Chip Station

The **chip station** (waste disposal point) is located at **loop segment 5**, positioned between the tool crib (segment 0) and the pallet magazine (segment 10). While the current model completes evacuation at the mill spur (the AGV's presence at the spur represents the full service cycle), the chip station is part of the factory layout for future extensions.

### 6.5 Scheduling Priority

Chip evacuation has **lower priority than production dispatch** (Phase 1) but **higher priority than traffic management** (Phase 3). This ordering means:
- Production jobs are dispatched first, using available vehicles.
- Remaining idle vehicles are then assigned to chip evacuation.
- A chip-full mill blocks until an AGV becomes available — extended high utilization can delay evacuation.

Mills in ChipFull state are **excluded from the WIP count** (like Faulted mills), so they do not trigger back-pressure. This prevents chip-full mills from artificially inflating WIP and blocking dispatch of new work to healthy mills.

### 6.6 Dashboard Indicators

- **Mill grid**: Each mill displays a small chip level bar at the bottom. The bar fills from left to right as chips accumulate (gray when below capacity, amber when full). Mills in ChipFull state have an amber border.
- **Metrics panel**: The "Chip evacs" counter shows the total number of chip evacuations dispatched.

---

## 7. Simulation Engine

### 7.1 Architecture

The simulation is driven by a **min-heap priority queue** of `TimedEvent`s. The engine:

1. Pops the event with the smallest time value.
2. Advances the simulation clock to that time.
3. Passes the event to the World handler.
4. The handler returns zero or more new events.
5. New events are enqueued.
6. Repeat until the queue is empty or the duration limit is reached.

There is **no real-time clock**. Simulated time advances discretely from event to event. If no events occur between t=100 and t=500, the clock jumps directly — no computation is wasted on empty intervals.

### 7.2 Event Types

| Event | Trigger | Effect |
|---|---|---|
| `JobArrival` | Poisson process | Enqueues a new job; schedules the next arrival |
| `SchedulerTick` | Every 5s | Runs all 7 scheduling phases |
| `MillLoadDone` | 45s after loading begins | Mill transitions to Machining |
| `MillMachiningDone` | 300–900s after machining begins | Mill transitions to Unloading |
| `MillUnloadDone` | 45s after unloading begins | Pallet returned, mill becomes Idle |
| `ToolChangeDone` | 120s after tool change begins | Mill returns to Idle with new tool |
| `AgvArrived` | 8s (AGV) or 6s (AMR) per segment hop | Vehicle enters next segment (or blocks) |
| `AgvLoadDone` | After pickup | AGV begins traveling with cargo |
| `AgvUnloadDone` | 45s after arriving at mill spur | Cargo delivered, AGV becomes Idle |
| `ChipEvacDone` | 60s after AGV arrives at chip-full mill | Mill chip bin emptied, mill returns to Idle |
| `WorkPrepDone` | 60–120s after processing starts | Prepared workpiece queued for mill delivery |
| `ReconciliationTick` | Every 30s | Compares cached belief against ground truth, logs drifts |
| `FaultOccur` | Exp(MTBF) | Equipment faults; repair scheduled |
| `FaultRepair` | Fixed duration after fault | Equipment restored; next fault scheduled |

### 7.3 Reproducibility

The simulation uses a seeded pseudorandom number generator (`StdRng` with seed 42 by default). Given the same seed and parameters, a run produces identical results. Use `--seed N` to change the seed for different stochastic realizations.

---

## 8. Assumptions and Simplifications

Understanding the simulator's assumptions is important for interpreting its results correctly:

1. **Single-operation dispatch.** Only the first operation in a job's operation list is dispatched. Multi-operation job chaining (routing a workpiece through sequential mills) is defined in the data model but not yet implemented in the scheduler.

2. **Uniform machining time distribution.** Machining durations are drawn uniformly from 300–900 seconds. Real machining times depend on part geometry, material, and toolpath — the simulator uses a simplified distribution.

3. **Instantaneous tool changes at the crib.** Tool checkout and checkin at the crib are immediate. The 120-second delay models the tool change at the mill spindle, not transportation from the crib.

4. **Simplified vehicle routing.** Vehicles use BFS shortest-path without congestion awareness. They do not reroute dynamically when encountering traffic. If blocked, they wait, are yielded past by idle vehicles, or are resolved by the deadlock detector.

5. **No vehicle battery model.** AGVs and AMRs operate continuously without charging or energy constraints.

6. **Fixed repair durations.** Equipment repairs take a fixed time regardless of failure mode. Real repairs vary by diagnosis complexity and part availability.

7. **No operator model.** All operations (loading, unloading, tool changes) are fully automated. There are no human operators, shift breaks, or manual intervention stations.

8. **Homogeneous mills.** All 25 mills are identical in capability. Any mill can process any operation. Specialization (e.g., 5-axis vs. 3-axis) is not modeled.

9. **Single-load vehicles.** Each AGV/AMR carries one item at a time. Multi-load optimization is not implemented.

10. **No preventive maintenance.** Equipment runs until it fails. There is no scheduled maintenance, condition monitoring, or predictive replacement.

11. **Poisson job arrivals.** Jobs arrive as a memoryless Poisson process with constant rate. Real production schedules are driven by customer orders, forecasts, and MRP/ERP systems.

---

## 9. Running the Simulation

### 9.1 Prerequisites

- **Rust toolchain** (stable, 2021 edition) — install via [rustup.rs](https://rustup.rs/)
- **Node.js 18+** and npm — required for the Electron dashboard

### 9.2 Building

```bash
# Build the Rust simulation binary
cargo build --release

# Install dashboard dependencies
cd dashboard
npm install
```

### 9.3 Batch Mode (CLI)

Run the simulation headless and view results in the terminal:

```bash
# Default 8-hour shift with faults
cargo run --release -- --duration 28800

# Fault-free run for deterministic analysis
cargo run --release -- --duration 28800 --no-faults

# JSON summary to stdout (parseable by scripts)
cargo run --release -- --duration 28800 --json

# Full snapshot history + summary as JSON
cargo run --release -- --duration 28800 --snapshots

# Custom parameters
cargo run --release -- --duration 14400 --mill-mtbf 14400 --agv-mtbf 21600 --seed 99

# WIP limit and fleet composition
cargo run --release -- --duration 28800 --max-wip 15 --num-amrs 4
```

### 9.4 CLI Flags Reference

| Flag | Type | Default | Description |
|---|---|---|---|
| `--duration` | seconds | 28800 (8h) | Simulation duration |
| `--no-faults` | boolean | false | Disable all stochastic fault injection |
| `--json` | boolean | false | Output final summary as JSON to stdout |
| `--snapshots` | boolean | false | Output full snapshot array + summary as JSON |
| `--ipc` | boolean | false | Dashboard mode: JSON-lines protocol on stdio |
| `--snapshot-interval` | seconds | 1.0 | How often to emit state snapshots (IPC mode) |
| `--mill-mtbf` | seconds | 28800 | Mean time between mill failures |
| `--agv-mtbf` | seconds | 43200 | Mean time between AGV failures |
| `--seed` | integer | 42 | RNG seed for reproducible runs |
| `--max-wip` | integer | 20 | Maximum work-in-progress before back-pressure holds dispatch |
| `--num-amrs` | integer | 2 | Number of AMRs in the fleet (in addition to the 6 AGVs) |

### 9.5 Batch Output

In default mode, the simulation prints diagnostic events (faults, repairs, deadlocks) to **stderr** during the run, followed by a summary:

```
factory-sim: running 28800s simulation (25 mills, 6 AGVs, 2 AMRs)
[185.0s] WORK-PREP: job 1 ready for mill 0
[195.0s] DEADLOCK resolved: retreated AGV 1 at seg 16
[2023.7s] FAULT: Mill 5 down
[3509.8s] FAULT: AGV 2 down
[3823.7s] REPAIR: Mill 5 back online
[4250.0s] CHIP-EVAC: dispatched AGV 1 to mill 0
[4409.8s] REPAIR: AGV 2 back online
...
═══ Simulation Summary ═══
Duration:           28800s (8.0 hours)
Events processed:   9570
Jobs dispatched:    252
Parts completed:    55
Avg utilization:    4.7%
Avg queue depth:    0.2
Deadlocks detected: 1197
Back-pressure:      0
Chip evacuations:   1022
Work prep jobs:     80
Reconciliation:     961 passes, 1663 drifts (max 15 per pass)
Equipment faults:   24
Throughput:         6.9 parts/hr
```

### 9.6 JSON Output

With `--json`, the summary is emitted as structured JSON to stdout:

```json
{
  "sim_duration": 28800.0,
  "events_processed": 9570,
  "jobs_completed": 55,
  "jobs_dispatched": 252,
  "deadlocks_detected": 1197,
  "total_faults": 24,
  "back_pressure_events": 0,
  "chip_evacuations": 1022,
  "work_prep_jobs": 80,
  "reconciliation_passes": 961,
  "reconciliation_drifts": 1663,
  "max_drifts_in_pass": 15,
  "mill_utilization": [0.130, 0.122, 0.104, ...],
  "avg_utilization": 0.047,
  "total_throughput": 55,
  "avg_queue_depth": 0.18
}
```

---

## 10. Electron Dashboard

### 10.1 Starting the Dashboard

```bash
cd dashboard
npm run dev
```

The dashboard launches an Electron window and automatically spawns the Rust simulation binary in IPC mode. It expects the release binary to be built first (`cargo build --release`).

### 10.2 Control Bar

The top bar provides transport controls:

- **Run** — Spawns the simulation process and waits for the `ready` message.
- **Start** — Sends the `start` command to begin the simulation.
- **Pause / Resume** — Suspends and resumes event processing.
- **Stop** — Terminates the simulation and displays the final summary.
- **Speed selector** — Cycles through speed multipliers: 1×, 4×, 16×, 64×, 256×. Higher speeds process more events per batch.
- **Fault Mill / Fault AGV** — Manually injects a fault on a random non-faulted piece of equipment for testing failure response.
- **Clock display** — Shows the current simulated time as HH:MM:SS.
- **Config summary** — Displays duration, seed, and fault status.

### 10.3 Factory Floor (SVG)

The main visualization is an SVG rendering of the factory floor showing:

- **Mill grid** — 5×5 rectangles color-coded by state:
  - Idle: dim
  - Machining/Loading/Unloading: green (active)
  - ToolChange: yellow
  - Faulted: red
  - ChipFull: amber (with amber border)
  - WaitingPallet/WaitingTool: orange
  - Each mill displays a chip level bar at the bottom (gray fill, amber when full)
- **Lane network** — The 20-segment loop and 25 spur segments drawn as lines
- **Vehicle positions** — AGVs shown as cyan circles labeled by ID number; AMRs shown as purple diamonds labeled "M". Each vehicle displays a projected path polyline showing its planned route. Faulted vehicles pulse red.

### 10.4 Metrics Panel

Displays key performance indicators updated in real time:

- Total throughput (parts completed)
- Throughput rate (parts/hour)
- Average mill utilization (percentage)
- Events processed
- Jobs dispatched
- Queue depth
- Deadlocks detected
- Total faults
- Active mills (currently machining/loading/unloading)
- WIP (current work-in-progress count / max WIP limit)
- Back-pressure events (how many times dispatch was held due to WIP limit)
- Chip evacuations (total chip bin evacuations dispatched)
- Work prep jobs (billets processed by the work prep station, with current queue depth)
- Reconciliation (total passes and total drifts detected)

Each utilization metric includes a visual bar indicator.

### 10.5 Resource Panel

Shows the current inventory state of both shared resource pools:

- **Tool Crib** — Available copies per tool set type (0–7), with badges showing count
- **Pallet Magazine** — Available pallets per fixture type (0–3), with badges showing count

### 10.6 Job Queue Panel

Displays the next 8 jobs in the priority queue with:

- Job ID
- Priority level (color-coded: critical=red, high=orange, normal=default, low=dim)
- Number of operations
- Wait time (how long the job has been in the queue)

### 10.7 Trend Charts

A Canvas 2D time-series chart with a dropdown to select between four metrics:

- **Throughput** — cumulative parts completed over time
- **Utilization** — average mill utilization percentage over time
- **Queue depth** — number of jobs waiting over time
- **Active mills** — count of mills currently processing work over time

The chart maintains a rolling history of up to 1,800 data points (30 minutes at 1-second snapshot intervals).

### 10.8 Event Log

The bottom panel shows a scrolling log of simulation events with:

- **Simulated time** — the in-simulation timestamp of each event
- **Event kind** — color-coded by type:
  - Faults (red)
  - Repairs (green)
  - Deadlocks (yellow/orange)
  - Dispatches (blue/default)
  - Completions (green)
  - Queue alerts (orange)
- **Event text** — human-readable description of what happened

The log auto-scrolls to show the most recent events and retains up to 500 entries.

---

## 11. IPC Protocol Reference

When running in `--ipc` mode, the simulator and dashboard communicate over stdio using **JSON-lines** (one JSON object per newline character).

### 11.1 Outgoing Messages (Sim → Dashboard, stdout)

#### `ready`
Sent once at startup with configuration and factory layout.
```json
{
  "type": "ready",
  "version": "0.1.0",
  "config": {
    "num_mills": 25,
    "num_agvs": 6,
    "num_amrs": 2,
    "duration": 28800.0,
    "snapshot_interval": 1.0,
    "faults_enabled": true,
    "mill_mtbf": 28800.0,
    "agv_mtbf": 43200.0,
    "seed": 42,
    "tool_types": 8,
    "pallet_types": 4,
    "loop_segments": 20,
    "total_segments": 45,
    "max_wip": 20
  },
  "layout": {
    "mills": [{"id": 0, "row": 0, "col": 0, "loop_seg": 2, "spur_seg": 20}, ...],
    "stations": {"tool_crib": 0, "pallet_magazine": 10, "chip_station": 5, "work_prep": 15}
  }
}
```

#### `snapshot`
Periodic state snapshot (default: every 1 second of simulated time).
```json
{
  "type": "snapshot",
  "time": 1234.5,
  "event_count": 5678,
  "mills": [{"id": 0, "state": "Machining", "job_id": 42, "op_index": 0, "loaded_tool": 3, "loaded_pallet": 7, "parts_completed": 12, "busy_time": 890.5, "fault_time": 0.0, "chip_level": 45.2, "chip_capacity": 100.0}, ...],
  "agvs": [{"id": 0, "vehicle_type": "Agv", "state": "Traveling", "segment": 5, "cargo": {"Workpiece": {"job_id": 43, "op_index": 0}}, "path": [6, 7, 8, 29], "path_cursor": 1, "delivered": 8}, ...],
  "lane_occupancy": [0, -1, -1, 2, ...],
  "tool_crib": {"inventory": {"0": 3, "1": 4, ...}, "total_issues": 156},
  "pallet_magazine": {"available": {"0": 6, "1": 7, ...}, "total_issued": 132},
  "job_queue": {"depth": 5, "next_8": [{"id": 44, "priority": "Normal", "ops": 2, "wait_time": 45.3}, ...]},
  "metrics": {"throughput": 28, "throughput_rate": 6.7, "avg_utilization": 0.047, "avg_queue_depth": 0.2, "deadlocks": 598, "faults": 12, "wip": 3, "max_wip": 20, "back_pressure_events": 0, "chip_evacuations": 510, "work_prep_jobs": 40, "work_prep_queue": 1, "work_prep_state": "Processing", "reconciliation_passes": 480, "reconciliation_drifts": 830, "max_drifts_in_pass": 15}
}
```

#### `event`
Discrete events of interest (faults, repairs, completions).
```json
{"type": "event", "time": 4231.5, "kind": "fault", "detail": {"target": {"Mill": 17}, "expected_repair": 6031.5}}
{"type": "event", "time": 6031.5, "kind": "repair", "detail": {"target": {"Mill": 17}}}
{"type": "event", "time": 1500.0, "kind": "completion", "detail": {"mill_id": 3, "job_id": 22}}
{"type": "event", "time": 2870.0, "kind": "chip_evac", "detail": {"mill_id": 0}}
{"type": "event", "time": 30.0, "kind": "drift", "detail": {"category": "MillState", "description": "Mill 0: believed Idle, actual Loading", "pass": 2}}
```

#### `summary`
Final summary sent when the simulation ends.

### 11.2 Incoming Commands (Dashboard → Sim, stdin)

| Command | Payload | Effect |
|---|---|---|
| `start` | `{"type":"start"}` | Begin event processing |
| `pause` | `{"type":"pause"}` | Suspend event processing |
| `resume` | `{"type":"resume"}` | Resume event processing |
| `stop` | `{"type":"stop"}` | Emit summary and terminate |
| `speed` | `{"type":"speed","multiplier":16}` | Set events-per-batch multiplier |
| `inject_fault` | `{"type":"inject_fault","target":{"Mill":5}}` | Force a fault on a specific target |
| `set_param` | `{"type":"set_param","param":"mill_mtbf","value":14400}` | Change a parameter at runtime (supports `mill_mtbf`, `agv_mtbf`, `max_wip`) |
| `step` | `{"type":"step","count":100}` | Advance N events then pause |

---

## 12. Metrics and Interpretation

### 12.1 Mill Utilization

Calculated as `busy_time / sim_duration` per mill. "Busy time" is the total time spent in the Machining state only. Loading, unloading, and tool changes are overhead and do not count as utilization. Average utilization is the mean across all 25 mills.

Typical values for an 8-hour run with default parameters: **3–5% utilization**. This reflects the heavy overhead of material handling in the simulated system: the two-leg mission through the work prep station, frequent lane deadlocks (which drop cargo and waste work), and the single-server work prep bottleneck all constrain how much time mills actually spend cutting metal. Only the first ~14 mills (those closest to the loop junction segments) receive steady work; the remaining mills rarely see jobs. This low utilization is characteristic of a transport-constrained system where scheduling and traffic management — not spindle time — are the bottleneck.

### 12.2 Throughput

Total parts completed across all mills, and the rate in parts per hour. With 25 mills and default job arrival rates, expect approximately **5–7 parts/hour** sustained throughput. The gap between jobs dispatched (~252) and parts completed (~55) reflects cargo dropped during deadlock victim retreats and missions aborted by equipment faults.

### 12.3 Queue Depth

Average number of jobs waiting in the queue. A growing queue indicates the system is overloaded (jobs arriving faster than they can be processed). A consistently empty queue indicates spare capacity.

### 12.4 Deadlock Count

Number of times the wait-for graph detected a cycle among blocked vehicles. With the default fleet of 6 AGVs and 2 AMRs on a 20-segment loop, expect **1,000–1,500 deadlocks** in an 8-hour shift — the lane network is heavily contended. Each deadlock causes one vehicle to drop its cargo and retreat, wasting the dispatched mission. This high deadlock rate is the primary throughput limiter in the default configuration and a realistic reflection of the traffic management challenge in shared-lane FMS installations.

### 12.5 Fault Count

Total equipment failure events (mills + AGVs + AMRs + work prep station). With default MTBF values, expect roughly **20–30 faults** in an 8-hour shift across all 34 pieces of equipment.

### 12.6 Back-Pressure Events

Number of scheduler ticks where dispatch was held because WIP had reached the `max_wip` limit. With the default `max_wip` of 20 and the high deadlock rate limiting effective throughput, the system typically shows **zero back-pressure events** — WIP stays well below the ceiling because the transport bottleneck limits how many mills can be kept busy simultaneously. Non-zero back-pressure would indicate either a very low `max_wip` setting or a configuration with fewer deadlocks (e.g., fewer vehicles, wider lanes).

### 12.7 Chip Evacuations

Total number of chip evacuation missions dispatched. Each evacuation ties up an AGV for the transit time to the mill spur plus 60 seconds of evacuation, competing directly with production dispatch. With default parameters expect **800–1,200 chip evacuations** in an 8-hour shift — the scheduler aggressively dispatches evacuations for any chip-full mill, and repeated deadlock-related cargo drops cause mills to cycle through chip-full states multiple times. Zero evacuations would mean chip bins never reached capacity (short simulation or very low throughput).

### 12.8 Reconciliation

The reconciliation subsystem simulates a real factory controller's periodic state check. Every 30 seconds of simulation time, the reconciler compares its cached belief of the factory state against the actual ground truth. Discrepancies — called "drifts" — indicate that events changed the factory between reconciliation ticks without the cache being updated. This is the same pattern used in distributed factory control systems to detect sensor lag, missed messages, and partial failures.

Drifts are categorized into five types:
- **MillState** — a mill's state changed (e.g., from Idle to Machining) since the last snapshot.
- **AgvPosition** — a vehicle moved to a different lane segment.
- **ToolInventory** — the tool crib's available count for a tool set changed.
- **PalletInventory** — the pallet magazine's available count for a fixture type changed.
- **WorkPrepState** — the work preparation station's state changed.

The summary reports three reconciliation metrics: total passes (one per 30s tick, ~961 in an 8-hour shift), total drifts across all passes (~1,600–1,700 with default parameters), and the maximum number of drifts found in any single pass (~15). Averaging 1–2 drifts per pass is normal in an active factory — it confirms events are occurring between checks. The most common drift types are MillState (mills transitioning between Idle/Loading/Machining), PalletInventory (pallets being checked out and returned), and ToolInventory (tool sets cycling through the crib). Anomalously high max-drifts-per-pass may indicate a burst of simultaneous state changes (e.g., a cascade of faults or a wave of job dispatches).

### 12.9 WIP (Work in Progress)

The current count of mills actively processing work (any state other than Idle, Faulted, or ChipFull). Displayed in the dashboard as `current/max`. When WIP equals `max_wip`, the scheduler enters back-pressure mode and holds further dispatch until a mill finishes and returns to Idle. ChipFull mills are excluded from WIP to prevent chip-full conditions from artificially triggering back-pressure.

---

## 13. Glossary

| Term | Definition |
|---|---|
| **AGV** | Automated Guided Vehicle — a robotic cart that transports materials along the lane network. Can enter spur segments to deliver to mills. |
| **AMR** | Autonomous Mobile Robot — a faster, more reliable vehicle restricted to the main loop. Cannot enter spur segments. |
| **Back-pressure** | The scheduler's response when WIP reaches the configured limit: new job dispatch is held until a mill finishes. |
| **BFS** | Breadth-First Search — the routing algorithm used to find shortest paths on the lane network. |
| **Chip bin** | A container at each mill that collects metal chips (swarf) generated during machining. Capacity: 100 units. Must be evacuated by an AGV when full. |
| **Chip evacuation** | The process of dispatching an AGV to a chip-full mill to empty its chip bin, competing with production dispatch for vehicle availability. |
| **Chip station** | The waste disposal point at loop segment 5 where chip bins are emptied. |
| **DES** | Discrete-Event Simulation — a simulation paradigm where state changes occur at discrete points in time driven by an event queue. |
| **FMS** | Flexible Manufacturing System — a production system with CNC machines, automated material handling, and computer-controlled scheduling. |
| **FSM** | Finite State Machine — a model with a fixed set of states and defined transitions between them. |
| **MTBF** | Mean Time Between Failures — the expected time a piece of equipment runs before its next failure, drawn from an exponential distribution. |
| **Drift** | A discrepancy between the reconciler's cached belief of factory state and the actual ground truth. Categorized by type: MillState, AgvPosition, ToolInventory, PalletInventory, WorkPrepState. |
| **Pallet** | A workholding fixture that secures a workpiece in the mill. Typed by the part geometry it accommodates. |
| **Reconciliation** | A periodic (every 30s) state check that compares the scheduler's cached belief against actual equipment state, logging any discrepancies as drift events. |
| **Segment** | One atomic unit of the lane network. Holds at most one AGV (mutual exclusion). |
| **Spur** | A dedicated lane segment branching from the main loop to a single mill. |
| **Tool set** | A logical group of cutting tools (e.g., end mill, drill, chamfer) identified by type ID. |
| **Victim retreat** | Deadlock resolution strategy: one vehicle in the cycle drops its cargo and returns to Idle, breaking the circular wait. |
| **Wait-for graph** | A directed graph where an edge from vehicle A to vehicle B means A is blocked waiting for a segment that B occupies. A cycle in this graph indicates deadlock. |
| **WIP** | Work in Progress — the count of mills actively processing jobs (not Idle, not Faulted). Controlled by the `max_wip` admission limit. |
| **Work prep station** | A robotic billet loading station at loop segment 15 that clamps raw stock onto pallet fixtures before mill delivery. Single-server queue with max depth 4. |
| **Yield** | When an idle vehicle blocks an active vehicle's path, the scheduler moves the idle vehicle to the nearest free loop segment. |
