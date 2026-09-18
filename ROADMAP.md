# Roadmap

## v0.1 — MVP

- Core simulation engine (event-driven, min-heap scheduler)
- 25 CNC mills as finite state machines (Idle, WaitingPallet, WaitingTool, Loading, Machining, Unloading, ToolChange, Faulted, ChipFull)
- Heterogeneous vehicle fleet: 6 AGVs (spur-capable) + 2 AMRs (main-loop only, faster, more reliable) on a shared-lane network (20-segment bidirectional loop + 25 mill spurs)
- Tool crib with 8 tool-set types × 4 copies each — finite inventory the scheduler must manage
- Pallet magazine with 4 fixture types × 8 pallets each
- Robotic work prep station — single-server billet loading queue at segment 15, two-leg mission dispatch (vehicle → work prep → AGV → mill spur)
- Priority job queue with Poisson arrivals and multi-operation work orders
- WIP admission control — configurable back-pressure limit (`--max-wip`) holds dispatch when active mills reach threshold
- Chip evacuation system — chip bins accumulate during machining, ChipFull mills block until an AGV evacuates the bin (competing resource flow with production dispatch)
- Look-ahead staging (examines next N jobs to pre-position tools/pallets)
- Deadlock detection via wait-for graph with victim retreat resolution
- Idle-vehicle yielding — blocked vehicles trigger relocation of idle vehicles occupying needed segments
- Stochastic fault injection (exponential MTBF for mills, AGVs, AMRs, and work prep station, configurable)
- Periodic state reconciliation — compares cached scheduler belief against ground truth every 30s, logs drift events
- Electron dashboard with real-time SVG factory floor visualization
- IPC protocol (JSON-lines over stdio) between sim and dashboard
- Metrics: utilization, throughput, queue depth, fault counts, chip evacuations, WIP, back-pressure, reconciliation drifts
- Event log with simulated-time timestamped display of simulation events

## v0.2 — Current (Scheduling & Quality)

### Advanced Scheduling
- Pluggable scheduling strategies (FIFO, SPT, EDD, Weighted Priority) with runtime switching via CLI and dashboard ✅
- A/B comparison mode — run two strategies side-by-side with identical seeds, lockstep advancement, and live metric deltas ✅
- Multi-operation job chains — jobs with sequential ops across different mills, enforcing precedence constraints
- Setup-aware sequencing — minimize changeovers by grouping jobs that share tool sets and pallet types (SMED-inspired)
- Due-date-driven scheduling with tardiness penalty functions and configurable urgency escalation
- Kanban/pull-system mode — demand-driven production with WIP limits per work center

### Quality & Process Control
- Statistical process control (SPC) — simulate Xbar-R charts on machining dimensions with configurable Cpk targets, out-of-control detection (Western Electric rules), and auto-hold on SPC violation
- Tool life management — track tool wear as a function of cut time and material hardness, predict remaining useful life, trigger pre-emptive tool changes before tolerance drift
- In-process gauging simulation — model probe cycles between ops, reject/rework routing for out-of-spec parts

### Material Tracking
- Lot and serial number traceability — assign unique IDs to each workpiece, track full genealogy from raw stock through every operation to finished part
- Barcode/RFID scan event simulation — model scan points at load/unload stations for WIP visibility

### Dashboard Enhancements
- Mill detail overlay on click — operation history, utilization sparkline, current tool wear state
- Session recording and replay — save simulation runs as `.jsonl`, replay in dashboard without running the sim
- Performance metrics export (CSV/JSON reports)

## v0.3 — Connectivity & Maintenance

### Industrial Protocol Integration
- OPC UA server interface — expose mill states, AGV positions, and tool crib inventory as OPC UA nodes; accept write commands for remote supervisory control
- MQTT broker bridge — publish events to configurable topics for integration with SCADA/HMI systems
- REST/WebSocket API — run sim on a remote host, connect multiple dashboard instances

### Maintenance & Reliability
- Preventive maintenance scheduling — calendar-based and usage-based PM triggers with maintenance windows that pre-empt production
- Condition-based maintenance — model vibration signatures and thermal profiles per mill, trigger maintenance on threshold exceedance
- CMMS work order integration — generate work orders on fault, track mean-time-to-repair by failure mode, model spare parts inventory
- Reliability-centered maintenance (RCM) analysis — failure mode and effects analysis (FMEA) data collection from sim runs, criticality ranking

### Alarm Management
- ISA-18.2 alarm model — alarm states (active/acknowledged/cleared/shelved), priority classification (emergency/high/medium/low/diagnostic), alarm flood detection and suppression
- Alarm rationalization metrics — alarm rate per operator per hour, standing alarm count, chattering alarm detection

### Energy & Sustainability
- Power consumption modeling — per-machine state power draw (spindle load, idle, standby), aggregate plant demand profile
- Energy-aware scheduling — optional objective to shift non-urgent jobs to off-peak periods, minimize peak demand charges

## v0.4 — Advanced Orchestration

### Multi-Cell & Scalability
- Multi-cell factory layout — define multiple manufacturing cells with inter-cell AGV transfer, cell-level scheduling with plant-level coordination
- Dynamic capacity scaling — model bringing machines online/offline for demand fluctuation, shift-handoff warm start from previous shift state
- Federated simulation — multiple factory instances exchanging parts through a supply chain layer

### Recipe & Program Management
- Part program versioning — CNC program library with revision control, program-to-operation binding, engineering change order (ECO) propagation
- Recipe parameter management — configurable machining parameters (feeds, speeds, depths) per material/tool combination, parameter download to mill at job start

### Digital Twin
- Real PLC connectivity — bridge to Modbus TCP, EtherNet/IP, or PROFINET devices to mirror real equipment state into the sim
- Model synchronization — bidirectional state sync: sim predicts forward, real sensors correct drift, discrepancy alarms on divergence beyond threshold

### AGV Fleet Intelligence
- Traffic-aware pathfinding — real-time congestion cost in routing decisions, dynamic rerouting on lane blockage
- Fleet right-sizing analysis — run parameter sweeps to find minimum fleet size for target throughput
- Battery/charging model — simulate AGV battery depletion and charging station scheduling
- Multi-load AGV support — model AGVs carrying multiple pallets with optimized pickup/dropoff sequences

## v0.5 — Analytics & Compliance

### OEE & Performance Analytics
- Overall Equipment Effectiveness (OEE) dashboard — availability × performance × quality decomposition per mill and plant-wide
- Loss categorization — classify downtime by the six big losses (breakdowns, setup, small stops, reduced speed, startup rejects, production rejects)
- Bottleneck detection — automatic identification of constraining resources using utilization and wait-time analysis (theory of constraints)
- Comparative run analysis — side-by-side dashboard view of runs with different configurations, parameter sensitivity heatmaps

### Traceability & Compliance
- AS9100/ISO 9001 traceability model — full process history per serial number, non-conformance report (NCR) generation on SPC violation, certificate of conformance (CoC) data assembly
- Audit trail — immutable log of all state transitions, parameter changes, and operator interventions with timestamps and attribution
- Electronic batch records — aggregate per-lot data (material certs, machine params, inspection results, operator sign-offs) into a structured record

### Operator Interface
- ANDON board simulation — model operator call buttons (material request, quality hold, maintenance needed), response time tracking, escalation chains
- Operator workload balancing — model manual stations alongside automated ones, track operator utilization and ergonomic exposure limits
- Shift handoff reports — auto-generated summary of production status, open issues, and pending jobs at shift boundary

## Ideas / Research

- Machine learning for predictive maintenance — train models on sim-generated fault data, deploy back into sim scheduling
- Reinforcement learning scheduler — train an RL agent to optimize dispatching policy against throughput/tardiness/WIP objectives
- 3D visualization using Three.js or Bevy (Rust-native) for immersive factory walkthroughs
- Augmented reality overlay — project sim state onto physical factory floor via AR headset
- Generative scenario planning — auto-generate "what if" scenarios (demand spike, machine loss, supply delay) and rank mitigation strategies
- Composite manufacturing workflow — model layup sequencing, autoclave cure cycle scheduling, and NDT inspection routing for composite part production
- Coordinate measuring machine (CMM) scheduling — integrate inspection station queuing with production flow, model first-article inspection gates
- MES/ERP integration layer — map sim events to ISA-95 (B2MMS) activity models for upstream enterprise system connectivity
