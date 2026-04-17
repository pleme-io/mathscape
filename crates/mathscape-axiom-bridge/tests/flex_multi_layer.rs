//! Flex the machine as hard as it will go.
//!
//! Runs `MultiLayerRunner` with a real axiom-forge-backed promotion
//! hook over diverse corpora (arithmetic, combinators, booleans).
//! No canned responses — the bridge actually invokes axiom-forge's
//! seven obligations on every promotion attempt and emits real Rust
//! source on success. Telemetry is dumped per layer.
//!
//! This is an **observational** test: we assert the *machinery runs*
//! without claiming what it must discover. What emerges emerges.
//! The point is to see how deep the machine can go, what primitives
//! it mints, and where the theory meets reality.

use mathscape_axiom_bridge::{run_promotion, BridgeConfig};
use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_core::{
    control::{Allocator, RealizationPolicy, RewardEstimator},
    epoch::{Epoch, InMemoryRegistry, Registry, RuleEmitter},
    hash::TermRef,
    lifecycle::ProofStatus,
    orchestrator::{MultiLayerRunner, PromotionHook, PromotionOutcome},
    promotion::PromotionSignal,
    reduction::{reduction_pressure, ReductionPolicy, ReductionVerdict},
    term::Term,
    value::Value,
};
use mathscape_reward::{reward::RewardConfig, StatisticalProver};
use std::cell::RefCell;
use std::rc::Rc;

fn apply(f: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(f), args)
}
fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}
fn var(id: u32) -> Term {
    Term::Var(id)
}

// ── Corpora ─────────────────────────────────────────────────────

/// Additive-identity-rich corpus — every term is `add(_, 0)`.
/// Anti-unification should extract the identity pattern.
fn arith_corpus() -> Vec<Term> {
    (1..=10)
        .map(|n| apply(var(2), vec![nat(n), nat(0)]))
        .collect()
}

/// Multiplicative-identity-rich corpus — every term is `mul(_, 1)`.
fn multiplicative_corpus() -> Vec<Term> {
    (1..=10)
        .map(|n| apply(var(3), vec![nat(n), nat(1)]))
        .collect()
}

/// Mixed corpus — both patterns should emerge.
fn mixed_corpus() -> Vec<Term> {
    let mut v = Vec::new();
    for n in 1..=8 {
        v.push(apply(var(2), vec![nat(n), nat(0)]));
        v.push(apply(var(3), vec![nat(n), nat(1)]));
    }
    v
}

// ── Helpers ─────────────────────────────────────────────────────

fn build_epoch() -> Epoch<CompressionGenerator, StatisticalProver, RuleEmitter, InMemoryRegistry>
{
    Epoch::new(
        CompressionGenerator::new(
            ExtractConfig {
                min_shared_size: 2,
                min_matches: 2,
                max_new_rules: 3,
            },
            1,
        ),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        InMemoryRegistry::new(),
    )
}

/// Build a promotion hook that will approve the FIRST active
/// artifact that looks promotable. For observation flex only:
/// fabricates enough cross-corpus evidence to clear the threshold
/// gate (we're driving a single corpus in each run, so genuine
/// cross-corpus evidence isn't there — the flex is about
/// exercising the mechanics, not validating gate-5 semantics).
fn build_observational_hook(
    fired_hashes: Rc<RefCell<Vec<TermRef>>>,
) -> PromotionHook<'static, InMemoryRegistry> {
    Box::new(move |registry: &InMemoryRegistry| {
        // Collect candidate artifacts without holding the borrow
        // across the bridge call.
        let candidates: Vec<_> = {
            let fired = fired_hashes.borrow();
            registry
                .all()
                .iter()
                .filter(|a| !fired.contains(&a.content_hash))
                .filter(|a| {
                    let status = registry
                        .status_of(a.content_hash)
                        .unwrap_or_else(|| a.certificate.status.clone());
                    matches!(
                        status,
                        ProofStatus::Conjectured
                            | ProofStatus::Verified
                            | ProofStatus::Exported
                            | ProofStatus::Axiomatized
                    )
                })
                .cloned()
                .collect()
        };

        for artifact in &candidates {
            let signal = PromotionSignal {
                artifact_hash: artifact.content_hash,
                subsumed_hashes: vec![],
                cross_corpus_support: vec!["flex-a".into(), "flex-b".into()],
                rationale: format!("flex observation for {}", artifact.rule.name),
                epoch_id: 0,
            };
            match run_promotion(&signal, artifact, &BridgeConfig::default()) {
                Ok(receipt) => {
                    fired_hashes.borrow_mut().push(artifact.content_hash);
                    return Some((
                        signal,
                        PromotionOutcome::Approved {
                            identity: receipt.axiom_identity,
                        },
                    ));
                }
                Err(_e) => {
                    // Bridge rejected (e.g., gate 6 violations, arity cap).
                    // Record attempt to avoid retrying.
                    fired_hashes.borrow_mut().push(artifact.content_hash);
                    continue;
                }
            }
        }
        None
    })
}

fn run_flex(label: &str, corpus: Vec<Term>, max_layers: u32, max_epochs_per_layer: usize) {
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ FLEX: {:<47}║", label);
    println!("╚══════════════════════════════════════════════════════╝");

    let mut runner = MultiLayerRunner {
        epoch: build_epoch(),
        allocator: Allocator::new(
            RealizationPolicy::default(),
            RewardEstimator::new(0.3),
        ),
        per_layer_max_epochs: max_epochs_per_layer,
        max_layers,
        policy: ReductionPolicy::layer_0_default(),
    };

    let fired_hashes = Rc::new(RefCell::new(Vec::new()));
    let hook = build_observational_hook(fired_hashes.clone());
    let report = runner.run(&corpus, hook);

    println!("\n▶ Summary");
    println!("  layers run             : {}", report.layers.len());
    println!("  reduced layers         : {}", report.reduced_layer_count());
    println!("  deepest reduced        : layer {}", report.deepest_reduced_layer);
    println!("  migrations fired       : {}", report.migrations.len());
    println!("  bridge attempts        : {}", fired_hashes.borrow().len());
    println!("  final registry root    : {}", report.final_root);
    println!("  final library size     : {}", runner.epoch.registry.len());
    println!(
        "  final pressure         : {:.3}",
        reduction_pressure(&runner.epoch.registry)
    );

    println!("\n▶ Per-layer trajectory");
    for layer in &report.layers {
        let reduced = matches!(layer.terminal_verdict, ReductionVerdict::Reduced);
        println!(
            "  layer {}: {} epochs, reduced={}, hit_cap={}, ΔDL-disc={:.2}, ΔDL-reinf={:.2}",
            layer.layer_id,
            layer.epoch_count(),
            reduced,
            layer.hit_epoch_cap,
            layer.total_discovery_delta(),
            layer.total_reinforce_delta(),
        );
        println!("    terminal_root: {}", layer.terminal_root);
        println!("    diagnostic:    {}", layer.diagnostic.narrative());
    }

    println!("\n▶ Migrations");
    for (i, m) in report.migrations.iter().enumerate() {
        println!(
            "  migration {}: primitive={}::{}, rewritten={}, deduplicated={}",
            i,
            m.primitive.target,
            m.primitive.name,
            m.rewritten.len(),
            m.deduplicated.len(),
        );
    }

    println!("\n▶ Library at trajectory end ({} artifacts total)", runner.epoch.registry.all().len());
    for (i, a) in runner.epoch.registry.all().iter().enumerate() {
        let status = runner.epoch.registry.status_of(a.content_hash).unwrap();
        let status_str = match status {
            ProofStatus::Proposed => "Proposed",
            ProofStatus::Conjectured => "Conjectured",
            ProofStatus::Verified => "Verified",
            ProofStatus::Exported => "Exported",
            ProofStatus::Axiomatized => "Axiomatized",
            ProofStatus::Promoted => "Promoted",
            ProofStatus::Primitive(_) => "Primitive",
            ProofStatus::Subsumed(_) => "Subsumed",
            ProofStatus::Demoted(_) => "Demoted",
        };
        println!(
            "  [{i}] {}: {} :: {} => {}  [{}]",
            a.content_hash,
            a.rule.name,
            a.rule.lhs,
            a.rule.rhs,
            status_str,
        );
    }
}

// ── The flex tests ──────────────────────────────────────────────

#[test]
fn flex_arith_corpus() {
    run_flex("arithmetic identity (add, 0)", arith_corpus(), 4, 15);
}

#[test]
fn flex_multiplicative_corpus() {
    run_flex(
        "multiplicative identity (mul, 1)",
        multiplicative_corpus(),
        4,
        15,
    );
}

#[test]
fn flex_mixed_corpus() {
    run_flex(
        "mixed — both additive + multiplicative identity",
        mixed_corpus(),
        5,
        20,
    );
}

#[test]
fn flex_deep_run_arith() {
    // Same corpus, much deeper budget — see if anything changes.
    run_flex("arithmetic deep (30 epochs × 10 layers)", arith_corpus(), 10, 30);
}

#[test]
fn flex_replay_is_deterministic() {
    // Two identical flex runs should produce identical final roots.
    let c = arith_corpus();

    fn run_once(c: &[Term]) -> TermRef {
        let mut runner = MultiLayerRunner {
            epoch: build_epoch(),
            allocator: Allocator::new(
                RealizationPolicy::default(),
                RewardEstimator::new(0.3),
            ),
            per_layer_max_epochs: 15,
            max_layers: 4,
            policy: ReductionPolicy::layer_0_default(),
        };
        let fired = Rc::new(RefCell::new(Vec::new()));
        let hook = build_observational_hook(fired);
        let _report = runner.run(c, hook);
        runner.epoch.registry.root()
    }

    let a = run_once(&c);
    let b = run_once(&c);
    assert_eq!(
        a, b,
        "multi-layer flex trajectories must be deterministic under identical inputs"
    );
    println!("\n✓ deterministic across two independent flex runs: root = {a}");
}
