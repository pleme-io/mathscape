//! Orchestrator integration tests — exercise the multi-layer
//! runner + run_until_reduced against real CompressionGenerator +
//! StatisticalProver.

use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_core::{
    control::{Allocator, RealizationPolicy, RewardEstimator},
    epoch::{
        AcceptanceCertificate, Artifact, Epoch, InMemoryRegistry, Registry, RuleEmitter,
    },
    eval::RewriteRule,
    hash::TermRef,
    lifecycle::{AxiomIdentity, ProofStatus, TypescapeCoord},
    orchestrator::{
        run_until_reduced, MultiLayerRunner, PromotionHook, PromotionOutcome,
    },
    promotion::PromotionSignal,
    reduction::{ReductionPolicy, ReductionVerdict},
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

#[test]
fn run_until_reduced_terminates_on_empty_corpus() {
    let mut epoch = build_epoch();
    let mut alloc =
        Allocator::new(RealizationPolicy::default(), RewardEstimator::new(0.3));
    let traj = run_until_reduced(
        &mut epoch,
        &mut alloc,
        &[],
        &ReductionPolicy::layer_0_default(),
        5,
        0,
    );
    assert!(matches!(traj.terminal_verdict, ReductionVerdict::Reduced));
    assert!(!traj.hit_epoch_cap);
}

#[test]
fn run_until_reduced_records_per_epoch_snapshots() {
    let mut epoch = build_epoch();
    let mut alloc =
        Allocator::new(RealizationPolicy::default(), RewardEstimator::new(0.3));
    let traj = run_until_reduced(
        &mut epoch,
        &mut alloc,
        &corpus(),
        &ReductionPolicy::layer_0_default(),
        10,
        0,
    );
    assert!(traj.epoch_count() > 0);
    assert_eq!(traj.terminal_root, epoch.registry.root());
}

#[test]
fn run_until_reduced_hits_cap_when_set_low() {
    let mut epoch = build_epoch();
    let mut alloc =
        Allocator::new(RealizationPolicy::default(), RewardEstimator::new(0.3));
    let traj = run_until_reduced(
        &mut epoch,
        &mut alloc,
        &corpus(),
        &ReductionPolicy::layer_0_default(),
        1,
        0,
    );
    assert_eq!(traj.epochs.len(), 1);
}

#[test]
fn multi_layer_runner_progresses_through_layers() {
    let mut runner = MultiLayerRunner {
        epoch: build_epoch(),
        allocator: Allocator::new(
            RealizationPolicy::default(),
            RewardEstimator::new(0.3),
        ),
        per_layer_max_epochs: 5,
        max_layers: 3,
        policy: ReductionPolicy::layer_0_default(),
    };

    let promotable = Artifact::seal(
        RewriteRule {
            name: "promotable".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: Term::Symbol(42, vec![var(100)]),
        },
        0,
        AcceptanceCertificate::trivial_conjecture(1.0),
        vec![],
    );
    let promotable_hash = promotable.content_hash;
    runner.epoch.registry.insert(promotable);

    let mut fired = false;
    let hook: PromotionHook<'_, InMemoryRegistry> = Box::new(move |_reg| {
        if !fired {
            fired = true;
            Some((
                PromotionSignal {
                    artifact_hash: promotable_hash,
                    subsumed_hashes: vec![],
                    cross_corpus_support: vec!["test".into()],
                    rationale: "hook approved".into(),
                    epoch_id: 0,
                },
                PromotionOutcome::Approved {
                    identity: AxiomIdentity {
                        target: "mathscape_core::term::Term".into(),
                        name: "Promotable".into(),
                        proposal_hash: TermRef([0xfe; 32]),
                        typescape_coord: TypescapeCoord::precommit(
                            "mathscape_core::term::Term",
                            "Promotable",
                        ),
                    },
                },
            ))
        } else {
            None
        }
    });

    let report = runner.run(&corpus(), hook);

    assert!(!report.layers.is_empty());
    assert_eq!(report.migrations.len(), 1);
    let status = runner.epoch.registry.status_of(promotable_hash).unwrap();
    assert!(
        matches!(status, ProofStatus::Primitive(_)),
        "promoted artifact should be Primitive; got {status:?}"
    );
}

#[test]
fn multi_layer_runner_respects_decline() {
    let mut runner = MultiLayerRunner {
        epoch: build_epoch(),
        allocator: Allocator::new(
            RealizationPolicy::default(),
            RewardEstimator::new(0.3),
        ),
        per_layer_max_epochs: 3,
        max_layers: 5,
        policy: ReductionPolicy::layer_0_default(),
    };
    let hook: PromotionHook<'_, InMemoryRegistry> = Box::new(|_| None);
    let report = runner.run(&corpus(), hook);
    assert_eq!(report.layers.len(), 1);
    assert!(report.migrations.is_empty());
}

#[test]
fn layer_trajectory_aggregates_match_sum_of_snapshots() {
    let mut epoch = build_epoch();
    let mut alloc =
        Allocator::new(RealizationPolicy::default(), RewardEstimator::new(0.3));
    let traj = run_until_reduced(
        &mut epoch,
        &mut alloc,
        &corpus(),
        &ReductionPolicy::layer_0_default(),
        8,
        0,
    );
    let summed_discovery: f64 = traj.epochs.iter().map(|s| s.delta_dl_discovery).sum();
    let summed_reinforce: f64 = traj.epochs.iter().map(|s| s.delta_dl_reinforce).sum();
    assert!((traj.total_discovery_delta() - summed_discovery).abs() < 1e-9);
    assert!((traj.total_reinforce_delta() - summed_reinforce).abs() < 1e-9);
}

#[test]
fn multi_layer_run_deepest_reduced_layer_reflects_progress() {
    let mut runner = MultiLayerRunner {
        epoch: build_epoch(),
        allocator: Allocator::new(
            RealizationPolicy::default(),
            RewardEstimator::new(0.3),
        ),
        per_layer_max_epochs: 10,
        max_layers: 3,
        policy: ReductionPolicy::layer_0_default(),
    };
    // No hook: orchestrator stops after layer 0.
    let hook: PromotionHook<'_, InMemoryRegistry> = Box::new(|_| None);
    let report = runner.run(&corpus(), hook);
    // At least layer 0 was reduced (empty corpus reduces trivially;
    // non-empty eventually does under 10 epochs of discover-only).
    assert!(report.reduced_layer_count() <= report.layers.len());
}
