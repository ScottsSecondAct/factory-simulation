# Factory Orchestration Simulator

## What this is
A discrete-event factory orchestration simulator modeling a flexible manufacturing
system (FMS): 25 CNC mills, 6 AGVs on a shared-lane network, tool crib, pallet
magazine, priority job queue. Written in Rust. The artifact that matters is a
working prototype of a real factory controller, not a toy.

## Architecture

**Simulation-first.** No real clock. The engine is a min-heap of `TimedEvent`s.
Pop → advance clock → dispatch → handlers return new events → enqueue. An 8-hour
shift completes in under a second.

**Event-driven, not tick-based.** Nothing happens between events. If you're
tempted to add a polling loop or a fixed-timestep update, stop — that's the
wrong pattern for this codebase.

**State machines, not flags.** Mills and AGVs are FSMs with explicit state enums.
Transitions happen in named methods (`begin_machining`, `finish_unloading`), not
by setting fields ad hoc. If a new state is needed, add it to the enum and handle
it in every match arm.

**Handlers return events, not mutate the queue.** `World::handle(&mut self, &TimedEvent)
-> Vec<TimedEvent>`. The caller enqueues them. This avoids borrow-checker fights and
keeps the event flow traceable.

## Module boundaries

| Module | Owns | Does NOT own |
|---|---|---|
| `engine.rs` | Event queue, clock, `TimedEvent` | Any domain logic |
| `factory.rs` | Mill FSM, ToolCrib, PalletMagazine | Scheduling decisions |
| `agv.rs` | AGV state, LaneNetwork, WaitForGraph | Route selection policy |
| `scheduler.rs` | Job dispatch, look-ahead, deadlock detection | Equipment state transitions |
| `fault.rs` | Stochastic failure model | Repair logic beyond duration |
| `metrics.rs` | Snapshots, summaries, JSON serialization | Simulation control flow |
| `types.rs` | IDs, enums, constants, layout geometry | Behavior |
| `main.rs` | World aggregate, event dispatch, CLI | Nothing else should go here |

If a change touches two modules, check whether you're violating a boundary.

## Domain vocabulary — use these terms consistently

- **Segment**: one atomic unit of lane. One AGV per segment. Mutual exclusion is
  at the segment level.
- **Spur**: a dedicated segment branching from the main loop to a single mill.
- **Tool set**: a logical group of cutting tools (not a single tool). Identified
  by `ToolSetId`. The crib stocks multiple copies of each set.
- **Pallet**: a fixture that holds a workpiece. Typed (different part geometries
  need different fixtures). Finite pool in the magazine.
- **Mission**: an AGV dispatch — pickup location, delivery location, cargo.
- **Wait-for graph**: directed graph where edge A→B means "AGV A is blocked
  waiting for a segment held by AGV B." A cycle = deadlock.
- **Victim retreat**: deadlock resolution by backing one AGV out of the cycle.
- **Look-ahead staging**: examining the next N jobs in the queue to pre-position
  tools/pallets before they're needed.

## Conventions

- **Rust style**: `cargo fmt`, `cargo clippy` clean. No `unwrap()` in library
  code except on invariants that are structurally guaranteed (document why).
- **Naming**: `SimTime` not `f64` for time values. Type aliases from `types.rs`
  everywhere — bare `usize` for a mill ID is a bug.
- **Constants**: all timing constants and layout dimensions in `types.rs`. No
  magic numbers in logic code.
- **Serialization**: anything that goes to the dashboard or JSON output gets
  `#[derive(Serialize)]`. Internal-only structs don't need it.
- **eprintln for ops, println for data**: fault/repair/deadlock messages go to
  stderr. Structured output (--json) goes to stdout. Never mix them.

## The dashboard

`dashboard/index.html` — standalone HTML/JS that runs its own simulation engine
mirroring the Rust architecture. It's a demo artifact, not a thin client. When
the Rust sim's behavior changes, the JS sim should be updated to match (same
state machines, same scheduling logic, same lane topology).

## What matters for this project

1. **Correctness of the concurrency model.** The shared-lane mutual exclusion
   and deadlock detection must be right. A bug here makes the whole sim
   meaningless.
2. **Clean state machines.** Every mill and AGV state must be reachable and
   exitable. No orphan states, no implicit transitions.
3. **Realistic parameterization.** Timing constants, MTBF rates, queue arrival
   rates should be in the ballpark of real FMS installations, not arbitrary.
4. **Readable architecture.** A hiring manager reading `main.rs` should
   understand the entire system in 5 minutes. The event dispatch match arm
   is the table of contents.

## What doesn't matter (yet)

- Performance optimization (the sim is already fast enough)
- Multi-threaded execution
- Network protocol for dashboard ↔ sim communication
- Persistent storage of simulation runs
- UI polish beyond functional clarity