//! Phase W.11: deep proof of the perpetual-improvement loop with
//! a REAL MetaLoop run driving the EventHub.
//!
//! Unlike the core-crate `perpetual_loop.rs` test, which
//! synthesizes MapEvents by hand, this test runs an actual
//! MetaLoop with the derive-laws extractor (genuine rule
//! discovery) and then threads the resulting outcome through the
//! event bus. The trainer + benchmark + plasticity controller
//! observe real mathematical discovery in progress.
//!
//! This is "prove its effectiveness further and deeper into the
//! mathscape" — the fixed-point claim, tested against actual
//! law extraction.

use mathscape_compress::derive_laws_from_corpus_instrumented;
use mathscape_core::bootstrap::{
    BootstrapCycle, BootstrapCycleSpec, CanonicalDeduper,
    DefaultCorpusGenerator, DefaultModelUpdater, ExperimentOutcome,
    ExperimentScenario, LawExtractor, PhaseOutcome, SpecExecutionError,
};
use mathscape_core::eval::RewriteRule;
use mathscape_core::hash::TermRef;
use mathscape_core::math_problem::{harder_problem_set, BenchmarkConsumer};
use mathscape_core::mathscape_map::EventHub;
use mathscape_core::meta_loop::{
    publish_outcome_events, HeuristicProposer, MetaLoop, MetaLoopConfig,
    ScenarioExecutor,
};
use mathscape_core::plasticity::{Plastic, PlasticityController};
use mathscape_core::policy::LinearPolicy;
use mathscape_core::streaming_policy::StreamingPolicyTrainer;
use mathscape_core::term::Term;
use mathscape_core::{AdaptiveCorpusGenerator, BanditProbe, BufferedConsumer};
use std::cell::RefCell;
use std::rc::Rc;

/// The real derive-laws extractor.
struct DerivedLawsExtractor {
    step_limit: usize,
    min_support: usize,
}

impl LawExtractor for DerivedLawsExtractor {
    fn extract(
        &self,
        corpus: &[Term],
        library: &[RewriteRule],
    ) -> Vec<RewriteRule> {
        let mut next_id: mathscape_core::term::SymbolId =
            (library.len() + 1) as u32;
        let (laws, _stats) = derive_laws_from_corpus_instrumented(
            corpus,
            library,
            self.step_limit,
            self.min_support,
            &mut next_id,
        );
        laws
    }
}

/// The real motor executor — runs BootstrapCycle with the
/// derive-laws extractor over whatever corpus the scenario
/// names.
struct RealMotorExecutor;

impl ScenarioExecutor for RealMotorExecutor {
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
            let outcome = run_spec(&spec)?;
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

fn run_spec(
    spec: &BootstrapCycleSpec,
) -> Result<mathscape_core::bootstrap::BootstrapOutcome, SpecExecutionError> {
    let n = spec.n_iterations;
    let extractor = DerivedLawsExtractor {
        step_limit: 300,
        min_support: 2,
    };
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
        other => {
            return Err(SpecExecutionError::UnknownLayer {
                role: "corpus_generator",
                name: other.to_string(),
            });
        }
    };

    Ok(outcome)
}

/// The deep proof: real MetaLoop → real discovery → events
/// published to the hub → trainer, benchmark, probe, and
/// plasticity controller all react to genuine mathematical
/// discovery.
#[test]
fn perpetual_loop_drives_real_motor_and_measures_improvement() {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║ PHASE W.11 — PERPETUAL LOOP x REAL MOTOR                ║");
    println!("║ genuine law extraction → hub → trainer + benchmark      ║");
    println!("╚════════════════════════════════════════════════════════╝");

    // ── Wire up the live components ───────────────────────────
    let hub = EventHub::new();
    let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
    let buffer = Rc::new(BufferedConsumer::new());
    let benchmark = Rc::new(BenchmarkConsumer::new(harder_problem_set()));

    hub.subscribe(trainer.clone());
    hub.subscribe(buffer.clone());

    // Bandit probe tunes learning rate live as the motor runs.
    let applied = Rc::new(RefCell::new(Vec::<f64>::new()));
    let applied_c = applied.clone();
    let trainer_for_probe = Rc::downgrade(&trainer);
    let probe = Rc::new(BanditProbe::new(
        "lr",
        vec![0.05, 0.1, 0.2],
        Box::new(move |v: &f64| {
            applied_c.borrow_mut().push(*v);
            if let Some(t) = trainer_for_probe.upgrade() {
                t.adjust_learning_rate(*v);
            }
        }),
        4,
        0.25,
    ));
    hub.subscribe(probe.clone());

    let controller = PlasticityController::new();
    controller.register(trainer.clone() as Rc<dyn Plastic>);
    controller.register(probe.clone() as Rc<dyn Plastic>);

    // ── Run the REAL motor ─────────────────────────────────────
    let loop_ = MetaLoop::new(
        RealMotorExecutor,
        HeuristicProposer::with_extractor("derived-laws"),
        MetaLoopConfig {
            max_phases: 4,
            sail_out_window: 0,
            policy_delta_threshold: 1e-9,
        },
    );
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

    let motor_outcome = loop_.run(seed_scenario).expect("motor runs");
    let final_library = motor_outcome.final_library().to_vec();
    println!(
        "\n  motor: {} phases, {} rules discovered",
        motor_outcome.history.len(),
        final_library.len()
    );

    // ── Translate motor outcome → hub events ───────────────────
    publish_outcome_events(&motor_outcome, &hub, 0.6);
    println!(
        "  hub:    {} events published, buffer holds {}",
        hub.published_count(),
        buffer.len()
    );

    // ── Benchmark the discovered library ───────────────────────
    //
    // Run the benchmark BEFORE motor-discovery sees it, then
    // AFTER, so we can measure any improvement the motor drove.
    let baseline_report = benchmark.benchmark_now(&Vec::new(), &hub);
    let post_motor_report = benchmark.benchmark_now(&final_library, &hub);
    println!(
        "  bench:  baseline {:.2}, post-motor {:.2}",
        baseline_report.solved_fraction(),
        post_motor_report.solved_fraction()
    );

    // ── Tick plasticity a few times ─────────────────────────────
    for _ in 0..3 {
        let _ = controller.tick();
    }

    // ── ASSERTIONS — deep fixed-point properties ────────────────

    // 1. The motor's outcome produced events on the hub.
    assert!(
        hub.published_count() >= 1,
        "motor events + benchmarks reached the hub"
    );

    // 2. The trainer saw those events and updated.
    assert!(trainer.events_seen() >= 1);
    assert!(trainer.updates_applied() >= 1);

    // 3. Benchmark post-motor ≥ baseline (might be equal if the
    //    motor didn't discover identity-style rules, but NEVER
    //    worse — the library monotonically grows).
    assert!(
        post_motor_report.solved_fraction()
            >= baseline_report.solved_fraction() - 1e-9,
        "benchmark non-decreasing with discovered library"
    );

    // 4. Trainer's policy is finite.
    let snap = trainer.snapshot();
    assert!(snap.bias.is_finite());
    for w in snap.weights.iter() {
        assert!(w.is_finite());
    }

    // 5. Plasticity controller ran exactly 3 ticks.
    assert_eq!(controller.tick_count(), 3);

    // 6. Hub + buffer agree (lossless fan-out).
    assert_eq!(buffer.len() as u64, hub.published_count());

    // 7. Benchmark history recorded both runs.
    assert_eq!(trainer.benchmark_history().len(), 2);

    // ── Log — the deep snapshot ────────────────────────────────
    println!(
        "\n  deep snapshot:\n    \
           motor phases:       {}\n    \
           rules discovered:   {}\n    \
           hub events:         {}\n    \
           trainer events:     {}\n    \
           trainer updates:    {}\n    \
           baseline score:     {:.4}\n    \
           post-motor score:   {:.4}\n    \
           score delta:        {:+.4}\n    \
           trained_steps:      {}\n    \
           has_anchor:         {}\n    \
           plasticity ticks:   {}\n    \
           probe lr picks:     {:?}\n",
        motor_outcome.history.len(),
        final_library.len(),
        hub.published_count(),
        trainer.events_seen(),
        trainer.updates_applied(),
        baseline_report.solved_fraction(),
        post_motor_report.solved_fraction(),
        post_motor_report.solved_fraction()
            - baseline_report.solved_fraction(),
        snap.trained_steps,
        trainer.has_anchor(),
        controller.tick_count(),
        applied.borrow(),
    );
}
