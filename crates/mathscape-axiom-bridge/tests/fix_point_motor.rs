//! Phase V end-to-end: the fix-point motor running against a real
//! derived-laws extractor. Demonstrates the closed loop:
//!
//!   observe → detect staleness → mutate the diet → observe novelty
//!
//! This is the first test where the meta-loop USES adaptive-diet
//! mutation against a real discovery extractor (not just null), so
//! the signal → intervention → reward cycle is visible end-to-end.

mod common;

use mathscape_compress::Motor;
use mathscape_core::{
    bootstrap::{
        BootstrapCycleSpec, ExperimentScenario,
    },
    meta_loop::{
        HeuristicProposer, MetaLoop, MetaLoopConfig,
    },
    policy::LinearPolicy,
};

fn scenario_label_for(record: &mathscape_core::meta_loop::MetaPhaseRecord) -> String {
    let s = &record.scenario;
    format!("{} ({})", s.name, s.phases[0].corpus_generator)
}

#[test]
fn fix_point_motor_runs_and_visibly_mutates_diet() {
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ PHASE V — FIX-POINT MOTOR RUNNING END-TO-END          ║");
    println!("╚══════════════════════════════════════════════════════╝");

    let loop_ = MetaLoop::new(
        Motor::default(),
        HeuristicProposer::with_extractor("derived-laws"),
        MetaLoopConfig {
            max_phases: 10,
            sail_out_window: 0, // never halt on sail-out so we observe the full dynamics
            policy_delta_threshold: 1e-9,
        },
    );

    // Seed: baseline spec on default tensor corpus.
    let seed_spec = BootstrapCycleSpec {
        corpus_generator: "default".into(),
        law_extractor: "derived-laws".into(),
        model_updater: "default".into(),
        deduper: "canonical".into(),
        n_iterations: 5,
        seed_library: Vec::new(),
        seed_policy: LinearPolicy::tensor_seeking_prior(),
        early_stop_after_stable: Some(1),
    };
    let seed_scenario = ExperimentScenario {
        name: "seed-default".into(),
        phases: vec![seed_spec],
    };

    let outcome = loop_.run(seed_scenario).expect("motor runs");

    println!("\n  phases executed     : {}", outcome.history.len());
    println!("  termination         : {:?}", outcome.terminated_reason);
    println!(
        "  final library size  : {}",
        outcome.final_library().len()
    );
    println!(
        "  final policy gen    : {}",
        outcome.final_policy().generation
    );
    println!(
        "  meta-attestation    : {:?}",
        outcome.meta_attestation
    );
    println!();

    println!("  phase-by-phase trace:");
    println!(
        "    {:>2}  {:<48}  {:<10}  {:>5}  {:>6}  {:>10}  {:<6}",
        "i", "scenario (corpus-gen)", "archetype", "lib", "growth", "δπ", "stale"
    );
    for (i, record) in outcome.history.iter().enumerate() {
        let obs = &record.observation;
        let label = scenario_label_for(record);
        let archetype_marker = if record.scenario.name.contains("adaptive-diet")
        {
            "DIET"
        } else if record.scenario.name.contains("extended-discovery") {
            "EXT"
        } else if record.scenario.name.contains("train-only") {
            "TRAIN"
        } else if record.scenario.name.contains("early-stop") {
            "EARLY"
        } else if record.scenario.name.contains("baseline") {
            "BASE"
        } else {
            "SEED"
        };
        println!(
            "    {:>2}  {:<48}  {:<10}  {:>5}  {:>6}  {:>10.4}  {:>5.2}",
            i,
            label,
            archetype_marker,
            obs.total_library_size,
            obs.net_growth(),
            obs.trained_policy_delta_norm,
            obs.staleness_score(),
        );
    }

    let diet_phases: Vec<usize> = outcome
        .history
        .iter()
        .enumerate()
        .filter(|(_, r)| r.scenario.name.contains("adaptive-diet"))
        .map(|(i, _)| i)
        .collect();

    println!(
        "\n  AdaptiveDiet archetypes fired at phases: {:?}",
        diet_phases
    );

    // ── Invariants ──────────────────────────────────────────────
    // 1. The motor ran at least one AdaptiveDiet phase — proving
    //    the staleness signal → diet mutation path is live on a
    //    real extractor, not just the null case.
    assert!(
        !diet_phases.is_empty(),
        "fix-point motor must trigger AdaptiveDiet at least once \
         when the default corpus saturates"
    );

    // 2. At least one phase post-seed showed growth — proving the
    //    pipeline discovers SOMETHING even when the corpus is
    //    the static default one.
    let total_growth: usize = outcome
        .observation_history()
        .iter()
        .map(|o| o.net_growth())
        .sum();
    assert!(
        total_growth > 0,
        "motor must produce non-zero cumulative library growth"
    );

    // 3. Staleness detected in at least one observation — otherwise
    //    the proposer would never have cause to mutate the diet.
    let max_staleness = outcome
        .observation_history()
        .iter()
        .map(|o| o.staleness_score())
        .fold(0.0f64, f64::max);
    assert!(
        max_staleness >= 0.5,
        "at least one observation must register non-trivial staleness \
         for the motor to have cause to act"
    );

    println!("\n  ══ MOTOR RUNS. ══");
    println!(
        "  The closed loop — observe staleness → mutate diet → observe \
         novelty — is working against a real derive-laws extractor."
    );
}

#[test]
fn fix_point_motor_is_deterministic() {
    let build_loop = || {
        MetaLoop::new(
            Motor::default(),
            HeuristicProposer::with_extractor("derived-laws"),
            MetaLoopConfig {
                max_phases: 6,
                sail_out_window: 0,
                policy_delta_threshold: 1e-9,
            },
        )
    };
    let seed = || ExperimentScenario {
        name: "seed-default".into(),
        phases: vec![BootstrapCycleSpec {
            corpus_generator: "default".into(),
            law_extractor: "derived-laws".into(),
            model_updater: "default".into(),
            deduper: "canonical".into(),
            n_iterations: 3,
            seed_library: Vec::new(),
            seed_policy: LinearPolicy::tensor_seeking_prior(),
            early_stop_after_stable: Some(1),
        }],
    };
    let a = build_loop().run(seed()).unwrap();
    let b = build_loop().run(seed()).unwrap();
    assert_eq!(a.history.len(), b.history.len());
    // Scenario name sequence is bit-identical across replays.
    let names_a: Vec<&str> =
        a.history.iter().map(|r| r.scenario.name.as_str()).collect();
    let names_b: Vec<&str> =
        b.history.iter().map(|r| r.scenario.name.as_str()).collect();
    assert_eq!(names_a, names_b, "motor decisions must be deterministic");
    // Meta-attestation is bit-identical.
    assert_eq!(a.meta_attestation, b.meta_attestation);
}
