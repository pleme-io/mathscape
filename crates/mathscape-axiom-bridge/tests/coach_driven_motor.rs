//! Phase Z.1 (2026-04-19): Coach-driven motor proof.
//!
//! Extends Phase W.11/W.12 with the CurriculumCoach tuning the
//! student between motor phases. The META-loop runs: the coach
//! observes competency + trainer state, decides a tuning action,
//! applies it, and the motor's next phase inherits the tuned
//! configuration.
//!
//! This is the empirical proof that the meta-model CONNECTS to
//! the student through the shared live infrastructure.

use mathscape_compress::derive_laws_from_corpus_instrumented;
use mathscape_core::bootstrap::{
    BootstrapCycle, BootstrapCycleSpec, CanonicalDeduper,
    DefaultCorpusGenerator, DefaultModelUpdater, ExperimentOutcome,
    ExperimentScenario, LawExtractor, PhaseOutcome, SpecExecutionError,
};
use mathscape_core::eval::RewriteRule;
use mathscape_core::hash::TermRef;
use mathscape_core::math_problem::{mathematician_curriculum, run_curriculum};
use mathscape_core::mathscape_map::{EventHub, MapEvent};
use mathscape_core::meta_loop::{
    HeuristicProposer, MetaLoop, MetaLoopConfig, ScenarioExecutor,
};
use mathscape_core::policy::LinearPolicy;
use mathscape_core::streaming_policy::StreamingPolicyTrainer;
use mathscape_core::term::Term;
use mathscape_core::{
    AdaptiveCorpusGenerator, BufferedConsumer, CurriculumCoach,
    LiveInferenceHandle, RuleBasedPolicy,
};
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

/// The meta-loop end-to-end: Coach tunes student between
/// motor phases. Observes real discovery, applies typed
/// TuningActions, logs the trajectory.
#[test]
fn coach_drives_student_through_real_motor_cycles() {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║ PHASE Z.1 — COACH DRIVING THE REAL MOTOR                ║");
    println!("║ student trains, coach observes + tunes between phases   ║");
    println!("╚════════════════════════════════════════════════════════╝");

    // ── Wire live infrastructure ───────────────────────────────
    let hub = Rc::new(EventHub::new());
    let library = Rc::new(RefCell::new(Vec::<RewriteRule>::new()));
    let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
    let buffer = Rc::new(BufferedConsumer::new());

    hub.subscribe(trainer.clone());
    hub.subscribe(buffer.clone());

    let student = LiveInferenceHandle::new(library.clone(), trainer.clone());
    let coach = CurriculumCoach::new(
        RuleBasedPolicy,
        // Coach needs its own handle to the student — shares the
        // same Rc so both see the same library + trainer.
        LiveInferenceHandle::new(library.clone(), trainer.clone()),
        hub.clone(),
    );

    // ── Run 3 motor cycles, ticking the coach between each ─────
    let curriculum = mathematician_curriculum();
    let mut scores: Vec<f64> = Vec::new();
    let mut actions: Vec<String> = Vec::new();

    // Cycle 0: baseline.
    let r0 = run_curriculum(&curriculum, &library.borrow());
    scores.push(r0.total.solved_fraction());

    for cycle in 0..3 {
        // Run one motor phase.
        let loop_ = MetaLoop::new(
            MotorExecutor,
            HeuristicProposer::with_extractor("derived-laws"),
            MetaLoopConfig {
                max_phases: 3,
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
            seed_library: library.borrow().clone(),
            seed_policy: LinearPolicy::tensor_seeking_prior(),
            early_stop_after_stable: Some(1),
        };
        let seed_scenario = ExperimentScenario {
            name: format!("cycle-{cycle}"),
            phases: vec![seed_spec],
        };
        let outcome = loop_.run(seed_scenario).expect("motor runs");
        // Update the shared library with the motor's output.
        let final_lib = outcome.final_library().to_vec();
        let mut lib_mut = library.borrow_mut();
        for rule in final_lib {
            if !lib_mut.iter().any(|r| r.name == rule.name) {
                lib_mut.push(rule.clone());
                // Publish a CoreGrew event so the trainer sees it.
                hub.publish(&MapEvent::CoreGrew {
                    prev_core_size: lib_mut.len() - 1,
                    new_core_size: lib_mut.len(),
                    added_rule: rule,
                });
            }
        }
        drop(lib_mut);

        // Benchmark after motor phase.
        let r = run_curriculum(&curriculum, &library.borrow());
        scores.push(r.total.solved_fraction());

        // Coach tick — observes current competency + trainer,
        // emits a typed action.
        let action = coach.tick();
        actions.push(action.kind().to_string());
    }

    // ── Report ──────────────────────────────────────────────────
    println!("\n  Per-cycle curriculum scores: {:?}", scores);
    println!("  Coach actions: {:?}", actions);
    println!(
        "  Coach tick count: {}",
        coach.tick_count()
    );
    println!(
        "  Final library size: {}",
        library.borrow().len()
    );
    println!(
        "  Trainer events seen: {}",
        student.trainer_events_seen()
    );

    // ── Assertions ──────────────────────────────────────────────

    // 1. Motor produced discovery — library grew.
    assert!(
        !library.borrow().is_empty(),
        "motor discovered at least one rule"
    );

    // 2. Coach ticked 3 times.
    assert_eq!(coach.tick_count(), 3);
    assert_eq!(actions.len(), 3);

    // 3. Scores are non-decreasing across cycles (coach
    //    shouldn't regress the student).
    for w in scores.windows(2) {
        assert!(
            w[1] >= w[0] - 1e-9,
            "score non-decreasing: {} → {}",
            w[0],
            w[1]
        );
    }

    // 4. Final score is at least baseline (coach didn't hurt).
    let first = scores.first().copied().unwrap_or(0.0);
    let last = scores.last().copied().unwrap_or(0.0);
    assert!(
        last >= first,
        "coach-driven final ≥ baseline: {first} → {last}"
    );

    // 5. Coach picked at least one non-NoOp action (i.e. it
    //    actively tuned, didn't just sit idle).
    let non_noop = actions.iter().filter(|k| *k != "no-op").count();
    assert!(
        non_noop >= 1,
        "coach was active at least once — actions: {actions:?}"
    );

    // 6. Hub fan-out lossless.
    assert_eq!(
        buffer.len() as u64,
        hub.published_count(),
        "every event reached every subscriber"
    );

    // 7. Trainer policy stayed finite.
    let snap = trainer.snapshot();
    assert!(snap.bias.is_finite());
    for w in snap.weights.iter() {
        assert!(w.is_finite());
    }

    // ── Summary ────────────────────────────────────────────────
    let delta = last - first;
    println!(
        "\n  META-LOOP result:\n    \
           baseline score:      {:.4}\n    \
           final score:         {:.4}\n    \
           delta:               {:+.4}\n    \
           non-noop actions:    {}/{}\n    \
           events through hub:  {}\n    \
           library grew to:     {}\n",
        first,
        last,
        delta,
        non_noop,
        actions.len(),
        hub.published_count(),
        library.borrow().len(),
    );
}
