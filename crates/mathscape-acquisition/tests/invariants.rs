//! Regression tests — one per typed invariant of the acquisition space
//! (`GARIMPO.md §3.5`), driving the interpreter through a **mocked**
//! `AcquisitionEnvironment` so no real e-graph / Lean / migration engine
//! is needed. The trait IS the testability contract.

use mathscape_acquisition::interp::{fold_back_checked, seal_proof_layer};
use mathscape_acquisition::{
    apply, AbsorbDecision, AcquisitionDAG, AcquisitionEnvironment, AcquisitionSpec,
    AcquisitionState, Complexity, Dl, Frontier, Layer, ProofKind, SealWitness, SpecError,
    StepReport, Tier,
};
use mathscape_core::eval::RewriteRule;
use mathscape_core::term::Term;
use mathscape_core::value::Value;

// ── A mock environment ────────────────────────────────────────────────

/// A deterministic mock. `seal_ok` decides whether the sample seal
/// passes; `dl_of` gives the DL of a layer set (its `.len()` scaled) so
/// tests can force a DL-increasing vs DL-decreasing absorb.
struct MockEnv {
    seal_ok: bool,
    /// Bits per layer. A negative sign is impossible (Dl is u64); tests
    /// control DL direction by choosing this map's shape.
    dl_per_layer: u64,
    /// When true, `Absorb` should model a fold-back that SHRINKS DL:
    /// `dl(folded) = dl(before) - shrink`. When false, DL grows.
    shrinks: bool,
    shrink_by: u64,
}

impl AcquisitionEnvironment for MockEnv {
    fn seal_sample(&self, _rule: &RewriteRule, k: usize) -> Option<SealWitness> {
        self.seal_ok.then(|| SealWitness::sample(k))
    }

    fn description_length(&self, layers: &[Layer]) -> Dl {
        // A large fixed base plus a per-layer term. When `shrinks` is
        // set, each layer's fold-back *saves* `shrink_by` bits, so DL
        // *decreases* with layer count (modelling a productive
        // abstraction). When not, DL grows with layer count (an
        // unproductive one). The fixed base keeps DL non-negative.
        const BASE: u64 = 1_000;
        let n = layers.len() as u64;
        if self.shrinks {
            Dl::bits(BASE - self.shrink_by * n)
        } else {
            Dl::bits(BASE + self.dl_per_layer * n)
        }
    }

    fn migrate_trusted(&self, _primitive: &Layer, earlier: &[Layer]) -> usize {
        earlier.len()
    }
    // seal_proof / check_equivalence intentionally NOT overridden — they
    // must return the typed gap.
}

fn rule(lhs: Term, rhs: Term) -> RewriteRule {
    RewriteRule {
        name: "test-rule".to_string(),
        lhs,
        rhs,
    }
}

fn identity_rule() -> RewriteRule {
    // add(?100, 0) => ?100   (a plausible identity abstraction)
    rule(
        Term::Apply(
            Box::new(Term::Var(2)), // add
            vec![Term::Var(100), Term::Number(Value::Nat(0))],
        ),
        Term::Var(100),
    )
}

// ── INV-1 (§3.5.1): a Layer is ground ONLY if sealed ──────────────────

#[test]
fn inv1_a_sample_sealed_layer_is_flagged_not_proof_sealed() {
    let layer = Layer::sampled(identity_rule(), 16);
    assert!(layer.is_sample_sealed(), "Sample seal is the weak floor");
    assert!(
        !layer.seal.is_proof_grade(),
        "a sampled layer is NOT proof-grade"
    );
    assert_eq!(layer.seal.tier(), Tier::OnlyMitigated);
}

#[test]
fn inv1_unsealable_candidate_is_rejected_never_committed() {
    let env = MockEnv {
        seal_ok: false,
        dl_per_layer: 10,
        shrinks: false,
        shrink_by: 0,
    };
    let spec = AcquisitionSpec::new("t", Complexity(64)).seal(identity_rule(), 16);
    let out = apply(&spec, &env).unwrap();
    // Rejected: only the genesis node exists; no layer committed.
    assert!(matches!(out.steps[0], StepReport::LayerRejected));
    assert_eq!(out.dag.head().unwrap().layers.len(), 0);
}

#[test]
fn inv1_the_proof_grade_seal_is_a_typed_gap_not_a_fake_pass() {
    let env = MockEnv {
        seal_ok: true,
        dl_per_layer: 10,
        shrinks: false,
        shrink_by: 0,
    };
    let r = identity_rule();
    let res = seal_proof_layer(&r, ProofKind::LeanKernel, &env);
    assert_eq!(
        res.unwrap_err(),
        SpecError::Interp {
            phase: "proof-seal"
        },
        "the Proof destination must be a TYPED GAP, never a fabricated pass"
    );
}

// ── INV-2 (§3.5.2): a FoldBack needs a semantics-preserving witness ───

#[test]
fn inv2_equivalence_checked_fold_back_is_a_typed_gap() {
    let env = MockEnv {
        seal_ok: true,
        dl_per_layer: 10,
        shrinks: false,
        shrink_by: 0,
    };
    let primitive = Layer::sampled(identity_rule(), 16);
    let earlier: Vec<Layer> = vec![];
    let res = fold_back_checked(primitive, &earlier, &env);
    assert_eq!(
        res.unwrap_err(),
        SpecError::Interp {
            phase: "equivalence-gate"
        },
        "the EquivalenceChecked destination must be a TYPED GAP"
    );
}

#[test]
fn inv2_trusted_fold_back_runs_and_reports_rewrites() {
    let env = MockEnv {
        seal_ok: true,
        dl_per_layer: 10,
        shrinks: false,
        shrink_by: 0,
    };
    // Seal two layers, then fold back through primitive index 0.
    let spec = AcquisitionSpec::new("fold", Complexity(64))
        .seal(identity_rule(), 16)
        .seal(
            rule(Term::Var(101), Term::Var(101)), // trivial second layer
            16,
        )
        .fold_back(0);
    let out = apply(&spec, &env).unwrap();
    let folded = out
        .steps
        .iter()
        .find(|s| matches!(s, StepReport::FoldedBack { .. }))
        .expect("a fold-back step must be recorded");
    match folded {
        StepReport::FoldedBack { rewritten } => {
            assert_eq!(*rewritten, 1, "one earlier layer rewritten (index 1)");
        }
        _ => unreachable!(),
    }
}

// ── §3.5.3: AbsorbDecision — DL-gated significance-as-a-type ──────────

#[test]
fn absorb_rejects_a_dl_increasing_layer() {
    // new_dl >= old_dl → no AbsorbDecision.
    assert!(AbsorbDecision::try_new(Dl::bits(100), Dl::bits(100)).is_none());
    assert!(AbsorbDecision::try_new(Dl::bits(100), Dl::bits(120)).is_none());
}

#[test]
fn absorb_accepts_only_a_dl_decreasing_layer() {
    let d = AbsorbDecision::try_new(Dl::bits(100), Dl::bits(70)).expect("70 < 100 absorbs");
    assert_eq!(d.bits_saved(), 30);
}

#[test]
fn absorb_op_not_significant_when_dl_grows() {
    // dl grows with layer count and does not shrink → NotSignificant.
    let env = MockEnv {
        seal_ok: true,
        dl_per_layer: 10,
        shrinks: false,
        shrink_by: 0,
    };
    let spec = AcquisitionSpec::new("grow", Complexity(64)).absorb(identity_rule(), 16);
    let out = apply(&spec, &env).unwrap();
    assert!(
        matches!(out.steps[0], StepReport::NotSignificant),
        "a DL-increasing absorb is refused (significance-as-a-type)"
    );
    // Not committed.
    assert_eq!(out.dag.head().unwrap().layers.len(), 0);
}

#[test]
fn absorb_op_significant_when_fold_shrinks_dl() {
    // shrinks=true forces dl(folded) < dl(before) → Absorbed.
    let env = MockEnv {
        seal_ok: true,
        dl_per_layer: 10,
        shrinks: true,
        shrink_by: 25,
    };
    let spec = AcquisitionSpec::new("shrink", Complexity(64)).absorb(identity_rule(), 16);
    let out = apply(&spec, &env).unwrap();
    assert!(
        matches!(out.steps[0], StepReport::Absorbed { .. }),
        "a DL-decreasing absorb pays for itself and is committed"
    );
    assert_eq!(out.dag.head().unwrap().layers.len(), 1);
}

// ── §3.5.4: Frontier — incompleteness as a type (no Closed) ───────────

#[test]
fn frontier_advance_never_closes_always_opens_higher() {
    let f = Frontier::open(Complexity(64));
    let f2 = f.advance(Complexity(128));
    match f2 {
        Frontier::Advancing { from, to } => {
            assert_eq!(from, Complexity(64));
            assert_eq!(to, Complexity(128));
        }
        // The type has NO Closed arm — this match is exhaustive with
        // just Open | Advancing, which is the whole point.
        Frontier::Open { .. } => panic!("advance must move to Advancing"),
    }
    // Advancing again still yields a non-terminal frontier.
    let f3 = f2.advance(Complexity(256));
    assert!(matches!(f3, Frontier::Advancing { .. }));
}

// ── §3.5.5: AcquisitionDAG — reversible by construction ───────────────

#[test]
fn dag_round_trips_and_reverts() {
    let mut dag = AcquisitionDAG::new();
    let g = dag.commit(AcquisitionState::genesis(Complexity(64)));

    // Advance to a second node.
    let head = dag.head().unwrap().clone();
    let n1 = AcquisitionState::seal(
        Some(head.content_hash),
        head.frontier.advance(Complexity(128)),
        head.layers.clone(),
    );
    let h1 = dag.commit(n1);
    assert_eq!(dag.len(), 2);
    assert_eq!(dag.head().unwrap().content_hash, h1);

    // Revert to genesis (git-revert for the model).
    let restored = dag.restore(&g).expect("genesis is a committed node");
    assert_eq!(restored.content_hash, g);
    assert_eq!(dag.head().unwrap().content_hash, g);

    // Both nodes remain reachable — no state is lost.
    assert!(dag.get(&g).is_some());
    assert!(dag.get(&h1).is_some());
}

#[test]
fn dag_restore_of_uncommitted_state_is_a_typed_error() {
    let mut dag = AcquisitionDAG::new();
    dag.commit(AcquisitionState::genesis(Complexity(64)));
    // A hash that was never committed.
    let bogus = AcquisitionState::genesis(Complexity(999)).content_hash;
    let res = dag.restore(&bogus);
    assert!(matches!(res, Err(SpecError::Invariant { .. })));
}

#[test]
fn dag_fork_carries_lineage_and_evolves_independently() {
    let mut dag = AcquisitionDAG::new();
    let g = dag.commit(AcquisitionState::genesis(Complexity(64)));
    let head = dag.head().unwrap().clone();
    let n1 = AcquisitionState::seal(
        Some(head.content_hash),
        head.frontier,
        vec![Layer::sampled(identity_rule(), 16)],
    );
    let h1 = dag.commit(n1);

    let mut forked = dag.fork(&h1).expect("fork a committed node");
    // Fork carries genesis + n1 (full lineage).
    assert_eq!(forked.len(), 2);
    assert!(forked.get(&g).is_some());
    assert!(forked.get(&h1).is_some());

    // Evolving the fork does not touch the original.
    let fhead = forked.head().unwrap().clone();
    forked.commit(AcquisitionState::seal(
        Some(fhead.content_hash),
        fhead.frontier.advance(Complexity(200)),
        fhead.layers,
    ));
    assert_eq!(forked.len(), 3);
    assert_eq!(dag.len(), 2, "original DAG is unchanged by fork evolution");
}
