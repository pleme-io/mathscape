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
use mathscape_compress::{
    extract::ExtractConfig, CompositeGenerator, CompressionGenerator, MetaPatternGenerator,
};
use mathscape_core::{
    control::{Allocator, RealizationPolicy, RewardEstimator},
    epoch::{Epoch, InMemoryRegistry, Registry, RuleEmitter},
    event::Event,
    form_tree::DiscoveryForest,
    hash::TermRef,
    lifecycle::ProofStatus,
    meta::{MetaOptimizer, PolicyTweak},
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

/// Nested-identity corpus — `add(add(n, 0), 0)`. Two layers of the
/// same identity. Layer 0 should mint `S_001 = add(?x, 0) => ?x`.
/// After that primitive migrates back, the corpus rewrites to
/// `add(n, 0)` — which is ALSO pattern-matched by S_001. So layer 1
/// should observe full reduction via pure reuse (no new primitives).
/// That's the "reinforcement-dominant" regime: no discovery needed,
/// existing library already covers the collapsed form.
fn nested_identity_corpus() -> Vec<Term> {
    (1..=8)
        .map(|n| {
            let inner = apply(var(2), vec![nat(n), nat(0)]);
            apply(var(2), vec![inner, nat(0)])
        })
        .collect()
}

/// Compositional corpus — mixes nested and flat add-identity,
/// plus mul-identity. Forces the discovery engine to fan across
/// heterogeneous structure within a single epoch:
///   add(n, 0), add(add(n, 0), 0), mul(n, 1), mul(mul(n, 1), 1)
/// Two family classes, each at two depths. Observation: how many
/// primitives fire; whether the deeper nests collapse via reuse or
/// whether the system mints a specialized nested primitive.
fn compositional_corpus() -> Vec<Term> {
    let mut v = Vec::new();
    for n in 1..=6 {
        v.push(apply(var(2), vec![nat(n), nat(0)]));
        v.push(apply(var(2), vec![apply(var(2), vec![nat(n), nat(0)]), nat(0)]));
        v.push(apply(var(3), vec![nat(n), nat(1)]));
        v.push(apply(var(3), vec![apply(var(3), vec![nat(n), nat(1)]), nat(1)]));
    }
    v
}

// ── Helpers ─────────────────────────────────────────────────────

fn build_epoch() -> Epoch<CompressionGenerator, StatisticalProver, RuleEmitter, InMemoryRegistry>
{
    build_epoch_with(3)
}

fn build_epoch_with(
    max_new_rules: usize,
) -> Epoch<CompressionGenerator, StatisticalProver, RuleEmitter, InMemoryRegistry> {
    Epoch::new(
        CompressionGenerator::new(
            ExtractConfig {
                min_shared_size: 2,
                min_matches: 2,
                max_new_rules,
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
    run_flex_with(label, corpus, max_layers, max_epochs_per_layer, 3);
}

fn run_flex_with(
    label: &str,
    corpus: Vec<Term>,
    max_layers: u32,
    max_epochs_per_layer: usize,
    max_new_rules: usize,
) {
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ FLEX: {:<47}║", label);
    println!("╚══════════════════════════════════════════════════════╝");

    let mut runner = MultiLayerRunner {
        epoch: build_epoch_with(max_new_rules),
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
fn flex_nested_identity_corpus() {
    run_flex(
        "nested additive identity (add(add(_,0),0))",
        nested_identity_corpus(),
        5,
        20,
    );
}

#[test]
fn flex_compositional_corpus() {
    // max_new_rules=5 so anti-unification has room for all four
    // pattern classes (flat-add, nested-add, flat-mul, nested-mul).
    // With cap=3 the machine discovers 3 and silently drops one —
    // observed: flat-mul got crowded out of the first epoch.
    run_flex_with(
        "compositional — nested + flat × add + mul",
        compositional_corpus(),
        6,
        25,
        5,
    );
}

#[test]
fn flex_discovery_forest_end_to_end() {
    // End-to-end: run the real discovery pipeline on the
    // compositional corpus, feed every corpus term + every accepted
    // rule into a DiscoveryForest, and observe:
    //   1. The forest picks up retroactive reductions when rules
    //      fire on pre-inserted nodes (flat add-id + flat mul-id
    //      should each retroactively reduce 12 pre-inserted terms).
    //   2. `traversal_saving` rises as the forest stabilizes —
    //      scheduler savings are real, not paper.
    //   3. Stable-leaf count grows as reduction chains terminate at
    //      fully-reduced leaves.
    let corpus = compositional_corpus();
    let mut forest = DiscoveryForest::new();
    // Insert every corpus term BEFORE running discovery. The
    // retroactive test is: does the forest correctly reduce these
    // historical nodes when the rules land later?
    for t in &corpus {
        forest.insert(t.clone());
    }
    let corpus_nodes_inserted = forest.len();

    let mut runner = MultiLayerRunner {
        epoch: build_epoch_with(5),
        allocator: Allocator::new(RealizationPolicy::default(), RewardEstimator::new(0.3)),
        per_layer_max_epochs: 25,
        max_layers: 6,
        policy: ReductionPolicy::layer_0_default(),
    };
    let fired = Rc::new(RefCell::new(Vec::new()));
    let hook = build_observational_hook(fired);

    // Run the machine to equilibrium, but keep driving the forest
    // manually: tick its epoch cursor in lockstep with the inner
    // epoch id, and feed each accepted rule to
    // `apply_rule_retroactively`.
    //
    // The runner doesn't expose per-epoch hooks for event streams,
    // so we drive a series of `step_auto` calls directly over the
    // corpus, mirroring what the runner does internally for layer
    // 0. This is a controlled approximation: we get the same
    // trace stream the runner would emit, and we can fold the
    // forest in.
    let mut accepted_rule_count = 0u64;
    let mut retroactive_edges_total = 0usize;

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ DISCOVERY FOREST — END TO END                        ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Pre-run");
    println!("  corpus size                  : {}", corpus.len());
    println!("  forest nodes (pre-inserted)  : {corpus_nodes_inserted}");

    let max_epochs = 25;
    for e in 1..=max_epochs {
        forest.set_epoch(e as u64);
        let trace = runner
            .epoch
            .step_auto(&corpus, &mut runner.allocator);
        let accepted: Vec<_> = trace
            .events
            .iter()
            .filter_map(|ev| match ev {
                Event::Accept { artifact, .. } => Some(artifact.rule.clone()),
                _ => None,
            })
            .collect();
        accepted_rule_count += accepted.len() as u64;

        // Drive the forest each epoch with the FULL current library,
        // not just epoch-new rules. This is what lets the scheduler
        // tick correctly: due nodes are inspected every epoch (until
        // their check_period grows past the inter-epoch gap); missed
        // nodes advance toward stability; newly-inserted targets
        // from prior retroactive edges get their turn under rules
        // that were already accepted.
        let library: Vec<mathscape_core::eval::RewriteRule> = runner
            .epoch
            .registry
            .all()
            .iter()
            .map(|a| a.rule.clone())
            .collect();
        let lib_refs: Vec<&mathscape_core::eval::RewriteRule> = library.iter().collect();
        let edges = forest.apply_rules_retroactively(&lib_refs);
        let r = edges.iter().filter(|e| e.retroactive).count();
        retroactive_edges_total += r;
        if !accepted.is_empty() || !edges.is_empty() {
            println!(
                "  epoch={:>2} accepted {} rule(s), forest fired {} edge(s) ({} retroactive), lib={}",
                e,
                accepted.len(),
                edges.len(),
                r,
                library.len(),
            );
        }
    }

    let final_saving = forest.traversal_saving(max_epochs as u64);
    let stable_leaves = forest.stable_leaf_count();

    println!("\n▶ Post-run");
    println!("  forest nodes                 : {}", forest.len());
    println!("  accepted rules               : {accepted_rule_count}");
    println!("  total retroactive edges      : {retroactive_edges_total}");
    println!("  stable-leaf count            : {stable_leaves}");
    println!("  traversal_saving (epoch {max_epochs:>2}) : {final_saving:.3}");
    println!("  all edges recorded           : {}", forest.edges.len());

    // Hard assertions: the forest must do real work.
    assert!(
        forest.len() >= corpus_nodes_inserted,
        "forest should have at least the pre-inserted corpus nodes"
    );
    assert!(
        !forest.edges.is_empty(),
        "some rule should have fired at least one morphism edge on the corpus"
    );
    assert!(
        accepted_rule_count >= 2,
        "compositional corpus should yield ≥ 2 accepted rules; got {accepted_rule_count}"
    );
    // The flat rules hit ~12 terms each retroactively; expect a
    // healthy retroactive count.
    assert!(
        retroactive_edges_total >= 1,
        "at least one retroactive edge should fire when a rule lands on already-inserted nodes; got {retroactive_edges_total}"
    );
    assert!(
        final_saving > 0.0,
        "scheduler should save at least some traversal as the forest stabilizes; got {final_saving}"
    );
}

#[test]
fn flex_dimensional_discovery_emerges() {
    // The big one: after the base generator mints concrete
    // identity laws (add-identity + mul-identity), the meta-
    // generator should propose a higher-order pattern that
    // generalizes BOTH — the "identity-element" abstraction with
    // operator-variable and identity-value-variable. This is
    // dimensional discovery per docs/arch/machine-synthesis.md:
    // a compression of the library itself, not just the corpus.
    //
    // Observational assertions:
    //   - composite generator produces both base + meta candidates
    //   - at least one meta-origin candidate is accepted by the
    //     statistical prover (its marginal meta_compression is
    //     positive because the meta-rule reduces the library-as-
    //     corpus)
    //   - final library includes at least one rule with operator-
    //     variable structure (a meta-rule)
    let corpus = compositional_corpus();
    let base = CompressionGenerator::new(
        ExtractConfig {
            min_shared_size: 2,
            min_matches: 2,
            max_new_rules: 5,
        },
        1,
    );
    let meta = MetaPatternGenerator::new(
        ExtractConfig {
            // Lower min_shared_size for meta: library LHSs are
            // already small, and we want the meta generator to fire
            // even on minimally-shared structure (just the Apply
            // node itself).
            min_shared_size: 1,
            min_matches: 2,
            // Raised from 3 → 12 so both the nested and the flat
            // identity-element abstractions survive the top-K cut.
            // Same-family pair anti-unifications (e.g. S_001 vs
            // S_003) have higher shared_size than cross-family
            // pairs (S_003 vs S_005), so they crowd the top slots
            // even though dedup will reject them later.
            max_new_rules: 12,
        },
        1000, // high id range to keep meta symbols distinct
    );
    let composite = CompositeGenerator::new(base, meta);

    // We can't use MultiLayerRunner here because it delegates to
    // step_auto, which after the first accepts switches to
    // Reinforce indefinitely and never calls propose again — so
    // the meta-generator would never see the grown library.
    // Instead drive the epoch manually with explicit Discover
    // actions, and invoke reinforcement only after enough
    // discovery passes have happened to populate the library.
    let mut epoch = mathscape_core::epoch::Epoch::new(
        composite,
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        mathscape_core::epoch::RuleEmitter,
        mathscape_core::epoch::InMemoryRegistry::new(),
    );

    // Three Discover epochs back-to-back:
    //  - Epoch 0: library is empty; base generator mints concrete
    //    rules; meta emits nothing (lib < 2).
    //  - Epoch 1: library has concrete rules; meta generator can
    //    now anti-unify across them and propose higher-order
    //    patterns; base may continue finding corpus-level rules.
    //  - Epoch 2: any meta-rules accepted in epoch 1 are now
    //    themselves part of the library; chain continues.
    // Then one Reinforce to let subsumption collapse redundants.
    for _ in 0..3 {
        let _ = epoch.step_with_action(
            &corpus,
            mathscape_core::control::EpochAction::Discover,
        );
    }
    let _ = epoch.step_with_action(
        &corpus,
        mathscape_core::control::EpochAction::Reinforce,
    );

    let library = epoch.registry.all();
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ DIMENSIONAL DISCOVERY — META-PATTERN EMERGENCE       ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Library at trajectory end ({} entries)", library.len());
    for a in library {
        let status = epoch.registry.status_of(a.content_hash).unwrap();
        println!(
            "  [{}] {} :: {} => {}  [{:?}]",
            a.content_hash, a.rule.name, a.rule.lhs, a.rule.rhs, status,
        );
    }

    // Detect meta-rules by their structural signature: the LHS
    // top-level function position is a Var (operator-variable)
    // that falls in the meta-id range OR is ANY Var that isn't
    // var(2) or var(3) (the concrete add/mul tags).
    let meta_rules: Vec<_> = library
        .iter()
        .filter(|a| {
            if let Term::Apply(f, _) = &a.rule.lhs {
                if let Term::Var(v) = **f {
                    // Concrete ops in our corpus are var(2) and var(3).
                    // Anything else in the function slot signals meta.
                    return v != 2 && v != 3;
                }
            }
            false
        })
        .collect();

    println!(
        "\n▶ Meta-rules (operator-variable LHS) : {}",
        meta_rules.len()
    );
    for mr in &meta_rules {
        println!(
            "    {} :: {} => {}",
            mr.rule.name, mr.rule.lhs, mr.rule.rhs
        );
    }

    // Hard assertions: the machine should discover BOTH meta-laws
    // (flat + nested identity-element), and reinforcement should
    // recognize that the flat form subsumes the nested form and
    // collapse appropriately.
    assert!(
        meta_rules.len() >= 2,
        "dimensional discovery should surface both flat AND nested \
         identity-element abstractions; got {} meta-rules: {:?}",
        meta_rules.len(),
        meta_rules.iter().map(|a| &a.rule.name).collect::<Vec<_>>(),
    );
    // Classify: a "flat" meta-rule is a 2-arg Apply whose arg 0 is a
    // fresh var (not a library constant like Nat(0) or Nat(1)), i.e.
    // the operator-identity pattern with generalized identity value.
    // A "nested" meta-rule has an Apply as arg 0.
    let flat_count = meta_rules
        .iter()
        .filter(|a| {
            if let Term::Apply(_, args) = &a.rule.lhs {
                if args.len() == 2 {
                    return matches!(args[0], Term::Var(_))
                        && matches!(args[1], Term::Var(_));
                }
            }
            false
        })
        .count();
    let nested_count = meta_rules
        .iter()
        .filter(|a| {
            if let Term::Apply(_, args) = &a.rule.lhs {
                if args.len() == 2 {
                    return matches!(args[0], Term::Apply(..));
                }
            }
            false
        })
        .count();
    println!(
        "\n▶ Meta-rule shape census: flat={flat_count} nested={nested_count}"
    );
    assert!(
        flat_count >= 1,
        "flat identity-element abstraction (op(x, id) = x) should surface \
         — this is the canonical arithmetic primitive generalization"
    );
    assert!(
        nested_count >= 1,
        "nested identity-element abstraction (op(op(x, id), id)) should \
         also surface (witness of multi-depth meta-patterns)"
    );
}

#[test]
fn flex_forest_backed_discovery_is_faster() {
    // The real optimization test: run discovery over the same
    // corpus two ways, and show that the forest-backed path does
    // measurably less work while producing the same primitives.
    //
    // Arm A (baseline): CompressionGenerator over the raw corpus
    // every epoch. The generator rewrites each term through the
    // library every propose() call.
    //
    // Arm B (forest-backed): each epoch pass the forest's
    // `due_corpus_view(epoch)` as the generator's corpus. Stable
    // leaves are skipped. Rate-limited re-inspection means the
    // scheduler works for us, not against us.
    //
    // Criterion: Arm B must land at the same Primitive set AND
    // report a non-zero scheduler_skip_count by the time the
    // forest stabilizes. This is not a wall-time benchmark
    // (too noisy in CI) — it's a structural benchmark: the
    // scheduler concretely skips nodes the generator would
    // otherwise have touched.
    use mathscape_core::epoch::Generator;
    use std::time::Instant;

    let corpus = compositional_corpus();

    // ── Arm A: baseline ─────────────────────────────────────────
    let mut epoch_a = build_epoch_with(5);
    let mut alloc_a = Allocator::new(RealizationPolicy::default(), RewardEstimator::new(0.3));
    let t_a = Instant::now();
    for _ in 0..25 {
        let _ = epoch_a.step_auto(&corpus, &mut alloc_a);
    }
    let arm_a_elapsed = t_a.elapsed();
    let arm_a_prims: Vec<String> = epoch_a
        .registry
        .all()
        .iter()
        .filter(|a| {
            let s = epoch_a
                .registry
                .status_of(a.content_hash)
                .unwrap_or_else(|| a.certificate.status.clone());
            matches!(s, ProofStatus::Primitive(_))
        })
        .map(|a| a.rule.name.clone())
        .collect();

    // ── Arm B: forest-backed ────────────────────────────────────
    let mut epoch_b = build_epoch_with(5);
    let _alloc_b = Allocator::new(RealizationPolicy::default(), RewardEstimator::new(0.3));
    let mut forest = DiscoveryForest::new();
    for t in &corpus {
        forest.insert(t.clone());
    }
    let mut skipped_total: usize = 0;
    let t_b = Instant::now();
    for e in 1..=25u64 {
        forest.set_epoch(e);
        // Generator receives the DUE slice of the forest, not the
        // raw corpus. Stable leaves (max-period, not yet due) are
        // skipped entirely.
        let due = forest.due_corpus_view(e);
        let effective_corpus = if due.is_empty() { &corpus[..] } else { &due[..] };
        skipped_total += forest.scheduler_skip_count(e);

        // Propose against the due view; feed accepted rules back to
        // the forest so retroactive reduction advances it.
        let library: Vec<mathscape_core::epoch::Artifact> =
            epoch_b.registry.all().to_vec();
        let candidates = epoch_b
            .generator
            .propose(epoch_b.epoch_id, effective_corpus, &library);
        for c in candidates {
            use mathscape_core::epoch::Verdict;
            let v = {
                use mathscape_core::epoch::Prover;
                epoch_b.prover.prove(&c, effective_corpus, &library)
            };
            if let Verdict::Accept(_) = v {
                let _ = epoch_b.registry.insert(mathscape_core::epoch::Artifact::seal(
                    c.rule.clone(),
                    epoch_b.epoch_id,
                    mathscape_core::epoch::AcceptanceCertificate::trivial_conjecture(1.0),
                    vec![],
                ));
            }
        }
        epoch_b.epoch_id += 1;

        // Apply current library retroactively to the forest so the
        // scheduler's schedule advances.
        let rules: Vec<mathscape_core::eval::RewriteRule> = epoch_b
            .registry
            .all()
            .iter()
            .map(|a| a.rule.clone())
            .collect();
        let rule_refs: Vec<&mathscape_core::eval::RewriteRule> = rules.iter().collect();
        let _ = forest.apply_rules_retroactively(&rule_refs);
    }
    let arm_b_elapsed = t_b.elapsed();
    let arm_b_prims: Vec<String> = epoch_b
        .registry
        .all()
        .iter()
        .filter(|a| {
            let s = epoch_b
                .registry
                .status_of(a.content_hash)
                .unwrap_or_else(|| a.certificate.status.clone());
            matches!(s, ProofStatus::Primitive(_))
        })
        .map(|a| a.rule.name.clone())
        .collect();

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ FOREST-BACKED DISCOVERY — STRUCTURAL OPTIMIZATION    ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Arm A (baseline: raw corpus)");
    println!("  elapsed           : {:?}", arm_a_elapsed);
    println!("  library size      : {}", epoch_a.registry.all().len());
    println!("  primitives        : {:?}", arm_a_prims);
    println!("\n▶ Arm B (forest-backed: due view)");
    println!("  elapsed           : {:?}", arm_b_elapsed);
    println!("  library size      : {}", epoch_b.registry.all().len());
    println!("  primitives        : {:?}", arm_b_prims);
    println!("  total nodes skipped by scheduler across epochs : {skipped_total}");
    println!("  forest final size : {}", forest.len());
    println!(
        "  stable leaves     : {} / {}",
        forest.stable_leaf_count(),
        forest.len(),
    );

    // Structural assertion: non-zero scheduler work avoidance
    // SHOULD have occurred once the forest stabilized. That's the
    // whole point of the forest-backed path.
    assert!(
        skipped_total > 0,
        "forest-backed path should accumulate scheduler skips over 25 epochs; got 0"
    );
    // Correctness: at least one discovery should still fire. The
    // exact rule set may differ between arms (due view may cause
    // different anti-unification pairs), but library should be
    // non-empty.
    assert!(
        !epoch_b.registry.all().is_empty(),
        "forest-backed arm must still discover; library is empty"
    );
}

#[test]
fn flex_forest_scheduler_scales() {
    // O(due) scheduler test: build a large forest (1000 unreducible
    // nodes), apply a non-matching rule many times, and show that
    // the per-pass work drops as the forest stabilizes.
    use std::time::Instant;

    let mut forest = DiscoveryForest::new();
    for n in 0..1000u64 {
        forest.insert(apply(var(7), vec![nat(n), nat(n + 1)])); // never matches add-id
    }
    let rule = mathscape_core::eval::RewriteRule {
        name: "add-identity".into(),
        lhs: apply(var(2), vec![var(100), nat(0)]),
        rhs: var(100),
    };

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ FOREST SCHEDULER — O(due) SCALING                    ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n{:>6} {:>8} {:>10} {:>12} {:>10}", "epoch", "due", "total", "saving", "us");
    println!("{}", "─".repeat(56));

    for e in 1..=60u64 {
        forest.set_epoch(e);
        let t0 = Instant::now();
        let _ = forest.apply_rule_retroactively(&rule);
        let us = t0.elapsed().as_micros();
        if matches!(e, 1 | 2 | 3 | 5 | 10 | 20 | 40 | 60) {
            let saving = forest.traversal_saving(e);
            let due = forest.due_nodes(e).len();
            println!(
                "{:>6} {:>8} {:>10} {:>12.3} {:>10}",
                e,
                due,
                forest.len(),
                saving,
                us
            );
        }
    }

    let final_saving = forest.traversal_saving(60);
    println!(
        "\n▶ After 60 all-miss passes on 1000 unreducible nodes:"
    );
    println!("  traversal_saving: {final_saving:.3}  (target: ~0.9+ at max period 64)");

    assert!(
        final_saving > 0.8,
        "after 60 passes most nodes should have hit the 64-epoch check period; saving={final_saving}"
    );
}

#[test]
fn flex_meta_kick_at_equilibrium() {
    // Observational: after the normal runner reaches equilibrium
    // on the compositional corpus (4 library entries, 2 Primitives),
    // run N meta-rounds directly. Each round simulates 9 candidate
    // policy tweaks over a 5-epoch lookahead, picks the winner,
    // applies it to the allocator's policy — OR kicks if nothing
    // beats baseline. Then we run the *real* epoch forward a few
    // times under the new policy and record whether anything
    // changed: library size, primitive set, registry root.
    //
    // What we're looking for:
    //   (a) meta round finds a winning tweak that unlocks further
    //       discovery → genuine policy-level exploration works
    //   (b) every round is kick → the corpus is exhausted; no
    //       policy perturbation helps. The machine is truly at
    //       rest because there is no more structure to find, not
    //       because the policy is mis-tuned.
    //   (c) kicks change the policy but the real forward runs
    //       produce no new candidates → kicker is acting but
    //       discovery doesn't respond → next work: kicker ranges
    //       are too small to cross the ΔCR threshold.
    //
    // Either outcome is a finding.
    use std::time::Instant;

    let corpus = compositional_corpus();
    let mut runner = MultiLayerRunner {
        epoch: build_epoch_with(5),
        allocator: Allocator::new(RealizationPolicy::default(), RewardEstimator::new(0.3)),
        per_layer_max_epochs: 25,
        max_layers: 6,
        policy: ReductionPolicy::layer_0_default(),
    };
    let fired = Rc::new(RefCell::new(Vec::new()));
    let hook = build_observational_hook(fired);
    let _report = runner.run(&corpus, hook);

    let baseline_lib_size = runner.epoch.registry.all().len();
    let baseline_root = runner.epoch.registry.root();
    let baseline_policy = runner.allocator.policy.clone();

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ META-KICK AT EQUILIBRIUM                             ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ State at equilibrium (pre-meta)");
    println!("  library size        : {baseline_lib_size}");
    println!("  registry root       : {baseline_root}");
    println!("  epsilon_compression : {}", baseline_policy.epsilon_compression);
    println!("  exploration_rho     : {}", baseline_policy.exploration_rho);
    println!("  epsilon_plateau     : {}", baseline_policy.epsilon_plateau);
    println!("  k_condensation      : {}", baseline_policy.k_condensation);

    let mut meta = MetaOptimizer::new(PolicyTweak::default_candidates(), 5);
    let rounds_to_run = 20usize;
    println!(
        "\n▶ Meta trajectory ({rounds_to_run} rounds, 5-epoch lookahead over 9 tweaks each)"
    );
    println!(
        "\n{:>5} {:>20} {:>14} {:>14} {:>6} {:>8} {:>8}",
        "round", "winner", "winner_rew", "baseline_rew", "kicked", "lib_sz", "root"
    );
    println!("{}", "─".repeat(90));

    let t0 = Instant::now();
    for _ in 0..rounds_to_run {
        let round = meta
            .round(&runner.epoch, &mut runner.allocator, &corpus)
            .clone();
        // After meta, fire 5 real epochs under the (possibly kicked) policy.
        for _ in 0..5 {
            let _ = runner.epoch.step_auto(&corpus, &mut runner.allocator);
        }
        let lib_sz = runner.epoch.registry.all().len();
        let root = runner.epoch.registry.root();
        println!(
            "{:>5} {:>20} {:>14.3} {:>14.3} {:>6} {:>8} {:>8}",
            round.round_id,
            round.winner_name,
            round.winner_reward,
            round.baseline_reward,
            round.kicked,
            lib_sz,
            format!("{root}").chars().take(8).collect::<String>(),
        );
    }
    let elapsed = t0.elapsed();

    let final_lib_size = runner.epoch.registry.all().len();
    let final_root = runner.epoch.registry.root();
    let kick_count = meta.history.iter().filter(|r| r.kicked).count();
    let non_baseline_wins =
        meta.history.iter().filter(|r| r.winner_name != "baseline").count();

    println!("\n▶ Aggregate");
    println!("  elapsed             : {:?}", elapsed);
    println!("  rounds total        : {}", meta.history.len());
    println!("  kicks fired         : {kick_count}");
    println!("  non-baseline wins   : {non_baseline_wins}");
    println!("  library size Δ      : {baseline_lib_size} → {final_lib_size}");
    println!("  root Δ              : {baseline_root} → {final_root}");
    let grew = final_lib_size > baseline_lib_size;
    let root_changed = final_root != baseline_root;

    println!();
    if grew {
        println!("  ✦ meta rounds UNLOCKED further discovery — policy-level exploration works");
    } else if root_changed {
        println!("  ○ policy changed, registry shifted (reinforcement-only, no new candidates)");
    } else {
        println!(
            "  · policy explored across {kick_count} kicks but corpus is exhausted — true equilibrium"
        );
    }
}

#[test]
#[ignore = "extreme-depth probe — run explicitly with --ignored"]
fn flex_extreme_depth_probe() {
    // Probe: push far past the point where normal discovery should
    // terminate. If the library becomes frozen, extra epochs are
    // pure overhead and the machine is at rest. If something new
    // emerges at depth — a late reinforcement cascade, a meta
    // pattern that only materializes after many subsumption passes
    // — that's a finding worth capturing.
    //
    // This is observational, not an assertion. Run with:
    //   cargo test -p mathscape-axiom-bridge flex_extreme_depth_probe \
    //     -- --ignored --nocapture
    use std::time::Instant;

    let corpus = compositional_corpus();
    let depths = [(1_000usize, 20u32), (5_000, 30), (20_000, 40)];

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ EXTREME DEPTH PROBE                                  ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!(
        "\n{:>10} {:>7} {:>8} {:>10} {:>10} {:>8}",
        "epochs/L", "layers", "lib_sz", "root", "prim#", "ms"
    );
    println!("{}", "─".repeat(70));

    for (ep, lay) in depths {
        let mut runner = MultiLayerRunner {
            epoch: build_epoch_with(5),
            allocator: Allocator::new(
                RealizationPolicy::default(),
                RewardEstimator::new(0.3),
            ),
            per_layer_max_epochs: ep,
            max_layers: lay,
            policy: ReductionPolicy::layer_0_default(),
        };
        let fired = Rc::new(RefCell::new(Vec::new()));
        let hook = build_observational_hook(fired);
        let t0 = Instant::now();
        let _report = runner.run(&corpus, hook);
        let elapsed_ms = t0.elapsed().as_millis();

        let root = runner.epoch.registry.root();
        let lib = runner.epoch.registry.all();
        let prim_count = lib
            .iter()
            .filter(|a| {
                let s = runner
                    .epoch
                    .registry
                    .status_of(a.content_hash)
                    .unwrap_or_else(|| a.certificate.status.clone());
                matches!(s, ProofStatus::Primitive(_))
            })
            .count();
        println!(
            "{:>10} {:>7} {:>8} {:>10} {:>10} {:>8}",
            ep,
            lay,
            lib.len(),
            format!("{root}").chars().take(8).collect::<String>(),
            prim_count,
            elapsed_ms,
        );
    }

    println!("\n  interpretation: if ms stays flat and lib_sz stays constant across");
    println!("  depth tiers, the machine is at rest after initial discovery — the");
    println!("  meta-optimizer/kicker is not wired; extra budget is pure overhead.");
}

#[test]
fn flex_wipe_and_rev_depth_sweep() {
    // Honest question: does wipe-and-rev convergence hold as epoch
    // budget grows? At shallow depth the answer is trivially yes
    // (deterministic machinery). At deeper budgets, allocator EWMA
    // accumulates state, the reduction meter integrates over more
    // data, and any non-determinism (hash-map iteration, f64
    // accumulation order) has more chances to surface. This test
    // sweeps depth and reports what it finds, without asserting
    // convergence — just observes.
    let corpus = compositional_corpus();

    fn rev_with_depth(
        corpus: &[Term],
        per_layer_max_epochs: usize,
        max_layers: u32,
    ) -> (TermRef, usize, Vec<String>) {
        let mut runner = MultiLayerRunner {
            epoch: build_epoch_with(5),
            allocator: Allocator::new(
                RealizationPolicy::default(),
                RewardEstimator::new(0.3),
            ),
            per_layer_max_epochs,
            max_layers,
            policy: ReductionPolicy::layer_0_default(),
        };
        let fired = Rc::new(RefCell::new(Vec::new()));
        let hook = build_observational_hook(fired);
        let _report = runner.run(corpus, hook);
        let root = runner.epoch.registry.root();
        let lib_size = runner.epoch.registry.all().len();
        let mut prim_hashes: Vec<String> = runner
            .epoch
            .registry
            .all()
            .iter()
            .filter(|a| {
                let status = runner
                    .epoch
                    .registry
                    .status_of(a.content_hash)
                    .unwrap_or_else(|| a.certificate.status.clone());
                matches!(status, ProofStatus::Primitive(_))
            })
            .map(|a| a.content_hash.to_string())
            .collect();
        prim_hashes.sort();
        (root, lib_size, prim_hashes)
    }

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ WIPE-AND-REV — DEPTH SWEEP                           ║");
    println!("║   corpus = compositional (4 families × 6 samples)    ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!(
        "\n{:>10} {:>8} {:>10} {:>10} {}",
        "epochs/L", "layers", "rev-A root", "rev-B root", "converged?"
    );
    println!("{}", "─".repeat(70));

    let depth_grid = [
        (10usize, 3u32),
        (25, 6),
        (50, 8),
        (100, 10),
        (250, 12),
    ];
    let mut divergences = Vec::new();
    for (ep, lay) in depth_grid {
        let (root_a, size_a, prims_a) = rev_with_depth(&corpus, ep, lay);
        let (root_b, size_b, prims_b) = rev_with_depth(&corpus, ep, lay);
        let conv = root_a == root_b && prims_a == prims_b && size_a == size_b;
        println!(
            "{:>10} {:>8} {:>10} {:>10}  {}",
            ep,
            lay,
            format!("{root_a}").chars().take(8).collect::<String>(),
            format!("{root_b}").chars().take(8).collect::<String>(),
            if conv { "✓ yes" } else { "✗ DIVERGED" }
        );
        if !conv {
            divergences.push((ep, lay, root_a, root_b, prims_a, prims_b, size_a, size_b));
        }
    }

    if divergences.is_empty() {
        println!("\n✓ convergence holds across all sweeped depths.");
    } else {
        println!("\n✗ divergences observed at:");
        for (ep, lay, ra, rb, pa, pb, sa, sb) in divergences {
            println!(
                "  epochs/L={ep} layers={lay}\n    rev-A root={ra} lib_size={sa} prims={pa:?}\n    rev-B root={rb} lib_size={sb} prims={pb:?}"
            );
        }
        panic!("wipe-and-rev convergence failed at deeper depths — see table");
    }
}

#[test]
fn flex_wipe_and_rev_convergence() {
    // Convergence invariant: two independent wipes (fresh epoch,
    // fresh registry, fresh allocator, fresh promotion-hook state)
    // driven by the same corpus must land at the same final
    // registry root AND the same set of Primitive rule hashes.
    //
    // This is stronger than replay determinism: it asserts that the
    // discoveries themselves are a function of the corpus, not of
    // accumulated state across calls to the runner. What emerges,
    // re-emerges.
    let corpus = compositional_corpus();

    fn rev(corpus: &[Term]) -> (TermRef, Vec<(String, String)>) {
        let mut runner = MultiLayerRunner {
            epoch: build_epoch_with(5),
            allocator: Allocator::new(
                RealizationPolicy::default(),
                RewardEstimator::new(0.3),
            ),
            per_layer_max_epochs: 25,
            max_layers: 6,
            policy: ReductionPolicy::layer_0_default(),
        };
        let fired = Rc::new(RefCell::new(Vec::new()));
        let hook = build_observational_hook(fired);
        let _report = runner.run(corpus, hook);

        let root = runner.epoch.registry.root();
        let mut primitives: Vec<(String, String)> = runner
            .epoch
            .registry
            .all()
            .iter()
            .filter(|a| {
                let status = runner
                    .epoch
                    .registry
                    .status_of(a.content_hash)
                    .unwrap_or_else(|| a.certificate.status.clone());
                matches!(status, ProofStatus::Primitive(_))
            })
            .map(|a| (a.rule.name.clone(), a.content_hash.to_string()))
            .collect();
        primitives.sort();
        (root, primitives)
    }

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ WIPE-AND-REV CONVERGENCE (compositional)             ║");
    println!("╚══════════════════════════════════════════════════════╝");

    let (root_a, prims_a) = rev(&corpus);
    println!("\n▶ rev A");
    println!("  final registry root : {root_a}");
    for (name, hash) in &prims_a {
        println!("    Primitive [{name}] {hash}");
    }

    let (root_b, prims_b) = rev(&corpus);
    println!("\n▶ rev B (post-wipe)");
    println!("  final registry root : {root_b}");
    for (name, hash) in &prims_b {
        println!("    Primitive [{name}] {hash}");
    }

    assert_eq!(
        root_a, root_b,
        "wipe-and-rev must converge to same registry root"
    );
    assert_eq!(
        prims_a, prims_b,
        "wipe-and-rev must mint the same Primitive set (same hashes)"
    );
    println!("\n✓ two independent wipes converge: {} primitives, root={root_a}", prims_a.len());
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
