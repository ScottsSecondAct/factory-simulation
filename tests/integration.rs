use factory_sim::engine::SimEngine;
use factory_sim::fault::FaultConfig;
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
