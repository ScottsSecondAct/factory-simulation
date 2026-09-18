//! Periodic state reconciliation.
//!
//! Simulates a real factory controller's heartbeat check: the scheduler
//! caches its last-known view of equipment states and resource levels.
//! Between reconciliation ticks, events (faults, completions, AGV
//! movement) change the ground truth without updating the cache. The
//! reconciliation pass compares the stale cache against reality, logs
//! discrepancies, and re-syncs — exactly the pattern used in distributed
//! factory control systems to detect sensor lag, missed messages, and
//! partial failures.

use serde::Serialize;
use std::collections::HashMap;

use crate::agv::Agv;
use crate::factory::{Mill, PalletMagazine, ToolCrib, WorkPrepStation};
use crate::types::*;

/// One discrepancy found during reconciliation.
#[derive(Debug, Clone, Serialize)]
pub struct Drift {
    pub category: DriftCategory,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum DriftCategory {
    MillState,
    AgvPosition,
    ToolInventory,
    PalletInventory,
    WorkPrepState,
}

/// Cached snapshot of believed factory state.
struct Belief {
    mill_states: Vec<MillState>,
    agv_segments: Vec<SegmentId>,
    tool_totals: HashMap<ToolSetId, u16>,
    pallet_totals: HashMap<u8, usize>,
    work_prep_state: WorkPrepState,
}

/// The reconciler compares cached beliefs against ground truth.
pub struct Reconciler {
    belief: Option<Belief>,
    pub passes: u64,
    pub total_drifts: u64,
    pub max_drifts_in_pass: u64,
}

impl Default for Reconciler {
    fn default() -> Self {
        Self::new()
    }
}

impl Reconciler {
    pub fn new() -> Self {
        Self {
            belief: None,
            passes: 0,
            total_drifts: 0,
            max_drifts_in_pass: 0,
        }
    }

    fn capture(
        mills: &[Mill],
        agvs: &[Agv],
        tool_crib: &ToolCrib,
        pallet_mag: &PalletMagazine,
        work_prep: &WorkPrepStation,
    ) -> Belief {
        Belief {
            mill_states: mills.iter().map(|m| m.state.clone()).collect(),
            agv_segments: agvs.iter().map(|a| a.segment).collect(),
            tool_totals: tool_crib.inventory().clone(),
            pallet_totals: pallet_mag.available_counts(),
            work_prep_state: work_prep.state.clone(),
        }
    }

    /// Compare cached beliefs against ground truth, return drifts, re-sync.
    pub fn reconcile(
        &mut self,
        mills: &[Mill],
        agvs: &[Agv],
        tool_crib: &ToolCrib,
        pallet_mag: &PalletMagazine,
        work_prep: &WorkPrepStation,
    ) -> Vec<Drift> {
        self.passes += 1;
        let mut drifts = Vec::new();

        if let Some(ref belief) = self.belief {
            for (i, (believed, actual)) in belief
                .mill_states
                .iter()
                .zip(mills.iter().map(|m| &m.state))
                .enumerate()
            {
                if believed != actual {
                    drifts.push(Drift {
                        category: DriftCategory::MillState,
                        description: format!("Mill {i}: believed {believed:?}, actual {actual:?}"),
                    });
                }
            }

            for (i, (&believed_seg, actual_seg)) in belief
                .agv_segments
                .iter()
                .zip(agvs.iter().map(|a| a.segment))
                .enumerate()
            {
                if believed_seg != actual_seg {
                    drifts.push(Drift {
                        category: DriftCategory::AgvPosition,
                        description: format!(
                            "Vehicle {i}: believed seg {believed_seg}, actual seg {actual_seg}"
                        ),
                    });
                }
            }

            for (tool_id, &believed_count) in &belief.tool_totals {
                let actual_count = tool_crib.available(*tool_id);
                if believed_count != actual_count {
                    drifts.push(Drift {
                        category: DriftCategory::ToolInventory,
                        description: format!(
                            "Tool set {tool_id}: believed {believed_count}, actual {actual_count}"
                        ),
                    });
                }
            }

            for (ptype, &believed_count) in &belief.pallet_totals {
                let actual_count = pallet_mag.available(*ptype);
                if believed_count != actual_count {
                    drifts.push(Drift {
                        category: DriftCategory::PalletInventory,
                        description: format!(
                            "Pallet type {ptype}: believed {believed_count}, actual {actual_count}"
                        ),
                    });
                }
            }

            if belief.work_prep_state != work_prep.state {
                drifts.push(Drift {
                    category: DriftCategory::WorkPrepState,
                    description: format!(
                        "Work prep: believed {:?}, actual {:?}",
                        belief.work_prep_state, work_prep.state
                    ),
                });
            }
        }

        let drift_count = drifts.len() as u64;
        self.total_drifts += drift_count;
        if drift_count > self.max_drifts_in_pass {
            self.max_drifts_in_pass = drift_count;
        }

        self.belief = Some(Self::capture(mills, agvs, tool_crib, pallet_mag, work_prep));
        drifts
    }
}
