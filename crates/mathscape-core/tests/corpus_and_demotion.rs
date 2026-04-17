//! Phase K + J integration: a CorpusLog accumulates cross-corpus
//! matches; artifacts that accrue evidence become PromotionGate-
//! eligible; artifacts that stop being used become DemotionGate-
//! eligible. Proves the two symmetric forces operate against the
//! same ArtifactHistory.

use mathscape_core::{
    corpus::{CorpusLog, CorpusSnapshot},
    demotion::{demote_artifact, DemotionGate, UsageFloorGate},
    epoch::{AcceptanceCertificate, Artifact, InMemoryRegistry, Registry},
    eval::RewriteRule,
    hash::TermRef,
    lifecycle::{DemotionReason, ProofStatus},
    promotion_gate::{PromotionGate, ThresholdGate},
    term::Term,
    value::Value,
};

fn var(id: u32) -> Term {
    Term::Var(id)
}
fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}
fn apply(f: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(f), args)
}

fn id_rule(sym: u32) -> RewriteRule {
    // add(?x, 0) => ?x  — matches every "add a b" where b == 0
    RewriteRule {
        name: format!("id-{sym}"),
        lhs: apply(var(2), vec![var(100), nat(0)]),
        rhs: var(100),
    }
}

fn arith_corpus(epoch: u64) -> CorpusSnapshot {
    CorpusSnapshot::new(
        "arith",
        vec![
            apply(var(2), vec![nat(3), nat(0)]),
            apply(var(2), vec![nat(7), nat(0)]),
        ],
        epoch,
    )
}

fn diff_corpus(epoch: u64) -> CorpusSnapshot {
    CorpusSnapshot::new(
        "diff",
        vec![
            apply(var(2), vec![nat(1), nat(0)]),
            apply(var(2), vec![nat(42), nat(0)]),
        ],
        epoch,
    )
}

fn mk(sym: u32) -> Artifact {
    Artifact::seal(
        id_rule(sym),
        0,
        AcceptanceCertificate::trivial_conjecture(1.0),
        vec![],
    )
}

#[test]
fn cross_corpus_accumulation_clears_gate_5_through_corpus_log() {
    let mut log = CorpusLog::new();
    let artifact = mk(1);

    // Epoch 0: scan arith
    log.scan_corpus(
        &arith_corpus(0),
        [(artifact.content_hash, artifact.rule.lhs.clone())],
        0,
    );
    // Epoch 1: scan diff
    log.scan_corpus(
        &diff_corpus(1),
        [(artifact.content_hash, artifact.rule.lhs.clone())],
        1,
    );

    let history = log.history_for(artifact.content_hash, 2, 100);
    assert_eq!(history.corpus_matches.len(), 2);

    let gate = ThresholdGate::new(0, 2);
    let signal = gate
        .evaluate(&artifact, &[artifact.clone()], &history, 2)
        .expect("both thresholds cleared: subsumed=0 (k=0), corpora=2 (n=2)");
    assert_eq!(signal.cross_corpus_support.len(), 2);
}

#[test]
fn single_corpus_does_not_clear_gate_5() {
    let mut log = CorpusLog::new();
    let artifact = mk(1);
    for epoch in 0..10 {
        log.scan_corpus(
            &arith_corpus(epoch),
            [(artifact.content_hash, artifact.rule.lhs.clone())],
            epoch,
        );
    }
    let history = log.history_for(artifact.content_hash, 10, 100);
    let gate = ThresholdGate::new(0, 2);
    assert!(gate.evaluate(&artifact, &[], &history, 10).is_none());
}

#[test]
fn unused_artifact_past_grace_period_yields_demotion_candidate() {
    let log = CorpusLog::new();
    let artifact = mk(1);
    // `first_seen_epoch` must be present for epochs_alive > 0.
    let mut log = log;
    log.record_match(artifact.content_hash, &"arith".into(), 0);

    // 100 epochs later, no further matches.
    let history = log.history_for(artifact.content_hash, 100, 30);
    assert!(history.epochs_alive >= 50); // w_grace_period clear
    assert_eq!(history.usage_in_window, 0);

    let gate = UsageFloorGate::new(1, 50);
    let candidate = gate
        .evaluate(
            artifact.content_hash,
            &ProofStatus::Conjectured,
            &history,
            100,
        )
        .expect("unused artifact past grace period should fire");
    assert!(matches!(
        candidate.proposed_reason,
        DemotionReason::StaleConjecture
    ));
}

#[test]
fn recent_usage_prevents_demotion() {
    let mut log = CorpusLog::new();
    let artifact = mk(1);
    log.record_match(artifact.content_hash, &"arith".into(), 0);
    // Recent match at epoch 95
    log.record_match(artifact.content_hash, &"arith".into(), 95);
    let history = log.history_for(artifact.content_hash, 100, 30);
    assert!(history.usage_in_window >= 1);
    let gate = UsageFloorGate::new(1, 50);
    assert!(gate
        .evaluate(
            artifact.content_hash,
            &ProofStatus::Conjectured,
            &history,
            100,
        )
        .is_none());
}

#[test]
fn demotion_closes_an_artifacts_active_lifecycle_in_the_registry() {
    let mut reg = InMemoryRegistry::new();
    let artifact = mk(1);
    let hash = artifact.content_hash;
    reg.insert(artifact);

    // Seed log with one hit then nothing.
    let mut log = CorpusLog::new();
    log.record_match(hash, &"arith".into(), 0);
    let history = log.history_for(hash, 100, 30);

    let gate = UsageFloorGate::new(1, 50);
    let cand = gate
        .evaluate(hash, &ProofStatus::Conjectured, &history, 100)
        .expect("candidate should fire");
    demote_artifact(&mut reg, &cand);

    let status = reg.status_of(hash).unwrap();
    assert!(matches!(
        status,
        ProofStatus::Demoted(DemotionReason::StaleConjecture)
    ));
    // Active-lifecycle check: the terminal status excludes the
    // artifact from further forward motion.
    assert!(!status.is_active());
}

#[test]
fn promoted_but_retired_primitive_falls_to_retired_primitive_status() {
    use mathscape_core::lifecycle::AxiomIdentity;
    let mut reg = InMemoryRegistry::new();
    let artifact = mk(1);
    let hash = artifact.content_hash;
    reg.insert(artifact);

    // Simulate that the artifact was promoted to Primitive.
    let identity = AxiomIdentity {
        target: "mathscape_core::term::Term".into(),
        name: "RetiredX".into(),
        proposal_hash: TermRef([0xbb; 32]),
    };
    reg.mark_status(hash, ProofStatus::Primitive(identity.clone()));

    // Seed log with ancient usage, nothing recent.
    let mut log = CorpusLog::new();
    log.record_match(hash, &"arith".into(), 0);
    let history = log.history_for(hash, 200, 30);

    let gate = UsageFloorGate::new(1, 50);
    let cand = gate
        .evaluate(
            hash,
            &ProofStatus::Primitive(identity),
            &history,
            200,
        )
        .expect("retired primitive should fire");
    assert!(matches!(
        cand.proposed_reason,
        DemotionReason::RetiredPrimitive
    ));
    demote_artifact(&mut reg, &cand);
    assert!(matches!(
        reg.status_of(hash),
        Some(ProofStatus::Demoted(DemotionReason::RetiredPrimitive))
    ));
}
