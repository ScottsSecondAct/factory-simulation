//! Pluggable scheduling strategies.
//!
//! Each strategy examines the job queue and returns the index of the
//! next job to dispatch. The scheduler calls the strategy instead of
//! always picking jobs in queue order. Four strategies ship out of
//! the box: FIFO, Shortest Processing Time, Earliest Due Date, and
//! Weighted Priority.

use crate::types::*;

/// Read-only context the strategy uses to make its decision.
pub struct SchedulingContext {
    pub now: SimTime,
    pub wip: usize,
    pub max_wip: usize,
}

/// A scheduling strategy picks which queued job to dispatch next.
pub trait SchedulingStrategy: Send {
    fn name(&self) -> StrategyName;
    /// Given the current queue and context, return the index of the
    /// job to dispatch next, or `None` to skip this tick.
    fn select(&self, queue: &[Job], ctx: &SchedulingContext) -> Option<usize>;
}

/// Build a boxed strategy from its name.
pub fn make_strategy(name: StrategyName) -> Box<dyn SchedulingStrategy> {
    match name {
        StrategyName::Fifo => Box::new(FifoStrategy),
        StrategyName::ShortestProcessingTime => Box::new(SptStrategy),
        StrategyName::EarliestDueDate => Box::new(EddStrategy),
        StrategyName::WeightedPriority => Box::new(WeightedPriorityStrategy),
    }
}

// ── FIFO ───────────────────────────────────────────────────────────

pub struct FifoStrategy;

impl SchedulingStrategy for FifoStrategy {
    fn name(&self) -> StrategyName {
        StrategyName::Fifo
    }

    fn select(&self, queue: &[Job], _ctx: &SchedulingContext) -> Option<usize> {
        if queue.is_empty() {
            None
        } else {
            Some(0)
        }
    }
}

// ── Shortest Processing Time ───────────────────────────────────────

pub struct SptStrategy;

impl SchedulingStrategy for SptStrategy {
    fn name(&self) -> StrategyName {
        StrategyName::ShortestProcessingTime
    }

    fn select(&self, queue: &[Job], _ctx: &SchedulingContext) -> Option<usize> {
        if queue.is_empty() {
            return None;
        }
        queue
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let a_dur: SimTime = a.operations.iter().map(|op| op.duration).sum();
                let b_dur: SimTime = b.operations.iter().map(|op| op.duration).sum();
                a_dur.partial_cmp(&b_dur).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
    }
}

// ── Earliest Due Date ──────────────────────────────────────────────

pub struct EddStrategy;

impl SchedulingStrategy for EddStrategy {
    fn name(&self) -> StrategyName {
        StrategyName::EarliestDueDate
    }

    fn select(&self, queue: &[Job], _ctx: &SchedulingContext) -> Option<usize> {
        if queue.is_empty() {
            return None;
        }
        queue
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let a_due = a.due_date.unwrap_or(f64::MAX);
                let b_due = b.due_date.unwrap_or(f64::MAX);
                a_due.partial_cmp(&b_due).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
    }
}

// ── Weighted Priority ──────────────────────────────────────────────

pub struct WeightedPriorityStrategy;

impl WeightedPriorityStrategy {
    fn score(job: &Job, now: SimTime) -> f64 {
        let priority_val = match job.priority {
            Priority::Critical => 4.0,
            Priority::High => 3.0,
            Priority::Normal => 2.0,
            Priority::Low => 1.0,
        };
        let wait_time = now - job.arrived_at;
        let processing_time: SimTime = job.operations.iter().map(|op| op.duration).sum();
        let due_urgency = job
            .due_date
            .map(|d| (d - now).max(0.0))
            .map(|slack| 1.0 / (1.0 + slack))
            .unwrap_or(0.0);

        WEIGHT_PRIORITY * priority_val
            + WEIGHT_WAIT_TIME * wait_time
            + WEIGHT_PROCESSING_TIME * processing_time
            + WEIGHT_DUE_DATE * due_urgency * 1000.0
    }
}

impl SchedulingStrategy for WeightedPriorityStrategy {
    fn name(&self) -> StrategyName {
        StrategyName::WeightedPriority
    }

    fn select(&self, queue: &[Job], ctx: &SchedulingContext) -> Option<usize> {
        if queue.is_empty() {
            return None;
        }
        queue
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                let sa = Self::score(a, ctx.now);
                let sb = Self::score(b, ctx.now);
                sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_job(id: JobId, priority: Priority, duration: SimTime, arrived: SimTime) -> Job {
        Job {
            id,
            priority,
            operations: vec![Operation {
                tool_set: 0,
                duration,
                pallet_type: 0,
            }],
            arrived_at: arrived,
            due_date: None,
        }
    }

    fn ctx(now: SimTime) -> SchedulingContext {
        SchedulingContext {
            now,
            wip: 0,
            max_wip: 20,
        }
    }

    #[test]
    fn fifo_picks_first() {
        let strat = FifoStrategy;
        let queue = vec![
            make_job(1, Priority::Low, 900.0, 0.0),
            make_job(2, Priority::Critical, 100.0, 10.0),
        ];
        assert_eq!(strat.select(&queue, &ctx(100.0)), Some(0));
    }

    #[test]
    fn spt_picks_shortest() {
        let strat = SptStrategy;
        let queue = vec![
            make_job(1, Priority::Normal, 900.0, 0.0),
            make_job(2, Priority::Normal, 200.0, 0.0),
            make_job(3, Priority::Normal, 500.0, 0.0),
        ];
        assert_eq!(strat.select(&queue, &ctx(0.0)), Some(1));
    }

    #[test]
    fn edd_picks_earliest_due() {
        let strat = EddStrategy;
        let mut queue = vec![
            make_job(1, Priority::Normal, 300.0, 0.0),
            make_job(2, Priority::Normal, 300.0, 0.0),
            make_job(3, Priority::Normal, 300.0, 0.0),
        ];
        queue[0].due_date = Some(5000.0);
        queue[1].due_date = Some(2000.0);
        queue[2].due_date = Some(8000.0);
        assert_eq!(strat.select(&queue, &ctx(0.0)), Some(1));
    }

    #[test]
    fn weighted_favors_critical_priority() {
        let strat = WeightedPriorityStrategy;
        let queue = vec![
            make_job(1, Priority::Low, 300.0, 0.0),
            make_job(2, Priority::Critical, 300.0, 0.0),
        ];
        assert_eq!(strat.select(&queue, &ctx(0.0)), Some(1));
    }

    #[test]
    fn empty_queue_returns_none() {
        let strat = FifoStrategy;
        assert_eq!(strat.select(&[], &ctx(0.0)), None);
    }
}
