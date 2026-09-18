//! Discrete-event simulation engine.
//!
//! A min-heap of [`TimedEvent`]s drives the simulation. The engine pops
//! the next event, advances the clock, and returns it for the world to
//! handle. Handlers produce zero or more future events that the caller
//! feeds back via [`schedule`] / [`schedule_many`].

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use serde::{Deserialize, Serialize};

use crate::types::*;

// ── Event variants ──────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    // Job lifecycle
    JobArrival(Job),
    MillLoadDone(MillId),
    MillMachiningDone {
        mill_id: MillId,
        job_id: JobId,
        op_index: usize,
        duration: SimTime,
    },
    MillUnloadDone(MillId),
    ToolChangeDone(MillId),

    // AGV movement
    AgvArrived {
        agv_id: AgvId,
        segment: SegmentId,
    },
    AgvLoadDone {
        agv_id: AgvId,
    },
    AgvUnloadDone {
        agv_id: AgvId,
    },

    // Resources
    ToolIssued {
        tool_set: ToolSetId,
        dest_mill: MillId,
    },
    PalletIssued {
        pallet_id: PalletId,
        dest_mill: MillId,
    },

    // Chip evacuation
    ChipEvacDone(MillId),

    // Faults
    FaultOccur(FaultTarget),
    FaultRepair(FaultTarget),

    // Scheduler heartbeat
    SchedulerTick,
}

// ── Timed event wrapper ─────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedEvent {
    pub time: SimTime,
    pub event: Event,
}

// Min-heap ordering: earliest time first.
impl PartialEq for TimedEvent {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}
impl Eq for TimedEvent {}

impl PartialOrd for TimedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for TimedEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse so BinaryHeap (max-heap) behaves as a min-heap.
        other
            .time
            .partial_cmp(&self.time)
            .unwrap_or(Ordering::Equal)
    }
}

// ── Simulation engine ───────────────────────────────────────────────
pub struct SimEngine {
    queue: BinaryHeap<TimedEvent>,
    clock: SimTime,
    event_count: u64,
}

impl Default for SimEngine {
    fn default() -> Self {
        Self {
            queue: BinaryHeap::with_capacity(4096),
            clock: 0.0,
            event_count: 0,
        }
    }
}

impl SimEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current simulation time.
    pub fn now(&self) -> SimTime {
        self.clock
    }

    /// Total events processed.
    pub fn events_processed(&self) -> u64 {
        self.event_count
    }

    /// Schedule a single future event.
    pub fn schedule(&mut self, time: SimTime, event: Event) {
        assert!(
            time >= self.clock,
            "cannot schedule event in the past: {time} < {}",
            self.clock
        );
        self.queue.push(TimedEvent { time, event });
    }

    /// Schedule many events at once (avoids repeated method calls).
    pub fn schedule_many(&mut self, events: Vec<TimedEvent>) {
        for te in events {
            assert!(
                te.time >= self.clock,
                "cannot schedule event in the past: {} < {}",
                te.time,
                self.clock
            );
            self.queue.push(te);
        }
    }

    /// Pop the next event, advance the clock, and return it.
    /// Returns `None` when the queue is empty.
    pub fn step(&mut self) -> Option<TimedEvent> {
        let te = self.queue.pop()?;
        self.clock = te.time;
        self.event_count += 1;
        Some(te)
    }

    /// Peek at the next event time without consuming it.
    pub fn next_time(&self) -> Option<SimTime> {
        self.queue.peek().map(|te| te.time)
    }

    /// Number of pending events.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }
}
