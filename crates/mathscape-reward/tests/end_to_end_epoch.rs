//! End-to-end integration test for Phase C: wires the real
//! `CompressionGenerator` + `StatisticalProver` + `RuleEmitter` +
//! `InMemoryRegistry` into a real `Epoch` and runs discovery passes
//! over a handcrafted corpus.
//!
//! Proves that the Phase B/C type cascade actually composes.

use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_core::{
    control::EpochAction,
    epoch::{Epoch, InMemoryRegistry, Registry, RuleEmitter},
    event::{Event, EventCategory},
    test_helpers::{apply, nat, var},
    term::Term,
};
use mathscape_reward::{reward::RewardConfig, StatisticalProver};

fn build_epoch() -> Epoch<CompressionGenerator, StatisticalProver, RuleEmitter, InMemoryRegistry> {
    Epoch::new(
        CompressionGenerator::new(
            ExtractConfig {
                min_shared_size: 2,
                min_matches: 2,
                max_new_rules: 5,
            },
            1,
        ),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        InMemoryRegistry::new(),
    )
}

fn corpus_with_shared_structure() -> Vec<Term> {
    // Every term is add(?x, zero) — strong shared structure, the
    // anti-unifier should produce `add(?a, zero) => ?a`.
    vec![
        apply(var(2), vec![nat(5), nat(0)]),
        apply(var(2), vec![nat(3), nat(0)]),
        apply(var(2), vec![nat(7), nat(0)]),
        apply(var(2), vec![nat(1), nat(0)]),
    ]
}

#[test]
fn single_epoch_advances_epoch_id() {
    let mut epoch = build_epoch();
    assert_eq!(epoch.epoch_id, 0);
    let _trace = epoch.step(&corpus_with_shared_structure());
    assert_eq!(epoch.epoch_id, 1);
}

#[test]
fn single_epoch_populates_trace() {
    let mut epoch = build_epoch();
    let corpus = corpus_with_shared_structure();
    let trace = epoch.step(&corpus);

    // Trace should include the dispatch action.
    assert_eq!(trace.action, Some(EpochAction::Discover));
    // Trace events should include at least one Proposal.
    assert!(
        trace
            .events
            .iter()
            .any(|e| matches!(e, Event::Proposal { .. })),
        "expected at least one Proposal event"
    );
    // Events should only belong to the Discovery category.
    assert!(
        trace.events.iter().all(|e| e.category() == EventCategory::Discovery),
        "Phase C discovery-only dispatch produced non-Discovery events"
    );
    // Proposal count matches the counter.
    let proposal_count = trace
        .events
        .iter()
        .filter(|e| matches!(e, Event::Proposal { .. }))
        .count();
    assert_eq!(proposal_count, trace.proposals);
}

#[test]
fn accepted_candidates_land_as_artifacts() {
    let mut epoch = build_epoch();
    let trace = epoch.step(&corpus_with_shared_structure());
    if trace.accepted > 0 {
        assert_eq!(epoch.registry.len(), trace.accepted);
        // Every Accept event's artifact hash should be in the trace's
        // artifact_hashes list.
        for ev in &trace.events {
            if let Event::Accept { artifact, .. } = ev {
                assert!(
                    trace.artifact_hashes.contains(&artifact.content_hash),
                    "artifact_hash not in trace.artifact_hashes"
                );
            }
        }
    }
}

#[test]
fn registry_grows_monotonically_across_epochs() {
    let mut epoch = build_epoch();
    let corpus = corpus_with_shared_structure();
    let mut prev_len = 0;
    for _ in 0..3 {
        epoch.step(&corpus);
        assert!(
            epoch.registry.len() >= prev_len,
            "registry shrunk between epochs"
        );
        prev_len = epoch.registry.len();
    }
}

#[test]
fn trace_total_delta_dl_equals_sum_of_events() {
    let mut epoch = build_epoch();
    let trace = epoch.step(&corpus_with_shared_structure());
    let manual: f64 = trace.events.iter().map(Event::delta_dl).sum();
    let reported = trace.total_delta_dl();
    assert!((manual - reported).abs() < 1e-9);
}

#[test]
fn rejecting_prover_produces_reject_events_no_artifacts() {
    let mut epoch = Epoch::new(
        CompressionGenerator::new(
            ExtractConfig {
                min_shared_size: 2,
                min_matches: 2,
                max_new_rules: 5,
            },
            1,
        ),
        // threshold so high nothing accepts
        StatisticalProver::new(RewardConfig::default(), 1e9),
        RuleEmitter,
        InMemoryRegistry::new(),
    );
    let trace = epoch.step(&corpus_with_shared_structure());
    assert_eq!(trace.accepted, 0);
    assert_eq!(epoch.registry.len(), 0);
    if trace.proposals > 0 {
        assert!(
            trace
                .events
                .iter()
                .any(|e| matches!(e, Event::Reject { .. })),
            "expected at least one Reject event when proposals exist"
        );
    }
}
