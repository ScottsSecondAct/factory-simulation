//! Factory floor model: CNC mills, tool crib, and pallet magazine.
//!
//! Each [`Mill`] is a finite state machine driven by simulation events.
//! The [`ToolCrib`] and [`PalletMagazine`] track finite inventories that
//! the scheduler must account for when assigning work.

use serde::Serialize;
use std::collections::HashMap;

use crate::types::*;

// ── CNC Mill ────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize)]
pub struct Mill {
    pub id: MillId,
    pub state: MillState,
    pub current_job: Option<JobId>,
    pub current_op: usize,
    pub loaded_tool: Option<ToolSetId>,
    pub loaded_pallet: Option<PalletId>,
    pub loaded_pallet_type: Option<u8>,
    /// Grid position (row, col) for layout / dashboard rendering.
    pub row: usize,
    pub col: usize,
    // Cumulative counters
    pub parts_completed: u64,
    pub busy_time: SimTime,
    pub fault_time: SimTime,
    fault_start: Option<SimTime>,
    pub chip_level: f64,
    pub chip_capacity: f64,
}

impl Mill {
    pub fn new(id: MillId) -> Self {
        let row = id / MILLS_PER_ROW;
        let col = id % MILLS_PER_ROW;
        Self {
            id,
            state: MillState::Idle,
            current_job: None,
            current_op: 0,
            loaded_tool: None,
            loaded_pallet: None,
            loaded_pallet_type: None,
            row,
            col,
            parts_completed: 0,
            busy_time: 0.0,
            fault_time: 0.0,
            fault_start: None,
            chip_level: 0.0,
            chip_capacity: CHIP_CAPACITY,
        }
    }

    pub fn is_available(&self) -> bool {
        self.state == MillState::Idle
    }

    pub fn needs_tool_change(&self, required: ToolSetId) -> bool {
        !self.loaded_tool.is_some_and(|t| t == required)
    }

    pub fn begin_loading(&mut self) {
        self.state = MillState::Loading;
    }

    pub fn begin_machining(&mut self, job_id: JobId, op_index: usize) {
        self.state = MillState::Machining;
        self.current_job = Some(job_id);
        self.current_op = op_index;
    }

    pub fn finish_machining(&mut self, duration: SimTime) {
        self.busy_time += duration;
        self.chip_level += duration * CHIP_RATE;
        self.state = MillState::Unloading;
    }

    pub fn finish_unloading(&mut self) {
        self.parts_completed += 1;
        self.current_job = None;
        self.loaded_pallet_type = None;
        if self.chip_level >= self.chip_capacity {
            self.state = MillState::ChipFull;
        } else {
            self.state = MillState::Idle;
        }
    }

    pub fn finish_chip_evac(&mut self) {
        self.chip_level = 0.0;
        self.state = MillState::Idle;
    }

    pub fn begin_tool_change(&mut self, new_tool: ToolSetId) {
        self.state = MillState::ToolChange;
        self.loaded_tool = Some(new_tool);
    }

    pub fn finish_tool_change(&mut self) {
        self.state = MillState::Idle;
    }

    pub fn fault(&mut self, now: SimTime) {
        self.state = MillState::Faulted;
        self.fault_start = Some(now);
    }

    pub fn repair(&mut self, now: SimTime) {
        if let Some(start) = self.fault_start.take() {
            self.fault_time += now - start;
        }
        self.state = MillState::Idle;
    }
}

// ── Tool Crib ───────────────────────────────────────────────────────
/// Finite inventory of tool sets. Each tool set is a logical group
/// (e.g., "roughing end mill + drill + chamfer") identified by a
/// [`ToolSetId`]. The crib tracks how many copies are available.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCrib {
    inventory: HashMap<ToolSetId, u16>,
    total_issues: u64,
}

impl ToolCrib {
    pub fn new(tool_types: u16, copies_each: u16) -> Self {
        let mut inv = HashMap::new();
        for t in 0..tool_types {
            inv.insert(t, copies_each);
        }
        Self {
            inventory: inv,
            total_issues: 0,
        }
    }

    /// Try to reserve one copy of a tool set. Returns `true` on success.
    pub fn checkout(&mut self, tool: ToolSetId) -> bool {
        if let Some(count) = self.inventory.get_mut(&tool) {
            if *count > 0 {
                *count -= 1;
                self.total_issues += 1;
                return true;
            }
        }
        false
    }

    /// Return a tool set to the crib.
    pub fn checkin(&mut self, tool: ToolSetId) {
        *self.inventory.entry(tool).or_insert(0) += 1;
    }

    pub fn available(&self, tool: ToolSetId) -> u16 {
        self.inventory.get(&tool).copied().unwrap_or(0)
    }

    pub fn total_issues(&self) -> u64 {
        self.total_issues
    }

    pub fn inventory(&self) -> &HashMap<ToolSetId, u16> {
        &self.inventory
    }
}

// ── Pallet Magazine ─────────────────────────────────────────────────
/// Finite set of pallet fixtures. Pallets are typed (e.g., fixture
/// variants for different part geometries). Each type has a pool.
#[derive(Debug, Clone, Serialize)]
pub struct PalletMagazine {
    pools: HashMap<u8, Vec<PalletId>>,
    next_id: PalletId,
    total_issued: u64,
}

impl PalletMagazine {
    pub fn new(pallet_types: u8, count_each: usize) -> Self {
        let mut pools = HashMap::new();
        let mut next = 0usize;
        for t in 0..pallet_types {
            let ids: Vec<PalletId> = (next..next + count_each).collect();
            next += count_each;
            pools.insert(t, ids);
        }
        Self {
            pools,
            next_id: next,
            total_issued: 0,
        }
    }

    /// Take a pallet of the given type. Returns its id or `None`.
    pub fn take(&mut self, pallet_type: u8) -> Option<PalletId> {
        let pool = self.pools.get_mut(&pallet_type)?;
        let id = pool.pop()?;
        self.total_issued += 1;
        Some(id)
    }

    /// Return a pallet to its pool.
    pub fn return_pallet(&mut self, pallet_type: u8, id: PalletId) {
        self.pools.entry(pallet_type).or_default().push(id);
    }

    pub fn available(&self, pallet_type: u8) -> usize {
        self.pools.get(&pallet_type).map_or(0, |v| v.len())
    }

    pub fn available_counts(&self) -> HashMap<u8, usize> {
        self.pools.iter().map(|(&t, v)| (t, v.len())).collect()
    }

    pub fn total_issued(&self) -> u64 {
        self.total_issued
    }
}
