//! Shared types, identifiers, and factory constants.
//!
//! Every module imports from here. Physical layout constants define the
//! factory floor: 25 mills in a 5×5 grid, a loop lane with spurs, and
//! fixed stations for the tool crib and pallet magazine.

use serde::{Deserialize, Serialize};

// ── Identifier types ────────────────────────────────────────────────
pub type SimTime = f64; // seconds
pub type MillId = usize;
pub type AgvId = usize;
pub type JobId = u64;
pub type ToolSetId = u16;
pub type PalletId = usize;
pub type SegmentId = usize;

// ── Factory dimensions ──────────────────────────────────────────────
pub const NUM_MILLS: usize = 25;
pub const NUM_AGVS: usize = 6;
pub const DEFAULT_NUM_AMRS: usize = 2;
pub const MILL_ROWS: usize = 5;
pub const MILLS_PER_ROW: usize = 5;

// ── Lane network geometry ───────────────────────────────────────────
// Main loop: segments 0..19 (ring road around the factory)
// Mill spurs: segments 20..44 (one per mill, branching off the loop)
pub const LOOP_SEGMENTS: usize = 20;
pub const SPUR_BASE: usize = LOOP_SEGMENTS; // first spur segment id
pub const TOTAL_SEGMENTS: usize = LOOP_SEGMENTS + NUM_MILLS;
pub const TOOL_CRIB_SEG: SegmentId = 0;
pub const PALLET_MAG_SEG: SegmentId = 10;

// ── Timing constants (seconds) ──────────────────────────────────────
pub const AGV_SEGMENT_TRAVEL: SimTime = 8.0;
pub const AMR_SEGMENT_TRAVEL: SimTime = 6.0;
pub const MILL_LOAD_TIME: SimTime = 45.0;
pub const MILL_UNLOAD_TIME: SimTime = 45.0;
pub const TOOL_CHANGE_TIME: SimTime = 120.0;
pub const TOOL_ISSUE_TIME: SimTime = 30.0;
pub const PALLET_ISSUE_TIME: SimTime = 20.0;
pub const SCHEDULER_INTERVAL: SimTime = 5.0;
pub const DEFAULT_MAX_WIP: usize = 20;
pub const CHIP_CAPACITY: f64 = 100.0;
pub const CHIP_RATE: f64 = 0.05; // chip units per second of machining
pub const CHIP_EVAC_TIME: SimTime = 60.0;
pub const CHIP_STATION_SEG: SegmentId = 5;
pub const WORK_PREP_SEG: SegmentId = 15;
pub const WORK_PREP_TIME_MIN: SimTime = 60.0;
pub const WORK_PREP_TIME_MAX: SimTime = 120.0;
pub const WORK_PREP_MAX_QUEUE: usize = 4;

// ── Mill state machine ──────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MillState {
    Idle,
    WaitingPallet,
    WaitingTool,
    Loading,
    Machining,
    Unloading,
    ToolChange,
    Faulted,
    ChipFull,
}

// ── Work prep station state machine ────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkPrepState {
    Idle,
    Processing,
    Faulted,
}

// ── AGV state machine ───────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgvState {
    Idle,
    Traveling,
    Loading,
    Unloading,
    Blocked, // waiting for a lane segment
    Faulted,
}

// ── Vehicle type ────────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VehicleType {
    Agv,
    Amr,
}

// ── AGV cargo ───────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Cargo {
    Empty,
    Pallet(PalletId),
    ToolSet(ToolSetId),
    Workpiece {
        job_id: JobId,
        op_index: usize,
    },
    ChipBin(MillId),
    PrepPallet {
        job_id: JobId,
        op_index: usize,
        mill_id: MillId,
    },
}

// ── Job priority ────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    Critical = 0,
    High = 1,
    Normal = 2,
    Low = 3,
}

// ── Machining operation (one step within a job) ─────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub tool_set: ToolSetId,
    pub duration: SimTime,
    pub pallet_type: u8, // 0..3 → four pallet fixture types
}

// ── Job (a work order with sequenced operations) ────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub priority: Priority,
    pub operations: Vec<Operation>,
    pub arrived_at: SimTime,
}

// ── Fault targets ───────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FaultTarget {
    Mill(MillId),
    Agv(AgvId),
    WorkPrep,
}

// ── Loop segment → spur mapping ─────────────────────────────────────
/// Which main-loop segment does mill `mid` branch off of?
pub fn mill_loop_segment(mid: MillId) -> SegmentId {
    let row = mid / MILLS_PER_ROW;
    // Rows connect at loop segments 2, 6, 10, 14, 18
    2 + row * 4
}

/// The spur segment for a given mill.
pub fn mill_spur(mid: MillId) -> SegmentId {
    SPUR_BASE + mid
}
