use factory_sim::engine::SimEngine;
use factory_sim::fault::FaultConfig;
use factory_sim::types::StrategyName;
use factory_sim::world::{World, WorldConfig};

fn default_config() -> WorldConfig {
    WorldConfig {
        fault_cfg: FaultConfig::default(),
        num_amrs: 2,
        seed: 42,
        tool_types: 8,
        tool_copies: 4,
        pallet_types: 4,
        pallet_copies: 8,
        snapshot_interval: 60.0,
        max_wip: 20,
        strategy: StrategyName::Fifo,
    }
}

#[test]
fn simulation_completes_without_panic() {
    let mut world = World::new(default_config());
    let mut engine = SimEngine::new();
    engine.schedule_many(world.seed_events());

    let duration = 3600.0; // 1-hour run
    while let Some(te) = engine.step() {
        if te.time > duration {
            break;
        }
        let (new_events, _notes) = world.handle(&te);
        engine.schedule_many(new_events);
    }

    assert!(engine.events_processed() > 0);
}

#[test]
fn simulation_is_deterministic() {
    let run = |seed: u64| -> (u64, u64) {
        let cfg = WorldConfig { seed, ..default_config() };
        let mut world = World::new(cfg);
        let mut engine = SimEngine::new();
        engine.schedule_many(world.seed_events());

        let duration = 7200.0;
        while let Some(te) = engine.step() {
            if te.time > duration {
                break;
            }
            let (new_events, _) = world.handle(&te);
            engine.schedule_many(new_events);
        }

        let summary = world.metrics.summarize(
            duration,
            engine.events_processed(),
            &world.mills,
            &world.scheduler,
            world.fault_inj.total_faults,
            &world.reconciler,
        );
        (summary.events_processed, summary.jobs_completed)
    };

    let (events_a, jobs_a) = run(42);
    let (events_b, jobs_b) = run(42);
    assert_eq!(events_a, events_b);
    assert_eq!(jobs_a, jobs_b);

    // Different seed should produce different results
    let (events_c, _) = run(99);
    assert_ne!(events_a, events_c);
}

#[test]
fn no_faults_mode_produces_zero_faults() {
    let cfg = WorldConfig {
        fault_cfg: FaultConfig {
            enabled: false,
            ..FaultConfig::default()
        },
        ..default_config()
    };
    let mut world = World::new(cfg);
    let mut engine = SimEngine::new();
    engine.schedule_many(world.seed_events());

    let duration = 3600.0;
    while let Some(te) = engine.step() {
        if te.time > duration {
            break;
        }
        let (new_events, _) = world.handle(&te);
        engine.schedule_many(new_events);
    }

    assert_eq!(world.fault_inj.total_faults, 0);
}

#[test]
fn jobs_dispatched_exceeds_completed() {
    let mut world = World::new(default_config());
    let mut engine = SimEngine::new();
    engine.schedule_many(world.seed_events());

    let duration = 7200.0;
    while let Some(te) = engine.step() {
        if te.time > duration {
            break;
        }
        let (new_events, _) = world.handle(&te);
        engine.schedule_many(new_events);
    }

    assert!(world.scheduler.jobs_dispatched > 0);
    let completed: u64 = world.mills.iter().map(|m| m.parts_completed).sum();
    assert!(world.scheduler.jobs_dispatched >= completed);
}

#[test]
fn all_strategies_complete_without_panic() {
    let strategies = [
        StrategyName::Fifo,
        StrategyName::ShortestProcessingTime,
        StrategyName::EarliestDueDate,
        StrategyName::WeightedPriority,
    ];

    for strategy in strategies {
        let cfg = WorldConfig { strategy, ..default_config() };
        let mut world = World::new(cfg);
        let mut engine = SimEngine::new();
        engine.schedule_many(world.seed_events());

        let duration = 3600.0;
        while let Some(te) = engine.step() {
            if te.time > duration {
                break;
            }
            let (new_events, _) = world.handle(&te);
            engine.schedule_many(new_events);
        }

        let completed: u64 = world.mills.iter().map(|m| m.parts_completed).sum();
        assert!(completed > 0, "{strategy} produced zero completions");
        assert!(
            world.scheduler.jobs_dispatched >= completed,
            "{strategy} dispatched fewer than completed"
        );
    }
}

#[test]
fn strategy_set_at_runtime_takes_effect() {
    let cfg = WorldConfig {
        strategy: StrategyName::Fifo,
        ..default_config()
    };
    let mut world = World::new(cfg);
    assert_eq!(world.scheduler.strategy_name, StrategyName::Fifo);
    world.scheduler.set_strategy(StrategyName::ShortestProcessingTime);
    assert_eq!(
        world.scheduler.strategy_name,
        StrategyName::ShortestProcessingTime
    );
}

#[test]
fn ab_comparison_both_produce_output() {
    let run = |strategy: StrategyName| -> (u64, u64, u64) {
        let cfg = WorldConfig {
            strategy,
            fault_cfg: FaultConfig { enabled: false, ..FaultConfig::default() },
            ..default_config()
        };
        let mut world = World::new(cfg);
        let mut engine = SimEngine::new();
        engine.schedule_many(world.seed_events());

        let duration = 7200.0;
        while let Some(te) = engine.step() {
            if te.time > duration {
                break;
            }
            let (new_events, _) = world.handle(&te);
            engine.schedule_many(new_events);
        }

        let completed: u64 = world.mills.iter().map(|m| m.parts_completed).sum();
        (world.scheduler.jobs_dispatched, completed, engine.events_processed())
    };

    let (fifo_disp, fifo_comp, _fifo_events) = run(StrategyName::Fifo);
    let (spt_disp, spt_comp, _spt_events) = run(StrategyName::ShortestProcessingTime);
    let (edd_disp, edd_comp, _edd_events) = run(StrategyName::EarliestDueDate);
    let (wp_disp, wp_comp, _wp_events) = run(StrategyName::WeightedPriority);

    for (name, disp, comp) in [
        ("FIFO", fifo_disp, fifo_comp),
        ("SPT", spt_disp, spt_comp),
        ("EDD", edd_disp, edd_comp),
        ("Weighted", wp_disp, wp_comp),
    ] {
        assert!(disp > 0, "{name} dispatched zero jobs");
        assert!(comp > 0, "{name} completed zero jobs");
    }
}
