//! Theory invariants — properties the core theory *claims* must
//! hold, asserted over real state. These tests are the foundation
//! on which "repeatedly and knowably" rests: if any of these fail,
//! the machinery has drifted from its theoretical specification.
//!
//! See `docs/arch/collapse-and-surprise.md`,
//! `docs/arch/axiomatization-pressure.md`, and
//! `docs/arch/knowability-criterion.md` for the claims being
//! tested here.
//!
//! The invariants grouped by category:
//!
//!   - **Determinism** — same input produces same output
//!   - **Monotonicity** — reinforcement never increases pressure;
//!     registry is append-only
//!   - **Conservation** — ΔDL is non-negative on accept/collapse
//!   - **Partition** — events fall into categories consistent with
//!     the action that produced them
//!   - **Idempotence** — running reinforcement twice on already-
//!     reduced state produces no new events
//!   - **Policy-relativity** — reduction verdict depends on the
//!     policy that emitted it

use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_core::{
    control::{Allocator, EpochAction, RealizationPolicy, RewardEstimator},
    epoch::{
        AcceptanceCertificate, Artifact, Epoch, InMemoryRegistry, Registry, RuleEmitter,
    },
    event::{Event, EventCategory},
    eval::RewriteRule,
    lifecycle::ProofStatus,
    reduction::{
        check_reduction, reduction_pressure, ReductionPolicy, ReductionVerdict,
    },
    term::Term,
    value::Value,
};
use mathscape_reward::{reward::RewardConfig, StatisticalProver};

fn apply(f: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(f), args)
}
fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}
fn var(id: u32) -> Term {
    Term::Var(id)
}

fn axiomatized(lhs: Term, rhs: Term, name: &str) -> Artifact {
    let mut cert = AcceptanceCertificate::trivial_conjecture(1.0);
    cert.status = ProofStatus::Axiomatized;
    Artifact::seal(
        RewriteRule {
            name: name.into(),
            lhs,
            rhs,
        },
        0,
        cert,
        vec![],
    )
}

fn corpus() -> Vec<Term> {
    vec![
        apply(var(2), vec![nat(3), nat(0)]),
        apply(var(2), vec![nat(5), nat(0)]),
        apply(var(2), vec![nat(7), nat(0)]),
        apply(var(2), vec![nat(11), nat(0)]),
    ]
}

fn build_epoch(
) -> Epoch<CompressionGenerator, StatisticalProver, RuleEmitter, InMemoryRegistry> {
    Epoch::new(
        CompressionGenerator::new(ExtractConfig::default(), 1),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        InMemoryRegistry::new(),
    )
}

// ══════════════════════════════════════════════════════════════════
// DETERMINISM — same inputs always produce same outputs
// ══════════════════════════════════════════════════════════════════

#[test]
fn invariant_registry_root_is_deterministic_over_insertion_order() {
    let mut a = InMemoryRegistry::new();
    let mut b = InMemoryRegistry::new();
    let r1 = axiomatized(Term::Symbol(1, vec![]), nat(1), "a");
    let r2 = axiomatized(Term::Symbol(2, vec![]), nat(2), "b");
    let r3 = axiomatized(Term::Symbol(3, vec![]), nat(3), "c");
    a.insert(r1.clone());
    a.insert(r2.clone());
    a.insert(r3.clone());
    b.insert(r3);
    b.insert(r1);
    b.insert(r2);
    assert_eq!(a.root(), b.root(), "registry root must be insertion-order independent");
}

#[test]
fn invariant_same_epoch_inputs_produce_same_registry_root() {
    // Run two epochs with identical inputs; registry root must match.
    let c = corpus();
    let mut a = build_epoch();
    let mut b = build_epoch();
    for _ in 0..5 {
        a.step_with_action(&c, EpochAction::Discover);
        b.step_with_action(&c, EpochAction::Discover);
    }
    assert_eq!(a.registry.root(), b.registry.root());
    assert_eq!(a.epoch_id, b.epoch_id);
}

// ══════════════════════════════════════════════════════════════════
// MONOTONICITY — pressure never rises under reinforcement; registry
// never shrinks
// ══════════════════════════════════════════════════════════════════

#[test]
fn invariant_reinforcement_never_raises_pressure() {
    let mut reg = InMemoryRegistry::new();
    reg.insert(axiomatized(
        apply(var(2), vec![var(100), nat(0)]),
        var(100),
        "g",
    ));
    for i in 0..5 {
        reg.insert(axiomatized(
            apply(var(2), vec![nat(i as u64 + 1), nat(0)]),
            nat(i as u64 + 1),
            &format!("s{i}"),
        ));
    }
    let before = reduction_pressure(&reg);
    let mut epoch = Epoch::new(
        CompressionGenerator::new(ExtractConfig::default(), 1),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        reg,
    );
    epoch.step_with_action(&[], EpochAction::Reinforce);
    let after = reduction_pressure(&epoch.registry);
    assert!(
        after <= before,
        "pressure must never rise under reinforcement: {before} -> {after}"
    );
}

#[test]
fn invariant_registry_is_append_only_under_discover() {
    let c = corpus();
    let mut epoch = build_epoch();
    let mut prev_len = 0;
    for _ in 0..5 {
        epoch.step_with_action(&c, EpochAction::Discover);
        let now = epoch.registry.len();
        assert!(now >= prev_len, "registry shrank under Discover: {prev_len} -> {now}");
        prev_len = now;
    }
}

#[test]
fn invariant_reinforcement_does_not_shrink_registry() {
    let mut reg = InMemoryRegistry::new();
    reg.insert(axiomatized(
        apply(var(2), vec![var(100), nat(0)]),
        var(100),
        "g",
    ));
    reg.insert(axiomatized(
        apply(var(2), vec![nat(42), nat(0)]),
        nat(42),
        "s",
    ));
    let before_len = reg.len();
    let mut epoch = Epoch::new(
        CompressionGenerator::new(ExtractConfig::default(), 1),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        reg,
    );
    epoch.step_with_action(&[], EpochAction::Reinforce);
    // Reinforcement marks-subsumed but does not remove from registry.
    assert_eq!(epoch.registry.len(), before_len);
}

// ══════════════════════════════════════════════════════════════════
// CONSERVATION — ΔDL non-negativity on accept/collapse events
// ══════════════════════════════════════════════════════════════════

#[test]
fn invariant_delta_dl_is_non_negative_on_accept() {
    let c = corpus();
    let mut epoch = build_epoch();
    let trace = epoch.step_with_action(&c, EpochAction::Discover);
    for ev in &trace.events {
        if let Event::Accept { delta_dl, .. } = ev {
            assert!(*delta_dl >= 0.0, "Accept event has negative ΔDL: {delta_dl}");
        }
    }
}

#[test]
fn invariant_delta_dl_is_positive_on_subsumption() {
    let mut reg = InMemoryRegistry::new();
    reg.insert(axiomatized(
        apply(var(2), vec![var(100), nat(0)]),
        var(100),
        "g",
    ));
    reg.insert(axiomatized(
        apply(var(2), vec![nat(42), nat(0)]),
        nat(42),
        "s",
    ));
    let mut epoch = Epoch::new(
        CompressionGenerator::new(ExtractConfig::default(), 1),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        reg,
    );
    let trace = epoch.step_with_action(&[], EpochAction::Reinforce);
    let subsumptions: Vec<_> = trace
        .events
        .iter()
        .filter_map(|e| {
            if let Event::Subsumption { delta_dl, .. } = e {
                Some(*delta_dl)
            } else {
                None
            }
        })
        .collect();
    assert!(!subsumptions.is_empty(), "expected at least one subsumption");
    for dl in &subsumptions {
        assert!(*dl > 0.0, "Subsumption ΔDL must be positive, got {dl}");
    }
}

// ══════════════════════════════════════════════════════════════════
// PARTITION — event categories align with the action that produced
// them
// ══════════════════════════════════════════════════════════════════

#[test]
fn invariant_discover_action_emits_only_discovery_events() {
    let c = corpus();
    let mut epoch = build_epoch();
    let trace = epoch.step_with_action(&c, EpochAction::Discover);
    for ev in &trace.events {
        assert_eq!(
            ev.category(),
            EventCategory::Discovery,
            "Discover action emitted non-Discovery event: {ev:?}"
        );
    }
}

#[test]
fn invariant_reinforce_action_emits_only_reinforce_events() {
    let mut reg = InMemoryRegistry::new();
    reg.insert(axiomatized(
        apply(var(2), vec![var(100), nat(0)]),
        var(100),
        "g",
    ));
    reg.insert(axiomatized(
        apply(var(2), vec![nat(42), nat(0)]),
        nat(42),
        "s",
    ));
    let mut epoch = Epoch::new(
        CompressionGenerator::new(ExtractConfig::default(), 1),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        reg,
    );
    let trace = epoch.step_with_action(&[], EpochAction::Reinforce);
    for ev in &trace.events {
        assert_eq!(
            ev.category(),
            EventCategory::Reinforce,
            "Reinforce action emitted non-Reinforce event: {ev:?}"
        );
    }
}

// ══════════════════════════════════════════════════════════════════
// IDEMPOTENCE — collapse already-resolved does nothing
// ══════════════════════════════════════════════════════════════════

#[test]
fn invariant_reinforcement_is_idempotent_when_reduced() {
    let mut reg = InMemoryRegistry::new();
    reg.insert(axiomatized(
        Term::Symbol(1, vec![]),
        nat(1),
        "a",
    ));
    reg.insert(axiomatized(
        Term::Symbol(2, vec![]),
        nat(2),
        "b",
    ));
    let mut epoch = Epoch::new(
        CompressionGenerator::new(ExtractConfig::default(), 1),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        reg,
    );
    let root_before = epoch.registry.root();
    let trace1 = epoch.step_with_action(&[], EpochAction::Reinforce);
    let trace2 = epoch.step_with_action(&[], EpochAction::Reinforce);
    // No subsumable pairs → no Subsumption events either time.
    let events1 = trace1.events.len();
    let events2 = trace2.events.len();
    assert_eq!(events1, 0, "reinforce on already-reduced emitted events: {events1}");
    assert_eq!(events2, 0);
    assert_eq!(
        epoch.registry.root(),
        root_before,
        "reinforce on already-reduced mutated the registry root"
    );
}

// ══════════════════════════════════════════════════════════════════
// POLICY-RELATIVITY — verdict depends on the policy
// ══════════════════════════════════════════════════════════════════

#[test]
fn invariant_policy_stricter_never_gives_fewer_barriers() {
    // Seed: one Verified rule (below Axiomatized). Under
    // layer_0_default (ceiling=Axiomatized) it's a barrier; under
    // a policy with ceiling=Verified it is not.
    let mut reg = InMemoryRegistry::new();
    let mut cert = AcceptanceCertificate::trivial_conjecture(1.0);
    cert.status = ProofStatus::Verified;
    reg.insert(Artifact::seal(
        RewriteRule {
            name: "r".into(),
            lhs: Term::Symbol(1, vec![]),
            rhs: nat(1),
        },
        0,
        cert,
        vec![],
    ));

    let default = ReductionPolicy::layer_0_default();
    let mut loose = ReductionPolicy::layer_0_default();
    loose.advance_ceiling = ProofStatus::Verified; // Verified OK

    let v_default = check_reduction(&reg, &default);
    let v_loose = check_reduction(&reg, &loose);

    // Default policy is stricter — has a barrier.
    assert!(matches!(v_default, ReductionVerdict::Barriers(_)));
    // Loose policy ceiling = Verified — no barrier.
    assert!(v_loose.is_reduced());
}

// ══════════════════════════════════════════════════════════════════
// ALLOCATOR — same state → same action
// ══════════════════════════════════════════════════════════════════

#[test]
fn invariant_allocator_is_deterministic() {
    let policy = RealizationPolicy::default();
    let est = RewardEstimator::new(0.3);
    let alloc = Allocator::new(policy.clone(), est.clone());
    let a = alloc.choose(10, 5);
    let b = alloc.choose(10, 5);
    assert_eq!(
        format!("{a:?}"),
        format!("{b:?}"),
        "allocator chose different actions for identical inputs"
    );
}

#[test]
fn invariant_empty_library_forces_discovery() {
    let policy = RealizationPolicy::default();
    let est = RewardEstimator::new(0.3);
    let alloc = Allocator::new(policy, est);
    // Library size = 0 → nothing to reinforce → Discover.
    let action = alloc.choose(10, 0);
    assert!(matches!(action, EpochAction::Discover));
}

// ══════════════════════════════════════════════════════════════════
// COMPOSED — running the machine respects all of the above
// simultaneously
// ══════════════════════════════════════════════════════════════════

#[test]
fn invariant_composed_trajectory_respects_all_invariants() {
    let c = corpus();
    let mut epoch = build_epoch();
    let mut alloc = Allocator::new(
        RealizationPolicy::default(),
        RewardEstimator::new(0.3),
    );

    let mut prev_lib_size = 0;
    let mut prev_root = epoch.registry.root();
    for i in 0..20 {
        let action_before_call = epoch.registry.len();
        let pressure_before = reduction_pressure(&epoch.registry);
        let trace = epoch.step_auto(&c, &mut alloc);

        // Registry monotone.
        assert!(epoch.registry.len() >= prev_lib_size, "registry shrank at epoch {i}");

        // Event category matches action.
        if let Some(act) = &trace.action {
            for ev in &trace.events {
                match (act, ev.category()) {
                    (EpochAction::Discover, EventCategory::Discovery) => {}
                    (EpochAction::Reinforce, EventCategory::Reinforce) => {}
                    (EpochAction::Promote(_), EventCategory::Promote) => {}
                    (EpochAction::Migrate(_), EventCategory::Promote) => {}
                    (a, c) => panic!(
                        "event category {:?} does not match action {:?} at epoch {i}",
                        c, a
                    ),
                }
            }
        }

        // Reinforce never raises pressure.
        if matches!(trace.action, Some(EpochAction::Reinforce)) {
            let after = reduction_pressure(&epoch.registry);
            assert!(
                after <= pressure_before,
                "reinforce raised pressure at epoch {i}: {pressure_before} -> {after}"
            );
        }

        // ΔDL non-negative on every Accept / Subsumption / Merge / Canonicalize event.
        for ev in &trace.events {
            let dl = ev.delta_dl();
            match ev {
                Event::Accept { .. }
                | Event::Subsumption { .. }
                | Event::Merge { .. }
                | Event::Canonicalize { .. } => {
                    assert!(dl >= 0.0, "negative ΔDL at epoch {i}: {dl} in {ev:?}");
                }
                _ => {}
            }
        }

        // If no work happened, root is unchanged.
        if action_before_call == epoch.registry.len() && trace.events.is_empty() {
            assert_eq!(epoch.registry.root(), prev_root);
        }

        prev_lib_size = epoch.registry.len();
        prev_root = epoch.registry.root();
    }

    // After 20 epochs, something measurable happened.
    assert!(epoch.epoch_id >= 20);
}

// ══════════════════════════════════════════════════════════════════
// REPEATABILITY — the core promise we build from
// ══════════════════════════════════════════════════════════════════

#[test]
fn invariant_fifty_epoch_trajectory_is_bit_identical_on_replay() {
    let c = corpus();
    fn run_once(c: &[Term]) -> (u64, mathscape_core::hash::TermRef) {
        let mut epoch = build_epoch();
        let mut alloc = Allocator::new(
            RealizationPolicy::default(),
            RewardEstimator::new(0.3),
        );
        for _ in 0..50 {
            epoch.step_auto(c, &mut alloc);
        }
        (epoch.epoch_id, epoch.registry.root())
    }
    let run_a = run_once(&c);
    let run_b = run_once(&c);
    assert_eq!(run_a.0, run_b.0, "epoch_id diverged across identical runs");
    assert_eq!(
        run_a.1, run_b.1,
        "registry root diverged across identical runs — knowability #2 violated"
    );
}
