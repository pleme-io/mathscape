//! Knowability criterion v1 — replayability + policy differentiation.
//!
//! See `docs/arch/knowability-criterion.md`. The v1 success bar
//! requires:
//!
//! 2. Replayability — same policy + corpus → byte-identical registry
//!    root
//! 3. Policy differentiation — different policy on same corpus →
//!    different registry root
//!
//! Criteria 1 (derivation chain completeness) and 5 (typescape leaf
//! binding) are exercised by earlier integration tests. Criterion 4
//! (external Lean 4) is out-of-process and reserved for CI.

use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_core::{
    epoch::{Epoch, InMemoryRegistry, Registry, RuleEmitter},
    term::Term,
    value::Value,
};
use mathscape_reward::{reward::RewardConfig, StatisticalProver};

fn var(id: u32) -> Term {
    Term::Var(id)
}
fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}
fn apply(f: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(f), args)
}

fn fixed_corpus() -> Vec<Term> {
    vec![
        apply(var(2), vec![nat(3), nat(0)]),
        apply(var(2), vec![nat(5), nat(0)]),
        apply(var(2), vec![nat(7), nat(0)]),
        apply(var(2), vec![nat(11), nat(0)]),
        apply(var(2), vec![nat(13), nat(0)]),
    ]
}

fn build_epoch(
    extract_cfg: ExtractConfig,
    min_score: f64,
) -> Epoch<CompressionGenerator, StatisticalProver, RuleEmitter, InMemoryRegistry> {
    Epoch::new(
        CompressionGenerator::new(extract_cfg, 1),
        StatisticalProver::new(RewardConfig::default(), min_score),
        RuleEmitter,
        InMemoryRegistry::new(),
    )
}

fn run_n_epochs(
    epoch: &mut Epoch<
        CompressionGenerator,
        StatisticalProver,
        RuleEmitter,
        InMemoryRegistry,
    >,
    corpus: &[Term],
    n: usize,
) {
    for _ in 0..n {
        epoch.step(corpus);
    }
}

#[test]
fn replay_under_identical_policy_produces_identical_root() {
    let corpus = fixed_corpus();
    let cfg_a = ExtractConfig {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 3,
    };
    let cfg_b = ExtractConfig {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 3,
    };

    let mut a = build_epoch(cfg_a, 0.0);
    let mut b = build_epoch(cfg_b, 0.0);

    run_n_epochs(&mut a, &corpus, 10);
    run_n_epochs(&mut b, &corpus, 10);

    // Same policy, same corpus, same seed (neither generator nor
    // prover uses RNG) → byte-identical registries.
    assert_eq!(
        a.registry.root(),
        b.registry.root(),
        "registries should have identical roots under identical policy + corpus"
    );
    assert_eq!(a.registry.len(), b.registry.len());
}

#[test]
fn replay_reports_same_epoch_id_advance() {
    let corpus = fixed_corpus();
    let mut a = build_epoch(ExtractConfig::default(), 0.0);
    let mut b = build_epoch(ExtractConfig::default(), 0.0);
    run_n_epochs(&mut a, &corpus, 7);
    run_n_epochs(&mut b, &corpus, 7);
    assert_eq!(a.epoch_id, b.epoch_id);
    assert_eq!(a.epoch_id, 7);
}

#[test]
fn different_min_score_produces_different_registry_roots() {
    let corpus = fixed_corpus();

    // Policy A: accept anything with reward ≥ 0
    let mut a = build_epoch(ExtractConfig::default(), 0.0);
    // Policy B: extreme threshold ⇒ accepts nothing
    let mut b = build_epoch(ExtractConfig::default(), 1e9);

    run_n_epochs(&mut a, &corpus, 5);
    run_n_epochs(&mut b, &corpus, 5);

    assert_ne!(
        a.registry.root(),
        b.registry.root(),
        "different min_score must yield different trajectories"
    );
    // Policy B rejects everything → empty registry.
    assert_eq!(b.registry.len(), 0);
    assert_eq!(b.registry.root().as_bytes(), &[0; 32]);
}

#[test]
fn different_extract_config_produces_different_roots() {
    let corpus = fixed_corpus();

    let mut a = build_epoch(
        ExtractConfig {
            min_shared_size: 2,
            min_matches: 2,
            max_new_rules: 3,
        },
        0.0,
    );
    let mut b = build_epoch(
        ExtractConfig {
            min_shared_size: 4, // much stricter
            min_matches: 2,
            max_new_rules: 3,
        },
        0.0,
    );

    run_n_epochs(&mut a, &corpus, 5);
    run_n_epochs(&mut b, &corpus, 5);

    // Stricter extract config should produce at most as many
    // artifacts, and in this corpus, likely strictly fewer ⇒
    // different roots.
    assert_ne!(a.registry.root(), b.registry.root());
    assert!(a.registry.len() >= b.registry.len());
}

#[test]
fn empty_registry_root_is_zero() {
    let epoch = build_epoch(ExtractConfig::default(), 0.0);
    assert_eq!(epoch.registry.root().as_bytes(), &[0; 32]);
}

#[test]
fn registry_root_is_insertion_order_independent() {
    use mathscape_core::epoch::{AcceptanceCertificate, Artifact};
    use mathscape_core::eval::RewriteRule;

    fn mk(sym: u32) -> Artifact {
        let rule = RewriteRule {
            name: format!("r{sym}"),
            lhs: Term::Symbol(sym, vec![]),
            rhs: Term::Point(sym as u64),
        };
        Artifact::seal(
            rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        )
    }

    let mut a = InMemoryRegistry::new();
    let mut b = InMemoryRegistry::new();
    a.insert(mk(1));
    a.insert(mk(2));
    a.insert(mk(3));
    // Same artifacts, different insertion order.
    b.insert(mk(3));
    b.insert(mk(1));
    b.insert(mk(2));
    assert_eq!(a.root(), b.root());
}
