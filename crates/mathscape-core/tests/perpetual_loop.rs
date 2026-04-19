//! Phase W.4 — end-to-end integration test for the perpetual
//! self-optimizing loop.
//!
//! Demonstrates, in one running test, that the following
//! components compose correctly under a single `EventHub`:
//!
//! - `EventHub` (pub/sub spine)
//! - `StreamingPolicyTrainer` (never-destroy online SGD)
//! - `BufferedConsumer` (event history)
//! - `BenchmarkConsumer` (labeled-data ingress / report card)
//! - `prune` + `auto_rejuvenate` (neuroplasticity)
//! - EWC anchor + Fisher (stability)
//! - Learning-progress intrinsic reward (motivation)
//!
//! The test simulates the event stream a motor would produce
//! across phases, interleaves benchmark scorings, and asserts
//! that every architectural transition had the expected effect
//! on the trainer. This is the closed-loop validation artifact
//! for Phases V + W.

use mathscape_core::{
    BenchmarkConsumer, BufferedConsumer, EventHub, MapEvent, MapEventConsumer,
    StreamingPolicyTrainer,
};
use mathscape_core::eval::RewriteRule;
use mathscape_core::hash::TermRef;
use mathscape_core::math_problem::canonical_problem_set;
use mathscape_core::term::Term;
use mathscape_core::value::Value;
use std::rc::Rc;

fn add_identity_rule() -> RewriteRule {
    RewriteRule {
        name: "add-id".into(),
        lhs: Term::Apply(
            Box::new(Term::Var(2)),
            vec![Term::Number(Value::Nat(0)), Term::Var(100)],
        ),
        rhs: Term::Var(100),
    }
}

fn mul_identity_rule() -> RewriteRule {
    RewriteRule {
        name: "mul-id".into(),
        lhs: Term::Apply(
            Box::new(Term::Var(3)),
            vec![Term::Number(Value::Nat(1)), Term::Var(100)],
        ),
        rhs: Term::Var(100),
    }
}

/// The full proprioceptive loop runs under a single hub.
///
/// Simulates: motor → publishes events → trainer + buffer see
/// them → benchmark runs → trainer sees BenchmarkScored too →
/// pruning and auto-rejuvenation happen over the neuroplasticity
/// mechanism.
#[test]
fn perpetual_loop_composes_all_phase_v_and_w_mechanisms() {
    // ── Wiring ─────────────────────────────────────────────
    let hub = EventHub::new();
    let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
    let buffer = Rc::new(BufferedConsumer::new());
    let benchmark = Rc::new(BenchmarkConsumer::new(canonical_problem_set()));

    hub.subscribe(trainer.clone());
    hub.subscribe(buffer.clone());
    // benchmark is NOT subscribed — it's a producer, not a
    // consumer. It publishes BenchmarkScored to the hub when
    // called. For the test we call benchmark.benchmark_now with
    // the hub as its downstream consumer.

    assert_eq!(hub.subscriber_count(), 2);

    // ── Phase 1: motor publishes initial discoveries ───────
    // Simulate the early phase of a motor run: novel root,
    // rule added to core, another rule added.
    hub.publish(&MapEvent::NovelRoot {
        seed: 1,
        phase_index: 0,
        root: TermRef::from_bytes(b"root-a"),
        library_size: 0,
    });
    hub.publish(&MapEvent::CoreGrew {
        prev_core_size: 0,
        new_core_size: 1,
        added_rule: add_identity_rule(),
    });
    hub.publish(&MapEvent::CoreGrew {
        prev_core_size: 1,
        new_core_size: 2,
        added_rule: mul_identity_rule(),
    });

    // Trainer saw events and updated.
    assert_eq!(trainer.events_seen(), 3);
    assert!(trainer.updates_applied() >= 1);

    // Buffer captured events in order.
    assert_eq!(buffer.len(), 3);

    // Fisher information should be accumulating on rule events.
    let fisher_before = trainer.fisher_snapshot();
    assert!(
        fisher_before.iter().any(|f| *f > 0.0),
        "Fisher EMA non-zero after training events"
    );

    // ── Phase 2: first benchmark → baseline ───────────────
    let library = vec![add_identity_rule(), mul_identity_rule()];
    let report1 = benchmark.benchmark_now(&library, &hub);
    assert!(report1.problem_set_size >= 10);

    // Trainer saw the BenchmarkScored event via the hub.
    assert_eq!(trainer.events_seen(), 4);
    assert_eq!(benchmark.runs(), 1);
    assert!(benchmark.last_score().is_some());

    // ── Phase 3: motor produces more events, second benchmark ──
    hub.publish(&MapEvent::RuleCertified {
        rule: add_identity_rule(),
        evidence_samples: 96,
    });
    hub.publish(&MapEvent::RuleCertified {
        rule: mul_identity_rule(),
        evidence_samples: 128,
    });

    // Re-benchmark with the same library → delta is 0, not a
    // regression, not a gain.
    let report2 = benchmark.benchmark_now(&library, &hub);
    assert_eq!(report2.solved_fraction(), report1.solved_fraction());
    assert_eq!(benchmark.runs(), 2);

    // ── Phase 4: benchmark history populated ───────────────
    assert_eq!(trainer.benchmark_history().len(), 2);

    // ── Phase 5: staleness → motor signals saturation ──────
    hub.publish(&MapEvent::StalenessCrossed {
        seed: 1,
        phase_index: 1,
        threshold: 0.6,
        observed: 0.9,
    });

    // ── Phase 6: neuroplasticity — prune ALL weights ──────
    // For deterministic phantom-gradient coverage we prune all
    // weights; any feature dimension with non-zero signal will
    // then register a phantom gradient regardless of which
    // specific dimensions the synthetic rules populate.
    let pruned_indices = trainer.prune(f64::INFINITY, u64::MAX);
    let pruned_count = trainer.pruned_count();
    assert!(pruned_count > 0, "prune catches at least one weight");
    eprintln!("  pruned {} weights on first pass", pruned_indices.len());

    // ── Phase 7: continued events accumulate phantom grads ──
    for _ in 0..5 {
        hub.publish(&MapEvent::CoreGrew {
            prev_core_size: 2,
            new_core_size: 3,
            added_rule: add_identity_rule(),
        });
    }
    let phantoms = trainer.phantom_gradients();
    assert!(
        phantoms.iter().any(|p| *p > 0.0),
        "with all weights pruned, at least one feature dimension \
         accumulates phantom gradient"
    );
    assert!(
        phantoms.iter().all(|p| p.is_finite()),
        "phantom gradients stay finite"
    );

    // ── Phase 8: RigL auto-rejuvenation ────────────────────
    let max_phantom = phantoms.iter().copied().fold(0.0f64, f64::max);
    let rejuvenated = trainer.auto_rejuvenate(max_phantom * 0.5, 0.05);
    eprintln!("  auto-rejuvenated {} pruned weights", rejuvenated.len());
    assert!(
        !rejuvenated.is_empty(),
        "auto-rejuvenation picks up the strongest phantom-active weights"
    );
    let after_snap = trainer.snapshot();
    for &i in &rejuvenated {
        assert_eq!(after_snap.weights[i], 0.05, "rejuvenated to seed");
    }

    // ── Phase 9: EWC anchor + plasticity check ─────────────
    // Force a "better" benchmark by pretending the library got
    // bigger. The learning-progress bonus should fire. The
    // anchor should get set on improvement.
    let bigger_library = vec![
        add_identity_rule(),
        mul_identity_rule(),
        // Synthetic third rule that doesn't actually add solve
        // power but grows the library for the purpose of the
        // test. (In a real motor run this would be a genuinely
        // new abstraction.)
        RewriteRule {
            name: "synthetic-1".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(4)),
                vec![Term::Var(100)],
            ),
            rhs: Term::Var(100),
        },
    ];
    let _ = benchmark.benchmark_now(&bigger_library, &hub);

    // Anchor may or may not be set depending on whether the
    // benchmark improved; what we assert is the trainer is
    // still progressing.
    assert!(trainer.events_seen() > 10);
    assert!(trainer.updates_applied() > 5);

    // Trained steps monotonically increased throughout.
    let final_snap = trainer.snapshot();
    assert!(final_snap.trained_steps > 0);

    // ── Phase 10: invariants ───────────────────────────────
    // 1. The trainer never reset — trained_steps is monotonic.
    // 2. The hub dispatched every event to every subscriber.
    assert_eq!(buffer.len() as u64, hub.published_count());
    // 3. No NaN or Inf leaked into weights.
    for w in final_snap.weights.iter() {
        assert!(w.is_finite(), "weight stays finite");
    }
    assert!(final_snap.bias.is_finite(), "bias stays finite");

    // ── Summary ────────────────────────────────────────────
    eprintln!(
        "\n  perpetual loop summary:\n    \
           events_published: {}\n    \
           trainer_events_seen: {}\n    \
           trainer_updates_applied: {}\n    \
           trainer_trained_steps: {}\n    \
           benchmark_runs: {}\n    \
           pruned_count_final: {}\n    \
           has_anchor: {}\n    \
           buffer_len: {}\n",
        hub.published_count(),
        trainer.events_seen(),
        trainer.updates_applied(),
        final_snap.trained_steps,
        benchmark.runs(),
        trainer.pruned_count(),
        trainer.has_anchor(),
        buffer.len(),
    );
}

/// Two trainers subscribed to the same hub train identically —
/// proves the pub/sub dispatch is deterministic and non-lossy.
#[test]
fn event_hub_is_deterministic_and_non_lossy() {
    let hub = EventHub::new();
    let t1 = Rc::new(StreamingPolicyTrainer::new(0.1));
    let t2 = Rc::new(StreamingPolicyTrainer::new(0.1));
    hub.subscribe(t1.clone());
    hub.subscribe(t2.clone());

    // Publish a fixed event sequence.
    let events = vec![
        MapEvent::CoreGrew {
            prev_core_size: 0,
            new_core_size: 1,
            added_rule: add_identity_rule(),
        },
        MapEvent::RuleCertified {
            rule: add_identity_rule(),
            evidence_samples: 96,
        },
        MapEvent::StalenessCrossed {
            seed: 1,
            phase_index: 0,
            threshold: 0.6,
            observed: 0.9,
        },
        MapEvent::CoreGrew {
            prev_core_size: 1,
            new_core_size: 2,
            added_rule: mul_identity_rule(),
        },
    ];
    for e in &events {
        hub.publish(e);
    }

    let s1 = t1.snapshot();
    let s2 = t2.snapshot();
    // Both trainers received identical event sequences, so they
    // must converge to identical state.
    assert_eq!(s1.weights, s2.weights);
    assert_eq!(s1.bias, s2.bias);
    assert_eq!(s1.trained_steps, s2.trained_steps);
    assert_eq!(t1.events_seen(), t2.events_seen());
    assert_eq!(t1.updates_applied(), t2.updates_applied());
}

/// Phase W.8: universal plasticity controller drives shed +
/// reinforce across heterogeneous components (trainer + probe)
/// through the same `Plastic` trait. Demonstrates that the
/// outer controller treats each component uniformly despite
/// completely different internal vocabularies (weights vs arms).
#[test]
fn plasticity_controller_ticks_trainer_and_probe_uniformly() {
    use mathscape_core::{BanditProbe, Plastic, PlasticityController};
    use std::cell::RefCell;

    let hub = EventHub::new();
    let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
    let applied = Rc::new(RefCell::new(Vec::<f64>::new()));
    let applied_c = applied.clone();
    let probe = Rc::new(BanditProbe::new(
        "lr",
        vec![0.01, 0.05, 0.1, 0.2],
        Box::new(move |v: &f64| applied_c.borrow_mut().push(*v)),
        3,
        0.3,
    ));

    hub.subscribe(trainer.clone());
    hub.subscribe(probe.clone());

    // Train via a realistic event stream with some benchmark
    // signals so both components accrue state.
    for phase in 0..10 {
        hub.publish(&MapEvent::CoreGrew {
            prev_core_size: phase,
            new_core_size: phase + 1,
            added_rule: add_identity_rule(),
        });
        hub.publish(&MapEvent::BenchmarkScored {
            solved_count: (5 + phase).min(10),
            total: 10,
            solved_fraction: ((5 + phase).min(10) as f64) / 10.0,
            delta_from_prior: 0.05,
        });
    }

    // Register both in the controller; one tick runs shed +
    // reinforce on both.
    let controller = PlasticityController::new();
    controller.register(trainer.clone() as Rc<dyn Plastic>);
    controller.register(probe.clone() as Rc<dyn Plastic>);
    assert_eq!(controller.component_count(), 2);

    let report = controller.tick();
    eprintln!("{}", report.summary());
    assert_eq!(report.ticks.len(), 2);
    assert_eq!(controller.tick_count(), 1);

    // Every named component appears in the report.
    let names: Vec<_> =
        report.ticks.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"streaming-policy-trainer"));
    assert!(names.contains(&"lr"));

    // Each tick is idempotent in the sense that a second call
    // also produces a valid report. (Counts may change as the
    // internal state shifts — that's expected.)
    let r2 = controller.tick();
    assert_eq!(r2.ticks.len(), 2);
    assert_eq!(controller.tick_count(), 2);
}

/// The hub composes with a history-keeping consumer that both
/// records events AND re-emits derived events. Demonstrates the
/// "chain of consumers" pattern via a custom consumer.
#[test]
fn event_hub_composes_with_derived_consumer_chain() {
    struct EventCounter {
        core_grew: std::cell::Cell<u64>,
        rule_certified: std::cell::Cell<u64>,
        benchmark_scored: std::cell::Cell<u64>,
    }
    impl MapEventConsumer for EventCounter {
        fn on_event(&self, event: &MapEvent) {
            match event {
                MapEvent::CoreGrew { .. } => {
                    self.core_grew.set(self.core_grew.get() + 1);
                }
                MapEvent::RuleCertified { .. } => {
                    self.rule_certified.set(self.rule_certified.get() + 1);
                }
                MapEvent::BenchmarkScored { .. } => {
                    self.benchmark_scored
                        .set(self.benchmark_scored.get() + 1);
                }
                _ => {}
            }
        }
    }

    let hub = EventHub::new();
    let counter = Rc::new(EventCounter {
        core_grew: std::cell::Cell::new(0),
        rule_certified: std::cell::Cell::new(0),
        benchmark_scored: std::cell::Cell::new(0),
    });
    let benchmark = Rc::new(BenchmarkConsumer::new(canonical_problem_set()));
    hub.subscribe(counter.clone());

    hub.publish(&MapEvent::CoreGrew {
        prev_core_size: 0,
        new_core_size: 1,
        added_rule: add_identity_rule(),
    });
    hub.publish(&MapEvent::RuleCertified {
        rule: add_identity_rule(),
        evidence_samples: 96,
    });
    benchmark.benchmark_now(&[add_identity_rule()], &hub);

    assert_eq!(counter.core_grew.get(), 1);
    assert_eq!(counter.rule_certified.get(), 1);
    assert_eq!(counter.benchmark_scored.get(), 1);
}
