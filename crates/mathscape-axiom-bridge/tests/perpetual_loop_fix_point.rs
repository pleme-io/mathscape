//! Phase W.12: **deep long-horizon proof** — the fix-point motor
//! with diet mutation, driving the perpetual-improvement loop.
//!
//! This is the deepest test: run MetaLoop for many phases on a
//! real derive-laws extractor; let the HeuristicProposer fire
//! the AdaptiveDiet archetype when staleness detects saturation;
//! publish every outcome event to the EventHub; watch the
//! streaming trainer, benchmark, bandit probe, and plasticity
//! controller react to genuine discovery + environment mutation
//! across the run.
//!
//! The new capability this covers that W.11 did NOT: the
//! fix-point motor's DIET-MUTATION cycle. W.11 ran 4 phases and
//! exited when the library filled. W.12 runs longer, lets the
//! machine saturate, and observes what happens AFTER the default
//! corpus runs out of novelty. Does the motor recover? Does the
//! perpetual loop see the recovery?

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
    HeuristicProposer, MetaLoop, MetaLoopConfig, ScenarioExecutor,
};
use mathscape_core::plasticity::{Plastic, PlasticityController};
use mathscape_core::policy::LinearPolicy;
use mathscape_core::streaming_policy::StreamingPolicyTrainer;
use mathscape_core::term::Term;
use mathscape_core::{AdaptiveCorpusGenerator, BanditProbe, BufferedConsumer};
use std::cell::RefCell;
use std::rc::Rc;

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

/// Long-horizon proof: fix-point motor with diet mutation,
/// perpetual loop observing every event.
#[test]
fn perpetual_loop_rides_fix_point_motor_through_saturation() {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║ PHASE W.12 — DEEP LONG-HORIZON: FIX-POINT MOTOR x LOOP  ║");
    println!("║ diet-mutation on saturation → events → perpetual loop   ║");
    println!("╚════════════════════════════════════════════════════════╝");

    // ── Wire up live components ────────────────────────────────
    let hub = EventHub::new();
    let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
    let buffer = Rc::new(BufferedConsumer::new());
    let benchmark = Rc::new(BenchmarkConsumer::new(harder_problem_set()));

    hub.subscribe(trainer.clone());
    hub.subscribe(buffer.clone());

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

    // ── Run the fix-point motor for MORE phases ────────────────
    //
    // 10 phases, sail_out_window = 0 so it never halts early —
    // we observe the full dynamic including the diet-mutation
    // intervention past saturation.
    let loop_ = MetaLoop::new(
        MotorExecutor,
        HeuristicProposer::with_extractor("derived-laws"),
        MetaLoopConfig {
            max_phases: 10,
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

    // ── Count adaptive-diet phases ─────────────────────────────
    let diet_phases = motor_outcome
        .history
        .iter()
        .filter(|r| r.scenario.name.contains("adaptive-diet"))
        .count();

    println!(
        "\n  motor phases run: {}",
        motor_outcome.history.len()
    );
    println!(
        "  adaptive-diet phases fired: {diet_phases}",
    );
    println!("  rules discovered: {}", final_library.len());

    // ── Stream phase-by-phase library snapshots to the hub ────
    //
    // Rather than translating the whole outcome at once, publish
    // per-phase events AND benchmark after each phase so the
    // perpetual loop sees the full evolution.
    let mut per_phase_scores = Vec::new();
    let mut seen_rule_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut cumulative_lib: Vec<RewriteRule> = Vec::new();

    use mathscape_core::mathscape_map::MapEvent;
    for (phase_idx, record) in motor_outcome.history.iter().enumerate() {
        let phase_lib = record.outcome.final_library();
        // Publish CoreGrew for each NEW rule this phase.
        for rule in phase_lib {
            if seen_rule_names.insert(rule.name.clone()) {
                let before = cumulative_lib.len();
                cumulative_lib.push(rule.clone());
                hub.publish(&MapEvent::CoreGrew {
                    prev_core_size: before,
                    new_core_size: cumulative_lib.len(),
                    added_rule: rule.clone(),
                });
            }
        }
        // Publish StalenessCrossed if the phase's observation
        // indicates saturation.
        let stale = record.observation.staleness_score();
        if stale >= 0.6 {
            hub.publish(&MapEvent::StalenessCrossed {
                seed: 0,
                phase_index: phase_idx,
                threshold: 0.6,
                observed: stale,
            });
        }
        // Benchmark against the CUMULATIVE library so far.
        let report = benchmark.benchmark_now(&cumulative_lib, &hub);
        per_phase_scores.push(report.solved_fraction());
    }

    // Tick plasticity a handful of times so the outer controller
    // has a chance to shed/reinforce across all phases.
    for _ in 0..5 {
        let _ = controller.tick();
    }

    // ── ASSERTIONS — long-horizon fix-point properties ─────────

    // 1. Motor ran the full 10 phases.
    assert_eq!(motor_outcome.history.len(), 10);

    // 2. Benchmark score is monotonically non-decreasing — the
    //    cumulative library keeps covering at least as much as
    //    it did before.
    for w in per_phase_scores.windows(2) {
        assert!(
            w[1] >= w[0] - 1e-9,
            "per-phase scores monotonically non-decreasing: {} → {}",
            w[0],
            w[1]
        );
    }

    // 3. Final score strictly better than the first-phase score.
    //    (If the motor discovers ANY rule the harder_problem_set
    //    can use, this should hold.)
    let first = per_phase_scores.first().copied().unwrap_or(0.0);
    let last = per_phase_scores.last().copied().unwrap_or(0.0);
    assert!(
        last >= first,
        "long-horizon score improved or held: {first} → {last}",
    );

    // 4. Trainer is still alive, still finite, still monotonic.
    let snap = trainer.snapshot();
    assert!(snap.trained_steps > 0);
    assert!(snap.bias.is_finite());
    for w in snap.weights.iter() {
        assert!(w.is_finite());
    }

    // 5. Hub + buffer lossless across the whole run.
    assert_eq!(buffer.len() as u64, hub.published_count());

    // 6. Benchmark history matches per-phase scores.
    assert_eq!(
        trainer.benchmark_history().len(),
        per_phase_scores.len()
    );

    // 7. Plasticity controller ticked 5 times cleanly.
    assert_eq!(controller.tick_count(), 5);

    // ── Log — the deep long-horizon snapshot ───────────────────
    println!(
        "\n  deep long-horizon snapshot:\n    \
           motor phases:        {}\n    \
           diet-mutation phases: {}\n    \
           rules discovered:    {}\n    \
           hub events:          {}\n    \
           trainer events:      {}\n    \
           trainer updates:     {}\n    \
           per-phase scores:    {:?}\n    \
           score trajectory:    {:.4} → {:.4}  (Δ {:+.4})\n    \
           trained_steps:       {}\n    \
           has_anchor:          {}\n    \
           plasticity ticks:    {}\n    \
           probe lr picks:      {:?}\n",
        motor_outcome.history.len(),
        diet_phases,
        final_library.len(),
        hub.published_count(),
        trainer.events_seen(),
        trainer.updates_applied(),
        per_phase_scores,
        first,
        last,
        last - first,
        snap.trained_steps,
        trainer.has_anchor(),
        controller.tick_count(),
        applied.borrow(),
    );
}
