//! Fault injection framework.
//!
//! Models stochastic equipment failures with configurable rates. Each
//! fault type has a mean time between failures (MTBF) drawn from an
//! exponential distribution and a fixed repair duration.
//!
//! The injector seeds initial faults at simulation start and schedules
//! the next failure after each repair.

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::engine::{Event, TimedEvent};
use crate::types::*;

// ── Configuration ───────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultConfig {
    /// Mean time between mill failures (seconds).
    pub mill_mtbf: SimTime,
    /// Mill repair duration (seconds).
    pub mill_repair: SimTime,
    /// Mean time between AGV failures (seconds).
    pub agv_mtbf: SimTime,
    /// AGV repair duration (seconds).
    pub agv_repair: SimTime,
    /// Mean time between AMR failures (seconds).
    pub amr_mtbf: SimTime,
    /// AMR repair duration (seconds).
    pub amr_repair: SimTime,
    /// Number of AMRs in the fleet.
    pub num_amrs: usize,
    /// Master enable: set false to run fault-free.
    pub enabled: bool,
}

impl Default for FaultConfig {
    fn default() -> Self {
        Self {
            mill_mtbf: 28_800.0,  // ~8 hours
            mill_repair: 1_800.0, // 30 minutes
            agv_mtbf: 43_200.0,   // ~12 hours
            agv_repair: 900.0,    // 15 minutes
            amr_mtbf: 57_600.0,   // ~16 hours (more reliable than AGVs)
            amr_repair: 600.0,    // 10 minutes (simpler to repair)
            num_amrs: DEFAULT_NUM_AMRS,
            enabled: true,
        }
    }
}

// ── Injector ────────────────────────────────────────────────────────
pub struct FaultInjector {
    pub config: FaultConfig,
    pub total_faults: u64,
}

impl FaultInjector {
    pub fn new(config: FaultConfig) -> Self {
        Self {
            config,
            total_faults: 0,
        }
    }

    /// Generate initial fault events for all equipment.
    pub fn seed_faults<R: Rng>(&self, rng: &mut R) -> Vec<TimedEvent> {
        if !self.config.enabled {
            return Vec::new();
        }
        let mut events = Vec::new();

        for mid in 0..NUM_MILLS {
            let t = self.exp_sample(rng, self.config.mill_mtbf);
            events.push(TimedEvent {
                time: t,
                event: Event::FaultOccur(FaultTarget::Mill(mid)),
            });
        }
        for aid in 0..NUM_AGVS {
            let t = self.exp_sample(rng, self.config.agv_mtbf);
            events.push(TimedEvent {
                time: t,
                event: Event::FaultOccur(FaultTarget::Agv(aid)),
            });
        }
        for amid in 0..self.config.num_amrs {
            let id = NUM_AGVS + amid;
            let t = self.exp_sample(rng, self.config.amr_mtbf);
            events.push(TimedEvent {
                time: t,
                event: Event::FaultOccur(FaultTarget::Agv(id)),
            });
        }
        events
    }

    fn is_amr(id: AgvId) -> bool {
        id >= NUM_AGVS
    }

    /// Schedule the repair event for a fault that just occurred.
    pub fn schedule_repair(&mut self, now: SimTime, target: &FaultTarget) -> TimedEvent {
        self.total_faults += 1;
        let repair_time = match target {
            FaultTarget::Mill(_) => self.config.mill_repair,
            FaultTarget::Agv(id) => {
                if Self::is_amr(*id) {
                    self.config.amr_repair
                } else {
                    self.config.agv_repair
                }
            }
        };
        TimedEvent {
            time: now + repair_time,
            event: Event::FaultRepair(target.clone()),
        }
    }

    /// After a repair, schedule the next failure for this equipment.
    pub fn schedule_next_fault<R: Rng>(
        &self,
        rng: &mut R,
        now: SimTime,
        target: &FaultTarget,
    ) -> TimedEvent {
        let mtbf = match target {
            FaultTarget::Mill(_) => self.config.mill_mtbf,
            FaultTarget::Agv(id) => {
                if Self::is_amr(*id) {
                    self.config.amr_mtbf
                } else {
                    self.config.agv_mtbf
                }
            }
        };
        TimedEvent {
            time: now + self.exp_sample(rng, mtbf),
            event: Event::FaultOccur(target.clone()),
        }
    }

    /// Exponential random variate.
    fn exp_sample<R: Rng>(&self, rng: &mut R, mean: SimTime) -> SimTime {
        -mean * rng.gen::<f64>().ln()
    }
}
