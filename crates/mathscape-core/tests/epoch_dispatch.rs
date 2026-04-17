//! `Epoch::step_with_action` dispatch tests.
//!
//! The dispatcher routes on EpochAction. This test asserts each
//! branch behaves correctly: Discover produces Discovery events;
//! Reinforce produces nothing (v0 scaffold); Promote/Migrate are
//! no-ops at the epoch level.

use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_core::{
    control::EpochAction,
    epoch::{Epoch, InMemoryRegistry, Registry, RuleEmitter},
    event::EventCategory,
    lifecycle::AxiomIdentity,
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
fn fixed_corpus() -> Vec<Term> {
    vec![
        apply(var(2), vec![nat(3), nat(0)]),
        apply(var(2), vec![nat(5), nat(0)]),
        apply(var(2), vec![nat(7), nat(0)]),
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

#[test]
fn discover_action_emits_discovery_events() {
    let mut e = build_epoch();
    let trace = e.step_with_action(&fixed_corpus(), EpochAction::Discover);
    assert_eq!(trace.action, Some(EpochAction::Discover));
    // At least the proposals that happened should be Discovery events.
    assert!(trace
        .events
        .iter()
        .all(|ev| ev.category() == EventCategory::Discovery));
}

#[test]
fn reinforce_action_is_empty_in_v0() {
    let mut e = build_epoch();
    // Seed library via one discovery epoch.
    e.step_with_action(&fixed_corpus(), EpochAction::Discover);
    let lib_size = e.registry.len();
    let trace = e.step_with_action(&fixed_corpus(), EpochAction::Reinforce);
    assert_eq!(trace.action, Some(EpochAction::Reinforce));
    assert!(trace.events.is_empty());
    // Reinforce does not change library state yet.
    assert_eq!(e.registry.len(), lib_size);
}

#[test]
fn promote_action_is_noop_at_epoch_layer() {
    let mut e = build_epoch();
    let trace = e.step_with_action(
        &fixed_corpus(),
        EpochAction::Promote(mathscape_core::hash::TermRef([0; 32])),
    );
    // No events, no registry change — orchestrator executes.
    assert!(trace.events.is_empty());
    assert!(matches!(trace.action, Some(EpochAction::Promote(_))));
}

#[test]
fn migrate_action_is_noop_at_epoch_layer() {
    let mut e = build_epoch();
    let identity = AxiomIdentity {
        target: "t::T".into(),
        name: "X".into(),
        proposal_hash: mathscape_core::hash::TermRef([0; 32]),
    };
    let trace =
        e.step_with_action(&fixed_corpus(), EpochAction::Migrate(identity.clone()));
    assert!(trace.events.is_empty());
    assert!(matches!(trace.action, Some(EpochAction::Migrate(_))));
}

#[test]
fn epoch_id_advances_under_every_action() {
    let mut e = build_epoch();
    assert_eq!(e.epoch_id, 0);
    e.step_with_action(&fixed_corpus(), EpochAction::Discover);
    assert_eq!(e.epoch_id, 1);
    e.step_with_action(&fixed_corpus(), EpochAction::Reinforce);
    assert_eq!(e.epoch_id, 2);
    e.step_with_action(
        &fixed_corpus(),
        EpochAction::Promote(mathscape_core::hash::TermRef([0; 32])),
    );
    assert_eq!(e.epoch_id, 3);
}

#[test]
fn step_defaults_to_discover_action() {
    let mut e = build_epoch();
    let trace = e.step(&fixed_corpus());
    assert_eq!(trace.action, Some(EpochAction::Discover));
}
