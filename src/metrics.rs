//! Metrics collection and reporting.
//!
//! Tracks throughput, utilization, queue depth, and fault counts across
//! the simulation. Produces a JSON summary consumable by the dashboard.

use serde::Serialize;

use crate::agv::Agv;
use crate::factory::Mill;
use crate::scheduler::Scheduler;
use crate::types::*;

// ── Snapshot (one point in time, for the event log) ─────────────────
#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub time: SimTime,
    pub mills: Vec<MillSnapshot>,
    pub agvs: Vec<AgvSnapshot>,
    pub queue_depth: usize,
    pub throughput: u64,
    pub deadlocks: u64,
}

#[derive(Debug, Serialize)]
pub struct MillSnapshot {
    pub id: MillId,
    pub state: MillState,
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Serialize)]
pub struct AgvSnapshot {
    pub id: AgvId,
    pub state: AgvState,
    pub segment: SegmentId,
}

// ── Cumulative metrics ──────────────────────────────────────────────
#[derive(Debug, Serialize)]
pub struct Summary {
    pub sim_duration: SimTime,
    pub events_processed: u64,
    pub jobs_completed: u64,
    pub jobs_dispatched: u64,
    pub deadlocks_detected: u64,
    pub total_faults: u64,
    pub back_pressure_events: u64,
    pub mill_utilization: Vec<f64>,
    pub avg_utilization: f64,
    pub total_throughput: u64,
    pub avg_queue_depth: f64,
}

// ── Collector ───────────────────────────────────────────────────────
pub struct Metrics {
    snapshots: Vec<Snapshot>,
    snapshot_interval: SimTime,
    last_snapshot: SimTime,
    queue_depth_sum: f64,
    queue_samples: u64,
}

impl Metrics {
    pub fn new(snapshot_interval: SimTime) -> Self {
        Self {
            snapshots: Vec::new(),
            snapshot_interval,
            last_snapshot: 0.0,
            queue_depth_sum: 0.0,
            queue_samples: 0,
        }
    }

    /// Call each scheduler tick to sample queue depth.
    pub fn sample_queue(&mut self, depth: usize) {
        self.queue_depth_sum += depth as f64;
        self.queue_samples += 1;
    }

    /// Capture a snapshot if the interval has elapsed.
    pub fn maybe_snapshot(
        &mut self,
        now: SimTime,
        mills: &[Mill],
        agvs: &[Agv],
        scheduler: &Scheduler,
    ) {
        if now - self.last_snapshot < self.snapshot_interval {
            return;
        }
        self.last_snapshot = now;

        let mill_snaps: Vec<MillSnapshot> = mills
            .iter()
            .map(|m| MillSnapshot {
                id: m.id,
                state: m.state.clone(),
                row: m.row,
                col: m.col,
            })
            .collect();

        let agv_snaps: Vec<AgvSnapshot> = agvs
            .iter()
            .map(|a| AgvSnapshot {
                id: a.id,
                state: a.state.clone(),
                segment: a.segment,
            })
            .collect();

        let throughput: u64 = mills.iter().map(|m| m.parts_completed).sum();

        self.snapshots.push(Snapshot {
            time: now,
            mills: mill_snaps,
            agvs: agv_snaps,
            queue_depth: scheduler.job_queue.len(),
            throughput,
            deadlocks: scheduler.deadlocks_detected,
        });
    }

    /// Build the final summary.
    pub fn summarize(
        &self,
        sim_duration: SimTime,
        events: u64,
        mills: &[Mill],
        scheduler: &Scheduler,
        total_faults: u64,
    ) -> Summary {
        let utilizations: Vec<f64> = mills
            .iter()
            .map(|m| {
                if sim_duration > 0.0 {
                    m.busy_time / sim_duration
                } else {
                    0.0
                }
            })
            .collect();

        let avg_util = utilizations.iter().sum::<f64>() / utilizations.len().max(1) as f64;
        let avg_queue = if self.queue_samples > 0 {
            self.queue_depth_sum / self.queue_samples as f64
        } else {
            0.0
        };
        let total_parts: u64 = mills.iter().map(|m| m.parts_completed).sum();

        Summary {
            sim_duration,
            events_processed: events,
            jobs_completed: total_parts,
            jobs_dispatched: scheduler.jobs_dispatched,
            deadlocks_detected: scheduler.deadlocks_detected,
            total_faults,
            back_pressure_events: scheduler.back_pressure_events,
            mill_utilization: utilizations,
            avg_utilization: avg_util,
            total_throughput: total_parts,
            avg_queue_depth: avg_queue,
        }
    }

    pub fn snapshots(&self) -> &[Snapshot] {
        &self.snapshots
    }

    /// Serialize all snapshots as a JSON array (for dashboard playback).
    pub fn snapshots_json(&self) -> String {
        serde_json::to_string_pretty(&self.snapshots).unwrap_or_default()
    }
}
