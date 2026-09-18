export type MillId = number;
export type AgvId = number;
export type SegmentId = number;
export type JobId = number;
export type ToolSetId = number;
export type PalletId = number;
export type SimTime = number;

export type MillState =
  | "Idle"
  | "WaitingPallet"
  | "WaitingTool"
  | "Loading"
  | "Machining"
  | "Unloading"
  | "ToolChange"
  | "Faulted";

export type AgvState =
  | "Idle"
  | "Traveling"
  | "Loading"
  | "Unloading"
  | "Blocked"
  | "Faulted";

export type Priority = "Critical" | "High" | "Normal" | "Low";

export type VehicleType = "Agv" | "Amr";

export interface ReadyConfig {
  num_mills: number;
  num_agvs: number;
  num_amrs: number;
  duration: SimTime;
  snapshot_interval: SimTime;
  faults_enabled: boolean;
  mill_mtbf: SimTime;
  agv_mtbf: SimTime;
  seed: number;
  tool_types: number;
  pallet_types: number;
  loop_segments: number;
  total_segments: number;
  max_wip: number;
}

export interface MillLayout {
  id: MillId;
  row: number;
  col: number;
  loop_seg: SegmentId;
  spur_seg: SegmentId;
}

export interface FactoryLayout {
  mills: MillLayout[];
  stations: { tool_crib: SegmentId; pallet_magazine: SegmentId };
}

export interface MillSnap {
  id: MillId;
  state: MillState;
  job_id: JobId | null;
  op_index: number;
  loaded_tool: ToolSetId | null;
  loaded_pallet: PalletId | null;
  parts_completed: number;
  busy_time: SimTime;
  fault_time: SimTime;
}

export interface AgvSnap {
  id: AgvId;
  vehicle_type: VehicleType;
  state: AgvState;
  segment: SegmentId;
  cargo: unknown;
  path: SegmentId[];
  path_cursor: number;
  delivered: number;
}

export interface ToolCribSnap {
  inventory: Record<string, number>;
  total_issues: number;
}

export interface PalletMagSnap {
  available: Record<string, number>;
  total_issued: number;
}

export interface JobPreview {
  id: JobId;
  priority: Priority;
  ops: number;
  wait_time: SimTime;
}

export interface JobQueueSnap {
  depth: number;
  next_8: JobPreview[];
}

export interface MetricsSnap {
  throughput: number;
  throughput_rate: number;
  avg_utilization: number;
  avg_queue_depth: number;
  deadlocks: number;
  faults: number;
  wip: number;
  max_wip: number;
  back_pressure_events: number;
}

export interface Snapshot {
  time: SimTime;
  event_count: number;
  mills: MillSnap[];
  agvs: AgvSnap[];
  lane_occupancy: number[];
  tool_crib: ToolCribSnap;
  pallet_magazine: PalletMagSnap;
  job_queue: JobQueueSnap;
  metrics: MetricsSnap;
}

export interface ReadyMessage {
  type: "ready";
  version: string;
  config: ReadyConfig;
  layout: FactoryLayout;
}

export interface SnapshotMessage extends Snapshot {
  type: "snapshot";
}

export interface SimEvent {
  type: "event";
  time: SimTime;
  kind: string;
  detail: Record<string, unknown>;
}

export interface SummaryMessage {
  type: "summary";
  sim_duration: SimTime;
  events_processed: number;
  jobs_completed: number;
  jobs_dispatched: number;
  deadlocks_detected: number;
  total_faults: number;
  back_pressure_events: number;
  mill_utilization: number[];
  avg_utilization: number;
  avg_queue_depth: number;
  throughput_rate: number;
  agv_distance: number[];
  agv_deliveries: number[];
}

export type SimMessage =
  | ReadyMessage
  | SnapshotMessage
  | SimEvent
  | SummaryMessage;

export interface LogEntry {
  time: SimTime;
  kind: string;
  text: string;
}

export interface TimeSeriesPoint {
  time: SimTime;
  value: number;
}

declare global {
  interface Window {
    simBridge: {
      start: (opts?: Record<string, unknown>) => Promise<{ ok?: boolean; error?: string }>;
      command: (cmd: Record<string, unknown>) => void;
      kill: () => Promise<void>;
      onMessage: (cb: (msg: SimMessage) => void) => () => void;
      onStderr: (cb: (line: string) => void) => () => void;
      onExit: (cb: (data: { code: number | null; lastStderr: string[] }) => void) => () => void;
      onError: (cb: (msg: string) => void) => () => void;
    };
  }
}
