//! R23 — Repeated-arrival observation harness.
//!
//! The user asked: "observe it naturally arrive, observe everything
//! about that, then destroy it and re-run over and over so we
//! learn from it."
//!
//! This harness runs autonomous traversal N times with a product
//! of variation dimensions (budget × depth × seed_offset). Between
//! each run, state is fully destroyed — fresh forest, fresh
//! registry, fresh epoch. For each run it records:
//!
//!   - every active rule (name + LHS + RHS)
//!   - R12 primitive classification
//!   - R8 tensor shape
//!   - apex rule set (the rules that reached Axiomatized)
//!   - run config that produced it
//!
//! At the end, aggregates:
//!
//!   - **Rule frequency**: which rule names appear across runs,
//!     how often, under which configs
//!   - **Shape frequency**: which R12 primitive SHAPES appear,
//!     how often
//!   - **Apex stability**: is the Axiomatized set invariant
//!     across variations, or does it drift with seed/budget?
//!   - **Novel discoveries**: rules that emerged in exactly one
//!     run (candidate rare findings worth investigating)
//!
//! This is the "learn from repeated arrivals" instrument. Running
//! it is cheap; the data it produces is the substrate for any
//! subsequent hypothesis about the machine's discovery dynamics.

mod common;

use common::{canonical_zoo, procedural};
use mathscape_compress::{
    extract::ExtractConfig, CompositeGenerator, CompressionGenerator,
    MetaPatternGenerator,
};
use mathscape_core::{
    epoch::{Epoch, InMemoryRegistry, Registry, RuleEmitter},
    eval::RewriteRule,
    form_tree::DiscoveryForest,
    lifecycle::ProofStatus,
    primitives::{classify_primitives, MlPrimitive},
    tensor::{self, TensorShape},
};
use mathscape_reward::{reward::RewardConfig, StatisticalProver};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
struct RunConfig {
    budget: usize,
    depth: usize,
    seed_offset: u64,
}

#[derive(Debug, Clone)]
struct RunOutcome {
    config: RunConfig,
    /// All ACTIVE rules in the library (not Subsumed, not Demoted).
    active_rules: Vec<RewriteRule>,
    /// Rules that reached Axiomatized status.
    axiomatized_names: Vec<String>,
    /// R12 primitive shapes that emerged (deduplicated labels).
    primitive_labels: Vec<String>,
    /// R8 tensor shapes counted.
    tensor_distributive: usize,
    tensor_meta_distributive: usize,
}

fn run_one(config: RunConfig) -> RunOutcome {
    let mut zoo = canonical_zoo();
    for seed in 1..=config.budget as u64 {
        let actual_seed = seed.wrapping_add(config.seed_offset);
        let d = 2 + (actual_seed as usize % (config.depth - 1).max(1));
        let count = 16 + (actual_seed as usize % 8);
        zoo.push((
            format!("proc-s{actual_seed}-d{d}"),
            procedural(actual_seed, d, count),
        ));
    }

    let mut forest = DiscoveryForest::new();
    let base = CompressionGenerator::new(
        ExtractConfig {
            min_shared_size: 2,
            min_matches: 2,
            max_new_rules: 10,
        },
        1,
    );
    let meta = MetaPatternGenerator::new(
        ExtractConfig {
            min_shared_size: 1,
            min_matches: 2,
            max_new_rules: 12,
        },
        10_000,
    );
    let mut epoch = Epoch::new(
        CompositeGenerator::new(base, meta),
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        InMemoryRegistry::new(),
    );

    let mut global_epoch = 0u64;
    for (_name, corpus) in &zoo {
        global_epoch += 1;
        forest.set_epoch(global_epoch);
        for t in corpus {
            forest.insert(t.clone());
        }
        for _ in 0..3 {
            let _ = epoch.step_with_action(
                corpus,
                mathscape_core::control::EpochAction::Discover,
            );
        }
        let _ = epoch.step_with_action(
            corpus,
            mathscape_core::control::EpochAction::Reinforce,
        );
        global_epoch += 1;
        forest.set_epoch(global_epoch);
        let library_rules: Vec<_> = epoch
            .registry
            .all()
            .iter()
            .map(|a| (a.content_hash, a.rule.clone()))
            .collect();
        let rule_refs: Vec<&RewriteRule> =
            library_rules.iter().map(|(_, r)| r).collect();
        let _ = forest.apply_rules_retroactively(&rule_refs);
    }

    // Harvest outcome and destroy the run state (forest, epoch, registry
    // go out of scope on function return).
    let mut active_rules: Vec<RewriteRule> = Vec::new();
    let mut axiomatized_names: Vec<String> = Vec::new();
    for artifact in epoch.registry.all() {
        let status = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        let is_active = !matches!(
            status,
            ProofStatus::Subsumed(_) | ProofStatus::Demoted(_)
        );
        if is_active {
            active_rules.push(artifact.rule.clone());
        }
        if let ProofStatus::Axiomatized = status {
            axiomatized_names.push(artifact.rule.name.clone());
        }
    }

    let mut primitive_labels: Vec<String> = Vec::new();
    for rule in &active_rules {
        for p in classify_primitives(rule) {
            let label = match p {
                MlPrimitive::LeftIdentity { op, .. } => {
                    format!("left-identity/{op}")
                }
                MlPrimitive::RightIdentity { op, .. } => {
                    format!("right-identity/{op}")
                }
                MlPrimitive::Involution { f } => format!("involution/{f}"),
                MlPrimitive::Idempotence { f } => format!("idempotence/{f}"),
                MlPrimitive::LeftDistributive { outer, inner } => {
                    format!("left-distrib/{outer}-{inner}")
                }
                MlPrimitive::RightDistributive { outer, inner } => {
                    format!("right-distrib/{outer}-{inner}")
                }
                MlPrimitive::Homomorphism { f, op } => {
                    format!("homomorphism/{f}-{op}")
                }
                MlPrimitive::MetaDistributive => "meta-distributive".into(),
                MlPrimitive::MetaIdentity => "meta-identity".into(),
            };
            if !primitive_labels.contains(&label) {
                primitive_labels.push(label);
            }
        }
    }
    primitive_labels.sort();

    let mut tensor_distributive = 0;
    let mut tensor_meta_distributive = 0;
    for rule in &active_rules {
        match tensor::classify(rule) {
            TensorShape::Distributive { .. } => tensor_distributive += 1,
            TensorShape::MetaDistributive => tensor_meta_distributive += 1,
            TensorShape::None => {}
        }
    }

    RunOutcome {
        config,
        active_rules,
        axiomatized_names,
        primitive_labels,
        tensor_distributive,
        tensor_meta_distributive,
    }
}

#[test]
fn repeated_natural_arrival_observation() {
    // Run the machine N times across varied config. Between runs,
    // destroy all state (forest, epoch, registry) by letting them
    // go out of scope. Re-run with fresh state. Collect outcomes.
    //
    // Variation dimensions:
    //   - budget: {5, 10, 15}
    //   - depth: {3, 4}
    //   - seed_offset: {0, 100, 1000}
    //
    // 3 × 2 × 3 = 18 runs.

    let budgets = [5usize, 10, 15];
    let depths = [3usize, 4];
    let seed_offsets = [0u64, 100, 1000];

    let mut outcomes: Vec<RunOutcome> = Vec::new();
    for b in &budgets {
        for d in &depths {
            for s in &seed_offsets {
                let cfg = RunConfig {
                    budget: *b,
                    depth: *d,
                    seed_offset: *s,
                };
                let out = run_one(cfg.clone());
                outcomes.push(out);
            }
        }
    }

    // ── Aggregate across runs ────────────────────────────────────
    let n_runs = outcomes.len();

    // Rule-name frequency (which S_NNN appeared how often).
    let mut rule_name_freq: BTreeMap<String, usize> = BTreeMap::new();
    for out in &outcomes {
        for r in &out.active_rules {
            *rule_name_freq.entry(r.name.clone()).or_default() += 1;
        }
    }

    // Apex frequency.
    let mut apex_freq: BTreeMap<String, usize> = BTreeMap::new();
    for out in &outcomes {
        for a in &out.axiomatized_names {
            *apex_freq.entry(a.clone()).or_default() += 1;
        }
    }

    // Primitive-shape frequency.
    let mut shape_freq: BTreeMap<String, usize> = BTreeMap::new();
    for out in &outcomes {
        for label in &out.primitive_labels {
            *shape_freq.entry(label.clone()).or_default() += 1;
        }
    }

    // Universal / intermittent / rare classification.
    let universal: Vec<&String> = rule_name_freq
        .iter()
        .filter(|(_, c)| **c == n_runs)
        .map(|(k, _)| k)
        .collect();
    let rare: Vec<&String> = rule_name_freq
        .iter()
        .filter(|(_, c)| **c == 1)
        .map(|(k, _)| k)
        .collect();
    let intermittent: Vec<(&String, &usize)> = rule_name_freq
        .iter()
        .filter(|(_, c)| **c > 1 && **c < n_runs)
        .map(|(k, v)| (k, v))
        .collect();

    // Apex stability: is the axiomatized set invariant across runs?
    let apex_universal: Vec<&String> = apex_freq
        .iter()
        .filter(|(_, c)| **c == n_runs)
        .map(|(k, _)| k)
        .collect();

    // ── Report ───────────────────────────────────────────────────
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ R23 REPEATED-ARRIVAL OBSERVATION                     ║");
    println!("║ {} runs across budget × depth × seed_offset        ║", n_runs);
    println!("║ Each run destroys prior state; starts fresh.         ║");
    println!("╚══════════════════════════════════════════════════════╝");

    println!("\n── Rule-name frequency ({} distinct) ─────────────────",
             rule_name_freq.len());
    for (name, count) in &rule_name_freq {
        println!("  {name:12} appeared in {count}/{n_runs} runs");
    }

    println!("\n── Apex frequency ({} distinct axiomatized) ──────────",
             apex_freq.len());
    for (name, count) in &apex_freq {
        println!("  {name:12} axiomatized in {count}/{n_runs} runs");
    }

    println!("\n── Shape (R12) frequency ({} distinct) ───────────────",
             shape_freq.len());
    for (label, count) in &shape_freq {
        println!("  {label:30} appeared in {count}/{n_runs} runs");
    }
    if shape_freq.is_empty() {
        println!("  (none — see R22 for why: the machine produces compression");
        println!("   rules, not law-shaped rules that R12 detects)");
    }

    println!("\n── Universality / rarity analysis ────────────────────");
    println!("  UNIVERSAL rules (present in all {n_runs} runs):");
    for r in &universal {
        println!("    ✓ {r}");
    }
    println!("  INTERMITTENT rules (some but not all runs):");
    for (r, c) in &intermittent {
        println!("    ~ {r} (in {c}/{n_runs})");
    }
    println!("  RARE rules (exactly 1 run — candidate novel discoveries):");
    for r in &rare {
        // Find which config produced this rare rule.
        let producing_config = outcomes
            .iter()
            .find(|o| o.active_rules.iter().any(|rule| rule.name == **r))
            .map(|o| o.config.clone());
        println!(
            "    · {r} (produced by config: {producing_config:?})"
        );
    }

    println!("\n── Apex stability ────────────────────────────────────");
    println!("  Apex rules universal across all {n_runs} configs:");
    for a in &apex_universal {
        println!("    ✓ {a}");
    }
    if apex_universal.len() == apex_freq.len() {
        println!("  → Apex is FULLY STABLE across all variation axes.");
        println!("    The machine's static arrival point is seed/budget-");
        println!("    invariant. Natural convergence target confirmed.");
    } else {
        println!("  → Apex VARIES by config. Investigate which configs");
        println!("    produce which apex rules.");
    }

    println!("\n── Tensor (R8) findings ──────────────────────────────");
    let total_tensor_dist: usize =
        outcomes.iter().map(|o| o.tensor_distributive).sum();
    let total_tensor_meta: usize =
        outcomes.iter().map(|o| o.tensor_meta_distributive).sum();
    println!("  Sum tensor-distributive detections: {total_tensor_dist}");
    println!("  Sum tensor-meta-distributive detections: {total_tensor_meta}");

    println!("\n── What we learned ───────────────────────────────────");
    println!("  1. Total distinct rules across {n_runs} runs: {}",
             rule_name_freq.len());
    println!("  2. Total distinct apex rules: {}", apex_freq.len());
    println!(
        "  3. ML-primitive shape instances detected: {}",
        shape_freq.values().sum::<usize>()
    );
    println!(
        "  4. Rule-discovery entropy (higher = more variation): universal={}, \
             intermittent={}, rare={}",
        universal.len(),
        intermittent.len(),
        rare.len()
    );
    println!(
        "  5. Apex-variation across {n_runs} configs: {}",
        if apex_universal.len() == apex_freq.len() {
            "ZERO (machine has a SINGLE fixed convergence target)"
        } else {
            "present (apex depends on config)"
        }
    );
}

/// Smaller/faster variant suitable for CI.
#[test]
fn repeated_arrival_smoke_test() {
    // 4 runs — minimum sufficient to detect variation patterns.
    let configs = [
        RunConfig { budget: 5, depth: 3, seed_offset: 0 },
        RunConfig { budget: 5, depth: 3, seed_offset: 100 },
        RunConfig { budget: 10, depth: 4, seed_offset: 0 },
        RunConfig { budget: 10, depth: 4, seed_offset: 100 },
    ];

    let outcomes: Vec<RunOutcome> = configs.iter().map(|c| run_one(c.clone())).collect();

    // Basic invariant: every run produced at least one active rule.
    for o in &outcomes {
        assert!(
            !o.active_rules.is_empty(),
            "every config must produce at least one rule; config: {:?}",
            o.config
        );
    }
}
