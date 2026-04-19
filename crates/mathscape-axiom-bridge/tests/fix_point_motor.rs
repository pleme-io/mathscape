//! Phase V end-to-end: the fix-point motor running against a real
//! derived-laws extractor. Demonstrates the closed loop:
//!
//!   observe → detect staleness → mutate the diet → observe novelty
//!
//! This is the first test where the meta-loop USES adaptive-diet
//! mutation against a real discovery extractor (not just null), so
//! the signal → intervention → reward cycle is visible end-to-end.

mod common;

use mathscape_compress::derive_laws_from_corpus_instrumented;
use mathscape_core::{
    bootstrap::{
        execute_scenario_core, BootstrapCycle, BootstrapCycleSpec,
        CanonicalDeduper, DefaultCorpusGenerator, DefaultModelUpdater,
        ExperimentOutcome, ExperimentScenario, LawExtractor, PhaseOutcome,
        SpecExecutionError,
    },
    eval::RewriteRule,
    hash::TermRef,
    meta_loop::{
        HeuristicProposer, MetaLoop, MetaLoopConfig, ScenarioExecutor,
    },
    policy::LinearPolicy,
    term::Term,
    AdaptiveCorpusGenerator,
};
use std::cell::RefCell;

/// Custom derived-laws extractor (same shape as other tests').
struct DerivedLawsExtractor {
    step_limit: usize,
    min_support: usize,
    stats: RefCell<Vec<mathscape_compress::LawGenStats>>,
}

impl DerivedLawsExtractor {
    fn new(step_limit: usize, min_support: usize) -> Self {
        Self {
            step_limit,
            min_support,
            stats: RefCell::new(Vec::new()),
        }
    }
}

impl LawExtractor for DerivedLawsExtractor {
    fn extract(
        &self,
        corpus: &[Term],
        library: &[RewriteRule],
    ) -> Vec<RewriteRule> {
        let mut next_id: mathscape_core::term::SymbolId =
            (library.len() + 1) as u32;
        let (laws, stats) = derive_laws_from_corpus_instrumented(
            corpus,
            library,
            self.step_limit,
            self.min_support,
            &mut next_id,
        );
        self.stats.borrow_mut().push(stats);
        laws
    }
}

/// Real executor: routes scenarios through a BootstrapCycle with
/// the derive-laws extractor, honoring corpus_generator names
/// ("default" and "adaptive") and early_stop_after_stable. This
/// is the motor — observe what scenarios the proposer picks AND
/// what their outcomes are.
struct MotorExecutor;

impl ScenarioExecutor for MotorExecutor {
    fn execute(
        &self,
        scenario: &ExperimentScenario,
    ) -> Result<ExperimentOutcome, SpecExecutionError> {
        let mut phases = Vec::new();
        let mut carry_lib = scenario
            .phases
            .first()
            .map(|p| p.seed_library.clone())
            .unwrap_or_default();
        let mut carry_pol = scenario
            .phases
            .first()
            .map(|p| p.seed_policy.clone())
            .unwrap_or_else(LinearPolicy::tensor_seeking_prior);
        for (idx, base_spec) in scenario.phases.iter().enumerate() {
            let mut spec = base_spec.clone();
            if idx > 0 {
                spec.seed_library = carry_lib.clone();
                spec.seed_policy = carry_pol.clone();
            }
            let outcome = run_spec_with_real_extractor(&spec)?;
            carry_lib = outcome.final_library.clone();
            carry_pol = outcome.final_policy.clone();
            phases.push(PhaseOutcome {
                phase_index: idx,
                spec_used: spec,
                cycle_outcome: outcome,
            });
        }
        let concat: Vec<u8> = phases
            .iter()
            .flat_map(|p| p.cycle_outcome.attestation.as_bytes().to_vec())
            .collect();
        let chain_attestation = TermRef::from_bytes(&concat);
        Ok(ExperimentOutcome {
            phases,
            chain_attestation,
            scenario_total_ns: 0,
        })
    }
}

fn run_spec_with_real_extractor(
    spec: &BootstrapCycleSpec,
) -> Result<mathscape_core::bootstrap::BootstrapOutcome, SpecExecutionError> {
    let n = spec.n_iterations;
    let extractor = DerivedLawsExtractor::new(300, 2);
    let seed_lib = spec.seed_library.clone();
    let seed_pol = spec.seed_policy.clone();

    let outcome = match spec.corpus_generator.as_str() {
        "default" => {
            let cycle = BootstrapCycle::new(
                DefaultCorpusGenerator,
                extractor,
                DefaultModelUpdater::default(),
                n,
            );
            if let Some(w) = spec.early_stop_after_stable {
                cycle.run_until_stable(seed_lib, seed_pol, &CanonicalDeduper, w)
            } else {
                cycle.run_with_dedup(seed_lib, seed_pol, &CanonicalDeduper)
            }
        }
        "adaptive" => {
            // Phase V: the diet-mutation target. Uses the new
            // AdaptiveCorpusGenerator which reads library state
            // and synthesizes residue-inviting terms.
            let cycle = BootstrapCycle::new(
                AdaptiveCorpusGenerator::default(),
                extractor,
                DefaultModelUpdater::default(),
                n,
            );
            if let Some(w) = spec.early_stop_after_stable {
                cycle.run_until_stable(seed_lib, seed_pol, &CanonicalDeduper, w)
            } else {
                cycle.run_with_dedup(seed_lib, seed_pol, &CanonicalDeduper)
            }
        }
        _ => {
            // Fallback: delegate to the core executor for null etc.
            let mini = ExperimentScenario {
                name: "fallback".into(),
                phases: vec![spec.clone()],
            };
            let inner = execute_scenario_core(&mini)?;
            return Ok(inner.phases.into_iter().next().unwrap().cycle_outcome);
        }
    };
    Ok(outcome)
}

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
        MotorExecutor,
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
            MotorExecutor,
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
