//! Integration test: the full pressure → collapse → release cycle.
//!
//! Seeds a registry with pairwise-subsumable rules, measures the
//! pressure leading indicator, runs the real reinforcement pass,
//! confirms Subsumption events fire, verifies the subsumed rules
//! are marked in the registry overlay, and confirms pressure drops
//! to zero after the collapse.
//!
//! This is the first end-to-end test of the phase-transition model
//! described in docs/arch/collapse-and-surprise.md.

use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_core::{
    control::EpochAction,
    epoch::{
        AcceptanceCertificate, Artifact, Epoch, InMemoryRegistry, Registry, RuleEmitter,
    },
    event::{Event, EventCategory},
    eval::RewriteRule,
    lifecycle::ProofStatus,
    reduction::{check_maximally_reduced, detect_subsumption_pairs, reduction_pressure},
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

fn mk_axiomatized(name: &str, lhs: Term, rhs: Term) -> Artifact {
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

fn build_epoch(
    reg: InMemoryRegistry,
) -> Epoch<CompressionGenerator, StatisticalProver, RuleEmitter, InMemoryRegistry> {
    Epoch::new(
        CompressionGenerator::new(ExtractConfig::default(), 1),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        reg,
    )
}

#[test]
fn pressure_rises_then_collapses_under_reinforcement() {
    // General rule + specific rule. The general rule subsumes the
    // specific. Both at Axiomatized so status advancement is not
    // the barrier — only subsumption is.
    let mut reg = InMemoryRegistry::new();
    reg.insert(mk_axiomatized(
        "add-identity",
        apply(var(2), vec![var(100), nat(0)]),
        var(100),
    ));
    reg.insert(mk_axiomatized(
        "add-42-zero",
        apply(var(2), vec![nat(42), nat(0)]),
        nat(42),
    ));
    reg.insert(mk_axiomatized(
        "add-99-zero",
        apply(var(2), vec![nat(99), nat(0)]),
        nat(99),
    ));

    // Before reinforcement: pressure should be non-zero; both
    // specific rules are subsumed by the general one.
    let pressure_before = reduction_pressure(&reg);
    assert!(
        pressure_before > 0.0,
        "expected pressure > 0 before reinforcement, got {pressure_before}"
    );

    let pairs = detect_subsumption_pairs(&reg);
    assert_eq!(
        pairs.len(),
        2,
        "expected 2 subsumption pairs (add-identity subsumes 2 specifics), got {}",
        pairs.len()
    );

    // Run reinforcement pass. Real work now fires.
    let mut epoch = build_epoch(reg);
    let trace = epoch.step_with_action(&[], EpochAction::Reinforce);

    // After reinforcement: action was Reinforce, 2 Subsumption
    // events fired, each with non-zero ΔDL.
    assert_eq!(trace.action, Some(EpochAction::Reinforce));
    let subsumption_events: Vec<_> = trace
        .events
        .iter()
        .filter(|e| matches!(e, Event::Subsumption { .. }))
        .collect();
    assert_eq!(subsumption_events.len(), 2);
    for ev in &subsumption_events {
        if let Event::Subsumption { delta_dl, .. } = ev {
            assert!(*delta_dl > 0.0, "Subsumption ΔDL should be positive");
        }
    }
    // All events are Reinforce category.
    assert!(
        trace.events.iter().all(|e| e.category() == EventCategory::Reinforce),
        "all events should be Reinforce category"
    );

    // After reinforcement: pressure drops to zero (no more
    // subsumable pairs in the active set).
    let pressure_after = reduction_pressure(&epoch.registry);
    assert_eq!(
        pressure_after, 0.0,
        "pressure should be 0 after reinforcement, got {pressure_after}"
    );

    // The subsumer (add-identity) is still Axiomatized and active;
    // the specifics are Subsumed.
    let identity_hash = epoch.registry.all()[0].content_hash; // insertion order preserved
    let status = epoch.registry.status_of(identity_hash).unwrap();
    assert!(
        matches!(status, ProofStatus::Axiomatized),
        "add-identity should stay Axiomatized, got {status:?}"
    );

    // Verify: check_maximally_reduced now returns Reduced.
    let verdict = check_maximally_reduced(&epoch.registry);
    assert!(
        verdict.is_reduced(),
        "after collapse, library should be maximally reduced under layer_0_default; got {verdict:?}"
    );
}

#[test]
fn reinforcement_is_idempotent() {
    // Running reinforcement twice should not fire events the
    // second time (pressure already zero after first pass).
    let mut reg = InMemoryRegistry::new();
    reg.insert(mk_axiomatized(
        "a",
        apply(var(2), vec![var(100), nat(0)]),
        var(100),
    ));
    reg.insert(mk_axiomatized(
        "b",
        apply(var(2), vec![nat(5), nat(0)]),
        nat(5),
    ));

    let mut epoch = build_epoch(reg);
    let trace1 = epoch.step_with_action(&[], EpochAction::Reinforce);
    let first_subsumptions = trace1
        .events
        .iter()
        .filter(|e| matches!(e, Event::Subsumption { .. }))
        .count();
    assert_eq!(first_subsumptions, 1);

    let trace2 = epoch.step_with_action(&[], EpochAction::Reinforce);
    let second_subsumptions = trace2
        .events
        .iter()
        .filter(|e| matches!(e, Event::Subsumption { .. }))
        .count();
    assert_eq!(
        second_subsumptions, 0,
        "second reinforcement should fire no collapse events"
    );
}

#[test]
fn empty_registry_has_zero_pressure() {
    let reg = InMemoryRegistry::new();
    assert_eq!(reduction_pressure(&reg), 0.0);
}

#[test]
fn reduced_library_has_zero_pressure() {
    let mut reg = InMemoryRegistry::new();
    // Two rules with no subsumption relationship.
    reg.insert(mk_axiomatized(
        "a",
        Term::Symbol(1, vec![]),
        nat(1),
    ));
    reg.insert(mk_axiomatized(
        "b",
        Term::Symbol(2, vec![]),
        nat(2),
    ));
    assert_eq!(reduction_pressure(&reg), 0.0);
}

#[test]
fn subsumption_pair_detection_is_deterministic() {
    let mut reg = InMemoryRegistry::new();
    reg.insert(mk_axiomatized(
        "a",
        apply(var(2), vec![var(100), nat(0)]),
        var(100),
    ));
    reg.insert(mk_axiomatized(
        "b",
        apply(var(2), vec![nat(5), nat(0)]),
        nat(5),
    ));
    reg.insert(mk_axiomatized(
        "c",
        apply(var(2), vec![nat(7), nat(0)]),
        nat(7),
    ));
    let pairs1 = detect_subsumption_pairs(&reg);
    let pairs2 = detect_subsumption_pairs(&reg);
    assert_eq!(pairs1, pairs2);
    assert_eq!(pairs1.len(), 2); // a subsumes b, a subsumes c
}
