//! Factory floor model: CNC mills, tool crib, and pallet magazine.
//!
//! Each [`Mill`] is a finite state machine driven by simulation events.
//! The [`ToolCrib`] and [`PalletMagazine`] track finite inventories that
//! the scheduler must account for when assigning work.

use serde::Serialize;
use std::collections::{HashMap, VecDeque};

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

    pub fn repair(&mut self, now: SimTime) -> Option<(u8, PalletId)> {
        if let Some(start) = self.fault_start.take() {
            self.fault_time += now - start;
        }
        self.state = MillState::Idle;
        self.current_job = None;
        self.current_op = 0;
        match (self.loaded_pallet_type.take(), self.loaded_pallet.take()) {
            (Some(pt), Some(pid)) => Some((pt, pid)),
            _ => None,
        }
    }
}

// ── Work Preparation Station ────────────────────────────────────────
/// A queued item awaiting robotic billet loading.
#[derive(Debug, Clone, Serialize)]
pub struct PrepItem {
    pub job_id: JobId,
    pub op_index: usize,
    pub mill_id: MillId,
}

/// Single-server robotic station that clamps raw billets onto pallet
/// fixtures before they are delivered to a mill for machining.
#[derive(Debug, Clone, Serialize)]
pub struct WorkPrepStation {
    pub queue: VecDeque<PrepItem>,
    pub processing: Option<PrepItem>,
    pub state: WorkPrepState,
    pub jobs_completed: u64,
    pub fault_time: SimTime,
    #[serde(skip)]
    fault_start: Option<SimTime>,
    generation: u64,
}

impl Default for WorkPrepStation {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkPrepStation {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            processing: None,
            state: WorkPrepState::Idle,
            jobs_completed: 0,
            fault_time: 0.0,
            fault_start: None,
            generation: 0,
        }
    }

    pub fn enqueue(&mut self, item: PrepItem) {
        self.queue.push_back(item);
    }

    /// If idle and queue non-empty, start processing the next item.
    /// Returns the item reference and a generation tag for the event.
    pub fn try_start(&mut self) -> Option<(&PrepItem, u64)> {
        if self.state != WorkPrepState::Idle || self.queue.is_empty() {
            return None;
        }
        self.processing = self.queue.pop_front();
        self.state = WorkPrepState::Processing;
        self.generation += 1;
        Some((self.processing.as_ref().unwrap(), self.generation))
    }

    /// Complete the current processing item.
    pub fn finish_processing(&mut self) -> Option<PrepItem> {
        self.jobs_completed += 1;
        self.state = WorkPrepState::Idle;
        self.processing.take()
    }

    pub fn current_generation(&self) -> u64 {
        self.generation
    }

    /// Total items in the station (queued + processing).
    pub fn total_items(&self) -> usize {
        self.queue.len() + usize::from(self.processing.is_some())
    }

    pub fn fault(&mut self, now: SimTime) {
        if let Some(item) = self.processing.take() {
            self.queue.push_front(item);
        }
        self.state = WorkPrepState::Faulted;
        self.fault_start = Some(now);
    }

    pub fn repair(&mut self, now: SimTime) {
        if let Some(start) = self.fault_start.take() {
            self.fault_time += now - start;
        }
        self.state = WorkPrepState::Idle;
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Mill FSM ───────────────────────────────────────────────────
    #[test]
    fn mill_starts_idle() {
        let mill = Mill::new(0);
        assert_eq!(mill.state, MillState::Idle);
        assert!(mill.is_available());
    }

    #[test]
    fn mill_full_lifecycle() {
        let mut mill = Mill::new(0);

        mill.begin_tool_change(3);
        assert_eq!(mill.state, MillState::ToolChange);
        assert_eq!(mill.loaded_tool, Some(3));

        mill.finish_tool_change();
        assert_eq!(mill.state, MillState::Idle);

        mill.begin_loading();
        assert_eq!(mill.state, MillState::Loading);

        mill.begin_machining(1, 0);
        assert_eq!(mill.state, MillState::Machining);
        assert_eq!(mill.current_job, Some(1));

        mill.finish_machining(100.0);
        assert_eq!(mill.state, MillState::Unloading);
        assert_eq!(mill.busy_time, 100.0);

        mill.finish_unloading();
        assert_eq!(mill.state, MillState::Idle);
        assert_eq!(mill.parts_completed, 1);
    }

    #[test]
    fn mill_chip_full_triggers_on_unload() {
        let mut mill = Mill::new(0);
        mill.chip_level = CHIP_CAPACITY; // already full
        mill.begin_loading();
        mill.begin_machining(1, 0);
        mill.finish_machining(100.0); // adds more chips
        mill.finish_unloading();
        assert_eq!(mill.state, MillState::ChipFull);
    }

    #[test]
    fn mill_chip_evac_resets_level() {
        let mut mill = Mill::new(0);
        mill.chip_level = CHIP_CAPACITY;
        mill.state = MillState::ChipFull;
        mill.finish_chip_evac();
        assert_eq!(mill.chip_level, 0.0);
        assert_eq!(mill.state, MillState::Idle);
    }

    #[test]
    fn mill_fault_and_repair() {
        let mut mill = Mill::new(0);
        mill.begin_loading();
        mill.loaded_pallet = Some(42);
        mill.loaded_pallet_type = Some(2);

        mill.fault(100.0);
        assert_eq!(mill.state, MillState::Faulted);
        assert!(!mill.is_available());

        let returned = mill.repair(200.0);
        assert_eq!(mill.state, MillState::Idle);
        assert_eq!(mill.fault_time, 100.0);
        assert_eq!(returned, Some((2, 42)));
        assert!(mill.loaded_pallet.is_none());
    }

    #[test]
    fn mill_repair_no_pallet_returns_none() {
        let mut mill = Mill::new(0);
        mill.fault(0.0);
        assert_eq!(mill.repair(10.0), None);
    }

    #[test]
    fn mill_needs_tool_change() {
        let mut mill = Mill::new(0);
        assert!(mill.needs_tool_change(5));
        mill.loaded_tool = Some(5);
        assert!(!mill.needs_tool_change(5));
        assert!(mill.needs_tool_change(3));
    }

    #[test]
    fn mill_grid_position() {
        let m0 = Mill::new(0);
        assert_eq!((m0.row, m0.col), (0, 0));
        let m6 = Mill::new(6);
        assert_eq!((m6.row, m6.col), (1, 1));
        let m24 = Mill::new(24);
        assert_eq!((m24.row, m24.col), (4, 4));
    }

    // ── Tool Crib ──────────────────────────────────────────────────
    #[test]
    fn tool_crib_checkout_and_checkin() {
        let mut crib = ToolCrib::new(4, 2);
        assert_eq!(crib.available(0), 2);

        assert!(crib.checkout(0));
        assert_eq!(crib.available(0), 1);

        assert!(crib.checkout(0));
        assert_eq!(crib.available(0), 0);

        assert!(!crib.checkout(0));

        crib.checkin(0);
        assert_eq!(crib.available(0), 1);
        assert_eq!(crib.total_issues(), 2);
    }

    #[test]
    fn tool_crib_unknown_tool_unavailable() {
        let crib = ToolCrib::new(2, 3);
        assert_eq!(crib.available(99), 0);
    }

    // ── Pallet Magazine ────────────────────────────────────────────
    #[test]
    fn pallet_take_and_return() {
        let mut mag = PalletMagazine::new(2, 3);
        assert_eq!(mag.available(0), 3);

        let p1 = mag.take(0).unwrap();
        assert_eq!(mag.available(0), 2);

        mag.return_pallet(0, p1);
        assert_eq!(mag.available(0), 3);
    }

    #[test]
    fn pallet_exhaustion() {
        let mut mag = PalletMagazine::new(1, 1);
        assert!(mag.take(0).is_some());
        assert!(mag.take(0).is_none());
    }

    // ── Work Prep Station ──────────────────────────────────────────
    #[test]
    fn work_prep_enqueue_and_process() {
        let mut wp = WorkPrepStation::new();
        wp.enqueue(PrepItem { job_id: 1, op_index: 0, mill_id: 0 });
        assert_eq!(wp.total_items(), 1);

        let (item, gen) = wp.try_start().unwrap();
        assert_eq!(item.job_id, 1);
        assert!(gen > 0);
        assert_eq!(wp.state, WorkPrepState::Processing);
        assert_eq!(wp.total_items(), 1); // still counts processing item

        let done = wp.finish_processing().unwrap();
        assert_eq!(done.job_id, 1);
        assert_eq!(wp.state, WorkPrepState::Idle);
        assert_eq!(wp.jobs_completed, 1);
    }

    #[test]
    fn work_prep_idle_with_empty_queue_returns_none() {
        let mut wp = WorkPrepStation::new();
        assert!(wp.try_start().is_none());
    }

    #[test]
    fn work_prep_fault_requeues_current() {
        let mut wp = WorkPrepStation::new();
        wp.enqueue(PrepItem { job_id: 1, op_index: 0, mill_id: 0 });
        wp.try_start();
        assert_eq!(wp.state, WorkPrepState::Processing);

        wp.fault(100.0);
        assert_eq!(wp.state, WorkPrepState::Faulted);
        assert_eq!(wp.total_items(), 1); // item requeued

        wp.repair(200.0);
        assert_eq!(wp.state, WorkPrepState::Idle);
        assert_eq!(wp.fault_time, 100.0);

        let (item, _) = wp.try_start().unwrap();
        assert_eq!(item.job_id, 1); // same item retried
    }
}
