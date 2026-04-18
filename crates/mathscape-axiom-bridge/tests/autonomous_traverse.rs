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

use mathscape_compress::{CompositeGenerator, CompressionGenerator, MetaPatternGenerator};
use mathscape_core::{
    epoch::{Epoch, InMemoryRegistry, Registry, RuleEmitter},
    form_tree::DiscoveryForest,
    hash::TermRef,
    lifecycle::ProofStatus,
    term::Term,
    value::Value,
};
use mathscape_compress::extract::ExtractConfig;

// ── Term builders ───────────────────────────────────────────────

fn apply(f: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(f), args)
}
fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}
fn var(id: u32) -> Term {
    Term::Var(id)
}

// ── Hand-crafted zoo corpora (always included) ──────────────────

fn arith_right_id() -> Vec<Term> {
    (1..=10).map(|n| apply(var(2), vec![nat(n), nat(0)])).collect()
}
fn mult_right_id() -> Vec<Term> {
    (1..=10).map(|n| apply(var(3), vec![nat(n), nat(1)])).collect()
}
fn compositional() -> Vec<Term> {
    let mut v = Vec::new();
    for n in 1..=6 {
        v.push(apply(var(2), vec![nat(n), nat(0)]));
        v.push(apply(var(2), vec![apply(var(2), vec![nat(n), nat(0)]), nat(0)]));
        v.push(apply(var(3), vec![nat(n), nat(1)]));
        v.push(apply(var(3), vec![apply(var(3), vec![nat(n), nat(1)]), nat(1)]));
    }
    v
}
fn left_identity() -> Vec<Term> {
    let mut v = Vec::new();
    for n in 1..=8 {
        v.push(apply(var(2), vec![nat(0), nat(n)]));
        v.push(apply(var(3), vec![nat(1), nat(n)]));
    }
    v
}
fn doubling() -> Vec<Term> {
    (1..=10).map(|n| apply(var(2), vec![nat(n), nat(n)])).collect()
}
fn successor_chain() -> Vec<Term> {
    let mut v = Vec::new();
    for base in 0..=3u64 {
        for depth in 1..=4usize {
            let mut t = nat(base);
            for _ in 0..depth {
                t = apply(var(4), vec![t]);
            }
            v.push(t);
        }
    }
    v
}
fn cross_op() -> Vec<Term> {
    let mut v = Vec::new();
    for n in 1..=6u64 {
        v.push(apply(
            var(2),
            vec![apply(var(3), vec![nat(n), nat(2)]), nat(0)],
        ));
        v.push(apply(
            var(3),
            vec![apply(var(2), vec![nat(n), nat(0)]), nat(3)],
        ));
    }
    v
}

/// Seeded xorshift procedural generator. Builds `term_count` terms
/// of depth ≤ `max_depth` using operator vocabulary {add, mul, succ}
/// and constants in [0, 10].
fn procedural(seed: u64, max_depth: usize, term_count: usize) -> Vec<Term> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).max(1);
    let mut next_u64 = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let ops: [u32; 3] = [2, 3, 4];

    fn build(
        depth: usize,
        max_depth: usize,
        ops: &[u32],
        next: &mut dyn FnMut() -> u64,
    ) -> Term {
        if depth >= max_depth || next() % 3 == 0 {
            let v = (next() % 11) as u64;
            return nat(v);
        }
        let op_idx = (next() % ops.len() as u64) as usize;
        let op = ops[op_idx];
        let arity = if op == 4 { 1 } else { 2 };
        let mut args = Vec::with_capacity(arity);
        for _ in 0..arity {
            args.push(build(depth + 1, max_depth, ops, next));
        }
        apply(var(op), args)
    }

    let mut out = Vec::with_capacity(term_count);
    for _ in 0..term_count {
        out.push(build(0, max_depth, &ops, &mut next_u64));
    }
    out
}

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

    let mut zoo: Vec<(String, Vec<Term>)> = vec![
        ("arith-right-id".into(), arith_right_id()),
        ("mult-right-id".into(), mult_right_id()),
        ("compositional".into(), compositional()),
        ("left-identity".into(), left_identity()),
        ("doubling".into(), doubling()),
        ("successor-chain".into(), successor_chain()),
        ("cross-op".into(), cross_op()),
    ];
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
            max_new_rules: 5,
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

    let t0 = Instant::now();
    for (name, corpus) in &zoo {
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
        let name_to_hash: HashMap<String, TermRef> = library_rules
            .iter()
            .map(|(h, r)| (r.name.clone(), *h))
            .collect();
        let edges_before = forest.edges.len();
        let rule_refs: Vec<&mathscape_core::eval::RewriteRule> =
            library_rules.iter().map(|(_, r)| r).collect();
        let _ = forest.apply_rules_retroactively(&rule_refs);
        for edge in &forest.edges[edges_before..] {
            if let Some(h) = name_to_hash.get(&edge.rule_name) {
                rule_to_corpora.entry(*h).or_default().insert(name.clone());
            }
        }

        per_step_lib_size.push(epoch.registry.all().len());
    }
    let elapsed_ms = t0.elapsed().as_millis();

    // Saturation: last step where library grew.
    let saturation_step = per_step_lib_size
        .windows(2)
        .rposition(|w| w[1] > w[0])
        .map(|i| i + 1);

    // Status tally + apex + fragile.
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
        if cross < 2 {
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
    let half = r.total_corpora / 2;
    for (name, support) in &r.axiomatized_rules {
        assert!(
            *support >= half,
            "Axiomatized rule {name} has only {support}/{} cross-support (need ≥{half}); \
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
