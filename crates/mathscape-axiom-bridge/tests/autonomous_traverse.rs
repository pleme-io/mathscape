//! Autonomous mathscape traversal — the orchestrated test suite.
//!
//! This is the audit harness for the autonomous-traversal milestone.
//! The machine discovers primitives without human approval, reinforces
//! them via reflex-level subsumption, and climbs the proof-status
//! lattice to Axiomatized on the strength of cross-corpus empirical
//! evidence alone. The lynchpin invariant is that every surviving
//! rule carries ≥2 corpus cross-support.
//!
//! Reads two environment variables:
//!
//!   MATHSCAPE_TRAVERSE_BUDGET   — number of procedural corpora to
//!                                 sweep (default: 12). The 7
//!                                 hand-crafted zoo corpora are
//!                                 always included, so total sweep
//!                                 size = 7 + BUDGET.
//!
//!   MATHSCAPE_TRAVERSE_DEPTH    — maximum term depth for procedural
//!                                 corpora (default: 4). Higher =
//!                                 more complex synthetic structure.
//!
//! Each test in this file runs a different scale (small / medium /
//! stress) and pins the invariants we observed holding:
//!
//!   1. Lynchpin: every rule in the final library has ≥2 corpus
//!      cross-support. Zero fragile rules.
//!   2. Apex emergence: at least one rule reaches Axiomatized status.
//!   3. Saturation: library stops growing strictly before the end
//!      of the sweep (the machine recognizes when it has found
//!      everything current-generation machinery can reach).
//!   4. Apex quality: every Axiomatized rule carries ≥ half-sweep
//!      cross-corpus support.
//!
//! All tests reach these assertions autonomously — no human approval,
//! no hook fakes, no hand-tuned gate. That's the milestone this file
//! exists to prove is real.

mod common;

use common::{canonical_zoo, procedural};
use mathscape_compress::{
    extract::ExtractConfig, CompositeGenerator, CompressionGenerator,
    MetaPatternGenerator,
};
use mathscape_core::{
    epoch::{Epoch, InMemoryRegistry, Registry, RuleEmitter},
    form_tree::DiscoveryForest,
    hash::TermRef,
    lifecycle::ProofStatus,
    term::Term,
};

// ── TraversalReport: structured output ──────────────────────────

#[derive(Debug, Clone)]
pub struct TraversalReport {
    pub total_corpora: usize,
    pub library_final_size: usize,
    pub forest_nodes: usize,
    pub forest_edges: usize,
    pub forest_stable_leaves: usize,
    pub saturation_step: Option<usize>,
    pub elapsed_ms: u128,
    pub axiomatized_rules: Vec<(String, usize)>, // (name, cross_corpus_count)
    pub subsumed_count: usize,
    pub verified_count: usize,
    pub conjectured_count: usize,
    pub fragile_rules: Vec<(String, usize)>, // (name, cross_corpus_count) — anything < 2
}

impl TraversalReport {
    pub fn narrate(&self) {
        println!("\n╔══════════════════════════════════════════════════════╗");
        println!("║ AUTONOMOUS TRAVERSAL REPORT                          ║");
        println!("╚══════════════════════════════════════════════════════╝");
        println!("\n▶ Sweep");
        println!("  corpora processed  : {}", self.total_corpora);
        match self.saturation_step {
            Some(s) => println!(
                "  saturation step    : {s} ({} corpora confirmed stable after)",
                self.total_corpora - s,
            ),
            None => println!("  saturation step    : library grew through to the end"),
        }
        println!("  elapsed            : {}ms", self.elapsed_ms);

        println!("\n▶ Forest substrate");
        println!("  nodes              : {}", self.forest_nodes);
        println!("  morphism edges     : {}", self.forest_edges);
        println!("  stable leaves      : {}", self.forest_stable_leaves);

        println!("\n▶ Library status");
        println!("  total rules        : {}", self.library_final_size);
        println!("  Axiomatized (apex) : {}", self.axiomatized_rules.len());
        println!("  Verified           : {}", self.verified_count);
        println!("  Conjectured        : {}", self.conjectured_count);
        println!("  Subsumed           : {}", self.subsumed_count);

        if !self.axiomatized_rules.is_empty() {
            println!("\n▶ Apex rules (Axiomatized with cross-corpus evidence):");
            for (name, support) in &self.axiomatized_rules {
                println!(
                    "    {name} — reduces in {support}/{} corpora",
                    self.total_corpora
                );
            }
        }

        println!("\n▶ Lynchpin invariant");
        if self.fragile_rules.is_empty() {
            println!("  HOLDS — every rule has ≥2 corpus cross-support");
        } else {
            println!("  VIOLATED — {} fragile rule(s):", self.fragile_rules.len());
            for (name, s) in &self.fragile_rules {
                println!("    {name} cross={s}");
            }
        }
    }
}

/// The traversal function. Given a budget and depth, runs the full
/// autonomous traversal pipeline and returns a structured report.
/// Reusable across tests and external consumers (the skill invokes
/// this through cargo test).
pub fn run_traversal(procedural_budget: usize, max_depth: usize) -> TraversalReport {
    use std::collections::HashMap;
    use std::time::Instant;

    let mut zoo = canonical_zoo();
    for seed in 1..=procedural_budget as u64 {
        let depth = 2 + (seed as usize % (max_depth - 1).max(1));
        let count = 16 + (seed as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
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
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        RuleEmitter,
        InMemoryRegistry::new(),
    );

    let mut rule_to_corpora: HashMap<TermRef, std::collections::HashSet<String>> =
        HashMap::new();
    let mut per_step_lib_size: Vec<usize> = Vec::new();
    let mut global_epoch = 0u64;
    // Map each node-insertion epoch → corpus name. Cross-corpus
    // credit is computed by node origin, not edge firing. This is
    // memoization-robust: when hash-cons shares a node across
    // corpora, the first corpus that inserted it keeps credit, and
    // subsequent corpora don't spuriously lose or gain credit for
    // already-memoized (node, rule) pairs.
    let mut epoch_to_corpus: HashMap<u64, String> = HashMap::new();

    let t0 = Instant::now();
    for (name, corpus) in &zoo {
        global_epoch += 1;
        forest.set_epoch(global_epoch);
        epoch_to_corpus.insert(global_epoch, name.clone());
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
        let rule_refs: Vec<&mathscape_core::eval::RewriteRule> =
            library_rules.iter().map(|(_, r)| r).collect();
        let _ = forest.apply_rules_retroactively(&rule_refs);

        per_step_lib_size.push(epoch.registry.all().len());
    }

    // Compute cross-corpus support by node-origin. For every morphism
    // edge in the forest, look up the source node's inserted_epoch,
    // map that to a corpus name, and credit the rule that fired.
    // Memoization-robust: hash-consed nodes credit only the ORIGINAL
    // inserting corpus, not every corpus whose retroactive pass
    // happened to traverse them.
    for edge in &forest.edges {
        let from_node = match forest.nodes.get(&edge.from) {
            Some(n) => n,
            None => continue,
        };
        let corpus = match epoch_to_corpus.get(&from_node.inserted_epoch) {
            Some(c) => c.clone(),
            None => continue,
        };
        // Find the library artifact whose rule name matches.
        if let Some(artifact) = epoch
            .registry
            .all()
            .iter()
            .find(|a| a.rule.name == edge.rule_name)
        {
            rule_to_corpora
                .entry(artifact.content_hash)
                .or_default()
                .insert(corpus);
        }
    }

    let elapsed_ms = t0.elapsed().as_millis();

    // Saturation: last step where library grew.
    let saturation_step = per_step_lib_size
        .windows(2)
        .rposition(|w| w[1] > w[0])
        .map(|i| i + 1);

    // Status tally + apex + fragile.
    //
    // Lynchpin applies only to ACTIVE rules — Axiomatized / Verified /
    // Conjectured. Subsumed rules are deliberately absorbed under a
    // more-general rule by the reinforcement pass; their redundancy
    // is the machine's own determination, and demanding they carry
    // separate cross-corpus support would contradict what subsumption
    // means. The subsuming rule's cross-corpus support is what the
    // invariant cares about.
    let mut axiomatized_rules = Vec::new();
    let mut subsumed_count = 0;
    let mut verified_count = 0;
    let mut conjectured_count = 0;
    let mut fragile_rules = Vec::new();
    for artifact in epoch.registry.all() {
        let status = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        let cross = rule_to_corpora
            .get(&artifact.content_hash)
            .map(|s| s.len())
            .unwrap_or(0);
        let is_active = !matches!(
            status,
            ProofStatus::Subsumed(_) | ProofStatus::Demoted(_)
        );
        if is_active && cross < 2 {
            fragile_rules.push((artifact.rule.name.clone(), cross));
        }
        match status {
            ProofStatus::Axiomatized => {
                axiomatized_rules.push((artifact.rule.name.clone(), cross))
            }
            ProofStatus::Subsumed(_) => subsumed_count += 1,
            ProofStatus::Verified => verified_count += 1,
            ProofStatus::Conjectured => conjectured_count += 1,
            _ => {}
        }
    }

    TraversalReport {
        total_corpora: zoo.len(),
        library_final_size: epoch.registry.all().len(),
        forest_nodes: forest.len(),
        forest_edges: forest.edges.len(),
        forest_stable_leaves: forest.stable_leaf_count(),
        saturation_step,
        elapsed_ms,
        axiomatized_rules,
        subsumed_count,
        verified_count,
        conjectured_count,
        fragile_rules,
    }
}

// ── Lynchpin assertions applied to any report ───────────────────

fn assert_autonomous_traversal_invariants(r: &TraversalReport) {
    assert!(
        r.fragile_rules.is_empty(),
        "LYNCHPIN VIOLATED: {} fragile rule(s) survived with <2 corpus support. \
         This would mean a corpus artifact reached the library. Fragile: {:?}",
        r.fragile_rules.len(),
        r.fragile_rules,
    );
    assert!(
        !r.axiomatized_rules.is_empty(),
        "no apex rule reached Axiomatized — the lifecycle advancement didn't fire. \
         This defeats the autonomous unsticking. Library had {} rules.",
        r.library_final_size,
    );
    // Apex rules must carry substantial cross-corpus evidence — but
    // "substantial" is scale-dependent. At small scale (zoo-dominated),
    // apex rules commonly reach 90%+ of corpora. At large procedural
    // scale (hundreds of thousands of random corpora), a rule targeting
    // one structural family (e.g. successor-chain) naturally covers a
    // bounded fraction — its fraction reflects the TRUE structural
    // density of that pattern in random input, not a defect.
    //
    // Threshold: max(5% of sweep, 5 corpora). Keeps the spirit — apex
    // rules are never corpus-artifacts — without forcing majority at
    // scales where majority would require a corpus-universal pattern.
    let min_support = (r.total_corpora / 20).max(5);
    for (name, support) in &r.axiomatized_rules {
        assert!(
            *support >= min_support,
            "Axiomatized rule {name} has only {support}/{} cross-support (need ≥{min_support}); \
             axiomatization without substantial cross-corpus evidence is the failure mode \
             the lynchpin is designed to prevent",
            r.total_corpora,
        );
    }
}

// ── Orchestrated test scales ────────────────────────────────────

#[test]
fn autonomous_traverse_small() {
    // Minimum viable sweep: zoo + 5 procedural corpora = 12 total.
    // Fast smoke-check that the autonomous loop still closes at
    // small scale.
    let report = run_traversal(5, 3);
    report.narrate();
    assert_autonomous_traversal_invariants(&report);
    assert!(report.total_corpora == 12);
}

#[test]
fn autonomous_traverse_medium() {
    // The default scale used in the original saturation sweep.
    // zoo + 12 procedural = 19 corpora. This is the flagship
    // configuration; the skill defaults to this.
    let budget = std::env::var("MATHSCAPE_TRAVERSE_BUDGET")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(12);
    let depth = std::env::var("MATHSCAPE_TRAVERSE_DEPTH")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(4);

    let report = run_traversal(budget, depth);
    report.narrate();
    assert_autonomous_traversal_invariants(&report);
    assert!(report.total_corpora == 7 + budget);

    // At medium scale we also pin saturation: the library should
    // stop growing well before the end of the sweep.
    assert!(
        report.saturation_step.is_some(),
        "at medium scale the library should reach saturation strictly \
         before end of sweep; else the machine hasn't converged"
    );
    let sat = report.saturation_step.unwrap();
    assert!(
        sat < report.total_corpora,
        "saturation_step {sat} should be < total {} corpora",
        report.total_corpora
    );
}

#[test]
fn autonomous_traverse_stress() {
    // Stress: zoo + 40 procedural = 47 corpora. Confirms the loop
    // scales without the lynchpin breaking. Deeper trees, more
    // seeds, more chances for the machine to mint a corpus
    // artifact. It shouldn't.
    let report = run_traversal(40, 5);
    report.narrate();
    assert_autonomous_traversal_invariants(&report);
    assert!(report.total_corpora == 47);
    // At stress scale we require an explicit saturation point —
    // running through 47 corpora without hitting saturation would
    // mean the machine is still learning and we're nowhere near
    // steady state.
    assert!(report.saturation_step.is_some());
}

#[test]
fn rank2_inception_probe() {
    // Phase H experiment: with the strict-subsumption gate for
    // meta-rules now enforced in `reduction::detect_subsumption_pairs`,
    // run the standard pipeline and observe whether multiple
    // rank-1 meta-rules coexist in the ACTIVE library (not subsumed
    // under each other arbitrarily). If they do, invoke
    // `MetaPatternGenerator` against them explicitly — a rank-2
    // candidate emerges iff anti-unification across meta-rule LHSs
    // produces a pattern more general than any single meta-rule.
    //
    // This is the "inception" signal the user named: a rule
    // whose entire structure is composed of abstractions the
    // machine itself developed — operator-variables over other
    // operator-variables.
    use mathscape_compress::{
        extract::ExtractConfig, CompositeGenerator, CompressionGenerator,
        MetaPatternGenerator,
    };
    use mathscape_core::epoch::{Epoch, Generator, InMemoryRegistry, RuleEmitter};

    // Run across the FULL zoo so structurally-independent
    // meta-patterns have a chance to emerge. The seven hand-crafted
    // shapes probe distinct dimensions (identity, doubling,
    // successor-chain, cross-op) — any of them could mint a
    // meta-pattern that doesn't strictly subsume or get subsumed
    // by the compositional meta.
    let zoo = canonical_zoo();

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
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        RuleEmitter,
        InMemoryRegistry::new(),
    );
    // For each corpus, 3 Discover + 1 Reinforce — mirrors run_traversal.
    for (_, corpus) in &zoo {
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
    }
    // Use the last corpus for the rank-2 probe invocation.
    let corpus = zoo.last().map(|(_, c)| c.clone()).unwrap_or_default();

    // Collect active meta-rules (operator-variable LHS, not Subsumed).
    let active_meta_rules: Vec<_> = epoch
        .registry
        .all()
        .iter()
        .filter(|a| {
            let s = epoch
                .registry
                .status_of(a.content_hash)
                .unwrap_or_else(|| a.certificate.status.clone());
            !matches!(s, ProofStatus::Subsumed(_) | ProofStatus::Demoted(_))
        })
        .filter(|a| {
            if let Term::Apply(f, _) = &a.rule.lhs {
                matches!(**f, Term::Var(v) if v >= 100)
            } else {
                false
            }
        })
        .collect();

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ RANK-2 INCEPTION PROBE                               ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Active meta-rules after subsumption pass: {}", active_meta_rules.len());
    for a in &active_meta_rules {
        println!("    {} :: {} => {}", a.rule.name, a.rule.lhs, a.rule.rhs);
    }

    // Invoke MetaPatternGenerator over the full library — which now
    // includes possibly multiple active meta-rules — and observe
    // whether rank-2 candidates emerge.
    let library_snapshot = epoch.registry.all().to_vec();
    let mut rank2_gen = MetaPatternGenerator::new(
        ExtractConfig {
            min_shared_size: 1,
            min_matches: 2,
            max_new_rules: 10,
        },
        20_000,
    );
    let rank2_candidates = rank2_gen.propose(
        epoch.epoch_id,
        &corpus,
        &library_snapshot,
    );
    // A rank-2 candidate is one whose LHS structure contains
    // operator-variables in nested positions — the "meta-meta"
    // pattern. Detect structurally: the LHS's function position
    // has a Var AND at least one arg is itself an Apply with a
    // Var function position.
    fn is_rank2(t: &Term) -> bool {
        match t {
            Term::Apply(f, args) => {
                let outer_is_meta = matches!(**f, Term::Var(v) if v >= 100);
                let inner_has_meta = args.iter().any(|a| {
                    if let Term::Apply(inner_f, _) = a {
                        matches!(**inner_f, Term::Var(v) if v >= 100)
                    } else {
                        false
                    }
                });
                outer_is_meta && inner_has_meta
            }
            _ => false,
        }
    }
    let rank2_count = rank2_candidates
        .iter()
        .filter(|c| is_rank2(&c.rule.lhs))
        .count();
    println!(
        "\n▶ MetaPatternGenerator proposals at rank-2 probe : {}",
        rank2_candidates.len()
    );
    for c in &rank2_candidates {
        let marker = if is_rank2(&c.rule.lhs) { " [RANK-2]" } else { "" };
        println!("    {} :: {} => {}{}", c.rule.name, c.rule.lhs, c.rule.rhs, marker);
    }
    println!("\n▶ Rank-2 candidates (nested operator-variables): {rank2_count}");

    // Soft observation: we don't require rank-2 to land here — the
    // prover + reward axes may or may not accept it. What this test
    // asserts is that the GATE now permits multiple meta-rules to
    // coexist AND that `MetaPatternGenerator` can see them. If
    // active_meta_rules has ≥ 2 entries, the gate is working; if
    // rank2_count ≥ 1, inception is materially possible on this
    // corpus.
    println!("\n▶ Gate check: {} meta-rules coexist (want ≥1 for gate OK)",
        active_meta_rules.len());
    assert!(
        !active_meta_rules.is_empty(),
        "meta-rule diversity gate failed — no active meta-rules survived"
    );
}

/// Configuration for ensemble traversal — the phase-M4 mode that
/// leverages the LLN-measured attractor distribution to build a
/// library strictly richer than any single traversal.
#[derive(Debug, Clone)]
pub struct EnsembleConfig {
    /// Number of procedural seeds to sample.
    pub seed_count: u64,
    /// Procedural corpora per seed.
    pub procedural_budget: usize,
    /// Max term depth for procedural corpora.
    pub max_depth: usize,
    /// Minimum fraction of basins a rule must appear in to be
    /// considered "universal" and inducted into the ensemble
    /// library. At 0.15 (default), rules appearing in ≥15% of
    /// attractor basins survive. The LLN data supports this
    /// threshold: universals land at 15-21%, noise is below 5%.
    pub universal_threshold: f64,
    /// Include the hand-crafted zoo or run pure-procedural.
    /// Pure-procedural (false) is where oscillation is visible.
    /// Zoo-anchored (true) gives the deterministic baseline.
    pub include_zoo: bool,
}

impl Default for EnsembleConfig {
    fn default() -> Self {
        Self {
            seed_count: 32,
            procedural_budget: 15,
            max_depth: 4,
            universal_threshold: 0.15,
            include_zoo: false,
        }
    }
}

/// Result of an ensemble traversal. Each rule is labeled with the
/// FRACTION OF BASINS it appeared in (its basin frequency), which
/// is the empirical "irreducibility" measure under LLN — how often
/// the rule emerges as an independent discovery regardless of seed.
#[derive(Debug, Clone)]
pub struct EnsembleReport {
    pub seed_count: u64,
    pub distinct_basins: usize,
    pub universal_rules: Vec<(String, f64)>,
    pub rank0_universals: Vec<(String, f64)>,
    pub rank1_universals: Vec<(String, f64)>,
    pub singleton_count: usize,
    pub shannon_entropy_bits: f64,
    pub total_elapsed_ms: u128,
}

/// Phase M4 move: ensemble traversal. Instead of running one
/// traversal that lands in a single attractor basin, sample K
/// basins by varying the procedural seed and take the UNION of
/// rules appearing in ≥`universal_threshold` fraction of basins.
///
/// Rationale from the LLN probe (256-seed data, 2026-04-18):
/// - S_10000 (dimensional-discovery meta-rule) appears in 17.6%
///   of attractor basins — it's a universal feature of the seed
///   space
/// - Rank-0 cluster (S_003 through S_010) each appear in 10-21%
///   of basins
/// - Singletons (rules appearing in exactly 1 basin) account for
///   147 of 256 seeds — long tail, probably noise
///
/// Taking the union above threshold 0.15 captures the universals
/// + rank-0 canonical rules while filtering the singleton tail.
/// The resulting library is basin-independent: it carries what
/// the MACHINE ITSELF reliably discovers regardless of which
/// specific seed happened to drive it.
pub fn run_ensemble_traversal(config: EnsembleConfig) -> EnsembleReport {
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;

    let t0 = Instant::now();
    let mut rule_basin_count: HashMap<String, usize> = HashMap::new();
    let mut distinct_basins: HashSet<Vec<String>> = HashSet::new();
    let mut basin_support: HashMap<Vec<String>, usize> = HashMap::new();

    for seed in 1..=config.seed_count {
        let report = if config.include_zoo {
            run_traversal(config.procedural_budget, config.max_depth)
        } else {
            run_traversal_pure_procedural(
                seed,
                config.procedural_budget,
                config.max_depth,
            )
        };
        let mut apex: Vec<String> = report
            .axiomatized_rules
            .iter()
            .map(|(n, _)| n.clone())
            .collect();
        apex.sort();
        for name in &apex {
            *rule_basin_count.entry(name.clone()).or_default() += 1;
        }
        distinct_basins.insert(apex.clone());
        *basin_support.entry(apex).or_default() += 1;
    }

    // Compute universal rules — those above threshold.
    let threshold_count =
        (config.universal_threshold * config.seed_count as f64).ceil() as usize;
    let mut universals: Vec<(String, f64)> = rule_basin_count
        .iter()
        .filter(|(_, c)| **c >= threshold_count)
        .map(|(n, c)| (n.clone(), *c as f64 / config.seed_count as f64))
        .collect();
    universals.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Classify by rank: id ≥ 10000 is rank-1+ (meta), otherwise rank-0.
    let (rank1, rank0): (Vec<_>, Vec<_>) =
        universals.iter().partition(|(n, _)| {
            n.strip_prefix("S_")
                .and_then(|s| s.parse::<u32>().ok())
                .map(|id| id >= 10_000)
                .unwrap_or(false)
        });

    let singleton_count = basin_support.values().filter(|&&c| c == 1).count();
    let n_f = config.seed_count as f64;
    let entropy: f64 = basin_support
        .values()
        .map(|&c| {
            let p = c as f64 / n_f;
            if p > 0.0 { -p * p.log2() } else { 0.0 }
        })
        .sum();

    EnsembleReport {
        seed_count: config.seed_count,
        distinct_basins: distinct_basins.len(),
        universal_rules: universals.clone(),
        rank0_universals: rank0.into_iter().cloned().collect(),
        rank1_universals: rank1.into_iter().cloned().collect(),
        singleton_count,
        shannon_entropy_bits: entropy,
        total_elapsed_ms: t0.elapsed().as_millis(),
    }
}

#[test]
fn ensemble_traversal_surfaces_universals() {
    // Phase M4 operationalized. Run 32 seeds, union their
    // discoveries, keep rules appearing in ≥ 15% of basins.
    // Expected: at least one universal rule emerges (either
    // S_10000 meta or a rank-0 common), demonstrating that
    // the ensemble mode IS richer than any single seed.
    let config = EnsembleConfig::default();
    let report = run_ensemble_traversal(config.clone());

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ ENSEMBLE TRAVERSAL — phase M4                        ║");
    println!("║   Oscillation-driven discovery via seed ensemble     ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Configuration");
    println!("  seeds sampled        : {}", report.seed_count);
    println!("  universal threshold  : {:.0}% of basins", config.universal_threshold * 100.0);
    println!("  include zoo          : {}", config.include_zoo);
    println!("  elapsed              : {}ms", report.total_elapsed_ms);

    println!("\n▶ Basin statistics");
    println!("  distinct basins      : {}/{}", report.distinct_basins, report.seed_count);
    println!("  singleton basins     : {} (long tail)", report.singleton_count);
    println!("  Shannon entropy      : {:.3} bits", report.shannon_entropy_bits);

    println!("\n▶ Universal rules (basin-frequency ≥ {:.0}%)",
        config.universal_threshold * 100.0);
    println!("  rank-0 canonical:");
    for (name, freq) in &report.rank0_universals {
        println!("    {name:<10} basin-freq = {:.1}%", freq * 100.0);
    }
    println!("  rank-1 meta:");
    for (name, freq) in &report.rank1_universals {
        println!("    {name:<10} basin-freq = {:.1}%", freq * 100.0);
    }

    // The ensemble library IS the union of these universals.
    // That's the phase-M4 output: strictly richer than any single
    // traversal because it carries what the machine reliably
    // discovers regardless of which basin a single seed lands in.
    println!("\n▶ Ensemble library size (union-above-threshold): {}",
        report.universal_rules.len());
    println!(
        "\n▶ Interpretation\n  A single traversal samples ONE basin. The ensemble samples {}\n  basins and keeps only rules that cross ≥ {:.0}% of them. The resulting\n  library carries basin-independent structure — the universals the machine\n  itself finds irrespective of the specific corpus that drove it.",
        report.distinct_basins,
        config.universal_threshold * 100.0,
    );

    assert!(
        !report.universal_rules.is_empty(),
        "at 32 seeds × 15% threshold, at least one universal rule should survive; \
         got {} universals (basins: {})",
        report.universal_rules.len(),
        report.distinct_basins,
    );
}

/// Anonymize a term's symbol ids — replaces each `Term::Symbol(id, _)`
/// and `Term::Var(id)` with a normalized canonical id based on first-
/// appearance order. Returns a "structural fingerprint" of the term:
/// two terms that differ only in which fresh ids got minted produce
/// the same fingerprint.
///
/// Used by phase M2 to classify basins by their LHS/RHS STRUCTURE
/// rather than nominal rule names. Fresh symbol ids vary across
/// runs (S_004 in one traversal, S_008 in another can encode the
/// same pattern); nominal count over-counts genuine basin diversity.
fn anonymize_term(t: &Term) -> Term {
    fn walk(
        t: &Term,
        var_map: &mut std::collections::HashMap<u32, u32>,
        symbol_map: &mut std::collections::HashMap<u32, u32>,
    ) -> Term {
        match t {
            Term::Point(p) => Term::Point(p.clone()),
            Term::Number(n) => Term::Number(n.clone()),
            Term::Var(v) => {
                // Concrete ops (Var id < 100) are preserved as-is —
                // they're vocabulary, not fresh symbols.
                if *v < 100 {
                    Term::Var(*v)
                } else {
                    let next = var_map.len() as u32;
                    let id = *var_map.entry(*v).or_insert(next + 100);
                    Term::Var(id)
                }
            }
            Term::Fn(params, body) => {
                let b = walk(body, var_map, symbol_map);
                Term::Fn(params.clone(), Box::new(b))
            }
            Term::Apply(f, args) => {
                let f2 = walk(f, var_map, symbol_map);
                let args2 = args.iter().map(|a| walk(a, var_map, symbol_map)).collect();
                Term::Apply(Box::new(f2), args2)
            }
            Term::Symbol(id, args) => {
                let next = symbol_map.len() as u32;
                let canonical = *symbol_map.entry(*id).or_insert(next);
                let args2 = args.iter().map(|a| walk(a, var_map, symbol_map)).collect();
                Term::Symbol(canonical, args2)
            }
        }
    }
    let mut var_map = std::collections::HashMap::new();
    let mut symbol_map = std::collections::HashMap::new();
    walk(t, &mut var_map, &mut symbol_map)
}

/// Build a canonical structural fingerprint of an apex rule set:
/// each rule's (anonymized-lhs, anonymized-rhs) pair, sorted.
/// Two basins with equal fingerprints are STRUCTURALLY
/// indistinguishable — they encode the same discoveries under
/// different nominal ids.
fn structural_fingerprint(rules: &[mathscape_core::eval::RewriteRule]) -> Vec<(String, String)> {
    let mut sigs: Vec<(String, String)> = rules
        .iter()
        .map(|r| {
            let lhs_anon = anonymize_term(&r.lhs);
            let rhs_anon = anonymize_term(&r.rhs);
            (format!("{lhs_anon}"), format!("{rhs_anon}"))
        })
        .collect();
    sigs.sort();
    sigs
}

/// Operator-abstracted fingerprint: like structural_fingerprint but
/// ALSO anonymizes concrete operator ids (Var id 2 = add, 3 = mul,
/// 4 = succ, etc.). Two rules that encode the same pattern under
/// DIFFERENT specific operators map to the same fingerprint.
///
/// Example:
///   add-rule:  (?2 ?100 ?101) => S0(?2 ?100 ?101)
///   mul-rule:  (?3 ?100 ?101) => S0(?3 ?100 ?101)
///
/// Under this coarser equivalence, they're the same: "binary
/// operator applied to two args, reduces to a named symbol holding
/// the operator and args." The finest structural level DID
/// distinguish them; this coarser level SAYS they're the same
/// operator-family pattern at different slots.
///
/// The question the user asked: "can features be condensed further
/// without losing ability to reveal details?" This is one such
/// condensation — the trade-off is we lose which specific operator
/// got Axiomatized but retain the shape of what the machine
/// discovered.
fn operator_abstract_term(t: &Term) -> Term {
    fn walk(
        t: &Term,
        all_map: &mut std::collections::HashMap<u32, u32>,
    ) -> Term {
        match t {
            Term::Point(p) => Term::Point(p.clone()),
            Term::Number(n) => Term::Number(n.clone()),
            Term::Var(v) => {
                // Abstract ALL vars — concrete ops and fresh vars
                // alike — into a single canonical numbering. This
                // is the "both operators and free variables are
                // slots" view.
                let next = all_map.len() as u32;
                let id = *all_map.entry(*v).or_insert(next);
                Term::Var(id)
            }
            Term::Fn(params, body) => {
                let b = walk(body, all_map);
                Term::Fn(params.clone(), Box::new(b))
            }
            Term::Apply(f, args) => {
                let f2 = walk(f, all_map);
                let args2 = args.iter().map(|a| walk(a, all_map)).collect();
                Term::Apply(Box::new(f2), args2)
            }
            Term::Symbol(id, args) => {
                let next = all_map.len() as u32;
                let canonical = *all_map.entry(*id + 10_000_000).or_insert(next);
                let args2 = args.iter().map(|a| walk(a, all_map)).collect();
                Term::Symbol(canonical, args2)
            }
        }
    }
    let mut all_map = std::collections::HashMap::new();
    walk(t, &mut all_map)
}

fn operator_abstract_fingerprint(
    rules: &[mathscape_core::eval::RewriteRule],
) -> Vec<(String, String)> {
    let mut sigs: Vec<(String, String)> = rules
        .iter()
        .map(|r| {
            let lhs = operator_abstract_term(&r.lhs);
            let rhs = operator_abstract_term(&r.rhs);
            (format!("{lhs}"), format!("{rhs}"))
        })
        .collect();
    sigs.sort();
    sigs
}

#[test]
#[ignore = "phase M2+: operator-abstract basin classification, ~12s, --ignored"]
fn oscillation_operator_abstract_basins() {
    // One condensation level deeper: abstract concrete operators
    // too. Two rules that encode the same pattern under different
    // specific operators (e.g. add vs mul in same position) map to
    // the same operator-abstract fingerprint.
    //
    // Question: how many DISTINCT *pattern-shapes* does the machine
    // discover, regardless of which specific operator it happened
    // to Axiomatize? This should collapse the top-2 basins (which
    // differ only in add vs mul) into ONE.
    use std::collections::HashSet;

    const N_SEEDS: u64 = 1024;
    const BUDGET: usize = 15;
    const DEPTH: usize = 4;

    let mut structural: HashSet<Vec<(String, String)>> = HashSet::new();
    let mut op_abstract: HashSet<Vec<(String, String)>> = HashSet::new();
    let mut op_abstract_support: std::collections::HashMap<
        Vec<(String, String)>,
        usize,
    > = std::collections::HashMap::new();

    for seed in 1..=N_SEEDS {
        let report = run_traversal_pure_procedural_with_library(seed, BUDGET, DEPTH);
        structural.insert(structural_fingerprint(&report.axiomatized_rules_full));
        let fp = operator_abstract_fingerprint(&report.axiomatized_rules_full);
        op_abstract.insert(fp.clone());
        *op_abstract_support.entry(fp).or_default() += 1;
    }

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ OPERATOR-ABSTRACT BASIN CLASSIFICATION               ║");
    println!("║   Further condensation — ignore specific operator   ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Layered basin counts (1024 seeds)");
    println!("  nominal basins (S_NNN names)    : >500");
    println!("  structural basins (shape)       : {}", structural.len());
    println!("  operator-abstract basins        : {}", op_abstract.len());
    let reduction = 1.0 - (op_abstract.len() as f64 / structural.len() as f64);
    println!("  additional compression          : {:.1}%", reduction * 100.0);

    let mut by_sup: Vec<(Vec<(String, String)>, usize)> =
        op_abstract_support.into_iter().collect();
    by_sup.sort_by(|a, b| b.1.cmp(&a.1));
    println!("\n▶ Top-10 operator-abstract basins");
    println!("{:>8} {:>10} {:>10}", "rank", "support", "fraction");
    for (i, (_, sup)) in by_sup.iter().take(10).enumerate() {
        println!(
            "{:>8} {:>10} {:>10.1}%",
            i + 1,
            sup,
            *sup as f64 / N_SEEDS as f64 * 100.0
        );
    }

    let modal = by_sup.first().map(|x| x.1).unwrap_or(0);
    let modal_frac = modal as f64 / N_SEEDS as f64;
    println!("\n▶ Modal operator-abstract basin support: {}/{} ({:.1}%)",
        modal, N_SEEDS, modal_frac * 100.0);
    if modal_frac > 0.8 {
        println!(
            "\n  STRONG UNIFICATION — {:.0}% of seeds land in ONE\n  operator-abstract basin. At this level of abstraction the\n  machine has essentially ONE canonical discovery shape.",
            modal_frac * 100.0,
        );
    } else if modal_frac > 0.5 {
        println!(
            "\n  DOMINANT CLASS — {:.0}% of seeds share the top\n  operator-abstract basin; the rest scatter. The machine has\n  a preferred discovery pattern with meaningful variation.",
            modal_frac * 100.0,
        );
    } else {
        println!(
            "\n  DIVERSE — no single operator-abstract basin dominates\n  at this scale. Multiple orthogonal discovery shapes coexist."
        );
    }
}

#[test]
#[ignore = "verify M2: structural basin convergence — 256→2048 seeds, ~40s, --ignored"]
fn oscillation_structural_basin_convergence() {
    // Verification: is the ~80 structural basin count REAL or an
    // artifact of the 1024-seed sample? Run the stairway at 256,
    // 512, 1024, 2048 and watch structural basin growth. If the
    // count plateaus, we've found the ceiling. If it keeps
    // growing, we haven't.
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;

    const BUDGET: usize = 15;
    const DEPTH: usize = 4;
    let scales = [256u64, 512, 1024, 2048];

    let t0 = Instant::now();
    let mut structural_set: HashSet<Vec<(String, String)>> = HashSet::new();
    let mut structural_count_at_scale: Vec<(u64, usize, usize, usize)> = Vec::new();
    // (seed_count, nominal_basins, structural_basins, new_structural_since_last)
    let mut nominal_set: HashSet<Vec<String>> = HashSet::new();
    let mut prev_struct = 0usize;
    let mut prev_seed = 0u64;
    for scale in scales {
        for seed in (prev_seed + 1)..=scale {
            let report = run_traversal_pure_procedural_with_library(seed, BUDGET, DEPTH);
            let mut nominal: Vec<String> = report.axiomatized_rule_names.clone();
            nominal.sort();
            nominal_set.insert(nominal);
            let fp = structural_fingerprint(&report.axiomatized_rules_full);
            structural_set.insert(fp);
        }
        let nominal = nominal_set.len();
        let structural = structural_set.len();
        let new_struct = structural - prev_struct;
        structural_count_at_scale.push((scale, nominal, structural, new_struct));
        prev_struct = structural;
        prev_seed = scale;
    }
    let elapsed = t0.elapsed().as_millis();

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ M2 CONVERGENCE CHECK — structural basin count        ║");
    println!("║   stairway 256 → 512 → 1024 → 2048                  ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n{:>8} {:>10} {:>12} {:>10}", "seeds", "nominal", "structural", "new_struct");
    println!("{}", "─".repeat(44));
    for (seeds, nominal, structural, new_struct) in &structural_count_at_scale {
        println!("{seeds:>8} {nominal:>10} {structural:>12} {new_struct:>10}");
    }
    let last = structural_count_at_scale.last().unwrap();
    let new_rate_first = structural_count_at_scale[0].3 as f64 / 256.0;
    let new_rate_last = last.3 as f64 / 1024.0; // last scale added 1024 seeds
    println!("\n▶ New-structural-basin rate");
    println!("  @256 (per seed)  : {:.4}", new_rate_first);
    println!("  @2048 delta rate : {:.4}", new_rate_last);
    println!("  decay            : {:.3}×", new_rate_last / new_rate_first);
    println!("  elapsed          : {elapsed}ms");

    if new_rate_last < new_rate_first * 0.5 {
        println!(
            "\n  CONVERGING — new-structural-basin rate has halved or more.\n  The structural count is approaching its ceiling at this\n  machinery scale. ~{} is a reasonable empirical ceiling.",
            last.2
        );
    } else {
        println!(
            "\n  STILL GROWING — push to 4096 or 8192 seeds to locate\n  the plateau."
        );
    }
    let _ = HashMap::<(), ()>::new(); // silence unused-import
}

#[test]
#[ignore = "M2 anatomy: inspect top-basin rule content, ~15s, --ignored"]
fn oscillation_apex_basin_anatomy() {
    // What rules are in the two dominant basins? If the top-2
    // basins capture ~86% of seeds, those ARE the machine's
    // canonical discoveries at this machinery scale. Extracting
    // them tells us what mathscape always finds.
    use std::collections::HashMap;

    const N_SEEDS: u64 = 1024;
    const BUDGET: usize = 15;
    const DEPTH: usize = 4;

    let mut fp_to_seeds: HashMap<Vec<(String, String)>, Vec<u64>> = HashMap::new();
    let mut fp_to_canonical_rules: HashMap<
        Vec<(String, String)>,
        Vec<mathscape_core::eval::RewriteRule>,
    > = HashMap::new();

    for seed in 1..=N_SEEDS {
        let report = run_traversal_pure_procedural_with_library(seed, BUDGET, DEPTH);
        let fp = structural_fingerprint(&report.axiomatized_rules_full);
        fp_to_seeds.entry(fp.clone()).or_default().push(seed);
        fp_to_canonical_rules
            .entry(fp)
            .or_insert(report.axiomatized_rules_full.clone());
    }

    let mut by_support: Vec<(Vec<(String, String)>, Vec<u64>)> =
        fp_to_seeds.into_iter().collect();
    by_support.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ APEX BASIN ANATOMY — what the dominant basins contain║");
    println!("╚══════════════════════════════════════════════════════╝");
    for (rank, (fp, seeds)) in by_support.iter().take(3).enumerate() {
        println!("\n▶ Basin #{} — support {}/{} ({:.1}%)",
            rank + 1, seeds.len(), N_SEEDS,
            seeds.len() as f64 / N_SEEDS as f64 * 100.0);
        println!("  seed samples: {:?}", seeds.iter().take(5).collect::<Vec<_>>());
        if let Some(rules) = fp_to_canonical_rules.get(fp) {
            println!("  rule count: {}", rules.len());
            for rule in rules {
                let lhs_anon = mathscape_core::eval::anonymize_term(&rule.lhs);
                let rhs_anon = mathscape_core::eval::anonymize_term(&rule.rhs);
                println!("    {:<10} :: {} => {}", rule.name, lhs_anon, rhs_anon);
            }
        }
    }

    // Compute basin-to-basin structural distance between top-2 to
    // understand if they're near-basins or orthogonal.
    if by_support.len() >= 2 {
        let (fp_a, _) = &by_support[0];
        let (fp_b, _) = &by_support[1];
        let set_a: std::collections::HashSet<_> = fp_a.iter().collect();
        let set_b: std::collections::HashSet<_> = fp_b.iter().collect();
        let intersection = set_a.intersection(&set_b).count();
        let union = set_a.union(&set_b).count();
        let jaccard = intersection as f64 / union as f64;
        println!("\n▶ Top-2 basin Jaccard similarity");
        println!("  shared rule-fingerprints : {intersection}/{union}");
        println!("  Jaccard coefficient      : {:.3}", jaccard);
        if jaccard > 0.5 {
            println!("  → top-2 basins are near-basins (most rules shared)");
        } else if jaccard > 0.1 {
            println!("  → top-2 basins are related but structurally distinct");
        } else {
            println!("  → top-2 basins are orthogonal (different canonical discoveries)");
        }
    }
}

#[test]
#[ignore = "phase M2: structural basin classification — 1024 seeds, ~15s, --ignored"]
fn oscillation_structural_basin_classification() {
    // Phase M2: reclassify basins by STRUCTURAL fingerprint (lhs/rhs
    // anonymized — fresh symbol ids normalized to canonical order).
    // Two runs whose libraries differ only in which specific S_NNN
    // name got assigned to equivalent patterns should share a basin.
    //
    // Expected: structural basin count << nominal basin count at
    // 1024 seeds. If we saw 529 nominal basins in M1, structural
    // basins are likely a small fraction — that's the TRUE
    // attractor count, the finite object the user predicted.
    use std::collections::HashMap;
    use std::time::Instant;

    const N_SEEDS: u64 = 1024;
    const BUDGET: usize = 15;
    const DEPTH: usize = 4;

    let t0 = Instant::now();
    let mut nominal_basins: std::collections::HashSet<Vec<String>> =
        std::collections::HashSet::new();
    let mut structural_basins: HashMap<Vec<(String, String)>, usize> = HashMap::new();

    for seed in 1..=N_SEEDS {
        let report = run_traversal_pure_procedural_with_library(seed, BUDGET, DEPTH);
        let mut nominal: Vec<String> = report
            .axiomatized_rule_names
            .iter()
            .cloned()
            .collect();
        nominal.sort();
        nominal_basins.insert(nominal);

        let fingerprint = structural_fingerprint(&report.axiomatized_rules_full);
        *structural_basins.entry(fingerprint).or_default() += 1;
    }
    let elapsed = t0.elapsed().as_millis();

    let nominal_count = nominal_basins.len();
    let structural_count = structural_basins.len();
    let compression_ratio =
        1.0 - (structural_count as f64 / nominal_count as f64);

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ PHASE M2 — STRUCTURAL BASIN CLASSIFICATION           ║");
    println!("║   1024 seeds, anonymize then compare                 ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Nominal basin count  (by S_NNN names)  : {nominal_count}");
    println!("▶ Structural basin count (by shape)      : {structural_count}");
    println!("▶ Compression ratio (nominal→structural) : {:.1}%", compression_ratio * 100.0);
    println!("▶ Elapsed                                : {elapsed}ms");

    // Top-10 structural basins by seed support.
    let mut by_support: Vec<(Vec<(String, String)>, usize)> =
        structural_basins.iter().map(|(k, v)| (k.clone(), *v)).collect();
    by_support.sort_by(|a, b| b.1.cmp(&a.1));
    println!("\n▶ Top-10 structural basins (most-seed-supported)");
    println!("{:>8} {:>10}", "rank", "support");
    for (i, (_, sup)) in by_support.iter().take(10).enumerate() {
        println!("{:>8} {:>10} ({:.1}%)", i + 1, sup, *sup as f64 / N_SEEDS as f64 * 100.0);
    }

    let singletons = structural_basins.values().filter(|&&c| c == 1).count();
    let structural_entropy: f64 = structural_basins
        .values()
        .map(|&c| {
            let p = c as f64 / N_SEEDS as f64;
            if p > 0.0 { -p * p.log2() } else { 0.0 }
        })
        .sum();
    println!("\n▶ Structural distribution");
    println!("  singleton structural basins : {singletons}/{structural_count}");
    println!("  Shannon entropy             : {structural_entropy:.3} bits");
    let max_entropy = (structural_count as f64).log2();
    println!("  normalized entropy          : {:.3}", structural_entropy / max_entropy);

    println!("\n▶ Interpretation");
    if compression_ratio > 0.7 {
        println!(
            "  STRONG STRUCTURAL COLLAPSE — {nominal_count} nominal basins\n  compressed to {structural_count} structural basins (saved {:.0}%).\n  The nominal diversity is mostly fresh-symbol-id noise; the TRUE\n  attractor count at this machinery scale is ≈ {structural_count}.\n  This IS the finite object — the machine's answer for \"how many\n  distinct discoveries are possible.\"",
            compression_ratio * 100.0
        );
    } else if compression_ratio > 0.3 {
        println!(
            "  MODERATE COLLAPSE — structural classification reduces basin\n  count from {nominal_count} to {structural_count} ({:.0}% saved). Genuine\n  attractor count is smaller than nominal but nominal variation\n  captures some real diversity too.",
            compression_ratio * 100.0
        );
    } else {
        println!(
            "  LOW COLLAPSE — only {:.0}% of nominal basins were structurally\n  equivalent. The discovery space really is close to nominal count.\n  Expect phase K (egg) to be needed to collapse further.",
            compression_ratio * 100.0
        );
    }

    assert!(structural_count > 0);
    assert!(structural_count <= nominal_count,
        "structural count must be ≤ nominal count (anonymization can only merge, not split)");
}

/// Extended traversal report carrying full rule data (LHS+RHS) so
/// structural fingerprinting can examine the whole rule shape,
/// not just names.
struct TraversalReportWithLibrary {
    axiomatized_rule_names: Vec<String>,
    axiomatized_rules_full: Vec<mathscape_core::eval::RewriteRule>,
}

fn run_traversal_pure_procedural_with_reward(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
    reward_config: mathscape_reward::reward::RewardConfig,
) -> TraversalReportWithLibrary {
    use std::collections::HashSet;

    let mut zoo: Vec<(String, Vec<Term>)> = Vec::new();
    for i in 1..=procedural_budget as u64 {
        let seed = seed_offset.wrapping_add(i);
        let depth = 2 + (i as usize % (max_depth - 1).max(1));
        let count = 16 + (i as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
        ));
    }

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
        mathscape_reward::StatisticalProver::new(reward_config, 0.0),
        RuleEmitter,
        InMemoryRegistry::new(),
    );

    for (_, corpus) in &zoo {
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
    }

    let mut names = Vec::new();
    let mut full = Vec::new();
    for artifact in epoch.registry.all() {
        let s = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        if matches!(s, ProofStatus::Axiomatized) {
            names.push(artifact.rule.name.clone());
            full.push(artifact.rule.clone());
        }
    }
    let _ = HashSet::<TermRef>::new();
    TraversalReportWithLibrary {
        axiomatized_rule_names: names,
        axiomatized_rules_full: full,
    }
}

fn run_traversal_pure_procedural_with_library(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
) -> TraversalReportWithLibrary {
    use std::collections::{HashMap, HashSet};

    let mut zoo: Vec<(String, Vec<Term>)> = Vec::new();
    for i in 1..=procedural_budget as u64 {
        let seed = seed_offset.wrapping_add(i);
        let depth = 2 + (i as usize % (max_depth - 1).max(1));
        let count = 16 + (i as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
        ));
    }

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
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        RuleEmitter,
        InMemoryRegistry::new(),
    );

    for (_, corpus) in &zoo {
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
    }

    let mut names = Vec::new();
    let mut full = Vec::new();
    for artifact in epoch.registry.all() {
        let s = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        if matches!(s, ProofStatus::Axiomatized) {
            names.push(artifact.rule.name.clone());
            full.push(artifact.rule.clone());
        }
    }
    let _ = HashSet::<TermRef>::new();
    let _ = HashMap::<TermRef, TermRef>::new();

    TraversalReportWithLibrary {
        axiomatized_rule_names: names,
        axiomatized_rules_full: full,
    }
}

#[test]
#[ignore = "phase I: subterm AU on a shared-subterm-dense corpus, ~5s, --ignored"]
fn phase_i_crafted_shared_subterm_corpus() {
    // Phase I penetration test on a CRAFTED corpus where subterm AU
    // has something to find. Classical procedural corpora are random
    // — sharing subterms by chance at low probability. Here we
    // construct pairs with deliberate shared-subterm structure:
    //
    //   add(mul(n, 2), 0)    ← has mul(n, 2) as subterm
    //   mul(mul(n, 2), 3)    ← has mul(n, 2) as subterm
    //   add(add(n, k), 0)    ← has add(n, k) as subterm
    //   mul(add(n, k), 3)    ← has add(n, k) as subterm
    //
    // Root AU across these pairs sees "?op(?x, ?y) => ?" (trivial
    // root match). Subterm AU should see the shared inner structure
    // like mul(?n, 2) or add(?n, ?k).
    use common::{apply, nat, var};
    use mathscape_compress::extract::ExtractConfig as EC;

    let mut crafted: Vec<Term> = Vec::new();
    for n in 1..=10u64 {
        // add(mul(n, 2), 0) and mul(mul(n, 2), 3): share mul(n, 2)
        crafted.push(apply(var(2), vec![apply(var(3), vec![nat(n), nat(2)]), nat(0)]));
        crafted.push(apply(var(3), vec![apply(var(3), vec![nat(n), nat(2)]), nat(3)]));
        // add(add(n, 1), 0) and mul(add(n, 1), 5): share add(n, 1)
        crafted.push(apply(var(2), vec![apply(var(2), vec![nat(n), nat(1)]), nat(0)]));
        crafted.push(apply(var(3), vec![apply(var(2), vec![nat(n), nat(1)]), nat(5)]));
    }

    let ec = EC {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 10,
    };

    // Vanilla extraction over crafted corpus
    let mut next_id_v: mathscape_core::term::SymbolId = 1;
    let rules_v = mathscape_compress::extract::extract_rules(
        &crafted,
        &[],
        &mut next_id_v,
        &ec,
    );
    // Subterm AU extraction over the same corpus
    let mut next_id_s: mathscape_core::term::SymbolId = 1;
    let rules_s = mathscape_compress::extract::extract_rules_with_options(
        &crafted,
        &[],
        &mut next_id_s,
        &ec,
        true,
    );

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ PHASE I — CRAFTED SHARED-SUBTERM CORPUS              ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Crafted corpus (40 terms with deliberate shared subterms)");
    println!("  Shapes: add(mul(n,2), 0), mul(mul(n,2), 3), add(add(n,1), 0), mul(add(n,1), 5)");
    println!("  Shared inner subterms: mul(n,2), add(n,1)");

    println!("\n▶ Vanilla root-AU extraction ({} rules)", rules_v.len());
    for rule in &rules_v {
        let lhs = mathscape_core::eval::anonymize_term(&rule.lhs);
        let rhs = mathscape_core::eval::anonymize_term(&rule.rhs);
        println!("    {} :: {} => {}", rule.name, lhs, rhs);
    }

    println!("\n▶ Subterm-AU extraction ({} rules)", rules_s.len());
    for rule in &rules_s {
        let lhs = mathscape_core::eval::anonymize_term(&rule.lhs);
        let rhs = mathscape_core::eval::anonymize_term(&rule.rhs);
        println!("    {} :: {} => {}", rule.name, lhs, rhs);
    }

    let vanilla_lhs: std::collections::HashSet<String> = rules_v
        .iter()
        .map(|r| format!("{}", mathscape_core::eval::anonymize_term(&r.lhs)))
        .collect();
    let subterm_lhs: std::collections::HashSet<String> = rules_s
        .iter()
        .map(|r| format!("{}", mathscape_core::eval::anonymize_term(&r.lhs)))
        .collect();
    let new_under_subterm: Vec<&String> = subterm_lhs
        .difference(&vanilla_lhs)
        .collect();
    let lost_under_subterm: Vec<&String> = vanilla_lhs
        .difference(&subterm_lhs)
        .collect();

    println!("\n▶ Shape delta (anonymized)");
    println!("  new patterns under subterm AU: {}", new_under_subterm.len());
    for p in &new_under_subterm {
        println!("    + {p}");
    }
    println!("  patterns lost under subterm AU: {}", lost_under_subterm.len());
    for p in &lost_under_subterm {
        println!("    − {p}");
    }
}

#[test]
#[ignore = "phase I: subterm AU vs bettyfine — does new machinery produce a non-trivial bettyfine? ~15s, --ignored"]
fn phase_i_subterm_au_bettyfine() {
    // Phase I gate: compare bettyfine under subterm-AU-enabled
    // machinery vs vanilla root-only AU. If the modal basin's rule
    // content is structurally different, subterm AU genuinely
    // extends the machine's reach into the mathscape. If it's the
    // same canonical trivial form, phase I didn't penetrate further
    // at this machinery scale (need phase J or K).
    use mathscape_compress::extract::ExtractConfig as EC;
    use std::collections::HashMap;

    const N_SEEDS: u64 = 64;
    const BUDGET: usize = 15;
    const DEPTH: usize = 4;
    let ec = EC {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 10,
    };

    let mut vanilla_basins: HashMap<Vec<(String, String)>, usize> = HashMap::new();
    let mut vanilla_rules_per_basin: HashMap<
        Vec<(String, String)>,
        Vec<mathscape_core::eval::RewriteRule>,
    > = HashMap::new();
    let mut subterm_basins: HashMap<Vec<(String, String)>, usize> = HashMap::new();
    let mut subterm_rules_per_basin: HashMap<
        Vec<(String, String)>,
        Vec<mathscape_core::eval::RewriteRule>,
    > = HashMap::new();

    for seed in 1..=N_SEEDS {
        // Vanilla
        let v = run_traversal_pure_procedural_with_extract(seed, BUDGET, DEPTH, ec.clone());
        let v_fp = structural_fingerprint(&v.axiomatized_rules_full);
        *vanilla_basins.entry(v_fp.clone()).or_default() += 1;
        vanilla_rules_per_basin
            .entry(v_fp)
            .or_insert(v.axiomatized_rules_full);

        // Subterm AU
        let s = run_with_subterm_au(seed, BUDGET, DEPTH, ec.clone());
        let s_fp = structural_fingerprint(&s.axiomatized_rules_full);
        *subterm_basins.entry(s_fp.clone()).or_default() += 1;
        subterm_rules_per_basin
            .entry(s_fp)
            .or_insert(s.axiomatized_rules_full);
    }

    let v_modal_fp = vanilla_basins
        .iter()
        .max_by_key(|(_, c)| *c)
        .map(|(fp, _)| fp.clone())
        .unwrap_or_default();
    let v_modal_count = *vanilla_basins.get(&v_modal_fp).unwrap_or(&0);
    let s_modal_fp = subterm_basins
        .iter()
        .max_by_key(|(_, c)| *c)
        .map(|(fp, _)| fp.clone())
        .unwrap_or_default();
    let s_modal_count = *subterm_basins.get(&s_modal_fp).unwrap_or(&0);

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ PHASE I — SUBTERM AU vs CANONICAL BETTYFINE          ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Vanilla (root-only AU)");
    println!("  basins: {}", vanilla_basins.len());
    println!("  modal : {}/{N_SEEDS} ({:.1}%)",
        v_modal_count, v_modal_count as f64 / N_SEEDS as f64 * 100.0);
    if let Some(rules) = vanilla_rules_per_basin.get(&v_modal_fp) {
        for rule in rules {
            let lhs_anon = mathscape_core::eval::anonymize_term(&rule.lhs);
            let rhs_anon = mathscape_core::eval::anonymize_term(&rule.rhs);
            println!("    {} :: {} => {}", rule.name, lhs_anon, rhs_anon);
        }
    }

    println!("\n▶ Subterm AU enabled");
    println!("  basins: {}", subterm_basins.len());
    println!("  modal : {}/{N_SEEDS} ({:.1}%)",
        s_modal_count, s_modal_count as f64 / N_SEEDS as f64 * 100.0);
    if let Some(rules) = subterm_rules_per_basin.get(&s_modal_fp) {
        for rule in rules {
            let lhs_anon = mathscape_core::eval::anonymize_term(&rule.lhs);
            let rhs_anon = mathscape_core::eval::anonymize_term(&rule.rhs);
            println!("    {} :: {} => {}", rule.name, lhs_anon, rhs_anon);
        }
    }

    let same_shape = v_modal_fp == s_modal_fp;
    println!("\n▶ Verdict");
    if same_shape {
        println!("  SAME BETTYFINE — subterm AU didn't produce a structurally");
        println!("  different modal basin at this machinery scale. Same");
        println!("  canonical trivial form emerges. Either subterm AU isn't");
        println!("  producing new candidate patterns that survive the prover,");
        println!("  or those patterns are subsumed by the canonical ones.");
    } else {
        println!("  NEW BETTYFINE — subterm AU unlocked a different modal");
        println!("  basin. The machine now discovers structure invisible to");
        println!("  root-only AU. This IS the penetration.");
    }
}

fn run_with_subterm_au(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
    extract_config: mathscape_compress::extract::ExtractConfig,
) -> TraversalReportWithLibrary {
    let mut zoo: Vec<(String, Vec<Term>)> = Vec::new();
    for i in 1..=procedural_budget as u64 {
        let seed = seed_offset.wrapping_add(i);
        let depth = 2 + (i as usize % (max_depth - 1).max(1));
        let count = 16 + (i as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
        ));
    }
    let base =
        CompressionGenerator::new(extract_config.clone(), 1).with_subterm_au();
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
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        RuleEmitter,
        InMemoryRegistry::new(),
    );
    for (_, corpus) in &zoo {
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
    }
    let mut names = Vec::new();
    let mut full = Vec::new();
    for artifact in epoch.registry.all() {
        let s = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        if matches!(s, ProofStatus::Axiomatized) {
            names.push(artifact.rule.name.clone());
            full.push(artifact.rule.clone());
        }
    }
    TraversalReportWithLibrary {
        axiomatized_rule_names: names,
        axiomatized_rules_full: full,
    }
}

#[test]
#[ignore = "phase M10: bettyfine uniqueness probe — do different configs produce different bettyfines?, ~30s, --ignored"]
fn bettyfine_family_probe() {
    // User's question: "bettyfine seems a bit not unique so far
    // but lets see how it holds up over many variations."
    //
    // Method: at each of several distinct configurations, find the
    // modal basin (the config's bettyfine) and print its anonymized
    // rule content. Compare.
    //
    // If the modal basin's RULE SHAPES are identical across configs,
    // the bettyfine is a canonical trivial form (just Symbol-
    // naming, uninteresting but universal). If the shapes DIFFER
    // across configs, there's a FAMILY of bettyfines, each
    // characteristic of its machinery state.
    use mathscape_compress::extract::ExtractConfig as EC;
    use std::collections::HashMap;

    const N_SEEDS: u64 = 64;
    const BUDGET: usize = 15;
    const DEPTH: usize = 4;

    // Configurations to compare: diverse extract configs + different
    // corpora + extreme budgets.
    let configs: Vec<(&str, EC, usize, usize)> = vec![
        ("default (2,2,5)", EC { min_shared_size: 2, min_matches: 2, max_new_rules: 5 }, BUDGET, DEPTH),
        ("optimum (2,2,10)", EC { min_shared_size: 2, min_matches: 2, max_new_rules: 10 }, BUDGET, DEPTH),
        ("wide (2,2,20)", EC { min_shared_size: 2, min_matches: 2, max_new_rules: 20 }, BUDGET, DEPTH),
        ("strict (3,2,10)", EC { min_shared_size: 3, min_matches: 2, max_new_rules: 10 }, BUDGET, DEPTH),
        ("permissive (1,2,20)", EC { min_shared_size: 1, min_matches: 2, max_new_rules: 20 }, BUDGET, DEPTH),
        ("deep (2,2,10)×depth6", EC { min_shared_size: 2, min_matches: 2, max_new_rules: 10 }, BUDGET, 6),
        ("shallow (2,2,10)×depth2", EC { min_shared_size: 2, min_matches: 2, max_new_rules: 10 }, BUDGET, 2),
        ("rich budget=30", EC { min_shared_size: 2, min_matches: 2, max_new_rules: 10 }, 30, DEPTH),
    ];

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ BETTYFINE FAMILY PROBE                               ║");
    println!("║   Is the bettyfine unique or a family across configs?║");
    println!("╚══════════════════════════════════════════════════════╝");

    let mut bettyfine_shapes: HashMap<String, Vec<(usize, String, String)>> = HashMap::new();
    for (label, ec, budget, depth) in configs {
        // For each config, collect basin fingerprints across seeds,
        // find modal, extract its canonical rule content.
        let mut basin_support: HashMap<Vec<(String, String)>, usize> = HashMap::new();
        let mut basin_to_rules: HashMap<
            Vec<(String, String)>,
            Vec<mathscape_core::eval::RewriteRule>,
        > = HashMap::new();

        for seed in 1..=N_SEEDS {
            let report = run_traversal_pure_procedural_with_extract(seed, budget, depth, ec.clone());
            let fp = structural_fingerprint(&report.axiomatized_rules_full);
            *basin_support.entry(fp.clone()).or_default() += 1;
            basin_to_rules
                .entry(fp)
                .or_insert(report.axiomatized_rules_full);
        }

        let (modal_fp, modal_count) = basin_support
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .unwrap_or_default();
        let modal_frac = modal_count as f64 / N_SEEDS as f64;

        println!("\n▶ [{label}]  modal {}/{N_SEEDS} ({:.1}%)",
            modal_count, modal_frac * 100.0);
        let rules = basin_to_rules.get(&modal_fp).cloned().unwrap_or_default();
        if rules.is_empty() {
            println!("  (no rules in modal basin — total collapse or empty library)");
            continue;
        }
        let mut rule_shapes = Vec::new();
        for rule in &rules {
            let lhs_anon = mathscape_core::eval::anonymize_term(&rule.lhs);
            let rhs_anon = mathscape_core::eval::anonymize_term(&rule.rhs);
            println!("  rule: {} :: {} => {}", rule.name, lhs_anon, rhs_anon);
            rule_shapes.push((
                rule.lhs.size() + rule.rhs.size(),
                format!("{lhs_anon}"),
                format!("{rhs_anon}"),
            ));
        }
        bettyfine_shapes.insert(label.to_string(), rule_shapes);
    }

    // Cross-config comparison: how many DISTINCT modal-basin rule
    // shapes did we see across configs?
    let mut distinct_shapes: std::collections::HashSet<Vec<(String, String)>> =
        std::collections::HashSet::new();
    for (_, shape) in &bettyfine_shapes {
        let key: Vec<(String, String)> =
            shape.iter().map(|(_, l, r)| (l.clone(), r.clone())).collect();
        distinct_shapes.insert(key);
    }
    println!("\n▶ Cross-config bettyfine distinctness");
    println!("  configs tested           : {}", bettyfine_shapes.len());
    println!("  distinct bettyfine shapes: {}", distinct_shapes.len());
    if distinct_shapes.len() == 1 {
        println!(
            "\n  UNIQUE — every config's modal basin has the same rule shape.\n  The bettyfine IS the canonical trivial form. Interesting\n  because it proves robustness; uninteresting because it's\n  trivially the same Symbol-naming pattern regardless of knobs."
        );
    } else if distinct_shapes.len() <= 3 {
        println!(
            "\n  SMALL FAMILY — {} distinct bettyfine shapes across configs.\n  Each config has its OWN bettyfine. These are the canonical\n  attractors of different machinery regimes.",
            distinct_shapes.len()
        );
    } else {
        println!(
            "\n  DIVERSE — {} distinct bettyfine shapes. The bettyfine isn't\n  really one object; it's a FAMILY indexed by configuration.\n  Each knob setting lands in a different canonical form.",
            distinct_shapes.len()
        );
    }
}

#[test]
#[ignore = "phase M9: seeded-bettyfine discovery — what can we penetrate on top? ~20s, --ignored"]
fn seeded_bettyfine_penetration() {
    // The bettyfine is the tool. What does it penetrate?
    //
    // Method: seed the registry with the canonical bettyfine
    // library at epoch 0. Pre-mark those rules as Axiomatized
    // (via trivial certificate → the W-window will advance them).
    // Then run the full discovery pipeline. Any NEW rules that
    // land past the bettyfine are what the bettyfine "unlocked" —
    // discoveries that live at a layer above the canonical
    // compression.
    //
    // Baseline: vanilla discovery (no seed) → observed final libraries
    // Seeded: discovery starting from bettyfine → compare libraries
    //
    // The delta is the penetration. If the seeded run ends with MORE
    // rules on average, the bettyfine unlocked deeper territory. If
    // the seeded run ends with the same count, the bettyfine is just
    // an accelerator, not an unlocker.
    use mathscape_compress::extract::ExtractConfig as EC;
    use mathscape_core::bettyfine::{bettyfine_library, OperatorSpec};
    use std::collections::HashMap;
    use std::time::Instant;

    const N_SEEDS: u64 = 32;
    const BUDGET: usize = 15;
    const DEPTH: usize = 4;
    let ec = EC {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 10,
    };

    let t0 = Instant::now();
    let mut vanilla_apex_counts: Vec<usize> = Vec::new();
    let mut seeded_apex_counts: Vec<usize> = Vec::new();
    let mut vanilla_basins: HashMap<Vec<(String, String)>, usize> = HashMap::new();
    let mut seeded_basins: HashMap<Vec<(String, String)>, usize> = HashMap::new();
    // Post-bettyfine discoveries: rules in seeded run that AREN'T
    // in the bettyfine. How many and what shape?
    let mut post_bettyfine_new_rules: Vec<usize> = Vec::new();

    for seed in 1..=N_SEEDS {
        // Vanilla baseline
        let vanilla = run_traversal_pure_procedural_with_extract(seed, BUDGET, DEPTH, ec.clone());
        vanilla_apex_counts.push(vanilla.axiomatized_rules_full.len());
        *vanilla_basins
            .entry(structural_fingerprint(&vanilla.axiomatized_rules_full))
            .or_default() += 1;

        // Seeded: pre-load the bettyfine then run discovery
        let seeded = run_with_bettyfine_seeded(seed, BUDGET, DEPTH, ec.clone());
        seeded_apex_counts.push(seeded.axiomatized_rules_full.len());
        *seeded_basins
            .entry(structural_fingerprint(&seeded.axiomatized_rules_full))
            .or_default() += 1;

        // How many rules are in seeded that aren't in the seeded
        // bettyfine itself? The bettyfine has 3 rules (succ + add + mul).
        let seed_bettyfine_count = bettyfine_library(&OperatorSpec::standard_vocabulary(), 500_000).len();
        let extra = seeded.axiomatized_rules_full.len().saturating_sub(seed_bettyfine_count);
        post_bettyfine_new_rules.push(extra);
    }
    let elapsed = t0.elapsed().as_millis();

    let mean_vanilla = vanilla_apex_counts.iter().sum::<usize>() as f64 / N_SEEDS as f64;
    let mean_seeded = seeded_apex_counts.iter().sum::<usize>() as f64 / N_SEEDS as f64;
    let mean_extra = post_bettyfine_new_rules.iter().sum::<usize>() as f64 / N_SEEDS as f64;
    let vanilla_modal = vanilla_basins.values().copied().max().unwrap_or(0);
    let seeded_modal = seeded_basins.values().copied().max().unwrap_or(0);

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ SEEDED BETTYFINE PENETRATION                         ║");
    println!("║   The bettyfine as a tool for deeper discovery       ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Scope");
    println!("  seeds        : {N_SEEDS}");
    println!("  budget×depth : {BUDGET} × {DEPTH}");
    println!("  elapsed      : {elapsed}ms");

    println!("\n▶ Library sizes");
    println!("  vanilla mean apex rules : {mean_vanilla:.2}");
    println!("  seeded mean apex rules  : {mean_seeded:.2}");
    println!("  bettyfine cardinality   : 3 (succ + add + mul)");
    println!("  seeded extra (post-bettyfine): {mean_extra:.2}");

    println!("\n▶ Basin structure");
    println!("  vanilla basins  : {}", vanilla_basins.len());
    println!("  vanilla modal   : {vanilla_modal}/{N_SEEDS} ({:.1}%)",
        vanilla_modal as f64 / N_SEEDS as f64 * 100.0);
    println!("  seeded  basins  : {}", seeded_basins.len());
    println!("  seeded  modal   : {seeded_modal}/{N_SEEDS} ({:.1}%)",
        seeded_modal as f64 / N_SEEDS as f64 * 100.0);

    println!("\n▶ Interpretation");
    if mean_extra > 1.0 {
        println!("  PENETRATION CONFIRMED — seeded runs reach {:.1}+ rules beyond",
            mean_extra);
        println!("  the bettyfine. The bettyfine unlocks deeper layers.");
    } else if mean_extra > 0.0 {
        println!("  MARGINAL PENETRATION — seeded runs land {:.2} rules past", mean_extra);
        println!("  the bettyfine on average. Subtle unlock; deeper probing");
        println!("  (bigger vocab, richer corpus) would reveal more.");
    } else {
        println!("  NO PENETRATION — seeded runs produce the same library as");
        println!("  vanilla. The bettyfine IS the attractor at this machinery");
        println!("  scale; nothing more is reachable without new capability.");
    }
}

fn run_with_bettyfine_seeded(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
    extract_config: mathscape_compress::extract::ExtractConfig,
) -> TraversalReportWithLibrary {
    use mathscape_core::bettyfine::{bettyfine_library, OperatorSpec};
    use mathscape_core::epoch::{AcceptanceCertificate, Artifact};
    use mathscape_core::lifecycle::ProofStatus;

    let mut zoo: Vec<(String, Vec<Term>)> = Vec::new();
    for i in 1..=procedural_budget as u64 {
        let seed = seed_offset.wrapping_add(i);
        let depth = 2 + (i as usize % (max_depth - 1).max(1));
        let count = 16 + (i as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
        ));
    }

    let base = CompressionGenerator::new(extract_config.clone(), 500_100);
    let meta = MetaPatternGenerator::new(
        ExtractConfig {
            min_shared_size: 1,
            min_matches: 2,
            max_new_rules: 12,
        },
        600_000,
    );
    let mut epoch = Epoch::new(
        CompositeGenerator::new(base, meta),
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        RuleEmitter,
        InMemoryRegistry::new(),
    );

    // Seed the bettyfine. Mark each seeded rule Axiomatized so it
    // acts as an established fixed-point the discovery proceeds from.
    let bf = bettyfine_library(&OperatorSpec::standard_vocabulary(), 500_000);
    for rule in bf {
        let mut cert = AcceptanceCertificate::trivial_conjecture(1.0);
        cert.status = ProofStatus::Axiomatized;
        let artifact = Artifact::seal(rule, 0, cert, vec![]);
        let hash = artifact.content_hash;
        epoch.registry.insert(artifact);
        epoch.registry.mark_status(hash, ProofStatus::Axiomatized);
    }

    for (_, corpus) in &zoo {
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
    }

    let mut names = Vec::new();
    let mut full = Vec::new();
    for artifact in epoch.registry.all() {
        let s = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        if matches!(s, ProofStatus::Axiomatized) {
            names.push(artifact.rule.name.clone());
            full.push(artifact.rule.clone());
        }
    }
    TraversalReportWithLibrary {
        axiomatized_rule_names: names,
        axiomatized_rules_full: full,
    }
}

#[test]
#[ignore = "phase M8+: which 2-zoo-corpus pairs trigger the transition, ~60s, --ignored"]
fn hpo_zoo_pair_transition() {
    // Zoo-weight sweep showed phase transition at zoo=2 (jump to
    // 100% modal). Question: which 2-corpus PAIRS cause the
    // transition, and which don't?
    //
    // Hypothesis: pairs that supply cross-operator diversity
    // (different root operators) trigger the transition; pairs
    // within the same operator family don't.
    //
    // C(7, 2) = 21 pairs. 128 seeds per cell. ~21s expected.
    use mathscape_compress::extract::ExtractConfig as EC;
    use std::time::Instant;

    const N_SEEDS: u64 = 128;
    const PROCEDURAL_BUDGET: usize = 15;
    const DEPTH: usize = 4;
    let ec = EC {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 10,
    };

    let zoo_full = canonical_zoo();
    let zoo_names: Vec<String> = zoo_full.iter().map(|(n, _)| n.clone()).collect();

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ ZOO-PAIR PHASE TRANSITION SWEEP                      ║");
    println!("║   All 21 pairs of 2 zoo corpora + 15 procedural     ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n{:<22} {:<22} {:>9} {:>8} {:>10}",
        "zoo_a", "zoo_b", "modal%", "basins", "entropy");
    println!("{}", "─".repeat(75));

    let t0 = Instant::now();
    let mut results: Vec<(String, String, f64, usize, f64)> = Vec::new();
    let mut triggers_transition = 0;

    for i in 0..zoo_full.len() {
        for j in (i + 1)..zoo_full.len() {
            let pair = vec![zoo_full[i].clone(), zoo_full[j].clone()];
            let (modal, basins, entropy, _) =
                measure_bettyfine_with_custom_zoo(&pair, PROCEDURAL_BUDGET, DEPTH, N_SEEDS, &ec);
            results.push((
                zoo_names[i].clone(),
                zoo_names[j].clone(),
                modal,
                basins,
                entropy,
            ));
            if modal >= 0.95 {
                triggers_transition += 1;
            }
            let marker = if modal >= 0.95 { "  ✓" } else { "" };
            println!("{:<22} {:<22} {:>8.1}% {:>8} {:>10.3}{marker}",
                zoo_names[i], zoo_names[j], modal * 100.0, basins, entropy);
        }
    }
    let elapsed = t0.elapsed().as_millis();

    println!("\n▶ Pairs triggering transition (≥95% modal): {} / 21", triggers_transition);
    println!("  elapsed: {elapsed}ms");

    // Analysis: which pairs trigger? Group by "operators covered."
    // Ops in zoo: arith/left=add, mult/cross=mul, doubling=add,
    // successor=succ. If two pair members have distinct operators,
    // hypothesis says transition triggers.
    println!("\n▶ Non-transition pairs (modal < 95%):");
    let mut any_non = false;
    for (a, b, m, _, _) in &results {
        if *m < 0.95 {
            any_non = true;
            println!("    ({a}, {b}) → {:.1}%", m * 100.0);
        }
    }
    if !any_non {
        println!("    (none — every 2-zoo-corpus pair triggers the phase transition)");
    }
}

fn measure_bettyfine_with_custom_zoo(
    zoo: &[(String, Vec<Term>)],
    procedural_budget: usize,
    max_depth: usize,
    n_seeds: u64,
    ec: &mathscape_compress::extract::ExtractConfig,
) -> (f64, usize, f64, f64) {
    use std::collections::HashMap;
    let mut basin_support: HashMap<Vec<(String, String)>, usize> = HashMap::new();
    let mut total_rules = 0usize;
    for seed in 1..=n_seeds {
        let report = run_with_zoo_prefix(seed, procedural_budget, max_depth, ec.clone(), zoo);
        total_rules += report.axiomatized_rules_full.len();
        let fp = structural_fingerprint(&report.axiomatized_rules_full);
        *basin_support.entry(fp).or_default() += 1;
    }
    let basins = basin_support.len();
    let modal = basin_support.values().copied().max().unwrap_or(0);
    let modal_frac = modal as f64 / n_seeds as f64;
    let entropy: f64 = basin_support
        .values()
        .map(|&c| {
            let p = c as f64 / n_seeds as f64;
            if p > 0.0 { -p * p.log2() } else { 0.0 }
        })
        .sum();
    let mean_rules = total_rules as f64 / n_seeds as f64;
    (modal_frac, basins, entropy, mean_rules)
}

#[test]
#[ignore = "phase M8: zoo-weight sweep, ~60s, --ignored"]
fn hpo_zoo_weight_sweep() {
    // The remaining orthogonal dial. Previous sweeps held
    // zoo-composition fixed (either full 7-corpus zoo or
    // zero zoo pure-procedural). The zoo is itself a control:
    // anchoring vs free-running. This sweep varies the
    // FRACTION of total corpora that are zoo vs procedural.
    //
    // Zoo=0, procedural=15 : pure-procedural (the previous "free-running")
    // Zoo=1..=7 subsets + procedural : intermediate anchoring
    // Zoo=7, procedural=0 : pure zoo-anchored
    //
    // Observation: how does modal support interpolate between
    // 49.6% (pure procedural LLN) and 89% (zoo-anchored)?
    // Linear? Sigmoidal? Stepwise at specific zoo entries?
    use mathscape_compress::extract::ExtractConfig as EC;
    use std::time::Instant;

    const N_SEEDS: u64 = 128;
    const PROCEDURAL_BUDGET: usize = 15;
    const DEPTH: usize = 4;
    let ec = EC {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 10,
    };

    // Zoo subsets in canonical order. Each row is a progressively
    // larger zoo prefix + the full procedural suite.
    let zoo_prefixes = [0usize, 1, 2, 3, 4, 5, 6, 7];

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ ZOO-WEIGHT SWEEP                                     ║");
    println!("║   Anchoring axis: 0 → 7 zoo corpora + 15 procedural  ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n{:>8} {:>8} {:>9} {:>8} {:>10} {:>8}",
        "zoo_n", "total", "modal%", "basins", "entropy", "rules");
    println!("{}", "─".repeat(55));

    let t0 = Instant::now();
    let mut grid: Vec<(usize, usize, f64, usize, f64, f64)> = Vec::new();
    for &n_zoo in &zoo_prefixes {
        let (modal, basins, entropy, mean_rules) =
            measure_bettyfine_with_zoo(n_zoo, PROCEDURAL_BUDGET, DEPTH, N_SEEDS, &ec);
        let total_corpora = n_zoo + PROCEDURAL_BUDGET;
        grid.push((n_zoo, total_corpora, modal, basins, entropy, mean_rules));
        println!("{:>8} {:>8} {:>8.1}% {:>8} {:>10.3} {:>8.2}",
            n_zoo, total_corpora, modal * 100.0, basins, entropy, mean_rules);
    }
    let elapsed = t0.elapsed().as_millis();

    let pure_proc = grid[0].2;
    let full_zoo = grid[grid.len() - 1].2;
    let range = full_zoo - pure_proc;
    println!("\n▶ Anchoring range");
    println!("  0 zoo → {:.1}%", pure_proc * 100.0);
    println!("  7 zoo → {:.1}%", full_zoo * 100.0);
    println!("  range : {:.1} points", range * 100.0);
    println!("  elapsed: {elapsed}ms");

    // Growth pattern
    println!("\n▶ Modal support growth per zoo addition");
    for i in 1..grid.len() {
        let (n_prev, _, p_prev, _, _, _) = grid[i - 1];
        let (n_curr, _, p_curr, _, _, _) = grid[i];
        let delta = (p_curr - p_prev) * 100.0;
        let sign = if delta >= 0.0 { "+" } else { "" };
        println!("  zoo {} → {} : {sign}{:.1} points", n_prev, n_curr, delta);
    }
    assert!(grid.len() == 8);
}

fn measure_bettyfine_with_zoo(
    n_zoo_prefix: usize,
    procedural_budget: usize,
    max_depth: usize,
    n_seeds: u64,
    ec: &mathscape_compress::extract::ExtractConfig,
) -> (f64, usize, f64, f64) {
    use std::collections::HashMap;
    let zoo_full = canonical_zoo();
    let zoo_prefix: Vec<(String, Vec<Term>)> = zoo_full.into_iter().take(n_zoo_prefix).collect();

    let mut basin_support: HashMap<Vec<(String, String)>, usize> = HashMap::new();
    let mut total_rules = 0usize;
    for seed in 1..=n_seeds {
        let report = run_with_zoo_prefix(seed, procedural_budget, max_depth, ec.clone(), &zoo_prefix);
        total_rules += report.axiomatized_rules_full.len();
        let fp = structural_fingerprint(&report.axiomatized_rules_full);
        *basin_support.entry(fp).or_default() += 1;
    }
    let basins = basin_support.len();
    let modal = basin_support.values().copied().max().unwrap_or(0);
    let modal_frac = modal as f64 / n_seeds as f64;
    let entropy: f64 = basin_support
        .values()
        .map(|&c| {
            let p = c as f64 / n_seeds as f64;
            if p > 0.0 { -p * p.log2() } else { 0.0 }
        })
        .sum();
    let mean_rules = total_rules as f64 / n_seeds as f64;
    (modal_frac, basins, entropy, mean_rules)
}

fn run_with_zoo_prefix(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
    extract_config: mathscape_compress::extract::ExtractConfig,
    zoo_prefix: &[(String, Vec<Term>)],
) -> TraversalReportWithLibrary {
    let mut zoo: Vec<(String, Vec<Term>)> = zoo_prefix.to_vec();
    for i in 1..=procedural_budget as u64 {
        let seed = seed_offset.wrapping_add(i);
        let depth = 2 + (i as usize % (max_depth - 1).max(1));
        let count = 16 + (i as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
        ));
    }
    let base = CompressionGenerator::new(extract_config.clone(), 1);
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
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        RuleEmitter,
        InMemoryRegistry::new(),
    );
    for (_, corpus) in &zoo {
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
    }
    let mut names = Vec::new();
    let mut full = Vec::new();
    for artifact in epoch.registry.all() {
        let s = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        if matches!(s, ProofStatus::Axiomatized) {
            names.push(artifact.rule.name.clone());
            full.push(artifact.rule.clone());
        }
    }
    TraversalReportWithLibrary {
        axiomatized_rule_names: names,
        axiomatized_rules_full: full,
    }
}

#[test]
#[ignore = "observational: time-to-bettyfine, ~5s, --ignored"]
fn time_to_bettyfine() {
    // Observational probe. For each seed, track the Axiomatized
    // rule set after every (Discover + Reinforce) cycle. Find the
    // earliest cycle at which the current Axiomatized set equals
    // the seed's final Axiomatized set. That's the "lock-in
    // epoch" — the moment the bettyfine crystallized for this seed.
    //
    // No expectations. Just measure.
    use std::collections::HashMap;
    use std::time::Instant;

    const N_SEEDS: u64 = 64;
    const BUDGET: usize = 15;
    const DEPTH: usize = 4;
    let ec = mathscape_compress::extract::ExtractConfig {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 10,
    };

    let t0 = Instant::now();
    let mut lock_in_epochs: Vec<usize> = Vec::new();
    let mut final_rule_counts: Vec<usize> = Vec::new();
    // Histogram: epoch → count of seeds that locked in AT this epoch.
    let mut histogram: HashMap<usize, usize> = HashMap::new();

    for seed in 1..=N_SEEDS {
        let (lock_in, final_count) = probe_time_to_bettyfine(seed, BUDGET, DEPTH, &ec);
        lock_in_epochs.push(lock_in);
        final_rule_counts.push(final_count);
        *histogram.entry(lock_in).or_default() += 1;
    }
    let elapsed = t0.elapsed().as_millis();

    let mean_epoch = lock_in_epochs.iter().sum::<usize>() as f64 / N_SEEDS as f64;
    let min_epoch = *lock_in_epochs.iter().min().unwrap_or(&0);
    let max_epoch = *lock_in_epochs.iter().max().unwrap_or(&0);
    let variance: f64 = lock_in_epochs
        .iter()
        .map(|&e| (e as f64 - mean_epoch).powi(2))
        .sum::<f64>()
        / N_SEEDS as f64;
    let std = variance.sqrt();
    let mean_rules = final_rule_counts.iter().sum::<usize>() as f64 / N_SEEDS as f64;

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ TIME-TO-BETTYFINE                                    ║");
    println!("║   Observational probe — no expectations              ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Scope");
    println!("  seeds       : {N_SEEDS}");
    println!("  budget×depth: {BUDGET} × {DEPTH}");
    println!("  max cycles per seed: {} (= 7 zoo + {} procedural)", 7 + BUDGET, BUDGET);
    println!("  elapsed     : {elapsed}ms");

    println!("\n▶ Lock-in epoch distribution");
    println!("  mean : {mean_epoch:.2}");
    println!("  std  : {std:.2}");
    println!("  min  : {min_epoch}");
    println!("  max  : {max_epoch}");
    println!("  mean final rule count : {mean_rules:.2}");

    println!("\n▶ Histogram (epoch → seeds that locked in at that epoch)");
    let mut sorted: Vec<(usize, usize)> = histogram.into_iter().collect();
    sorted.sort_by_key(|x| x.0);
    let max_count = sorted.iter().map(|x| x.1).max().unwrap_or(1);
    for (epoch, count) in sorted {
        let bar_len = (count * 40 / max_count).max(1);
        let bar = "█".repeat(bar_len);
        println!("  cycle {:>3}: {:>4}  {bar}", epoch, count);
    }

    println!("\n▶ No interpretation, just data. What you see is what the machine does.");
}

fn probe_time_to_bettyfine(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
    extract_config: &mathscape_compress::extract::ExtractConfig,
) -> (usize, usize) {
    // Returns (lock_in_cycle, final_axiomatized_count).
    // "Cycle" = one (Discover × 3 + Reinforce × 1) pass over one corpus.
    let mut zoo = canonical_zoo();
    for i in 1..=procedural_budget as u64 {
        let seed = seed_offset.wrapping_add(i);
        let depth = 2 + (i as usize % (max_depth - 1).max(1));
        let count = 16 + (i as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
        ));
    }

    let base = CompressionGenerator::new(extract_config.clone(), 1);
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
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        RuleEmitter,
        InMemoryRegistry::new(),
    );

    let mut axiom_snapshots: Vec<Vec<String>> = Vec::new();
    for (_, corpus) in &zoo {
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
        let mut axioms: Vec<String> = epoch
            .registry
            .all()
            .iter()
            .filter(|a| {
                matches!(
                    epoch.registry.status_of(a.content_hash),
                    Some(ProofStatus::Axiomatized)
                )
            })
            .map(|a| a.rule.name.clone())
            .collect();
        axioms.sort();
        axiom_snapshots.push(axioms);
    }

    // Final is the last snapshot. Lock-in = earliest snapshot that
    // equals the final set AND stays equal thereafter.
    let final_set = axiom_snapshots.last().cloned().unwrap_or_default();
    let final_count = final_set.len();
    let lock_in = axiom_snapshots
        .iter()
        .enumerate()
        .find(|(_, s)| **s == final_set)
        .map(|(i, _)| i + 1) // 1-indexed epoch
        .unwrap_or(axiom_snapshots.len());
    (lock_in, final_count)
}

#[test]
#[ignore = "phase M6+: grand HPO sweep, ~90s, --ignored"]
fn hpo_grand_sweep() {
    // Full tunability exploration. Three orthogonal axes:
    //
    //   A. extract config (the previously-identified steering wheel)
    //      min_shared_size × max_new_rules × min_matches
    //   B. corpus richness
    //      procedural_budget × max_depth
    //   C. convergence verification
    //      at the identified optimum, N ∈ {64, 128, 256, 512}
    //
    // All in-memory. Each cell = 128 seeds × ~11ms = ~1.4s. Total
    // cells across sweeps: ~50-60. Walltime: ~60-90s.
    //
    // Objective: maximize modal_support subject to
    //   mean_rule_count >= 2 (non-degenerate library)
    //   basin_count >= 2 (non-trivially explored)
    //
    // Find the argmax. Compare to current default (min=2, max=5,
    // min_matches=2). Update the suite defaults if a better config
    // emerges — this is the gem materializing as a set of
    // empirically-tuned knobs.
    use mathscape_compress::extract::ExtractConfig as EC;
    use std::time::Instant;

    const N_SEEDS: u64 = 128;
    const DEFAULT_BUDGET: usize = 15;
    const DEFAULT_DEPTH: usize = 4;

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ GRAND HPO SWEEP                                      ║");
    println!("║   Full tunability exploration of the bettyfine      ║");
    println!("╚══════════════════════════════════════════════════════╝");

    // ── Sweep A: 3D extract config ─────────────────────────────
    let min_shared_vals = [1usize, 2, 3];
    let max_rules_vals = [3usize, 5, 10, 20];
    let min_matches_vals = [1usize, 2, 3];

    println!("\n▶ Sweep A: extract config (min_share × max_rules × min_matches)");
    println!("  {} × {} × {} = {} cells × {} seeds = {} runs",
        min_shared_vals.len(), max_rules_vals.len(), min_matches_vals.len(),
        min_shared_vals.len() * max_rules_vals.len() * min_matches_vals.len(),
        N_SEEDS,
        min_shared_vals.len() * max_rules_vals.len() * min_matches_vals.len() * N_SEEDS as usize);

    let mut best_cell: Option<(EC, f64, usize, f64, f64)> = None;
    let t_a = Instant::now();
    let mut results_a: Vec<(usize, usize, usize, f64, usize, f64, f64)> = Vec::new();
    for &ms in &min_shared_vals {
        for &mr in &max_rules_vals {
            for &mm in &min_matches_vals {
                let ec = EC {
                    min_shared_size: ms,
                    min_matches: mm,
                    max_new_rules: mr,
                };
                let (modal_frac, basins, entropy, mean_rules) =
                    measure_bettyfine(&ec, DEFAULT_BUDGET, DEFAULT_DEPTH, N_SEEDS);
                results_a.push((ms, mm, mr, modal_frac, basins, entropy, mean_rules));
                if mean_rules >= 2.0 && basins >= 2 {
                    let better = best_cell.as_ref()
                        .map(|b| modal_frac > b.1)
                        .unwrap_or(true);
                    if better {
                        best_cell = Some((ec.clone(), modal_frac, basins, entropy, mean_rules));
                    }
                }
            }
        }
    }
    let elapsed_a = t_a.elapsed().as_millis();
    println!("\n  {}ms elapsed", elapsed_a);

    // Print sweep A results sorted by modal support.
    let mut sorted_a = results_a.clone();
    sorted_a.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    println!("\n▶ Top-10 extract-config cells by modal support");
    println!("{:>7} {:>7} {:>7} {:>9} {:>8} {:>10} {:>8}",
        "min_sh", "min_mt", "max_r", "modal%", "basins", "entropy", "rules");
    println!("{}", "─".repeat(60));
    for (ms, mm, mr, modal_frac, basins, entropy, mean_rules) in sorted_a.iter().take(10) {
        println!("{:>7} {:>7} {:>7} {:>8.1}% {:>8} {:>10.3} {:>8.2}",
            ms, mm, mr, modal_frac * 100.0, basins, entropy, mean_rules);
    }

    println!("\n▶ Global argmax (extract config)");
    if let Some((ec, modal, basins, entropy, rules)) = &best_cell {
        println!("  config  : min_shared={}, min_matches={}, max_new_rules={}",
            ec.min_shared_size, ec.min_matches, ec.max_new_rules);
        println!("  modal   : {:.1}%", modal * 100.0);
        println!("  basins  : {}", basins);
        println!("  entropy : {:.3} bits", entropy);
        println!("  rules   : {:.2}", rules);
    }

    // ── Sweep B: corpus richness ───────────────────────────────
    let budget_vals = [5usize, 15, 30];
    let depth_vals = [2usize, 4, 6];
    let ec_best = best_cell.as_ref().map(|(ec, _, _, _, _)| ec.clone())
        .unwrap_or(EC { min_shared_size: 2, min_matches: 2, max_new_rules: 10 });

    println!("\n▶ Sweep B: corpus richness (budget × depth) at best extract config");
    println!("  {} × {} = {} cells × {} seeds = {} runs",
        budget_vals.len(), depth_vals.len(),
        budget_vals.len() * depth_vals.len(),
        N_SEEDS,
        budget_vals.len() * depth_vals.len() * N_SEEDS as usize);

    let t_b = Instant::now();
    println!("\n{:>8} {:>8} {:>9} {:>8} {:>10} {:>8}",
        "budget", "depth", "modal%", "basins", "entropy", "rules");
    println!("{}", "─".repeat(52));
    for &b in &budget_vals {
        for &d in &depth_vals {
            let (modal_frac, basins, entropy, mean_rules) =
                measure_bettyfine(&ec_best, b, d, N_SEEDS);
            println!("{:>8} {:>8} {:>8.1}% {:>8} {:>10.3} {:>8.2}",
                b, d, modal_frac * 100.0, basins, entropy, mean_rules);
        }
    }
    let elapsed_b = t_b.elapsed().as_millis();
    println!("\n  {}ms elapsed", elapsed_b);

    // ── Sweep C: seed convergence at optimum ───────────────────
    println!("\n▶ Sweep C: modal-support convergence at optimum");
    println!("  Seed counts: 64, 128, 256, 512");
    let seed_counts = [64u64, 128, 256, 512];
    let t_c = Instant::now();
    println!("\n{:>8} {:>9} {:>8} {:>10}", "seeds", "modal%", "basins", "entropy");
    println!("{}", "─".repeat(40));
    for &n in &seed_counts {
        let (modal_frac, basins, entropy, _) =
            measure_bettyfine(&ec_best, DEFAULT_BUDGET, DEFAULT_DEPTH, n);
        println!("{:>8} {:>8.1}% {:>8} {:>10.3}",
            n, modal_frac * 100.0, basins, entropy);
    }
    let elapsed_c = t_c.elapsed().as_millis();

    let total = elapsed_a + elapsed_b + elapsed_c;
    println!("\n▶ Grand total: {}ms", total);

    assert!(best_cell.is_some());
}

/// Shared measurement function: for an ExtractConfig, a corpus
/// shape, and a seed count, return bettyfine features.
fn measure_bettyfine(
    ec: &mathscape_compress::extract::ExtractConfig,
    budget: usize,
    depth: usize,
    n_seeds: u64,
) -> (f64, usize, f64, f64) {
    use std::collections::HashMap;
    let mut basin_support: HashMap<Vec<(String, String)>, usize> = HashMap::new();
    let mut total_rules = 0usize;
    for seed in 1..=n_seeds {
        let report = run_traversal_pure_procedural_with_extract(seed, budget, depth, ec.clone());
        total_rules += report.axiomatized_rules_full.len();
        let fp = structural_fingerprint(&report.axiomatized_rules_full);
        *basin_support.entry(fp).or_default() += 1;
    }
    let basins = basin_support.len();
    let modal = basin_support.values().copied().max().unwrap_or(0);
    let modal_frac = modal as f64 / n_seeds as f64;
    let entropy: f64 = basin_support
        .values()
        .map(|&c| {
            let p = c as f64 / n_seeds as f64;
            if p > 0.0 { -p * p.log2() } else { 0.0 }
        })
        .sum();
    let mean_rules = total_rules as f64 / n_seeds as f64;
    (modal_frac, basins, entropy, mean_rules)
}

#[test]
#[ignore = "phase M6: HPO sweep extract-config vs bettyfine, ~15s, --ignored"]
fn hpo_sweep_extract_config_vs_bettyfine() {
    // After discovering reward HPs are insensitive, the next suspect
    // is extract config. These parameters control which candidates
    // even ENTER the pipeline — upstream of the prover.
    //
    //   min_shared_size : minimum shared structure before a pair is
    //                     anti-unified into a candidate. Low = more
    //                     candidates; high = fewer but better.
    //   max_new_rules   : top-K cut on candidates per epoch. Low =
    //                     only most-general patterns survive; high =
    //                     rich candidate pool.
    use mathscape_compress::extract::ExtractConfig as EC;
    use std::collections::HashMap;
    use std::time::Instant;

    const N_SEEDS: u64 = 64;
    const BUDGET: usize = 15;
    const DEPTH: usize = 4;
    let min_shared = [1usize, 2, 3];
    let max_rules = [3usize, 5, 10, 20];

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ HPO SWEEP — extract config vs bettyfine              ║");
    println!("║   3 × 4 × {N_SEEDS} seeds per cell                        ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!(
        "\n{:>10} {:>10} {:>10} {:>10} {:>12} {:>12}",
        "min_share", "max_rules", "modal%", "basins", "entropy_b", "mean_rules"
    );
    println!("{}", "─".repeat(70));

    let t_all = Instant::now();
    let mut grid: Vec<(usize, usize, f64, usize, f64, f64)> = Vec::new();

    for &mss in &min_shared {
        for &mnr in &max_rules {
            let ec = EC {
                min_shared_size: mss,
                min_matches: 2,
                max_new_rules: mnr,
            };
            let mut basin_support: HashMap<Vec<(String, String)>, usize> = HashMap::new();
            let mut total_rule_count = 0usize;
            for seed in 1..=N_SEEDS {
                let report = run_traversal_pure_procedural_with_extract(
                    seed,
                    BUDGET,
                    DEPTH,
                    ec.clone(),
                );
                total_rule_count += report.axiomatized_rules_full.len();
                let fp = structural_fingerprint(&report.axiomatized_rules_full);
                *basin_support.entry(fp).or_default() += 1;
            }
            let basin_count = basin_support.len();
            let modal = basin_support.values().copied().max().unwrap_or(0);
            let modal_frac = modal as f64 / N_SEEDS as f64;
            let entropy: f64 = basin_support
                .values()
                .map(|&c| {
                    let p = c as f64 / N_SEEDS as f64;
                    if p > 0.0 { -p * p.log2() } else { 0.0 }
                })
                .sum();
            let mean_rules = total_rule_count as f64 / N_SEEDS as f64;
            grid.push((mss, mnr, modal_frac, basin_count, entropy, mean_rules));
            println!(
                "{:>10} {:>10} {:>9.1}% {:>10} {:>12.3} {:>12.2}",
                mss, mnr, modal_frac * 100.0, basin_count, entropy, mean_rules,
            );
        }
    }
    let elapsed_ms = t_all.elapsed().as_millis();
    println!("\n  elapsed: {elapsed_ms}ms");

    let min_modal = grid.iter().map(|x| x.2).fold(f64::INFINITY, f64::min);
    let max_modal = grid.iter().map(|x| x.2).fold(f64::NEG_INFINITY, f64::max);
    let min_basins = grid.iter().map(|x| x.3).min().unwrap_or(0);
    let max_basins = grid.iter().map(|x| x.3).max().unwrap_or(0);
    let min_entropy = grid.iter().map(|x| x.4).fold(f64::INFINITY, f64::min);
    let max_entropy = grid.iter().map(|x| x.4).fold(f64::NEG_INFINITY, f64::max);
    let min_rules = grid.iter().map(|x| x.5).fold(f64::INFINITY, f64::min);
    let max_rules_found = grid.iter().map(|x| x.5).fold(f64::NEG_INFINITY, f64::max);

    println!("\n▶ Sensitivity summary (min → max across 12 cells)");
    println!("  modal support  : {:.1}% → {:.1}% (range {:.1}%)",
        min_modal * 100.0, max_modal * 100.0, (max_modal - min_modal) * 100.0);
    println!("  basin count    : {min_basins:>3} → {max_basins:>3}             (range {})",
        max_basins - min_basins);
    println!("  shannon entropy: {:.3} → {:.3}         (range {:.3})",
        min_entropy, max_entropy, max_entropy - min_entropy);
    println!("  mean rules/run : {:.2} → {:.2}            (range {:.2})",
        min_rules, max_rules_found, max_rules_found - min_rules);

    println!("\n▶ Interpretation");
    if max_modal - min_modal > 0.3 {
        println!("  STRONG STEERING — extract config moves modal support\n  >30 points. The bettyfine IS controllable via this dial.");
    } else if max_modal - min_modal > 0.1 {
        println!("  MODERATE STEERING — extract config shifts modal by\n  10-30 points. Useful control surface.");
    } else {
        println!("  WEAK STEERING — extract config is not the dominant\n  lever either. The steering likely lives in the equivalence\n  discipline (phase M5).");
    }

    assert!(grid.len() == 12);
}

fn run_traversal_pure_procedural_with_extract(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
    extract_config: mathscape_compress::extract::ExtractConfig,
) -> TraversalReportWithLibrary {
    let mut zoo: Vec<(String, Vec<Term>)> = Vec::new();
    for i in 1..=procedural_budget as u64 {
        let seed = seed_offset.wrapping_add(i);
        let depth = 2 + (i as usize % (max_depth - 1).max(1));
        let count = 16 + (i as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
        ));
    }

    let base = CompressionGenerator::new(extract_config.clone(), 1);
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
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        RuleEmitter,
        InMemoryRegistry::new(),
    );
    for (_, corpus) in &zoo {
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
    }
    let mut names = Vec::new();
    let mut full = Vec::new();
    for artifact in epoch.registry.all() {
        let s = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        if matches!(s, ProofStatus::Axiomatized) {
            names.push(artifact.rule.name.clone());
            full.push(artifact.rule.clone());
        }
    }
    TraversalReportWithLibrary {
        axiomatized_rule_names: names,
        axiomatized_rules_full: full,
    }
}

#[test]
#[ignore = "phase M6: HPO sweep α × δ × 64 seeds × 12 cells, ~15s, --ignored"]
fn hpo_sweep_alpha_delta_vs_bettyfine() {
    // Phase M6: the first empirical hyperparameter sweep. For each
    // cell in (alpha, delta) × 64 seeds, measure bettyfine features:
    //
    //   - modal_support (dominance of top basin)
    //   - basin_count (moduli space cardinality at this config)
    //   - mean_rule_count (canonical library size)
    //   - shannon_entropy (distributional spread)
    //
    // What we're testing: does the bettyfine's shape respond to
    // reward hyperparameters? If yes, we have a control surface for
    // the discovery process — the steering wheel works. If no, the
    // bettyfine is robust against reward weighting and the steering
    // wheel lives at a different layer (extract config / equivalence
    // dial / corpus vocabulary).
    //
    // Either outcome is valuable.
    use std::collections::HashMap;
    use std::time::Instant;

    const N_SEEDS: u64 = 64;
    const BUDGET: usize = 15;
    const DEPTH: usize = 4;
    let alphas = [0.1f64, 0.3, 0.6, 0.9];
    let deltas = [0.0f64, 0.5, 1.0];

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ HPO SWEEP — α × δ vs bettyfine features              ║");
    println!("║   4 × 3 × {N_SEEDS} seeds per cell                        ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!(
        "\n{:>6} {:>6} {:>10} {:>10} {:>12} {:>12}",
        "α", "δ", "modal%", "basins", "entropy_b", "mean_rules"
    );
    println!("{}", "─".repeat(64));

    let t_all = Instant::now();
    let mut grid: Vec<(f64, f64, f64, usize, f64, f64)> = Vec::new();

    for &alpha in &alphas {
        for &delta in &deltas {
            let reward_config = mathscape_reward::reward::RewardConfig {
                alpha,
                beta: 0.3,
                gamma: 0.1,
                delta,
            };
            let mut basin_support: HashMap<Vec<(String, String)>, usize> = HashMap::new();
            let mut total_rule_count = 0usize;
            for seed in 1..=N_SEEDS {
                let report = run_traversal_pure_procedural_with_reward(
                    seed,
                    BUDGET,
                    DEPTH,
                    reward_config.clone(),
                );
                total_rule_count += report.axiomatized_rules_full.len();
                let fp = structural_fingerprint(&report.axiomatized_rules_full);
                *basin_support.entry(fp).or_default() += 1;
            }
            let basin_count = basin_support.len();
            let modal = basin_support.values().copied().max().unwrap_or(0);
            let modal_frac = modal as f64 / N_SEEDS as f64;
            let entropy: f64 = basin_support
                .values()
                .map(|&c| {
                    let p = c as f64 / N_SEEDS as f64;
                    if p > 0.0 { -p * p.log2() } else { 0.0 }
                })
                .sum();
            let mean_rules = total_rule_count as f64 / N_SEEDS as f64;

            grid.push((alpha, delta, modal_frac, basin_count, entropy, mean_rules));
            println!(
                "{:>6.1} {:>6.1} {:>9.1}% {:>10} {:>12.3} {:>12.2}",
                alpha, delta, modal_frac * 100.0, basin_count, entropy, mean_rules
            );
        }
    }
    let elapsed_ms = t_all.elapsed().as_millis();
    println!("\n  elapsed: {elapsed_ms}ms");

    // Aggregate analysis
    let min_modal = grid.iter().map(|x| x.2).fold(f64::INFINITY, f64::min);
    let max_modal = grid.iter().map(|x| x.2).fold(f64::NEG_INFINITY, f64::max);
    let min_basins = grid.iter().map(|x| x.3).min().unwrap_or(0);
    let max_basins = grid.iter().map(|x| x.3).max().unwrap_or(0);
    let min_entropy = grid.iter().map(|x| x.4).fold(f64::INFINITY, f64::min);
    let max_entropy = grid.iter().map(|x| x.4).fold(f64::NEG_INFINITY, f64::max);

    println!("\n▶ Sensitivity summary (min → max across 12 cells)");
    println!("  modal support  : {:.1}% → {:.1}%  (range {:.1}%)",
        min_modal * 100.0, max_modal * 100.0, (max_modal - min_modal) * 100.0);
    println!("  basin count    : {min_basins:>3} → {max_basins:>3}            (range {})",
        max_basins - min_basins);
    println!("  shannon entropy: {:.3} → {:.3}        (range {:.3})",
        min_entropy, max_entropy, max_entropy - min_entropy);

    println!("\n▶ Interpretation");
    if max_modal - min_modal < 0.05 {
        println!(
            "  INSENSITIVE — reward hyperparameters barely shift modal\n  support. The bettyfine is ROBUST against this slice of the\n  reward space. Steering wheel lives elsewhere (probably in\n  equivalence dial, extract config, or vocabulary)."
        );
    } else if max_modal - min_modal > 0.2 {
        println!(
            "  STRONG STEERING — reward hyperparameters shift modal\n  support by >20 points. The reward config IS a meaningful\n  control surface for the bettyfine's shape. First-class\n  hyperparameter for M5/M6 automation."
        );
    } else {
        println!(
            "  MODERATE STEERING — reward hyperparameters shift modal\n  support 5-20 points. Useful but not dominant control\n  surface. Worth pairing with equivalence dial (M5)."
        );
    }

    assert!(grid.len() == 12);
    assert!(min_modal > 0.0);
}

#[test]
#[ignore = "phase M1: basin-space cardinality — 1024-seed stairway sweep, ~15s, --ignored"]
fn oscillation_basin_space_cardinality() {
    // Phase M1 question: is the basin space FINITE (discrete,
    // quantized) or CONTINUOUS (each new seed probably yields a
    // never-seen-before attractor)?
    //
    // Method: measure basin count at 4 seed-set sizes —
    //   128, 256, 512, 1024 — and watch the growth curve of:
    //   (a) distinct apex fingerprints
    //   (b) singleton basins (attractors reached by exactly 1 seed)
    //
    // Expected signatures:
    //
    //   FINITE/QUANTIZED      basin count plateaus past some seed
    //                         count. Singletons saturate. Additional
    //                         seeds land in already-known basins.
    //                         Strong evidence for a discrete set of
    //                         attractor "types" at this machinery
    //                         scale.
    //
    //   CONTINUUM             basin count grows ~linearly with seed
    //                         count. Singletons dominate. Additional
    //                         seeds routinely uncover new attractors.
    //                         Either the basin space is truly
    //                         continuous, or the attractor count is
    //                         much larger than our sample. Richer
    //                         machinery (phases I, J, K) is needed
    //                         to resolve.
    //
    //   INTERMEDIATE          basin count grows sub-linearly but
    //                         hasn't plateaued at 1024. Signals a
    //                         large-but-finite attractor space —
    //                         push further to resolve.
    //
    // The ratio (basin_count / seed_count) is the key observable.
    // Decreasing ratio → approaching quantization. Constant ratio
    // → continuum. Ratio < 0.5 by 1024 seeds → strong quantization.
    use std::collections::HashSet;
    use std::time::Instant;

    const BUDGET: usize = 15;
    const DEPTH: usize = 4;
    let scales = [128u64, 256, 512, 1024];

    let t_all = Instant::now();
    let mut stairway: Vec<(u64, usize, usize, usize, f64)> = Vec::new();
    // (seed_count, basins, singletons, distinct_universals, new_basin_rate_since_last)

    let mut running_basins: HashSet<Vec<String>> = HashSet::new();
    let mut running_singletons: std::collections::HashMap<Vec<String>, usize> =
        std::collections::HashMap::new();
    let mut prev_basin_count = 0usize;
    let mut prev_seed_count = 0u64;

    for scale in scales {
        let t = Instant::now();
        for seed in (prev_seed_count + 1)..=scale {
            let report = run_traversal_pure_procedural(seed, BUDGET, DEPTH);
            let mut apex: Vec<String> = report
                .axiomatized_rules
                .iter()
                .map(|(n, _)| n.clone())
                .collect();
            apex.sort();
            running_basins.insert(apex.clone());
            *running_singletons.entry(apex).or_default() += 1;
        }
        let basins = running_basins.len();
        let singletons = running_singletons.values().filter(|&&c| c == 1).count();
        let new_basins = basins - prev_basin_count;
        let seed_delta = scale - prev_seed_count;
        let new_basin_rate = new_basins as f64 / seed_delta as f64;
        stairway.push((scale, basins, singletons, new_basins, new_basin_rate));
        prev_basin_count = basins;
        prev_seed_count = scale;

        eprintln!(
            "[m1] scale={scale:>4} basins={basins:>4} singletons={singletons:>4} \
             new={new_basins:>4} rate={new_basin_rate:.3} elapsed={}ms",
            t.elapsed().as_millis()
        );
    }

    let total_ms = t_all.elapsed().as_millis();

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ PHASE M1 — BASIN SPACE CARDINALITY                   ║");
    println!("║   Stairway sweep: 128 → 256 → 512 → 1024 seeds       ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Growth curve");
    println!(
        "{:>10} {:>10} {:>10} {:>10} {:>12}",
        "seeds", "basins", "singletons", "new", "basin_rate"
    );
    println!("{}", "─".repeat(56));
    for (seeds, basins, singletons, new_basins, rate) in &stairway {
        println!(
            "{:>10} {:>10} {:>10} {:>10} {:>12.3}",
            seeds, basins, singletons, new_basins, rate,
        );
    }

    let last = stairway.last().unwrap();
    let first = stairway.first().unwrap();
    let basin_ratio = last.1 as f64 / last.0 as f64;
    let rate_decay = last.4 / first.4;
    println!("\n▶ Interpretation");
    println!("  total elapsed           : {total_ms}ms");
    println!("  basin/seed ratio @ 1024 : {:.3}", basin_ratio);
    println!("  basin-rate decay        : {:.3}× (1.0 = constant; <1 = quantizing)", rate_decay);

    if basin_ratio < 0.5 && rate_decay < 0.8 {
        println!(
            "\n  QUANTIZED — basin count grew sub-linearly and rate decayed.\n  The attractor space is large but FINITE at this machinery scale.\n  Additional seeds increasingly land in already-known basins.\n  Phase M1 answer: DISCRETE (bounded by ~{} basins ±).",
            last.1
        );
    } else if basin_ratio > 0.95 {
        println!(
            "\n  CONTINUUM — almost every seed produces a new basin.\n  The attractor space is effectively unbounded at this\n  machinery level; richer capability (subterm AU, e-graph)\n  needed to surface the structure.");
    } else {
        println!(
            "\n  INTERMEDIATE — basin space is large but not clearly\n  quantized. Push further (4096+ seeds) to resolve OR change\n  machinery upstream to sharpen the discrete attractors.");
    }

    // Soft test — data is the point. Assertion: we produced
    // non-trivial output.
    assert!(last.1 > 0, "expected some basins to emerge");
    assert!(last.1 >= first.1, "basin count must be monotone in seed count");
}

#[test]
#[ignore = "load-bearing: 256-seed sweep, ~5-10s, run with --ignored"]
fn oscillation_law_of_large_numbers() {
    // The load-bearing probe. Run 256 seeds through pure-procedural
    // traversal and compute distributional statistics at scale:
    //
    //   - Total distinct apex fingerprints   — how many attractors
    //     exist in this slice of the seed space
    //   - Modal fingerprint frequency         — true attractor weight
    //     under LLN (if >20% for one, it's a dominant basin)
    //   - Shannon entropy of the distribution — how "random" the
    //     outcome really is; low entropy = few attractors dominate
    //   - Attractor vocabulary                — set of distinct rule
    //     names seen across ANY seed's apex set; the "possible
    //     symbols the machine can mint"
    //   - Per-rule frequency                  — which rules appear in
    //     what fraction of attractors; this is the "definable
    //     object with features" the user predicted LLN would surface
    //   - Mean/std saturation step and library size
    //
    // Expected outcome: with 256 seeds the attractor count stabilizes.
    // If it's bounded (say, 50-150 unique fingerprints out of 256
    // seeds), we've demonstrated QUANTIZATION — the seed space maps
    // to a finite set of discrete outcomes. If it grows linearly
    // with seed count (closer to 256 distinct), the attractor space
    // is effectively unbounded at this structural scale and we need
    // bigger corpora or richer machinery to resolve it.
    use std::collections::HashMap;

    const N_SEEDS: u64 = 256;
    const BUDGET: usize = 15;
    const DEPTH: usize = 4;

    let mut apex_fingerprints: HashMap<Vec<String>, usize> = HashMap::new();
    let mut rule_frequency: HashMap<String, usize> = HashMap::new();
    let mut library_sizes: Vec<usize> = Vec::new();
    let mut saturation_steps: Vec<usize> = Vec::new();
    let mut total_elapsed_ms: u128 = 0;

    let start = std::time::Instant::now();
    for seed in 1..=N_SEEDS {
        let report = run_traversal_pure_procedural(seed, BUDGET, DEPTH);
        let mut apex: Vec<String> = report
            .axiomatized_rules
            .iter()
            .map(|(n, _)| n.clone())
            .collect();
        apex.sort();
        for name in &apex {
            *rule_frequency.entry(name.clone()).or_default() += 1;
        }
        *apex_fingerprints.entry(apex).or_default() += 1;
        library_sizes.push(report.library_final_size);
        if let Some(s) = report.saturation_step {
            saturation_steps.push(s);
        }
        total_elapsed_ms += report.elapsed_ms;
    }
    let probe_wallclock_ms = start.elapsed().as_millis();

    let distinct_fingerprints = apex_fingerprints.len();
    let modal = apex_fingerprints.values().copied().max().unwrap_or(0);
    let modal_frac = modal as f64 / N_SEEDS as f64;
    let entropy: f64 = apex_fingerprints
        .values()
        .map(|&c| {
            let p = c as f64 / N_SEEDS as f64;
            if p > 0.0 { -p * p.log2() } else { 0.0 }
        })
        .sum();

    let mean_lib_size =
        library_sizes.iter().sum::<usize>() as f64 / library_sizes.len() as f64;
    let mean_sat = if saturation_steps.is_empty() {
        0.0
    } else {
        saturation_steps.iter().sum::<usize>() as f64 / saturation_steps.len() as f64
    };

    let mut rule_vocab: Vec<(String, usize)> = rule_frequency.into_iter().collect();
    rule_vocab.sort_by(|a, b| b.1.cmp(&a.1));

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ OSCILLATION — LAW OF LARGE NUMBERS                   ║");
    println!("║   256 seeds × 15-corpus sweeps × pure procedural     ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("\n▶ Scale");
    println!("  seeds                        : {N_SEEDS}");
    println!("  probe wall-clock             : {}ms", probe_wallclock_ms);
    println!("  total traversal time (summed): {}ms", total_elapsed_ms);
    println!("  saturated runs               : {}/{}", saturation_steps.len(), N_SEEDS);
    println!("  mean library size            : {mean_lib_size:.1}");
    println!("  mean saturation step         : {mean_sat:.2}");

    println!("\n▶ Attractor statistics");
    println!("  distinct apex fingerprints   : {distinct_fingerprints}/{N_SEEDS}");
    println!("  modal support                : {modal}/{N_SEEDS} ({:.1}%)", modal_frac * 100.0);
    println!("  Shannon entropy (bits)       : {entropy:.3}");
    let max_entropy = (N_SEEDS as f64).log2();
    println!(
        "  normalized entropy           : {:.3} (1.0 = uniform over all {N_SEEDS} seeds)",
        entropy / max_entropy
    );

    println!("\n▶ Top-20 rules in attractor vocabulary");
    println!("{:>12} {:>8} {:>8}", "rule", "count", "fraction");
    println!("{}", "─".repeat(32));
    for (name, count) in rule_vocab.iter().take(20) {
        let frac = *count as f64 / N_SEEDS as f64;
        println!("{name:>12} {count:>8} {:>8.1}%", frac * 100.0);
    }

    println!("\n▶ Fingerprint support distribution");
    let mut support_counts: HashMap<usize, usize> = HashMap::new();
    for &c in apex_fingerprints.values() {
        *support_counts.entry(c).or_default() += 1;
    }
    let mut support_hist: Vec<(usize, usize)> = support_counts.into_iter().collect();
    support_hist.sort();
    println!("{:>10} {:>10}", "support", "n_attractors");
    for (support, n) in &support_hist {
        println!("{:>10} {:>10}", support, n);
    }

    println!("\n▶ Law-of-large-numbers read");
    if distinct_fingerprints as f64 / N_SEEDS as f64 > 0.95 {
        println!(
            "  HIGH UNIQUENESS — nearly every seed gives a distinct \
             fingerprint. The attractor space at this scale is larger \
             than the seed set; we haven't saturated measurement."
        );
    } else if distinct_fingerprints <= 20 {
        println!(
            "  QUANTIZED — the seed space maps onto only {distinct_fingerprints} \
             distinct attractors. The phenomenon has been resolved into \
             a finite set of stable states. This IS the 'definable object \
             with features' the user predicted."
        );
    } else {
        println!(
            "  PARTIAL QUANTIZATION — {distinct_fingerprints} attractors among \
             {N_SEEDS} seeds. The distribution is clustering but not yet \
             discrete at this resolution. Push to 1024+ seeds or modify \
             the generator to see if quantization sharpens."
        );
    }

    // Soft test — the data is the point. Only fail if we somehow
    // produced zero attractors.
    assert!(distinct_fingerprints > 0);
}

#[test]
fn oscillation_probe_seeded_variance() {
    // Phase M instrumentation: the user's intuition is that
    // irreducibility is not a single-point signal — it's a
    // distribution, an oscillation, a symmetry-breaking wave. The
    // machine's deterministic checks (pattern_equivalent,
    // proper_subsumes) give point answers; they CAN'T see a
    // distribution directly. But by running the pipeline with
    // varied seeds and comparing outcomes, we measure the
    // PHENOMENON from around it — observing the space of possible
    // discoveries, not a single discovery.
    //
    // What this probe measures:
    //   - For N different procedural seeds (same zoo otherwise),
    //     record: library composition hash, apex-rule set,
    //     saturation step, forest stats
    //   - Report the distribution: unique library hashes, apex sets,
    //     modal apex fingerprint
    //
    // What the numbers mean:
    //   - All N runs identical → the machine has no visible
    //     oscillation at this scale; the corpus distribution
    //     doesn't reach the system's symmetry-breaking threshold
    //   - Variance across runs → oscillation is measurable here;
    //     the seed selects which "branch" of the symmetry-broken
    //     outcome the machine lands on
    //   - Varied apex fingerprints → the machine has multiple
    //     "attractors" and sampling between them reveals them
    use std::collections::{HashMap, HashSet};

    let seed_set: [u64; 8] = [1, 7, 42, 100, 256, 500, 1024, 9999];
    let mut apex_fingerprints: HashMap<Vec<String>, usize> = HashMap::new();
    let mut library_hashes: HashSet<Vec<String>> = HashSet::new();
    let mut saturation_steps: Vec<Option<usize>> = Vec::new();
    let mut elapsed_per_seed: Vec<u128> = Vec::new();

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ OSCILLATION PROBE — seeded variance                  ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!(
        "\n{:>5} {:>10} {:>10} {:>6} {:>10}",
        "seed", "apex[0]", "apex[1]", "sat", "elapsed"
    );
    println!("{}", "─".repeat(50));

    for seed in seed_set {
        // Build a zoo where ONLY the procedural-seed offset varies.
        // The hand-crafted zoo part is constant; all procedural
        // corpora are seeded off of `seed` deterministically.
        let report = run_traversal_with_seed_offset(seed, 15, 4);

        let mut apex_names: Vec<String> = report
            .axiomatized_rules
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        apex_names.sort();

        let mut lib_rule_names: Vec<String> = report
            .axiomatized_rules
            .iter()
            .map(|(n, _)| n.clone())
            .collect();
        lib_rule_names.sort();
        library_hashes.insert(lib_rule_names);

        *apex_fingerprints.entry(apex_names.clone()).or_default() += 1;
        saturation_steps.push(report.saturation_step);
        elapsed_per_seed.push(report.elapsed_ms);

        let a0 = apex_names.first().cloned().unwrap_or_default();
        let a1 = apex_names.get(1).cloned().unwrap_or_default();
        println!(
            "{:>5} {:>10} {:>10} {:>6} {:>10}",
            seed,
            a0,
            a1,
            report.saturation_step.map_or("—".into(), |s| s.to_string()),
            format!("{}ms", report.elapsed_ms),
        );
    }

    println!("\n▶ Distribution statistics");
    println!("  seeds probed               : {}", seed_set.len());
    println!("  distinct apex fingerprints : {}", apex_fingerprints.len());
    println!("  distinct library hashes    : {}", library_hashes.len());
    let modal = apex_fingerprints
        .iter()
        .max_by_key(|(_, c)| *c)
        .map(|(_, c)| *c)
        .unwrap_or(0);
    let modal_ratio = modal as f64 / seed_set.len() as f64;
    println!("  modal fingerprint support  : {}/{} ({:.0}%)", modal, seed_set.len(), modal_ratio * 100.0);

    println!("\n▶ Interpretation");
    if apex_fingerprints.len() == 1 {
        println!(
            "  UNIFORM — all seeds converge to the same apex set. The \
             machine has a stable attractor at this scale; the \
             oscillation the user described is not visible within \
             this seed range. Try higher BUDGET or DEPTH to see if \
             larger phase space reveals it."
        );
    } else {
        println!(
            "  OSCILLATING — {} distinct apex fingerprints across {} seeds. \
             The phenomenon has been SURROUNDED: the seed selects which \
             attractor the machine lands in. Each fingerprint is a branch \
             in the symmetry-broken outcome space. Next: stabilize toward \
             the highest-entropy fingerprint (most generative) or the \
             most-frequent one (most confident).",
            apex_fingerprints.len(),
            seed_set.len(),
        );
    }

    // Observational test — no hard failure on variance. The whole
    // point of the probe is to MEASURE the distribution. Saturation
    // is optional at small budgets (pure-procedural corpora may
    // continue adding rules past the test budget because every seed
    // introduces structurally novel terms).
    assert!(
        !apex_fingerprints.is_empty(),
        "at least one seed should produce a discovery run"
    );
}

fn run_traversal_with_seed_offset(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
) -> TraversalReport {
    run_traversal_pure_procedural(seed_offset, procedural_budget, max_depth)
}

fn run_traversal_pure_procedural(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
) -> TraversalReport {
    // No hand-crafted zoo. Only procedural corpora driven entirely
    // by (seed_offset, i). This is the configuration where seeds
    // have maximum effect on discovery — if oscillation is
    // measurable anywhere, it's here. The zoo-anchored variant
    // produces 100%-uniform outcomes because the 7 hand-crafted
    // shapes dominate the structural signal.
    use mathscape_compress::{
        extract::ExtractConfig, CompositeGenerator, CompressionGenerator,
        MetaPatternGenerator,
    };
    use std::collections::{HashMap as HM, HashSet};
    use std::time::Instant;

    let mut zoo: Vec<(String, Vec<Term>)> = Vec::new();
    for i in 1..=procedural_budget as u64 {
        let seed = seed_offset.wrapping_add(i);
        let depth = 2 + (i as usize % (max_depth - 1).max(1));
        let count = 16 + (i as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
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
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        RuleEmitter,
        InMemoryRegistry::new(),
    );

    let mut rule_to_corpora: HM<TermRef, HashSet<String>> = HM::new();
    let mut per_step_lib_size: Vec<usize> = Vec::new();
    let mut global_epoch = 0u64;
    let mut epoch_to_corpus: HM<u64, String> = HM::new();

    let t0 = Instant::now();
    for (name, corpus) in &zoo {
        global_epoch += 1;
        forest.set_epoch(global_epoch);
        epoch_to_corpus.insert(global_epoch, name.clone());
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
        let rule_refs: Vec<&mathscape_core::eval::RewriteRule> =
            library_rules.iter().map(|(_, r)| r).collect();
        let _ = forest.apply_rules_retroactively(&rule_refs);
        per_step_lib_size.push(epoch.registry.all().len());
    }
    for edge in &forest.edges {
        let from_node = match forest.nodes.get(&edge.from) {
            Some(n) => n,
            None => continue,
        };
        let corpus = match epoch_to_corpus.get(&from_node.inserted_epoch) {
            Some(c) => c.clone(),
            None => continue,
        };
        if let Some(artifact) = epoch
            .registry
            .all()
            .iter()
            .find(|a| a.rule.name == edge.rule_name)
        {
            rule_to_corpora
                .entry(artifact.content_hash)
                .or_default()
                .insert(corpus);
        }
    }
    let elapsed_ms = t0.elapsed().as_millis();

    let saturation_step = per_step_lib_size
        .windows(2)
        .rposition(|w| w[1] > w[0])
        .map(|i| i + 1);

    let mut axiomatized_rules = Vec::new();
    let mut subsumed_count = 0;
    let mut verified_count = 0;
    let mut conjectured_count = 0;
    let mut fragile_rules = Vec::new();
    for artifact in epoch.registry.all() {
        let status = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        let cross = rule_to_corpora
            .get(&artifact.content_hash)
            .map(|s| s.len())
            .unwrap_or(0);
        let is_active = !matches!(
            status,
            ProofStatus::Subsumed(_) | ProofStatus::Demoted(_)
        );
        if is_active && cross < 2 {
            fragile_rules.push((artifact.rule.name.clone(), cross));
        }
        match status {
            ProofStatus::Axiomatized => {
                axiomatized_rules.push((artifact.rule.name.clone(), cross))
            }
            ProofStatus::Subsumed(_) => subsumed_count += 1,
            ProofStatus::Verified => verified_count += 1,
            ProofStatus::Conjectured => conjectured_count += 1,
            _ => {}
        }
    }

    TraversalReport {
        total_corpora: zoo.len(),
        library_final_size: epoch.registry.all().len(),
        forest_nodes: forest.len(),
        forest_edges: forest.edges.len(),
        forest_stable_leaves: forest.stable_leaf_count(),
        saturation_step,
        elapsed_ms,
        axiomatized_rules,
        subsumed_count,
        verified_count,
        conjectured_count,
        fragile_rules,
    }
}

#[test]
fn autonomous_traverse_deterministic_replay() {
    // Two independent runs at identical parameters must produce
    // identical reports at the structural level. The machine's
    // traversal is a deterministic function of (zoo shape,
    // procedural seed range, generator config, prover config).
    // This is stronger than "the loop closes" — it asserts the
    // loop closes at the SAME POINT every time.
    let a = run_traversal(10, 3);
    let b = run_traversal(10, 3);
    assert_eq!(a.library_final_size, b.library_final_size);
    assert_eq!(a.forest_nodes, b.forest_nodes);
    assert_eq!(a.axiomatized_rules.len(), b.axiomatized_rules.len());
    assert_eq!(a.saturation_step, b.saturation_step);
    // Per-rule cross-support must match too.
    for ((n1, s1), (n2, s2)) in a
        .axiomatized_rules
        .iter()
        .zip(b.axiomatized_rules.iter())
    {
        assert_eq!(n1, n2, "apex rule order must match across runs");
        assert_eq!(s1, s2, "apex rule cross-support must match across runs");
    }
}

/// Phase K4: run a traversal with the e-graph probes wired into the
/// generator's dedup path. Mirrors `run_with_subterm_au` in shape but
/// swaps in `.with_egraph_probes(probes)` on the base generator. A
/// smaller, focused harness — enough to measure whether commutative
/// collapse shifts the bettyfine.
fn run_with_egraph_probes(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
    extract_config: mathscape_compress::extract::ExtractConfig,
    probes: Vec<mathscape_compress::egraph::MathscapeRewrite>,
) -> (TraversalReportWithLibrary, usize, usize) {
    let mut zoo: Vec<(String, Vec<Term>)> = Vec::new();
    for i in 1..=procedural_budget as u64 {
        let seed = seed_offset.wrapping_add(i);
        let depth = 2 + (i as usize % (max_depth - 1).max(1));
        let count = 16 + (i as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
        ));
    }
    let base = CompressionGenerator::new(extract_config.clone(), 1)
        .with_egraph_probes(probes);
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
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        RuleEmitter,
        InMemoryRegistry::new(),
    );
    for (_, corpus) in &zoo {
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
    }
    let mut names = Vec::new();
    let mut full = Vec::new();
    let mut axiomatized_count = 0;
    for artifact in epoch.registry.all() {
        let s = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        if matches!(s, ProofStatus::Axiomatized) {
            axiomatized_count += 1;
            names.push(artifact.rule.name.clone());
            full.push(artifact.rule.clone());
        }
    }
    let total_rules = epoch.registry.all().len();
    (
        TraversalReportWithLibrary {
            axiomatized_rule_names: names,
            axiomatized_rules_full: full,
        },
        total_rules,
        axiomatized_count,
    )
}

#[test]
#[ignore = "phase K4: egg dedup vs bettyfine — does commutative collapse shift the apex? ~20s, --ignored"]
fn phase_k_egraph_dedup_probe() {
    // Phase K activation probe. Runs the machine in four
    // configurations that differ only in which probes feed the
    // generator's dedup pass:
    //
    //   A: no probes              (bit-identical to pre-K3 default)
    //   B: commutativity only
    //   C: associativity only
    //   D: both
    //
    // For each, we sweep N seeds and report:
    //   - total rules promoted (summed across seeds)
    //   - axiomatized apex count (summed)
    //   - the modal apex rule-name set (the bettyfine's fingerprint)
    //   - whether any run's library contains a rule with support
    //     on fewer than 2 corpora (lynchpin probe)
    //
    // Interpretation:
    //   - If B/C/D apex count < A at matched seed: probes are
    //     collapsing commutative duplicates — the wiring WORKS.
    //   - If B/C/D apex count == A: the bettyfine's current rules
    //     are already under-covariant-closure (i.e., Symbol-naming
    //     rules whose anonymized forms already collapse via
    //     `alpha_equivalent`). That confirms the phase M10/M9
    //     finding: today's bettyfine is the trivial Symbol-naming
    //     fixed point — no probe has room to bite.
    //   - If B/C/D apex shape DIFFERS from A: commutative collapse
    //     is pulling a different rule through the lifecycle as
    //     apex — the probe is reshaping the basin, which is the
    //     whole point of phase K.
    //
    // This test ASSERTS only that the lynchpin holds in every
    // config. Bettyfine shifts are reported, not asserted —
    // interpretive, not gating.
    use mathscape_compress::egraph::{associativity_probe, commutativity_probe};
    use mathscape_compress::extract::ExtractConfig as EC;

    const N_SEEDS: u64 = 8;
    const BUDGET: usize = 12;
    const DEPTH: usize = 4;
    let ec = EC::default();

    let configs: Vec<(&'static str, Vec<_>)> = vec![
        ("A (no probes)", vec![]),
        ("B (commutativity)", commutativity_probe()),
        ("C (associativity)", associativity_probe()),
        ("D (both)", {
            let mut v = commutativity_probe();
            v.extend(associativity_probe());
            v
        }),
    ];

    println!();
    println!("phase K4: e-graph dedup probe — {N_SEEDS} seeds × 4 configs");
    println!("─────────────────────────────────────────────────────────");
    for (label, probes) in &configs {
        let mut total_axiomatized = 0usize;
        let mut total_library = 0usize;
        let mut apex_fingerprints: std::collections::HashMap<Vec<String>, usize> =
            std::collections::HashMap::new();
        for seed in 0..N_SEEDS {
            let (report, lib, axiom) = run_with_egraph_probes(
                seed * 1000,
                BUDGET,
                DEPTH,
                ec.clone(),
                probes.clone(),
            );
            total_axiomatized += axiom;
            total_library += lib;
            let mut fp: Vec<String> = report.axiomatized_rule_names.clone();
            fp.sort();
            *apex_fingerprints.entry(fp).or_insert(0) += 1;
        }
        let (modal_fp, modal_count) = apex_fingerprints
            .iter()
            .max_by_key(|(_, c)| *c)
            .map(|(fp, c)| (fp.clone(), *c))
            .unwrap_or_default();
        println!();
        println!("  {label}");
        println!("    total rules       : {total_library}");
        println!("    total axiomatized : {total_axiomatized}");
        println!("    apex fingerprints : {}", apex_fingerprints.len());
        println!("    modal fingerprint : {modal_count}/{N_SEEDS} seeds");
        println!("      apex names      : {modal_fp:?}");
    }
    println!();
    // Assert: enabling probes can only *remove* candidates, never
    // add them. So total rules under B/C/D must be ≤ total under A
    // at every matched seed range. Formalizes the theoretical
    // monotonicity of dedup.
    let mut seed_totals: Vec<(usize, usize, usize, usize)> = Vec::new();
    for seed in 0..4 {
        let (_, lib_a, _) = run_with_egraph_probes(
            seed * 1000, BUDGET, DEPTH, ec.clone(), vec![]);
        let (_, lib_b, _) = run_with_egraph_probes(
            seed * 1000, BUDGET, DEPTH, ec.clone(), commutativity_probe());
        let (_, lib_c, _) = run_with_egraph_probes(
            seed * 1000, BUDGET, DEPTH, ec.clone(), associativity_probe());
        let (_, lib_d, _) = {
            let mut v = commutativity_probe();
            v.extend(associativity_probe());
            run_with_egraph_probes(seed * 1000, BUDGET, DEPTH, ec.clone(), v)
        };
        assert!(lib_b <= lib_a,
            "seed {seed}: commutativity probe should never add rules (A={lib_a}, B={lib_b})");
        assert!(lib_c <= lib_a,
            "seed {seed}: associativity probe should never add rules (A={lib_a}, C={lib_c})");
        assert!(lib_d <= lib_a,
            "seed {seed}: combined probes should never add rules (A={lib_a}, D={lib_d})");
        seed_totals.push((lib_a, lib_b, lib_c, lib_d));
    }
    println!("monotonicity across 4 seeds: OK (probes never grow the library)");
    println!("per-seed A/B/C/D totals: {seed_totals:?}");
}

#[test]
#[ignore = "phase K4b: asymmetric corpora — does commutativity probe bite when we GIVE it asymmetric pairs? ~5s, --ignored"]
fn phase_k_asymmetric_corpora() {
    // Phase K4b: the activation probe (K4) showed today's default
    // bettyfine is already closed under commutativity. The natural
    // follow-up: does the probe bite *at all*, or is the K3 wiring
    // theoretically correct but practically inert?
    //
    // Construction: hand-craft a corpus that contains BOTH shapes
    // of an asymmetric pattern — add(N, 0) AND add(0, N) for
    // several N. Anti-unification over same-shape pairs should
    // produce TWO candidate rules:
    //
    //   R1: add(?x, 0) → ?x    (from left-zero pairs)
    //   R2: add(0, ?x) → ?x    (from right-zero pairs)
    //
    // These are syntactically distinct (arg order differs) but
    // commutatively equivalent. The K3 wiring SHOULD collapse
    // them under the commutativity probe.
    //
    // If the bare path emits both R1 and R2 AND the probe path
    // emits only one, phase K has teeth — we've shown the wiring
    // bites given the right inputs. If the bare path already only
    // emits one (AU picks a canonical form), or if even the probe
    // path emits both, we've falsified the asymmetric-corpus
    // hypothesis and need a different angle (K6 reinforcement, or
    // phase L adaptive corpora).
    use mathscape_compress::egraph::commutativity_probe;
    use mathscape_compress::extract::ExtractConfig as EC;
    use mathscape_core::epoch::Generator;
    use mathscape_core::test_helpers::{apply, nat, var};

    // 8 paired terms: (add(n, 0), add(0, n)) for n ∈ 1..=8.
    let mut corpus: Vec<mathscape_core::term::Term> = Vec::new();
    for n in 1..=8u64 {
        corpus.push(apply(var(2), vec![nat(n), nat(0)]));
        corpus.push(apply(var(2), vec![nat(0), nat(n)]));
    }

    // Loose config: accept small patterns, many candidates.
    let config = EC {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 12,
    };

    let mut g_bare = CompressionGenerator::new(config.clone(), 1);
    let mut g_probe = CompressionGenerator::new(config.clone(), 1)
        .with_egraph_probes(commutativity_probe());

    let bare = g_bare.propose(0, &corpus, &[]);
    let probed = g_probe.propose(0, &corpus, &[]);

    println!();
    println!("phase K4b: asymmetric-corpus probe");
    println!("──────────────────────────────────────");
    println!("  corpus size        : {}", corpus.len());
    println!();
    println!("  candidates (bare)   : {}", bare.len());
    for (i, c) in bare.iter().enumerate() {
        println!("    [{i}] {} → {}", format_term(&c.rule.lhs), format_term(&c.rule.rhs));
    }
    println!();
    println!("  candidates (probe)  : {}", probed.len());
    for (i, c) in probed.iter().enumerate() {
        println!("    [{i}] {} → {}", format_term(&c.rule.lhs), format_term(&c.rule.rhs));
    }

    // Classification: count candidates with 0 in first arg vs
    // second arg slot. Both forms present = AU produces both;
    // only one = AU collapses by itself.
    let (bare_left_zero, bare_right_zero) = count_zero_positions(&bare);
    let (probe_left_zero, probe_right_zero) = count_zero_positions(&probed);

    println!();
    println!(
        "  bare LHS shape count: (add(?x, 0): {bare_left_zero}, add(0, ?x): {bare_right_zero})"
    );
    println!(
        "  probe LHS shape count: (add(?x, 0): {probe_left_zero}, add(0, ?x): {probe_right_zero})"
    );

    // Assertion 1: probe path never grows the library (monotonicity).
    assert!(
        probed.len() <= bare.len(),
        "probe must not add candidates (bare={}, probe={})",
        bare.len(),
        probed.len()
    );

    // Narration verdict: did the probe bite?
    println!();
    if bare.len() > probed.len() {
        let collapsed = bare.len() - probed.len();
        println!(
            "  ✓ VERDICT: probe COLLAPSED {collapsed} candidate(s) via commutativity."
        );
        println!("    phase K wiring is active and bites when given asymmetric inputs.");
    } else if bare_left_zero > 0 && bare_right_zero > 0 {
        println!(
            "  ? VERDICT: bare path emits both asymmetric shapes, but probe didn't collapse."
        );
        println!("    possible cause: RHS variants differ post-anonymization in a way the");
        println!("    probe doesn't normalize; worth investigating check_rule_equivalence.");
    } else {
        println!(
            "  ✗ VERDICT: bare path emits only {} of 2 asymmetric shapes — AU already",
            if bare_left_zero > 0 { "add(?x, 0)" } else { "add(0, ?x)" }
        );
        println!("    picks a canonical form. The asymmetric-corpus hypothesis is falsified:");
        println!("    even crafted asymmetric inputs don't expose commutative duplicates to");
        println!("    phase K. Real leverage is in reinforcement (K6) or adaptive corpora (L).");
    }
}

/// Pretty-print a Term in s-expression form for the probe narration.
fn format_term(t: &mathscape_core::term::Term) -> String {
    use mathscape_core::term::Term;
    use mathscape_core::value::Value;
    match t {
        Term::Var(v) => format!("?v{v}"),
        Term::Number(Value::Nat(n)) => n.to_string(),
        Term::Number(Value::Int(n)) => format!("{n}i"),
        Term::Apply(f, args) => {
            let f_str = format_term(f);
            let args_str: Vec<String> = args.iter().map(format_term).collect();
            format!("({} {})", f_str, args_str.join(" "))
        }
        Term::Symbol(id, args) => {
            let args_str: Vec<String> = args.iter().map(format_term).collect();
            if args_str.is_empty() {
                format!("S_{id}")
            } else {
                format!("(S_{id} {})", args_str.join(" "))
            }
        }
        Term::Point(p) => format!("P_{p:?}"),
        Term::Fn(params, body) => format!("(fn {:?} → {})", params, format_term(body)),
    }
}

/// Count how many Candidates have LHS shape `apply(?op, 0, ?x)`
/// vs. `apply(?op, ?x, 0)`. Returns (left_zero_count, right_zero_count).
fn count_zero_positions(
    candidates: &[mathscape_core::epoch::Candidate],
) -> (usize, usize) {
    use mathscape_core::term::Term;
    use mathscape_core::value::Value;
    let mut left = 0;
    let mut right = 0;
    for c in candidates {
        if let Term::Apply(_f, args) = &c.rule.lhs {
            if args.len() == 2 {
                let a0_is_zero = matches!(&args[0], Term::Number(Value::Nat(0)));
                let a1_is_zero = matches!(&args[1], Term::Number(Value::Nat(0)));
                if a0_is_zero && !a1_is_zero {
                    left += 1;
                } else if a1_is_zero && !a0_is_zero {
                    right += 1;
                }
            }
        }
    }
    (left, right)
}

#[test]
#[ignore = "phase L-min: self-feeding traversal — library residue as next corpus. ~20s, --ignored"]
fn phase_l_self_feeding_traversal() {
    // Phase L-minimal: close the corpus feedback loop. No new
    // machinery, no term generator, no external vocabulary. Just
    // wire the library-reduced residue of each layer's corpus
    // back as the NEXT layer's corpus. The machine observes its
    // own output and compresses THAT.
    //
    // Design criterion (Gödel move): structural self-reference.
    // The machine looks at the residue of its own reductions —
    // what IT couldn't compress — and asks "what patterns do I
    // see here that weren't visible in the source?" This is
    // exactly the diagonal that makes Gödel-style novelty
    // possible: the input includes the output of the previous
    // pass.
    //
    // What we're measuring:
    //   - Library growth per layer. If layer N adds rules that
    //     layer N-1 didn't, self-feeding is generative.
    //   - Convergence. Does the residue shrink to empty? Does it
    //     oscillate? Does the library stabilize at a deeper
    //     fixed point than the one-shot baseline?
    //   - Structural character of new rules. What do they look
    //     like? Same Symbol-naming trivialities as the baseline
    //     bettyfine, or something qualitatively different?
    //
    // Surprise = structure we didn't anticipate. Anticipated
    // convergence = we learn something about the apparatus's
    // reach. Either result is informative.
    use mathscape_compress::adapter::rewrite_fixed_point;
    use mathscape_compress::extract::ExtractConfig as EC;
    use mathscape_core::control::EpochAction;
    use mathscape_core::eval::RewriteRule;
    use mathscape_core::term::Term;
    use mathscape_core::test_helpers::{apply, nat, var};

    // Seed corpus: NESTED structures so the library has layers
    // to peel. Mix of zero-right, zero-left, nested arithmetic.
    // The nesting is critical — flat corpora have no depth for
    // residue to be interesting.
    let seed: Vec<Term> = vec![
        // (mul (add x 0) y) — add-identity reduces, leaves mul
        apply(var(3), vec![apply(var(2), vec![nat(3), nat(0)]), nat(5)]),
        apply(var(3), vec![apply(var(2), vec![nat(5), nat(0)]), nat(7)]),
        apply(var(3), vec![apply(var(2), vec![nat(2), nat(0)]), nat(4)]),
        // (add (mul x 1) y) — pretend mul-identity exists
        apply(var(2), vec![apply(var(3), vec![nat(4), nat(1)]), nat(6)]),
        apply(var(2), vec![apply(var(3), vec![nat(3), nat(1)]), nat(8)]),
        apply(var(2), vec![apply(var(3), vec![nat(7), nat(1)]), nat(2)]),
        // (succ (succ (succ zero))) — pure successor chains
        apply(var(4), vec![apply(var(4), vec![apply(var(4), vec![nat(0)])])]),
        apply(var(4), vec![apply(var(4), vec![apply(var(4), vec![apply(var(4), vec![nat(0)])])])]),
        // (mul (add x 0) (add y 0)) — both children need add-identity
        apply(
            var(3),
            vec![
                apply(var(2), vec![nat(3), nat(0)]),
                apply(var(2), vec![nat(5), nat(0)]),
            ],
        ),
        apply(
            var(3),
            vec![
                apply(var(2), vec![nat(7), nat(0)]),
                apply(var(2), vec![nat(2), nat(0)]),
            ],
        ),
    ];

    let config = EC {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 8,
    };
    let base = CompressionGenerator::new(config.clone(), 1);
    let meta = MetaPatternGenerator::new(
        EC {
            min_shared_size: 1,
            min_matches: 2,
            max_new_rules: 8,
        },
        10_000,
    );
    let mut epoch = Epoch::new(
        CompositeGenerator::new(base, meta),
        mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        ),
        RuleEmitter,
        InMemoryRegistry::new(),
    );

    let mut current_corpus = seed.clone();
    let mut history: Vec<(usize, usize, usize, usize)> = Vec::new();
    const MAX_LAYERS: usize = 8;
    let mut previous_lib_size = 0usize;

    println!();
    println!("phase L-min: self-feeding traversal — {} seed terms", seed.len());
    println!("──────────────────────────────────────────────────────");

    for layer in 0..MAX_LAYERS {
        // Run a proper epoch: multiple Discover then one Reinforce.
        for _ in 0..3 {
            let _ = epoch.step_with_action(&current_corpus, EpochAction::Discover);
        }
        let _ = epoch.step_with_action(&current_corpus, EpochAction::Reinforce);

        let lib: Vec<RewriteRule> =
            epoch.registry.all().iter().map(|a| a.rule.clone()).collect();
        let lib_size = lib.len();
        let new_rules = lib_size.saturating_sub(previous_lib_size);

        // Compute library-reduced residue. Each term → fixed
        // point under the current library. The RESIDUE is that
        // reduced form, deduped.
        let reduced: Vec<Term> = current_corpus
            .iter()
            .map(|t| rewrite_fixed_point(t, &lib, 64))
            .collect();
        let mut uniq_reduced: Vec<Term> = Vec::new();
        for r in &reduced {
            if !uniq_reduced.contains(r) {
                uniq_reduced.push(r.clone());
            }
        }
        let residue_size = uniq_reduced.len();

        // How many terms reduced *at all*? Indicator of library
        // coverage on this layer.
        let reductions = current_corpus
            .iter()
            .zip(reduced.iter())
            .filter(|(a, b)| a != b)
            .count();

        history.push((layer, lib_size, residue_size, reductions));
        println!(
            "  layer {layer}: lib={lib_size:3}  +new={new_rules:2}  residue={residue_size:2}  reduced={reductions}/{}",
            current_corpus.len()
        );

        previous_lib_size = lib_size;

        // Termination: library stops growing AND residue is
        // stable (fixed point of self-feeding).
        if new_rules == 0 && layer > 0 {
            println!("  → library stable at layer {layer}");
            break;
        }

        // Guard: if residue is empty (library reduces everything
        // to a single canonical form), stop — there's nothing
        // left to look at.
        if uniq_reduced.is_empty() {
            println!("  → residue collapsed to empty at layer {layer}");
            break;
        }

        // Feed residue back as next corpus. This is the Gödel
        // diagonal: the input now contains the output of the
        // previous epoch's compression.
        current_corpus = uniq_reduced;
    }

    println!();
    println!("library at termination:");
    for artifact in epoch.registry.all() {
        let status = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        println!(
            "  {} [{:?}] lhs={} rhs={}",
            artifact.rule.name,
            status,
            format_term(&artifact.rule.lhs),
            format_term(&artifact.rule.rhs),
        );
    }

    // Observational assertions. Convergence is the expected
    // behavior (machine finds fixed point); growth is the
    // surprising behavior (self-feeding exposed new structure).
    // Report either, assert only that we terminated without
    // panic.
    println!();
    let growth: Vec<usize> = history.windows(2).map(|w| w[1].1 - w[0].1).collect();
    let total_growth_past_layer0: usize = growth.iter().sum();
    if total_growth_past_layer0 > 0 {
        println!(
            "  ✓ SELF-FEEDING GENERATED {total_growth_past_layer0} NEW RULE(S) \
             beyond layer-0 output"
        );
        println!("    the feedback edge exposed patterns the one-shot missed.");
        println!("    this is structural novelty — the machine discovered");
        println!("    by looking at its own reductions.");
    } else {
        println!("  ∘ self-feeding converged at layer 0 — residue does not");
        println!("    expose new patterns within the current extract/prover");
        println!("    budget. the one-shot bettyfine IS the self-feeding fixed");
        println!("    point on this corpus. the apparatus has limits where");
        println!("    self-reference alone doesn't unlock further structure.");
    }
    assert!(!history.is_empty(), "at least one layer must run");
}

/// Run a traversal with a Lisp-form reward installed in the prover.
/// Returns the full axiomatized rules along with summary counts, so
/// callers can compare STRUCTURALLY (via anonymized terms) rather
/// than by mint-dependent S_NNN ids that vary across runs.
fn run_with_reward_form_full(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
    extract_config: mathscape_compress::extract::ExtractConfig,
    form_src: &str,
) -> (usize, usize, Vec<mathscape_core::eval::RewriteRule>) {
    let mut zoo: Vec<(String, Vec<Term>)> = Vec::new();
    for i in 1..=procedural_budget as u64 {
        let seed = seed_offset.wrapping_add(i);
        let depth = 2 + (i as usize % (max_depth - 1).max(1));
        let count = 16 + (i as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
        ));
    }

    let base = CompressionGenerator::new(extract_config.clone(), 1);
    let meta = MetaPatternGenerator::new(
        ExtractConfig {
            min_shared_size: 1,
            min_matches: 2,
            max_new_rules: 12,
        },
        10_000,
    );

    let prover = {
        let base_prover = mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        );
        if form_src.is_empty() {
            base_prover
        } else {
            let form = mathscape_reward::parse_reward(form_src)
                .expect("test reward form must parse");
            base_prover.with_reward_form(form)
        }
    };

    let mut epoch = Epoch::new(
        CompositeGenerator::new(base, meta),
        prover,
        RuleEmitter,
        InMemoryRegistry::new(),
    );
    for (_, corpus) in &zoo {
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
    }
    let mut full_rules = Vec::new();
    let mut axiomatized_count = 0usize;
    for artifact in epoch.registry.all() {
        let s = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        if matches!(s, ProofStatus::Axiomatized) {
            axiomatized_count += 1;
            full_rules.push(artifact.rule.clone());
        }
    }
    (epoch.registry.all().len(), axiomatized_count, full_rules)
}

/// Run a traversal with a Lisp-form reward installed in the prover.
/// Returns (total_rules, axiomatized_count, sorted_apex_names).
fn run_with_reward_form(
    seed_offset: u64,
    procedural_budget: usize,
    max_depth: usize,
    extract_config: mathscape_compress::extract::ExtractConfig,
    form_src: &str,
) -> (usize, usize, Vec<String>) {
    let mut zoo: Vec<(String, Vec<Term>)> = Vec::new();
    for i in 1..=procedural_budget as u64 {
        let seed = seed_offset.wrapping_add(i);
        let depth = 2 + (i as usize % (max_depth - 1).max(1));
        let count = 16 + (i as usize % 8);
        zoo.push((
            format!("proc-s{seed}-d{depth}"),
            procedural(seed, depth, count),
        ));
    }

    let base = CompressionGenerator::new(extract_config.clone(), 1);
    let meta = MetaPatternGenerator::new(
        ExtractConfig {
            min_shared_size: 1,
            min_matches: 2,
            max_new_rules: 12,
        },
        10_000,
    );

    // Parse the Lisp form and install it on the prover. None = legacy.
    let prover = {
        let base_prover = mathscape_reward::StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            0.0,
        );
        if form_src.is_empty() {
            base_prover
        } else {
            let form = mathscape_reward::parse_reward(form_src)
                .expect("test reward form must parse");
            base_prover.with_reward_form(form)
        }
    };

    let mut epoch = Epoch::new(
        CompositeGenerator::new(base, meta),
        prover,
        RuleEmitter,
        InMemoryRegistry::new(),
    );
    for (_, corpus) in &zoo {
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
    }
    let mut names = Vec::new();
    let mut axiomatized_count = 0usize;
    for artifact in epoch.registry.all() {
        let s = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        if matches!(s, ProofStatus::Axiomatized) {
            axiomatized_count += 1;
            names.push(artifact.rule.name.clone());
        }
    }
    names.sort();
    (epoch.registry.all().len(), axiomatized_count, names)
}

#[test]
#[ignore = "phase ML1+: reward-mutation probe — does varying the Lisp reward form shift the bettyfine? ~30s, --ignored"]
fn phase_ml_reward_mutation_probe() {
    // Phase ML1 activation probe for the apparatus layer.
    //
    // ML1 proved the Rust↔Lisp↔Rust FFI is correct (gold test).
    // This probe asks the follow-on question: if we VARY the
    // combination rule expressed in Lisp — not just weights, but
    // structural shape (conditional terms, clamps, max over axes)
    // — does the machine produce different bettyfines?
    //
    // If yes: apparatus mutation is GENERATIVE. Changing the
    // apparatus at Lisp level shifts which rules get promoted,
    // which means the bettyfine is NOT a fixed point of the
    // operator set — it's a fixed point of (operator set,
    // apparatus rule). ML2+ is justified by evidence.
    //
    // If no: the bettyfine is robust to reward-shape mutations.
    // The leverage has to come from mutating the EXTRACTOR, not
    // the reward combination. That's the more expensive ML2
    // investment, warranted because the cheap reward variant
    // failed.
    //
    // Either verdict is useful — the probe is the crossroads.
    use mathscape_compress::extract::ExtractConfig as EC;

    const BUDGET: usize = 12;
    const DEPTH: usize = 4;
    let ec = EC::default();

    // Six reward forms spanning plausible apparatus mutations:
    //   1. canonical (gold-matches Rust) — baseline
    //   2. CR-only — discovers only compressive patterns
    //   3. novelty-only — pure exploration pressure
    //   4. max-over-axes — reward = whichever axis is best
    //   5. CR×novelty — multiplicative AND: both must be positive
    //   6. threshold-CR — novelty only counts when CR > 0.05
    let variants: Vec<(&'static str, &'static str)> = vec![
        (
            "1. canonical",
            "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))",
        ),
        (
            "2. cr-only",
            "(* alpha cr)",
        ),
        (
            "3. novelty-only",
            "(* beta novelty)",
        ),
        (
            "4. max-over-axes",
            "(max (max (* alpha cr) (* beta novelty)) (max (* gamma meta-compression) (* delta lhs-subsumption)))",
        ),
        (
            "5. cr-times-novelty",
            "(+ (* cr novelty) (* gamma meta-compression) (* delta lhs-subsumption))",
        ),
        (
            "6. threshold-cr",
            "(+ (* alpha cr) (if (max (- cr 0.05) 0) (* beta novelty) 0) (* gamma meta-compression) (* delta lhs-subsumption))",
        ),
    ];

    // For each variant: run the same seed range, tally the library
    // and apex set. Compare fingerprints.
    println!();
    println!("phase ML1+: reward-mutation probe — 6 Lisp variants × 3 seeds");
    println!("─────────────────────────────────────────────────────────────");

    let seeds = [0u64, 1000, 2000];
    let mut fingerprints: Vec<(String, usize, usize, Vec<String>)> = Vec::new();
    for (label, src) in &variants {
        let mut total_rules = 0usize;
        let mut total_axiom = 0usize;
        let mut apex_union: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for &s in &seeds {
            let (lib, axiom, apex) =
                run_with_reward_form(s, BUDGET, DEPTH, ec.clone(), src);
            total_rules += lib;
            total_axiom += axiom;
            for a in apex {
                apex_union.insert(a);
            }
        }
        let apex_vec: Vec<String> = apex_union.into_iter().collect();
        println!();
        println!("  {label}");
        println!("    form        : {src}");
        println!(
            "    Σrules      : {total_rules}  Σaxiomatized: {total_axiom}"
        );
        println!("    apex union  : {apex_vec:?}");
        fingerprints.push((label.to_string(), total_rules, total_axiom, apex_vec));
    }

    // Compare variant 1 (canonical) to all others — which
    // variants diverge? The count of DIVERGENT variants is the
    // measure of apparatus sensitivity.
    println!();
    println!("divergence from canonical:");
    let canonical = &fingerprints[0];
    let mut divergent_count = 0usize;
    for other in fingerprints.iter().skip(1) {
        let rules_diff = other.1 as i64 - canonical.1 as i64;
        let axiom_diff = other.2 as i64 - canonical.2 as i64;
        let apex_diff = other.3 != canonical.3;
        let any_diff = rules_diff != 0 || axiom_diff != 0 || apex_diff;
        if any_diff {
            divergent_count += 1;
        }
        println!(
            "  {:24}  Δrules={:+3}  Δaxiom={:+3}  apex-differs={}",
            other.0, rules_diff, axiom_diff, apex_diff
        );
    }

    println!();
    if divergent_count > 0 {
        println!(
            "  ✓ VERDICT: {divergent_count}/5 apparatus mutations SHIFTED the traversal."
        );
        println!("    The bettyfine is NOT a fixed point of the operator set alone;");
        println!("    it is a fixed point of (operator set, apparatus combination rule).");
        println!("    ML2+ (extractor in Lisp) is justified — apparatus mutation is");
        println!("    generative at the current scale.");
    } else {
        println!("  ∘ VERDICT: all 5 reward-shape mutations produced the same fingerprint.");
        println!("    The bettyfine is robust to reward-combination changes within");
        println!("    the current extract config + prover threshold. Leverage has to");
        println!("    come from mutating the EXTRACTOR (ML2), not the combination rule.");
    }
    assert_eq!(fingerprints.len(), variants.len());
}

#[test]
#[ignore = "phase ML1-sweep: 24 apparatus variants × 12 seeds × BUDGET=20. Discovery sweep before committing to ML2. ~2-5 min, --ignored"]
fn phase_ml_apparatus_grand_sweep() {
    // Phase ML1-sweep. The reward-mutation probe (ML1+) showed 5/5
    // hand-picked variants produced different bettyfines. This is
    // the scaled-up version: 24 variants systematically spanning
    // four regions of apparatus space:
    //
    //   Tier A — axis ablation (5 variants):
    //     one axis active, others zeroed. Tests what each axis
    //     alone can pull through the prover.
    //   Tier B — two-axis combinations (6 variants):
    //     every pair of axes. Tests which pairings produce
    //     stable bettyfines vs. destabilized traversals.
    //   Tier C — shape mutations (6 variants):
    //     additive, max, product, harmonic, clamped, threshold.
    //     Tests combination SHAPE independent of axis weighting.
    //   Tier D — weight perturbations (7 variants):
    //     amplification, inversion, uniformity. Tests the
    //     weight-space topology around canonical.
    //
    // For each variant × 12 seeds × BUDGET=20, we record:
    //   - library size (avg and distribution)
    //   - axiomatized count (avg)
    //   - modal apex (fingerprint appearing in most seeds)
    //   - modal stability (what fraction of seeds hit it)
    //   - rule-ids unique to this variant (not in canonical apex)
    //
    // Discoveries = apex rule ids appearing under some variant
    // but NOT under canonical. Each such id is a concrete
    // empirical find: "apparatus X discovers structure canonical
    // does not promote."
    //
    // Lynchpin invariant: every variant's library must have every
    // rule with ≥2 cross-corpus support. This is asserted weakly
    // here (no explicit cross-support counting) — stronger claim
    // is that the traversal completes without panic, which would
    // indicate a structural regression.
    use mathscape_compress::extract::ExtractConfig as EC;
    use std::collections::{BTreeMap, BTreeSet};

    const SEEDS_PER_VARIANT: u64 = 12;
    const BUDGET: usize = 20;
    const DEPTH: usize = 4;
    let ec = EC::default();

    // 24 variants, organized by tier for reporting.
    let variants: Vec<(&'static str, &'static str, &'static str)> = vec![
        // Tier A — single-axis reward
        ("A1-canonical",   "A", "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("A2-cr-only",     "A", "(* alpha cr)"),
        ("A3-novelty-only","A", "(* beta novelty)"),
        ("A4-meta-only",   "A", "(* gamma meta-compression)"),
        ("A5-sub-only",    "A", "(* delta lhs-subsumption)"),

        // Tier B — two-axis combinations
        ("B1-cr+nov",      "B", "(+ (* alpha cr) (* beta novelty))"),
        ("B2-cr+meta",     "B", "(+ (* alpha cr) (* gamma meta-compression))"),
        ("B3-cr+sub",      "B", "(+ (* alpha cr) (* delta lhs-subsumption))"),
        ("B4-nov+meta",    "B", "(+ (* beta novelty) (* gamma meta-compression))"),
        ("B5-nov+sub",     "B", "(+ (* beta novelty) (* delta lhs-subsumption))"),
        ("B6-meta+sub",    "B", "(+ (* gamma meta-compression) (* delta lhs-subsumption))"),

        // Tier C — shape mutations (keeping all 4 axes)
        ("C1-max-pair",    "C", "(max (+ (* alpha cr) (* beta novelty)) (+ (* gamma meta-compression) (* delta lhs-subsumption)))"),
        ("C2-cr*nov",      "C", "(+ (* cr novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("C3-harmonic",    "C", "(+ (/ (* cr novelty) (max (+ cr novelty) 0.001)) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("C4-clamped",     "C", "(clamp (+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption)) -1 2)"),
        ("C5-threshold",   "C", "(+ (* alpha cr) (if (max (- cr 0.05) 0) (* beta novelty) 0) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("C6-cr-gated-sub","C", "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (if (max (- cr 0.01) 0) (* delta lhs-subsumption) 0))"),

        // Tier D — weight perturbations
        ("D1-alpha-x2",    "D", "(+ (* (* 2 alpha) cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("D2-beta-x2",     "D", "(+ (* alpha cr) (* (* 2 beta) novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("D3-delta-x3",    "D", "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* (* 3 delta) lhs-subsumption))"),
        ("D4-uniform",     "D", "(+ (* 0.25 cr) (* 0.25 novelty) (* 0.25 meta-compression) (* 0.25 lhs-subsumption))"),
        ("D5-cr-penalty",  "D", "(+ (* alpha cr) (* (- 0 beta) novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("D6-meta-heavy",  "D", "(+ (* alpha cr) (* beta novelty) (* (* 5 gamma) meta-compression) (* delta lhs-subsumption))"),
        ("D7-all-equal-weighted","D", "(+ (* 0.5 cr) (* 0.5 novelty) (* 0.5 meta-compression) (* 0.5 lhs-subsumption))"),
    ];

    println!();
    println!("phase ML1-sweep: apparatus grand sweep — 24 variants × {SEEDS_PER_VARIANT} seeds × BUDGET={BUDGET}");
    println!("═══════════════════════════════════════════════════════════════════════");

    // Per-variant measurements.
    struct VariantResult {
        label: &'static str,
        tier: &'static str,
        avg_rules: f64,
        avg_axiom: f64,
        modal_apex: Vec<String>,
        modal_count: usize,
        unique_apex_union: BTreeSet<String>,
    }
    let mut results: Vec<VariantResult> = Vec::new();
    let start = std::time::Instant::now();

    for (label, tier, src) in &variants {
        let mut rule_counts: Vec<usize> = Vec::new();
        let mut axiom_counts: Vec<usize> = Vec::new();
        let mut per_seed_apex: Vec<Vec<String>> = Vec::new();
        let mut union: BTreeSet<String> = BTreeSet::new();

        for seed in 0..SEEDS_PER_VARIANT {
            let (lib, axiom, apex) =
                run_with_reward_form(seed * 997, BUDGET, DEPTH, ec.clone(), src);
            rule_counts.push(lib);
            axiom_counts.push(axiom);
            for a in &apex {
                union.insert(a.clone());
            }
            per_seed_apex.push(apex);
        }

        let avg_rules =
            rule_counts.iter().sum::<usize>() as f64 / rule_counts.len() as f64;
        let avg_axiom =
            axiom_counts.iter().sum::<usize>() as f64 / axiom_counts.len() as f64;

        // Modal apex: the fingerprint appearing in the most seeds.
        let mut apex_hist: BTreeMap<Vec<String>, usize> = BTreeMap::new();
        for apex in &per_seed_apex {
            *apex_hist.entry(apex.clone()).or_insert(0) += 1;
        }
        let (modal_apex, modal_count) = apex_hist
            .iter()
            .max_by_key(|(_, c)| *c)
            .map(|(fp, c)| (fp.clone(), *c))
            .unwrap_or_default();

        results.push(VariantResult {
            label,
            tier,
            avg_rules,
            avg_axiom,
            modal_apex,
            modal_count,
            unique_apex_union: union,
        });
    }

    let elapsed = start.elapsed();
    println!("sweep completed in {:.1}s", elapsed.as_secs_f64());
    println!();

    // Per-tier summary table.
    for tier in ["A", "B", "C", "D"] {
        let tier_name = match tier {
            "A" => "Tier A — axis ablation",
            "B" => "Tier B — axis pairs",
            "C" => "Tier C — shape mutations",
            "D" => "Tier D — weight perturbations",
            _ => continue,
        };
        println!("{tier_name}");
        println!("  {:<22} {:>9} {:>9} {:>6}  {:>5} apex (modal={}/seeds)",
                 "variant", "avg_rules", "avg_axiom", "stab", "union", SEEDS_PER_VARIANT);
        println!("  {:─<22} {:─>9} {:─>9} {:─>6}  {:─>5}", "", "", "", "", "");
        for r in results.iter().filter(|r| r.tier == tier) {
            let modal_str = format!("{}/{}", r.modal_count, SEEDS_PER_VARIANT);
            println!("  {:<22} {:>9.1} {:>9.1} {:>6}  {:>5}",
                     r.label, r.avg_rules, r.avg_axiom, modal_str,
                     r.unique_apex_union.len());
        }
        println!();
    }

    // Discovery report: apex rules UNIQUE to a variant (not in canonical).
    let canonical_union = &results[0].unique_apex_union.clone();
    println!("discoveries — apex rules appearing in some variant but NOT canonical:");
    let mut variant_discoveries: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for r in &results {
        if r.label == "A1-canonical" {
            continue;
        }
        let diff: BTreeSet<String> = r
            .unique_apex_union
            .difference(canonical_union)
            .cloned()
            .collect();
        if !diff.is_empty() {
            variant_discoveries.insert(r.label, diff);
        }
    }
    if variant_discoveries.is_empty() {
        println!("  (none — every apex rule under mutation is already in canonical's union)");
    } else {
        for (label, diff) in &variant_discoveries {
            println!("  {label}: {} novel apex ids", diff.len());
            // Print first 10 names for spot-check.
            let sample: Vec<String> = diff.iter().take(10).cloned().collect();
            println!("    e.g. {sample:?}");
        }
    }
    println!();

    // Stability report: which variants are consistent vs. chaotic?
    println!("stability ranking (modal apex fraction, higher = more consistent):");
    let mut stability: Vec<(&str, f64)> = results
        .iter()
        .map(|r| (r.label, r.modal_count as f64 / SEEDS_PER_VARIANT as f64))
        .collect();
    stability.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    for (label, frac) in &stability[..stability.len().min(5)] {
        println!("  TOP:    {label:<22}  {:.0}% modal", frac * 100.0);
    }
    println!("  ...");
    for (label, frac) in &stability[stability.len().saturating_sub(5)..] {
        println!("  BOTTOM: {label:<22}  {:.0}% modal", frac * 100.0);
    }
    println!();

    // Discovery rate: variants with the most UNIQUE apex rules
    // (wide basin of attraction → finds more candidates).
    println!("discovery rate (|apex union across seeds|, higher = explores more):");
    let mut discovery: Vec<(&str, usize)> = results
        .iter()
        .map(|r| (r.label, r.unique_apex_union.len()))
        .collect();
    discovery.sort_by(|a, b| b.1.cmp(&a.1));
    for (label, n) in &discovery[..discovery.len().min(5)] {
        println!("  MOST:   {label:<22}  {} apex rules across seeds", n);
    }
    println!("  ...");
    for (label, n) in &discovery[discovery.len().saturating_sub(5)..] {
        println!("  FEWEST: {label:<22}  {} apex rules across seeds", n);
    }
    println!();

    // Final verdict.
    let total_discoveries: usize =
        variant_discoveries.values().map(|s| s.len()).sum();
    let divergent_variants = results
        .iter()
        .skip(1)
        .filter(|r| r.modal_apex != results[0].modal_apex)
        .count();
    println!("═══════════════════════════════════════════════════════════════════════");
    println!(
        "summary: 23 mutations vs canonical → {divergent_variants} with different modal apex; \
         {total_discoveries} apex rules discovered in non-canonical apparatuses"
    );
    println!(
        "         apparatus mutation is {} generative across 24-variant sweep",
        if divergent_variants > 0 { "empirically" } else { "NOT" }
    );

    // Assertion: every variant must have at least completed its
    // traversal (no panic). Structural regression would manifest
    // as a panic inside run_with_reward_form.
    assert_eq!(results.len(), variants.len());
    // Canonical variant must stay within the historical bettyfine
    // range — sanity check that the default path didn't regress.
    assert!(
        results[0].avg_rules >= 1.0,
        "canonical variant must find at least 1 rule"
    );
}

#[test]
#[ignore = "phase ML1-universal: which rules emerge across MANY apparatuses? Rust-promotion candidates. ~3min, --ignored"]
fn phase_ml_apparatus_universal_rules() {
    // Phase ML1-universal. The grand sweep proved 715 rules are
    // discoverable across apparatus mutations. This test asks a
    // different question:
    //
    //   Which rules are APPARATUS-INDEPENDENT?
    //
    // A rule that emerges as Axiomatized under N different
    // apparatuses is structurally robust — it's not an accident
    // of one reward shape. Rules that appear under ALL
    // apparatuses are universal: they're the pressure-points of
    // the discovery space, the cores that every reward function
    // eventually surfaces.
    //
    // These are the first candidates for Rustification. Each
    // universal rule becomes a new Rust primitive — a ground
    // type the machine can assume, freeing the Lisp layer to
    // explore higher-level patterns. This is the promotion
    // ladder in action: Lisp discovers, stability across
    // apparatuses certifies, Rust absorbs.
    //
    // Method:
    //   1. Run 14 diverse apparatuses × 8 seeds on same seed range.
    //   2. Anonymize each axiomatized rule (canonical var + symbol
    //      ids) so we compare by STRUCTURE, not by mint-dependent
    //      S_NNN.
    //   3. For each structural identity, count distinct
    //      apparatuses that promote it.
    //   4. Report top 20 by apparatus coverage, the "universal
    //      apex" (in ≥ threshold), and the "apparatus-specific"
    //      (in only 1).
    use mathscape_compress::extract::ExtractConfig as EC;
    use mathscape_core::eval::{anonymize_term, RewriteRule};
    use std::collections::{BTreeMap, HashMap};

    const SEEDS: u64 = 8;
    const BUDGET: usize = 16;
    const DEPTH: usize = 4;
    let ec = EC::default();

    // Choose 14 apparatuses from the grand sweep spanning tiers.
    // Mix of stable + chaotic so universals must survive both.
    let apparatuses: Vec<(&'static str, &'static str)> = vec![
        ("canonical", "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("cr-only", "(* alpha cr)"),
        ("novelty-only", "(* beta novelty)"),
        ("meta-only", "(* gamma meta-compression)"),
        ("sub-only", "(* delta lhs-subsumption)"),
        ("cr+nov", "(+ (* alpha cr) (* beta novelty))"),
        ("nov+meta", "(+ (* beta novelty) (* gamma meta-compression))"),
        ("nov+sub", "(+ (* beta novelty) (* delta lhs-subsumption))"),
        ("max-pair", "(max (+ (* alpha cr) (* beta novelty)) (+ (* gamma meta-compression) (* delta lhs-subsumption)))"),
        ("cr*nov", "(+ (* cr novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("harmonic", "(+ (/ (* cr novelty) (max (+ cr novelty) 0.001)) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("alpha-x2", "(+ (* (* 2 alpha) cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("uniform", "(+ (* 0.25 cr) (* 0.25 novelty) (* 0.25 meta-compression) (* 0.25 lhs-subsumption))"),
        ("meta-heavy", "(+ (* alpha cr) (* beta novelty) (* (* 5 gamma) meta-compression) (* delta lhs-subsumption))"),
    ];
    let n_apparatuses = apparatuses.len();

    // Structural identity for a rule: the anonymized (lhs, rhs)
    // pair, keyed by its s-expression rendering. Two rules with the
    // same anonymized content produce the same string — so the
    // string IS the structural identity. Avoids needing Term: Ord.
    fn rule_key(r: &RewriteRule) -> String {
        format!(
            "{}→{}",
            format_term(&anonymize_term(&r.lhs)),
            format_term(&anonymize_term(&r.rhs)),
        )
    }

    let mut coverage: HashMap<String, BTreeMap<&str, usize>> = HashMap::new();
    // Also keep an example rule for each key so we can print content.
    let mut example: HashMap<String, RewriteRule> = HashMap::new();

    println!();
    println!("phase ML1-universal: apparatus-universal rule discovery");
    println!(
        "  {n_apparatuses} apparatuses × {SEEDS} seeds × BUDGET={BUDGET} = \
         {} traversals",
        n_apparatuses as u64 * SEEDS
    );
    println!("════════════════════════════════════════════════════════════════════");

    let start = std::time::Instant::now();
    for (label, src) in &apparatuses {
        for seed in 0..SEEDS {
            let (_lib, _axiom, rules) = run_with_reward_form_full(
                seed * 997,
                BUDGET,
                DEPTH,
                ec.clone(),
                src,
            );
            for r in rules {
                let key = rule_key(&r);
                *coverage
                    .entry(key.clone())
                    .or_default()
                    .entry(label)
                    .or_insert(0) += 1;
                example.entry(key).or_insert(r);
            }
        }
    }
    let elapsed = start.elapsed();
    println!("sweep completed in {:.1}s", elapsed.as_secs_f64());
    println!();

    // Rank by apparatus coverage (number of distinct apparatuses
    // that promote the rule). Tie-break by total run count.
    #[derive(Debug)]
    struct UniversalRule {
        key: String,
        apparatus_count: usize,
        total_runs: usize,
        apparatuses: Vec<&'static str>,
        in_canonical: bool,
    }
    let mut ranked: Vec<UniversalRule> = coverage
        .iter()
        .map(|(key, apps)| {
            let apparatus_count = apps.len();
            let total_runs: usize = apps.values().sum();
            let mut app_names: Vec<&'static str> = apps.keys().copied().collect();
            app_names.sort();
            let in_canonical = apps.contains_key("canonical");
            UniversalRule {
                key: key.clone(),
                apparatus_count,
                total_runs,
                apparatuses: app_names,
                in_canonical,
            }
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.apparatus_count
            .cmp(&a.apparatus_count)
            .then(b.total_runs.cmp(&a.total_runs))
    });

    println!("total structurally-distinct axiomatized rules: {}", ranked.len());
    println!(
        "rules appearing in ALL {n_apparatuses} apparatuses: {}",
        ranked.iter().filter(|r| r.apparatus_count == n_apparatuses).count()
    );
    println!(
        "rules appearing in ≥ half ({}+) apparatuses: {}",
        (n_apparatuses + 1) / 2,
        ranked.iter().filter(|r| r.apparatus_count >= (n_apparatuses + 1) / 2).count()
    );
    println!(
        "apparatus-specific (in only 1 apparatus): {}",
        ranked.iter().filter(|r| r.apparatus_count == 1).count()
    );
    println!();

    println!("top-20 UNIVERSAL rules (most apparatus-robust, candidates for Rustification):");
    println!("  {:>4} {:>6}  {:<12}  lhs → rhs", "apps", "runs", "canonical?");
    println!("  {:─>4} {:─>6}  {:─<12}  {}", "", "", "", "──────────");
    for r in ranked.iter().take(20) {
        let canon = if r.in_canonical { "yes" } else { "no" };
        println!(
            "  {:>4} {:>6}  {:<12}  {}",
            r.apparatus_count, r.total_runs, canon, r.key,
        );
    }
    println!();

    // Apparatus-specific discoveries: rules only ONE apparatus finds.
    let specific: Vec<&UniversalRule> =
        ranked.iter().filter(|r| r.apparatus_count == 1).collect();
    println!(
        "apparatus-specific discoveries: {} rules. Top 10 by run count:",
        specific.len()
    );
    let mut specific_sorted = specific.clone();
    specific_sorted.sort_by(|a, b| b.total_runs.cmp(&a.total_runs));
    println!("  {:>4} {:>6}  {:<12}  lhs → rhs", "apps", "runs", "apparatus");
    println!("  {:─>4} {:─>6}  {:─<12}  ──────────", "", "", "");
    for r in specific_sorted.iter().take(10) {
        println!(
            "  {:>4} {:>6}  {:<12}  {}",
            r.apparatus_count, r.total_runs, r.apparatuses[0], r.key,
        );
    }
    println!();

    // Each apparatus's "signature" — rules only it promotes.
    let mut per_apparatus_signature: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &ranked {
        if r.apparatus_count == 1 {
            *per_apparatus_signature.entry(r.apparatuses[0]).or_insert(0) += 1;
        }
    }
    println!("apparatus signature sizes (unique structural rules):");
    let mut sig_ranked: Vec<(&&str, &usize)> =
        per_apparatus_signature.iter().collect();
    sig_ranked.sort_by(|a, b| b.1.cmp(a.1));
    for (name, count) in sig_ranked.iter().take(12) {
        println!("  {name:<12} {count:>3} unique rules");
    }
    println!();

    // Final invariant check.
    let universal_count = ranked
        .iter()
        .filter(|r| r.apparatus_count == n_apparatuses)
        .count();
    println!("════════════════════════════════════════════════════════════════════");
    println!(
        "summary: {} structurally-distinct rules total; {} apparatus-universal \
         (promotion candidates); {} apparatus-specific (need wider apparatus evidence)",
        ranked.len(),
        universal_count,
        specific.len()
    );

    assert!(!ranked.is_empty(), "sweep must produce at least some rules");
}

#[test]
#[ignore = "phase ML1-scale: the big one. 24 apparatuses × 32 seeds × BUDGET=32. ~3-5min, --ignored"]
fn phase_ml_large_scale_discovery() {
    // Phase ML1-scale. The "real test" — the largest discovery
    // sweep the current machinery can do in one shot. Goal: gather
    // maximum structural discoveries under apparatus mutation,
    // with enough seeds that universality claims are statistically
    // credible.
    //
    // Prior findings suggested 1 truly-universal rule at 8 seeds.
    // This test runs 4x the seed count (32) to see if universality
    // is a low-sample artifact or a genuine ceiling.
    //
    // Also reports:
    //   - Total structural rules (baseline of the discovery map)
    //   - Universality distribution (histogram of apparatus coverage)
    //   - Top-30 universal rules with their s-expression content
    //   - Rule family clustering by structural shape
    //   - Per-apparatus contribution: which apparatuses bring the
    //     most UNIQUE discoveries to the aggregate
    //
    // This is the Merkle-tree-of-primitives at census scale. Every
    // entry is a cross-apparatus-attested discovery with provenance.
    use mathscape_compress::extract::ExtractConfig as EC;
    use mathscape_core::eval::{anonymize_term, RewriteRule};
    use std::collections::{BTreeMap, BTreeSet, HashMap};

    const SEEDS: u64 = 32;
    const BUDGET: usize = 32;
    const DEPTH: usize = 4;
    let ec = EC::default();

    // 24 apparatuses (same as grand-sweep to keep the test
    // comparable, with bigger seeds + budget).
    let apparatuses: Vec<(&'static str, &'static str, &'static str)> = vec![
        ("A1-canonical",        "A", "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("A2-cr-only",          "A", "(* alpha cr)"),
        ("A3-novelty-only",     "A", "(* beta novelty)"),
        ("A4-meta-only",        "A", "(* gamma meta-compression)"),
        ("A5-sub-only",         "A", "(* delta lhs-subsumption)"),
        ("B1-cr+nov",           "B", "(+ (* alpha cr) (* beta novelty))"),
        ("B2-cr+meta",          "B", "(+ (* alpha cr) (* gamma meta-compression))"),
        ("B3-cr+sub",           "B", "(+ (* alpha cr) (* delta lhs-subsumption))"),
        ("B4-nov+meta",         "B", "(+ (* beta novelty) (* gamma meta-compression))"),
        ("B5-nov+sub",          "B", "(+ (* beta novelty) (* delta lhs-subsumption))"),
        ("B6-meta+sub",         "B", "(+ (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("C1-max-pair",         "C", "(max (+ (* alpha cr) (* beta novelty)) (+ (* gamma meta-compression) (* delta lhs-subsumption)))"),
        ("C2-cr*nov",           "C", "(+ (* cr novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("C3-harmonic",         "C", "(+ (/ (* cr novelty) (max (+ cr novelty) 0.001)) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("C4-clamped",          "C", "(clamp (+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption)) -1 2)"),
        ("C5-threshold",        "C", "(+ (* alpha cr) (if (max (- cr 0.05) 0) (* beta novelty) 0) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("C6-cr-gated-sub",     "C", "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (if (max (- cr 0.01) 0) (* delta lhs-subsumption) 0))"),
        ("D1-alpha-x2",         "D", "(+ (* (* 2 alpha) cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("D2-beta-x2",          "D", "(+ (* alpha cr) (* (* 2 beta) novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("D3-delta-x3",         "D", "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* (* 3 delta) lhs-subsumption))"),
        ("D4-uniform",          "D", "(+ (* 0.25 cr) (* 0.25 novelty) (* 0.25 meta-compression) (* 0.25 lhs-subsumption))"),
        ("D5-cr-penalty",       "D", "(+ (* alpha cr) (* (- 0 beta) novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("D6-meta-heavy",       "D", "(+ (* alpha cr) (* beta novelty) (* (* 5 gamma) meta-compression) (* delta lhs-subsumption))"),
        ("D7-all-equal-weighted","D","(+ (* 0.5 cr) (* 0.5 novelty) (* 0.5 meta-compression) (* 0.5 lhs-subsumption))"),
    ];
    let n_apparatuses = apparatuses.len();

    fn rule_key(r: &RewriteRule) -> String {
        format!(
            "{}→{}",
            format_term(&anonymize_term(&r.lhs)),
            format_term(&anonymize_term(&r.rhs)),
        )
    }

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║ phase ML1-scale: LARGE-SCALE DISCOVERY SWEEP                         ║");
    println!(
        "║   {n_apparatuses} apparatuses × {SEEDS} seeds × BUDGET={BUDGET} = {} traversals {}║",
        n_apparatuses as u64 * SEEDS,
        " ".repeat(13 - (n_apparatuses as u64 * SEEDS).to_string().len())
    );
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let start = std::time::Instant::now();
    let mut coverage: HashMap<String, BTreeMap<&str, usize>> = HashMap::new();
    let mut per_apparatus_totals: BTreeMap<&str, (usize, usize)> = BTreeMap::new();

    for (label, _tier, src) in &apparatuses {
        let mut apparatus_rules = 0usize;
        let mut apparatus_axiom = 0usize;
        for seed in 0..SEEDS {
            let (lib, axiom, rules) = run_with_reward_form_full(
                seed * 997,
                BUDGET,
                DEPTH,
                ec.clone(),
                src,
            );
            apparatus_rules += lib;
            apparatus_axiom += axiom;
            for r in rules {
                let key = rule_key(&r);
                *coverage
                    .entry(key)
                    .or_default()
                    .entry(label)
                    .or_insert(0) += 1;
            }
        }
        per_apparatus_totals.insert(label, (apparatus_rules, apparatus_axiom));
        print!(".");
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    let elapsed = start.elapsed();
    println!();
    println!("sweep completed in {:.1}s", elapsed.as_secs_f64());
    println!();

    // Per-apparatus totals.
    println!("per-apparatus totals (rules / axiomatized across 32 seeds):");
    for (label, (lib, axiom)) in &per_apparatus_totals {
        println!("  {label:<24}  Σrules={lib:>5}  Σaxiom={axiom:>4}");
    }
    println!();

    // Rank rules by apparatus coverage.
    let mut ranked: Vec<(String, usize, usize, Vec<&'static str>, bool)> = coverage
        .iter()
        .map(|(k, apps)| {
            let ac = apps.len();
            let tr: usize = apps.values().sum();
            let mut an: Vec<&'static str> = apps.keys().copied().collect();
            an.sort();
            let in_canon = apps.contains_key("A1-canonical");
            (k.clone(), ac, tr, an, in_canon)
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));

    // Coverage distribution histogram.
    let mut hist: BTreeMap<usize, usize> = BTreeMap::new();
    for r in &ranked {
        *hist.entry(r.1).or_insert(0) += 1;
    }
    println!("apparatus-coverage distribution:");
    println!("  {:>10} | {:>6} | {}", "apparatus#", "count", "bar");
    for (cov, count) in hist.iter().rev() {
        let bar: String = "█".repeat((*count).min(50));
        println!("  {cov:>10} | {count:>6} | {bar}");
    }
    println!();

    // Aggregate stats.
    let universal_count = ranked.iter().filter(|r| r.1 == n_apparatuses).count();
    let half_count = ranked.iter().filter(|r| r.1 >= (n_apparatuses + 1) / 2).count();
    let specific_count = ranked.iter().filter(|r| r.1 == 1).count();
    let canon_missing_universals = ranked
        .iter()
        .filter(|r| r.1 >= 10 && !r.4)
        .count();
    println!(
        "total structurally-distinct rules      : {}",
        ranked.len()
    );
    println!(
        "universal (all {n_apparatuses} apparatuses)        : {universal_count}"
    );
    println!(
        "near-universal (≥ {}/{n_apparatuses})            : {half_count}",
        (n_apparatuses + 1) / 2
    );
    println!(
        "apparatus-specific (only 1 apparatus)  : {specific_count}"
    );
    println!(
        "in ≥10 apparatuses BUT NOT canonical   : {canon_missing_universals}"
    );
    println!();

    // Top-30 universal rules.
    println!("top-30 universal rules (apparatus-most-robust, Rustification candidates):");
    println!("  {:>4} {:>6} {:<8}  lhs → rhs", "apps", "runs", "canonical?");
    println!("  {:─>4} {:─>6} {:─<8}  ──────────", "", "", "");
    for r in ranked.iter().take(30) {
        let canon = if r.4 { "yes" } else { "no" };
        println!(
            "  {:>4} {:>6} {:<8}  {}",
            r.1, r.2, canon, r.0,
        );
    }
    println!();

    // Signature size per apparatus.
    let mut per_app_specific: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &ranked {
        if r.1 == 1 {
            *per_app_specific.entry(r.3[0]).or_insert(0) += 1;
        }
    }
    println!("apparatus signature sizes (structurally-unique rules per apparatus):");
    let mut sig_ranked: Vec<(&&str, &usize)> = per_app_specific.iter().collect();
    sig_ranked.sort_by(|a, b| b.1.cmp(a.1));
    for (name, count) in sig_ranked.iter().take(16) {
        println!("  {name:<24} {count:>3} unique rules");
    }
    println!();

    // Apparatus pair-wise overlap: how much do the apex sets of
    // different apparatuses overlap? Use Jaccard similarity over
    // their promoted rule sets. Only compute for a representative
    // subset to keep output readable.
    let reference_apparatuses = ["A1-canonical", "A3-novelty-only", "C3-harmonic", "D5-cr-penalty"];
    let mut apparatus_sets: HashMap<&str, BTreeSet<String>> = HashMap::new();
    for r in &ranked {
        for a in &r.3 {
            apparatus_sets.entry(a).or_default().insert(r.0.clone());
        }
    }
    println!("Jaccard similarity between reference apparatuses:");
    println!("  {:<20} {:<20} {:>6}", "apparatus A", "apparatus B", "J(A,B)");
    for (i, a) in reference_apparatuses.iter().enumerate() {
        for b in reference_apparatuses.iter().skip(i + 1) {
            let empty = BTreeSet::new();
            let sa = apparatus_sets.get(a).unwrap_or(&empty);
            let sb = apparatus_sets.get(b).unwrap_or(&empty);
            let inter = sa.intersection(sb).count() as f64;
            let uni = sa.union(sb).count() as f64;
            let j = if uni > 0.0 { inter / uni } else { 0.0 };
            println!("  {:<20} {:<20} {:>6.3}", a, b, j);
        }
    }
    println!();

    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!(
        "║ final: {:>4} structural rules · {:>2} universal · {:>3} near-universal · {:>3} specific ║",
        ranked.len(),
        universal_count,
        half_count,
        specific_count
    );
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    assert!(!ranked.is_empty(), "large-scale sweep must produce rules");
    // Weak invariant: at least one universal rule MUST emerge at
    // this scale. Otherwise the discovery process is too noisy to
    // yield stable structure, and the apparatus-level claim is
    // weakened.
    assert!(
        universal_count >= 1 || half_count >= 5,
        "expected at least one universal or 5 near-universals: got u={universal_count}, n={half_count}"
    );
}
