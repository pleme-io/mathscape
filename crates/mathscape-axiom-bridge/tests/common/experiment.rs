//! Experiment harness for the apparatus-level discovery campaign.
//!
//! Parameterizes a single apparatus-sweep experiment as DATA — every
//! axis (apparatus set, corpus family, seed count, extract config,
//! budget, depth, prover threshold) is a struct field. The catalog
//! then becomes a `Vec<Experiment>` — 100+ experiments expressed
//! declaratively, each a hypothesis the runner can answer empirically.
//!
//! This is the Gödel-diagonal move applied to the TEST infrastructure:
//! one runner, many configurations, each configuration a claim about
//! what the machine does. The common-case experiment takes ~1-3
//! seconds so the whole catalog runs in a few minutes at release
//! build.
//!
//! Separation of concerns:
//!   - `Experiment`           : the hypothesis, as data
//!   - `CorpusFamily`         : closed set of corpus generators
//!   - `ExperimentReport`     : the measured outcome
//!   - `run_experiment(e)`    : single-experiment runner
//!
//! Test files pull in the harness via `mod common; use common::experiment::*;`.

#![allow(dead_code)]

use super::{
    apply, canonical_zoo, cross_op, doubling, left_identity, nat, procedural, var,
};
use mathscape_compress::{
    extract::ExtractConfig, CompositeGenerator, CompressionGenerator, MetaPatternGenerator,
};
use mathscape_core::{
    control::EpochAction,
    epoch::{Epoch, InMemoryRegistry, Registry, RuleEmitter},
    eval::{anonymize_term, RewriteRule},
    lifecycle::ProofStatus,
    term::Term,
};
use mathscape_reward::StatisticalProver;
use std::collections::{BTreeMap, HashMap};

/// A corpus family — closed set of corpus generators the catalog
/// can choose from. Each family is a deterministic function of
/// (seed, budget, depth) so experiments are replayable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusFamily {
    /// `procedural(seed, depth, count)` — xorshift-generated terms
    /// over `{add, mul, succ}`. The bread-and-butter corpus.
    Procedural,
    /// Hand-crafted canonical zoo (7 corpora of distinct shapes)
    /// followed by procedural. Mirrors the autonomous-traversal
    /// milestone corpus.
    ZooPlusProcedural,
    /// `asymmetric_arith(seed)` — mixed add(N, 0) + add(0, N) pairs,
    /// mul(N, 1) + mul(1, N). Surfaces commutative duplicates.
    AsymmetricArith,
    /// Deeply nested arithmetic: `(mul (add x 0) (add y 0))` shapes
    /// with 3-4 levels of nesting. Probes cross-level compression.
    DeeplyNested,
    /// Successor chains of varying depth: `succ(succ(...succ(n)))`.
    /// Probes the universal rule `(?v4 (?v4 ?v100))` identified in
    /// phase ML1-universal.
    SuccessorChain,
    /// Mixed: procedural + one zoo corpus (compositional). Midway
    /// between pure procedural and full zoo.
    MixedOperators,
    /// Full arithmetic: {add=2, mul=3, sub=5, div=6} mixed.
    /// Expanded operator vocabulary — produces patterns the
    /// 3-operator families cannot.
    FullArithmetic,
    /// Peano symmetric: {succ=4, pred=7} alternating chains.
    /// Probes inverse-pair patterns (pred(succ(x))) that the
    /// succ-only corpus cannot expose.
    PeanoSymmetric,
    /// Cross-operator distributivity scaffold: mul(x, add(y, z))
    /// patterns. Primes distributivity discovery without
    /// asserting the law.
    DistributivityScaffold,
}

impl CorpusFamily {
    /// Build the corpus sequence for this family given seed + params.
    /// Returns `Vec<(name, terms)>` so the caller can iterate.
    pub fn build(self, seed: u64, budget: usize, depth: usize) -> Vec<(String, Vec<Term>)> {
        match self {
            CorpusFamily::Procedural => {
                let mut v = Vec::with_capacity(budget);
                for i in 1..=budget as u64 {
                    let s = seed.wrapping_add(i);
                    let d = 2 + (i as usize % (depth - 1).max(1));
                    let count = 16 + (i as usize % 8);
                    v.push((format!("proc-s{s}-d{d}"), procedural(s, d, count)));
                }
                v
            }
            CorpusFamily::ZooPlusProcedural => {
                let mut out = canonical_zoo();
                out.extend(CorpusFamily::Procedural.build(seed, budget, depth));
                out
            }
            CorpusFamily::AsymmetricArith => {
                let mut out = Vec::with_capacity(budget);
                for i in 1..=budget as u64 {
                    let s = seed.wrapping_add(i);
                    let mut terms = Vec::new();
                    for n in 1..=8u64 {
                        terms.push(apply(var(2), vec![nat(n), nat(0)]));
                        terms.push(apply(var(2), vec![nat(0), nat(n)]));
                        terms.push(apply(var(3), vec![nat(n), nat(1)]));
                        terms.push(apply(var(3), vec![nat(1), nat(n)]));
                    }
                    // Rotate a little per-seed so different seeds
                    // sample different pair orderings.
                    let rot = (s as usize) % terms.len();
                    terms.rotate_left(rot);
                    out.push((format!("asym-s{s}"), terms));
                }
                out
            }
            CorpusFamily::DeeplyNested => {
                let mut out = Vec::with_capacity(budget);
                for i in 1..=budget as u64 {
                    let s = seed.wrapping_add(i);
                    let mut terms = Vec::new();
                    for n in 1..=6u64 {
                        // (mul (add n 0) (add (n+1) 0))
                        terms.push(apply(
                            var(3),
                            vec![
                                apply(var(2), vec![nat(n), nat(0)]),
                                apply(var(2), vec![nat(n + 1), nat(0)]),
                            ],
                        ));
                        // (add (mul n 1) (mul (n+2) 1))
                        terms.push(apply(
                            var(2),
                            vec![
                                apply(var(3), vec![nat(n), nat(1)]),
                                apply(var(3), vec![nat(n + 2), nat(1)]),
                            ],
                        ));
                        // succ(add(n, 0))
                        terms.push(apply(
                            var(4),
                            vec![apply(var(2), vec![nat(n), nat(0)])],
                        ));
                    }
                    let rot = (s as usize) % terms.len();
                    terms.rotate_left(rot);
                    out.push((format!("nest-s{s}"), terms));
                }
                out
            }
            CorpusFamily::SuccessorChain => {
                let mut out = Vec::with_capacity(budget);
                for i in 1..=budget as u64 {
                    let s = seed.wrapping_add(i);
                    let d = (depth + (i as usize % 3)).min(8);
                    let mut terms = Vec::new();
                    for base in 0..=4u64 {
                        for chain_depth in 1..=d {
                            let mut t = nat(base);
                            for _ in 0..chain_depth {
                                t = apply(var(4), vec![t]);
                            }
                            terms.push(t);
                        }
                    }
                    let rot = (s as usize) % terms.len();
                    terms.rotate_left(rot);
                    out.push((format!("succ-s{s}-d{d}"), terms));
                }
                out
            }
            CorpusFamily::MixedOperators => {
                // Half procedural, half hand-crafted mixed-op.
                let mut out = CorpusFamily::Procedural
                    .build(seed, budget / 2, depth);
                out.push(("doubling".into(), doubling()));
                out.push(("left-identity".into(), left_identity()));
                out.push(("cross-op".into(), cross_op()));
                out
            }
            CorpusFamily::FullArithmetic => {
                // Extended vocab: {add=2, mul=3, sub=5, div=6}.
                let mut out = Vec::with_capacity(budget);
                for i in 1..=budget as u64 {
                    let s = seed.wrapping_add(i);
                    let mut terms = Vec::new();
                    for n in 1..=6u64 {
                        terms.push(apply(var(2), vec![nat(n), nat(0)]));
                        terms.push(apply(var(5), vec![nat(n), nat(0)]));
                        terms.push(apply(var(5), vec![nat(n), nat(n)]));
                        terms.push(apply(var(3), vec![nat(n), nat(1)]));
                        terms.push(apply(var(6), vec![nat(n), nat(1)]));
                        terms.push(apply(var(6), vec![nat(n), nat(n)]));
                        terms.push(apply(
                            var(5),
                            vec![apply(var(2), vec![nat(n), nat(n)]), nat(n)],
                        ));
                        terms.push(apply(
                            var(6),
                            vec![apply(var(3), vec![nat(n), nat(n)]), nat(n)],
                        ));
                    }
                    let rot = (s as usize) % terms.len();
                    terms.rotate_left(rot);
                    out.push((format!("full-arith-s{s}"), terms));
                }
                out
            }
            CorpusFamily::PeanoSymmetric => {
                // {succ=4, pred=7} alternating chains.
                let mut out = Vec::with_capacity(budget);
                for i in 1..=budget as u64 {
                    let s = seed.wrapping_add(i);
                    let d = (depth + (i as usize % 3)).min(8);
                    let mut terms = Vec::new();
                    for base in 0..=4u64 {
                        for k in 1..=d {
                            // succ(succ(...pred(pred(base))))
                            let mut t = nat(base);
                            for step in 0..k {
                                let op = if step % 2 == 0 { 4 } else { 7 };
                                t = apply(var(op), vec![t]);
                            }
                            terms.push(t);
                            // pred(succ(base))
                            let inv = apply(
                                var(7),
                                vec![apply(var(4), vec![nat(base)])],
                            );
                            terms.push(inv);
                            // succ(pred(base))
                            let inv2 = apply(
                                var(4),
                                vec![apply(var(7), vec![nat(base)])],
                            );
                            terms.push(inv2);
                        }
                    }
                    let rot = (s as usize) % terms.len();
                    terms.rotate_left(rot);
                    out.push((format!("peano-s{s}-d{d}"), terms));
                }
                out
            }
            CorpusFamily::DistributivityScaffold => {
                // mul(x, add(y, z)) + add(mul(x, y), mul(x, z)) shapes.
                let mut out = Vec::with_capacity(budget);
                for i in 1..=budget as u64 {
                    let s = seed.wrapping_add(i);
                    let mut terms = Vec::new();
                    for x in 1..=4u64 {
                        for y in 1..=4u64 {
                            for z in 1..=2u64 {
                                terms.push(apply(
                                    var(3),
                                    vec![
                                        nat(x),
                                        apply(var(2), vec![nat(y), nat(z)]),
                                    ],
                                ));
                                terms.push(apply(
                                    var(2),
                                    vec![
                                        apply(var(3), vec![nat(x), nat(y)]),
                                        apply(var(3), vec![nat(x), nat(z)]),
                                    ],
                                ));
                            }
                        }
                    }
                    let rot = (s as usize) % terms.len();
                    terms.rotate_left(rot);
                    out.push((format!("distrib-s{s}"), terms));
                }
                out
            }
        }
    }
}

/// A single experiment — hypothesis expressed as data.
#[derive(Clone)]
pub struct Experiment {
    /// Short name, e.g. "apparatus-ablation-procedural".
    pub name: &'static str,
    /// One-line hypothesis the runner is testing.
    pub hypothesis: &'static str,
    /// Label + Lisp reward form for each apparatus in the sweep.
    /// Empty form string = use the legacy Rust formula (default).
    pub apparatuses: Vec<(&'static str, &'static str)>,
    /// Corpus family for all traversals in this experiment.
    pub corpus_family: CorpusFamily,
    /// Seeds per apparatus.
    pub seeds: u64,
    /// Budget passed to the corpus factory.
    pub budget: usize,
    /// Depth passed to the corpus factory.
    pub depth: usize,
    /// Extract config for the CompressionGenerator.
    pub extract_config: ExtractConfig,
    /// Prover min_score (acceptance threshold).
    pub min_score: f64,
    /// Whether to compose MetaPatternGenerator onto the base
    /// generator. Off = pure anti-unification, no rank-1 meta.
    pub use_meta_gen: bool,
}

/// Outcome of running a single experiment.
#[derive(Clone, Debug)]
pub struct ExperimentReport {
    pub name: &'static str,
    pub hypothesis: &'static str,
    pub apparatuses: Vec<&'static str>,
    pub elapsed_secs: f64,
    pub total_structural_rules: usize,
    pub universal_count: usize,
    pub near_universal_count: usize,
    pub apparatus_specific_count: usize,
    pub canon_missing_at_half_plus: usize,
    pub per_apparatus_unique: BTreeMap<String, usize>,
    /// Top-10 universal rules as (key, apparatus_count, total_runs).
    pub top_universals: Vec<(String, usize, usize)>,
    /// Rules appearing in canonical? Count.
    pub canonical_apex_size: usize,
}

/// Fast single-traversal runner for the dense-campaign hot path.
/// Lighter than `run_one`: 1 Discover + 1 Reinforce per corpus
/// (instead of 3+1), no meta-gen by default, no registry-full
/// bookkeeping beyond axiomatized rule extraction. Returns just
/// the axiomatized rule list.
///
/// Runtime at the default settings: ~1-3ms per call. Thread-safe
/// — no shared mutable state; each call spins up its own Epoch.
pub fn run_probe_fast(
    apparatus_src: &str,
    corpus: &[(String, Vec<Term>)],
    extract_config: &ExtractConfig,
    min_score: f64,
) -> Vec<RewriteRule> {
    run_probe_fast_with_substrate(
        apparatus_src,
        corpus,
        extract_config,
        min_score,
        &[],
    )
}

/// Fast runner with an explicit substrate of pre-validated rules.
/// The substrate rules are REDUCED through the corpus BEFORE
/// anti-unification runs. This is the mechanism the edge-riding
/// loop uses to expand the territory each cycle — Rustified
/// theorems from previous cycles pre-reduce the corpus so
/// anti-unification sees the RESIDUE, i.e. the next-layer
/// unprovable frontier.
pub fn run_probe_fast_with_substrate(
    apparatus_src: &str,
    corpus: &[(String, Vec<Term>)],
    extract_config: &ExtractConfig,
    min_score: f64,
    substrate: &[RewriteRule],
) -> Vec<RewriteRule> {
    use mathscape_compress::adapter::rewrite_fixed_point;
    let base = CompressionGenerator::new(extract_config.clone(), 1);
    let prover = {
        let base_prover = StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            min_score,
        );
        if apparatus_src.is_empty() {
            base_prover
        } else {
            let form = match mathscape_reward::parse_reward(apparatus_src) {
                Ok(f) => f,
                Err(_) => return Vec::new(),
            };
            base_prover.with_reward_form(form)
        }
    };
    let mut epoch = Epoch::new(base, prover, RuleEmitter, InMemoryRegistry::new());

    // If substrate is nonempty, pre-reduce each corpus instance
    // through it. The RESIDUE is what the discovery pipeline sees.
    // This is the Rustification → frontier-expansion mechanism.
    let reduced_corpus: Vec<(String, Vec<Term>)> = if substrate.is_empty() {
        corpus.to_vec()
    } else {
        corpus
            .iter()
            .map(|(name, terms)| {
                let reduced: Vec<Term> = terms
                    .iter()
                    .map(|t| rewrite_fixed_point(t, substrate, 64))
                    .collect();
                (name.clone(), reduced)
            })
            .collect()
    };

    for (_, c) in &reduced_corpus {
        let _ = epoch.step_with_action(c, EpochAction::Discover);
        let _ = epoch.step_with_action(c, EpochAction::Reinforce);
    }
    let mut rules = Vec::new();
    for artifact in epoch.registry.all() {
        let s = epoch
            .registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        if matches!(s, ProofStatus::Axiomatized) {
            rules.push(artifact.rule.clone());
        }
    }
    rules
}

/// Single-traversal runner — one apparatus, one seed, one corpus
/// instance. Returns (total_rules, axiomatized_count, full_rules).
pub fn run_one(
    apparatus_src: &str,
    corpus: &[(String, Vec<Term>)],
    extract_config: &ExtractConfig,
    min_score: f64,
    use_meta_gen: bool,
) -> (usize, usize, Vec<RewriteRule>) {
    let base = CompressionGenerator::new(extract_config.clone(), 1);

    let prover = {
        let base_prover = StatisticalProver::new(
            mathscape_reward::reward::RewardConfig::default(),
            min_score,
        );
        if apparatus_src.is_empty() {
            base_prover
        } else {
            let form = mathscape_reward::parse_reward(apparatus_src)
                .expect("apparatus source must parse");
            base_prover.with_reward_form(form)
        }
    };

    if use_meta_gen {
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
            prover,
            RuleEmitter,
            InMemoryRegistry::new(),
        );
        for (_, c) in corpus {
            for _ in 0..3 {
                let _ = epoch.step_with_action(c, EpochAction::Discover);
            }
            let _ = epoch.step_with_action(c, EpochAction::Reinforce);
        }
        collect_axiomatized(&epoch.registry, epoch.registry.all().len())
    } else {
        let mut epoch = Epoch::new(base, prover, RuleEmitter, InMemoryRegistry::new());
        for (_, c) in corpus {
            for _ in 0..3 {
                let _ = epoch.step_with_action(c, EpochAction::Discover);
            }
            let _ = epoch.step_with_action(c, EpochAction::Reinforce);
        }
        collect_axiomatized(&epoch.registry, epoch.registry.all().len())
    }
}

fn collect_axiomatized(
    registry: &InMemoryRegistry,
    total_lib: usize,
) -> (usize, usize, Vec<RewriteRule>) {
    let mut rules = Vec::new();
    let mut axiom = 0;
    for artifact in registry.all() {
        let s = registry
            .status_of(artifact.content_hash)
            .unwrap_or_else(|| artifact.certificate.status.clone());
        if matches!(s, ProofStatus::Axiomatized) {
            axiom += 1;
            rules.push(artifact.rule.clone());
        }
    }
    (total_lib, axiom, rules)
}

pub fn format_term(t: &Term) -> String {
    use mathscape_core::value::Value;
    match t {
        Term::Var(v) => format!("?v{v}"),
        Term::Number(Value::Nat(n)) => n.to_string(),
        Term::Apply(f, args) => {
            let fs = format_term(f);
            let ass: Vec<String> = args.iter().map(format_term).collect();
            format!("({} {})", fs, ass.join(" "))
        }
        Term::Symbol(id, args) => {
            let ass: Vec<String> = args.iter().map(format_term).collect();
            if ass.is_empty() {
                format!("S_{id}")
            } else {
                format!("(S_{id} {})", ass.join(" "))
            }
        }
        Term::Point(p) => format!("P_{p:?}"),
        Term::Fn(params, body) => format!("(fn {:?} → {})", params, format_term(body)),
    }
}

pub fn rule_key(r: &RewriteRule) -> String {
    format!(
        "{}→{}",
        format_term(&anonymize_term(&r.lhs)),
        format_term(&anonymize_term(&r.rhs)),
    )
}

/// Execute a single experiment. Iterates apparatuses × seeds, runs
/// a full traversal per cell, collects structurally-keyed rules,
/// and builds the report.
pub fn run_experiment(e: &Experiment) -> ExperimentReport {
    let start = std::time::Instant::now();
    // rule_key -> apparatus_label -> count
    let mut coverage: HashMap<String, BTreeMap<&str, usize>> = HashMap::new();

    for (label, src) in &e.apparatuses {
        for seed in 0..e.seeds {
            let corpus = e.corpus_family.build(seed * 997, e.budget, e.depth);
            let (_lib, _axiom, rules) =
                run_one(src, &corpus, &e.extract_config, e.min_score, e.use_meta_gen);
            for r in rules {
                let key = rule_key(&r);
                *coverage
                    .entry(key)
                    .or_default()
                    .entry(label)
                    .or_insert(0) += 1;
            }
        }
    }

    let n_apparatuses = e.apparatuses.len();
    let mut ranked: Vec<(String, usize, usize, Vec<&'static str>, bool)> = coverage
        .iter()
        .map(|(k, apps)| {
            let ac = apps.len();
            let tr: usize = apps.values().sum();
            let mut an: Vec<&'static str> = apps.keys().copied().collect();
            an.sort();
            let canon_label = e
                .apparatuses
                .first()
                .map(|(l, _)| *l)
                .unwrap_or("canonical");
            let in_canon = apps.contains_key(canon_label);
            (k.clone(), ac, tr, an, in_canon)
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));

    let universal_count = ranked.iter().filter(|r| r.1 == n_apparatuses).count();
    let half = (n_apparatuses + 1) / 2;
    let near_universal_count = ranked.iter().filter(|r| r.1 >= half).count();
    let apparatus_specific_count = ranked.iter().filter(|r| r.1 == 1).count();
    let canon_missing_at_half_plus = ranked
        .iter()
        .filter(|r| r.1 >= half && !r.4)
        .count();

    // Per-apparatus signature sizes (rules unique to each apparatus).
    let mut per_apparatus_unique: BTreeMap<String, usize> = BTreeMap::new();
    for r in &ranked {
        if r.1 == 1 {
            *per_apparatus_unique.entry(r.3[0].to_string()).or_insert(0) += 1;
        }
    }

    // Canonical apex size: union across seeds for the first
    // apparatus (by convention, canonical is slot 0).
    let canon_label = e
        .apparatuses
        .first()
        .map(|(l, _)| *l)
        .unwrap_or("canonical");
    let canonical_apex_size = ranked.iter().filter(|r| r.3.contains(&canon_label)).count();

    let top_universals: Vec<(String, usize, usize)> = ranked
        .iter()
        .take(10)
        .map(|r| (r.0.clone(), r.1, r.2))
        .collect();

    ExperimentReport {
        name: e.name,
        hypothesis: e.hypothesis,
        apparatuses: e.apparatuses.iter().map(|(l, _)| *l).collect(),
        elapsed_secs: start.elapsed().as_secs_f64(),
        total_structural_rules: ranked.len(),
        universal_count,
        near_universal_count,
        apparatus_specific_count,
        canon_missing_at_half_plus,
        per_apparatus_unique,
        top_universals,
        canonical_apex_size,
    }
}

/// Pretty-print a single report to stdout. Called per-experiment in
/// the catalog runner. Compact by design — rich detail is available
/// in the report struct for post-processing.
pub fn print_report(r: &ExperimentReport) {
    println!();
    println!("── {} ─────────────────────────────────", r.name);
    println!("  hypothesis  : {}", r.hypothesis);
    println!(
        "  apparatuses : {} | seeds: ?, took {:.2}s",
        r.apparatuses.len(),
        r.elapsed_secs
    );
    println!(
        "  rules: {}  universal: {}  ≥half: {}  canon-missing(≥half): {}  apparatus-specific: {}",
        r.total_structural_rules,
        r.universal_count,
        r.near_universal_count,
        r.canon_missing_at_half_plus,
        r.apparatus_specific_count,
    );
    if !r.top_universals.is_empty() {
        let top = &r.top_universals[0];
        println!("  top universal: [{}/{}] {}", top.1, r.apparatuses.len(), top.0);
    }
}

// ── Canonical apparatus sets ─────────────────────────────────────
//
// Reusable apparatus sets the catalog can reference. Keeping these
// here centralizes the Lisp source so the catalog stays readable.

/// 6 apparatuses: canonical + 5 axis-ablations.
pub fn apparatus_set_ablation() -> Vec<(&'static str, &'static str)> {
    vec![
        ("canonical",    "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("cr-only",      "(* alpha cr)"),
        ("novelty-only", "(* beta novelty)"),
        ("meta-only",    "(* gamma meta-compression)"),
        ("sub-only",     "(* delta lhs-subsumption)"),
        ("cr+nov",       "(+ (* alpha cr) (* beta novelty))"),
    ]
}

/// 6 apparatuses: canonical + 5 shape mutations.
pub fn apparatus_set_shapes() -> Vec<(&'static str, &'static str)> {
    vec![
        ("canonical",     "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("max-pair",      "(max (+ (* alpha cr) (* beta novelty)) (+ (* gamma meta-compression) (* delta lhs-subsumption)))"),
        ("cr*nov",        "(+ (* cr novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("harmonic",      "(+ (/ (* cr novelty) (max (+ cr novelty) 0.001)) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("clamped",       "(clamp (+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption)) -1 2)"),
        ("threshold-cr",  "(+ (* alpha cr) (if (max (- cr 0.05) 0) (* beta novelty) 0) (* gamma meta-compression) (* delta lhs-subsumption))"),
    ]
}

/// 6 apparatuses: canonical + 5 weight perturbations.
pub fn apparatus_set_weights() -> Vec<(&'static str, &'static str)> {
    vec![
        ("canonical",    "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("alpha-x2",     "(+ (* (* 2 alpha) cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("beta-x2",      "(+ (* alpha cr) (* (* 2 beta) novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("delta-x3",     "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* (* 3 delta) lhs-subsumption))"),
        ("uniform",      "(+ (* 0.25 cr) (* 0.25 novelty) (* 0.25 meta-compression) (* 0.25 lhs-subsumption))"),
        ("meta-heavy",   "(+ (* alpha cr) (* beta novelty) (* (* 5 gamma) meta-compression) (* delta lhs-subsumption))"),
    ]
}

/// 8 apparatuses: canonical + 7 spanning all three families.
/// Good for mid-size experiments.
pub fn apparatus_set_spanning_8() -> Vec<(&'static str, &'static str)> {
    vec![
        ("canonical",    "(+ (* alpha cr) (* beta novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("cr-only",      "(* alpha cr)"),
        ("novelty-only", "(* beta novelty)"),
        ("cr+nov",       "(+ (* alpha cr) (* beta novelty))"),
        ("max-pair",     "(max (+ (* alpha cr) (* beta novelty)) (+ (* gamma meta-compression) (* delta lhs-subsumption)))"),
        ("cr*nov",       "(+ (* cr novelty) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("harmonic",     "(+ (/ (* cr novelty) (max (+ cr novelty) 0.001)) (* gamma meta-compression) (* delta lhs-subsumption))"),
        ("uniform",      "(+ (* 0.25 cr) (* 0.25 novelty) (* 0.25 meta-compression) (* 0.25 lhs-subsumption))"),
    ]
}

// ── Catalog ──────────────────────────────────────────────────────
//
// The catalog is a `Vec<Experiment>` organized by hypothesis family.
// Each entry tests a specific claim about the discovery space, and
// the runner aggregates across all entries to expose which claims
// hold universally, which hold in some corpus families but not
// others, and which apparatus axes are productive vs. noise.
//
// Families:
//   F01 — apparatus ablation on procedural corpus (baseline)
//   F02 — apparatus ablation on zoo-plus-procedural
//   F03 — apparatus ablation on asymmetric arithmetic
//   F04 — apparatus ablation on deeply-nested structures
//   F05 — apparatus ablation on successor chains
//   F06 — spanning-8 apparatus vs corpus family (5 experiments)
//   F07 — extract-config sweep (min_shared_size × max_new_rules)
//   F08 — prover threshold sweep (min_score)
//   F09 — meta-generator on vs off
//   F10 — seed-count sensitivity (universality stability)
//   F11 — shape mutations vs corpus family
//   F12 — weight perturbations vs corpus family
//
// Approximate count: 40-80 experiments. Each runs in ~1-3 seconds
// in release mode.

pub fn catalog() -> Vec<Experiment> {
    let mut out = Vec::new();
    let base_ec = ExtractConfig::default();

    // Helper to build one experiment with sensible defaults.
    let mk = |name: &'static str,
              hyp: &'static str,
              apparatuses: Vec<(&'static str, &'static str)>,
              corpus_family: CorpusFamily,
              seeds: u64,
              budget: usize,
              depth: usize,
              ec: ExtractConfig,
              min_score: f64,
              use_meta_gen: bool|
     -> Experiment {
        Experiment {
            name,
            hypothesis: hyp,
            apparatuses,
            corpus_family,
            seeds,
            budget,
            depth,
            extract_config: ec,
            min_score,
            use_meta_gen,
        }
    };

    // F01 — apparatus ablation × procedural (6 experiments)
    // One experiment per ablated axis, plus canonical.
    out.push(mk(
        "F01-canonical-procedural",
        "baseline on procedural corpus",
        apparatus_set_ablation(),
        CorpusFamily::Procedural,
        8, 16, 4, base_ec.clone(), 0.0, true,
    ));

    // F02 — ablation × zoo-plus-procedural
    out.push(mk(
        "F02-ablation-zoo-plus-procedural",
        "does adding the canonical zoo change ablation results?",
        apparatus_set_ablation(),
        CorpusFamily::ZooPlusProcedural,
        8, 12, 4, base_ec.clone(), 0.0, true,
    ));

    // F03 — ablation × asymmetric arithmetic
    out.push(mk(
        "F03-ablation-asymmetric",
        "asymmetric corpus exposes left/right-identity pairs",
        apparatus_set_ablation(),
        CorpusFamily::AsymmetricArith,
        8, 12, 4, base_ec.clone(), 0.0, true,
    ));

    // F04 — ablation × deeply nested
    out.push(mk(
        "F04-ablation-nested",
        "nested structure probes cross-level compression",
        apparatus_set_ablation(),
        CorpusFamily::DeeplyNested,
        8, 12, 4, base_ec.clone(), 0.0, true,
    ));

    // F05 — ablation × successor chain
    out.push(mk(
        "F05-ablation-succ",
        "successor-only corpus probes the universal rule",
        apparatus_set_ablation(),
        CorpusFamily::SuccessorChain,
        8, 12, 4, base_ec.clone(), 0.0, true,
    ));

    // F06 — spanning-8 × corpus family (5 experiments, one per family)
    for (family, family_name) in [
        (CorpusFamily::Procedural, "procedural"),
        (CorpusFamily::ZooPlusProcedural, "zoo+proc"),
        (CorpusFamily::AsymmetricArith, "asymmetric"),
        (CorpusFamily::DeeplyNested, "nested"),
        (CorpusFamily::SuccessorChain, "successor"),
    ] {
        let name = Box::leak(format!("F06-span8-{family_name}").into_boxed_str());
        let hyp = Box::leak(
            format!("8-apparatus span on {family_name} corpus").into_boxed_str(),
        );
        out.push(mk(
            name,
            hyp,
            apparatus_set_spanning_8(),
            family,
            8, 12, 4, base_ec.clone(), 0.0, true,
        ));
    }

    // F07 — extract config sweep (4 experiments)
    for (min_shared, max_new, label) in [
        (2, 5, "loose-low"),
        (3, 5, "default"),
        (4, 5, "strict"),
        (3, 12, "wider-emission"),
    ] {
        let name = Box::leak(format!("F07-ec-{label}").into_boxed_str());
        let hyp = Box::leak(
            format!("extract config: min_shared_size={min_shared}, max_new_rules={max_new}").into_boxed_str(),
        );
        let ec = ExtractConfig {
            min_shared_size: min_shared,
            min_matches: 2,
            max_new_rules: max_new,
        };
        out.push(mk(
            name, hyp,
            apparatus_set_spanning_8(),
            CorpusFamily::Procedural,
            8, 12, 4, ec, 0.0, true,
        ));
    }

    // F08 — prover threshold sweep (4 experiments)
    for threshold in [0.0, 0.01, 0.1, 0.5] {
        let name = Box::leak(format!("F08-threshold-{threshold:.2}").into_boxed_str());
        let hyp = Box::leak(
            format!("prover min_score={threshold}").into_boxed_str(),
        );
        out.push(mk(
            name, hyp,
            apparatus_set_spanning_8(),
            CorpusFamily::Procedural,
            6, 12, 4, base_ec.clone(), threshold, true,
        ));
    }

    // F09 — meta-gen on vs off (2 experiments × 2 corpora = 4)
    for (corpus, cname) in [
        (CorpusFamily::Procedural, "procedural"),
        (CorpusFamily::ZooPlusProcedural, "zoo"),
    ] {
        for (meta, mname) in [(false, "nometa"), (true, "meta")] {
            let name = Box::leak(
                format!("F09-{mname}-{cname}").into_boxed_str(),
            );
            let hyp = Box::leak(
                format!("meta-gen {} on {cname}", if meta { "enabled" } else { "disabled" }).into_boxed_str(),
            );
            out.push(mk(
                name, hyp,
                apparatus_set_ablation(),
                corpus,
                8, 12, 4, base_ec.clone(), 0.0, meta,
            ));
        }
    }

    // F10 — seed-count sensitivity (4 experiments)
    for seeds in [4u64, 8, 16, 24] {
        let name = Box::leak(format!("F10-seeds-{seeds}").into_boxed_str());
        let hyp = Box::leak(
            format!("universality stability at {seeds} seeds").into_boxed_str(),
        );
        out.push(mk(
            name, hyp,
            apparatus_set_spanning_8(),
            CorpusFamily::Procedural,
            seeds, 12, 4, base_ec.clone(), 0.0, true,
        ));
    }

    // F11 — shape mutations × 3 corpora
    for (corpus, cname) in [
        (CorpusFamily::Procedural, "procedural"),
        (CorpusFamily::AsymmetricArith, "asymmetric"),
        (CorpusFamily::DeeplyNested, "nested"),
    ] {
        let name = Box::leak(format!("F11-shapes-{cname}").into_boxed_str());
        let hyp = Box::leak(
            format!("shape mutations on {cname}").into_boxed_str(),
        );
        out.push(mk(
            name, hyp,
            apparatus_set_shapes(),
            corpus,
            8, 12, 4, base_ec.clone(), 0.0, true,
        ));
    }

    // F12 — weight perturbations × 3 corpora
    for (corpus, cname) in [
        (CorpusFamily::Procedural, "procedural"),
        (CorpusFamily::AsymmetricArith, "asymmetric"),
        (CorpusFamily::SuccessorChain, "successor"),
    ] {
        let name = Box::leak(format!("F12-weights-{cname}").into_boxed_str());
        let hyp = Box::leak(
            format!("weight perturbations on {cname}").into_boxed_str(),
        );
        out.push(mk(
            name, hyp,
            apparatus_set_weights(),
            corpus,
            8, 12, 4, base_ec.clone(), 0.0, true,
        ));
    }

    // F13 — budget sensitivity (3 experiments)
    for budget in [8usize, 16, 24] {
        let name = Box::leak(format!("F13-budget-{budget}").into_boxed_str());
        let hyp = Box::leak(
            format!("BUDGET={budget} effect on discovery").into_boxed_str(),
        );
        out.push(mk(
            name, hyp,
            apparatus_set_spanning_8(),
            CorpusFamily::Procedural,
            6, budget, 4, base_ec.clone(), 0.0, true,
        ));
    }

    // F14 — depth sensitivity (3 experiments)
    for depth in [2usize, 4, 6] {
        let name = Box::leak(format!("F14-depth-{depth}").into_boxed_str());
        let hyp = Box::leak(
            format!("DEPTH={depth} effect on discovery").into_boxed_str(),
        );
        out.push(mk(
            name, hyp,
            apparatus_set_spanning_8(),
            CorpusFamily::Procedural,
            6, 12, depth, base_ec.clone(), 0.0, true,
        ));
    }

    out
}

/// Cross-experiment aggregation. Given the list of per-experiment
/// reports, surface the "core truths" — invariants that hold across
/// many experiments, not just one.
pub fn print_catalog_summary(reports: &[ExperimentReport]) {
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║ CATALOG CROSS-EXPERIMENT AGGREGATION                                 ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let total_experiments = reports.len();
    let total_elapsed: f64 = reports.iter().map(|r| r.elapsed_secs).sum();

    // Aggregate: for each unique top-universal rule across all
    // experiments, count in how many experiments it appears with
    // apparatus coverage ≥ 3 AND ≥ half of that experiment's apparatuses.
    let mut cross_experiment_coverage: HashMap<String, usize> = HashMap::new();
    let mut rules_with_provenance: HashMap<String, Vec<String>> = HashMap::new();
    for r in reports {
        for (key, app_count, _runs) in &r.top_universals {
            let half = (r.apparatuses.len() + 1) / 2;
            if *app_count >= half.max(3) {
                *cross_experiment_coverage.entry(key.clone()).or_insert(0) += 1;
                rules_with_provenance
                    .entry(key.clone())
                    .or_default()
                    .push(r.name.to_string());
            }
        }
    }

    let mut xe_ranked: Vec<(&String, &usize)> =
        cross_experiment_coverage.iter().collect();
    xe_ranked.sort_by(|a, b| b.1.cmp(a.1));

    println!();
    println!("ran {total_experiments} experiments in {total_elapsed:.1}s total");
    println!(
        "  (mean {:.2}s per experiment)",
        total_elapsed / total_experiments as f64
    );
    println!();

    println!("top-20 CROSS-EXPERIMENT universal rules (rules appearing as");
    println!("near-universals in multiple experiments — strongest promotion candidates):");
    println!("  {:>5}  {}", "xeps", "rule");
    println!("  {:─>5}  {}", "", "─────────");
    for (key, count) in xe_ranked.iter().take(20) {
        println!("  {:>5}  {}", count, key);
    }
    println!();

    // Summary stats.
    let mean_universal =
        reports.iter().map(|r| r.universal_count).sum::<usize>() as f64 / total_experiments as f64;
    let mean_near_universal =
        reports.iter().map(|r| r.near_universal_count).sum::<usize>() as f64 / total_experiments as f64;
    let mean_structural =
        reports.iter().map(|r| r.total_structural_rules).sum::<usize>() as f64 / total_experiments as f64;
    println!("per-experiment averages:");
    println!("  avg universal rules  : {mean_universal:.2}");
    println!("  avg near-universal   : {mean_near_universal:.2}");
    println!("  avg structural rules : {mean_structural:.2}");
    println!();

    // Experiments with highest discovery rate.
    let mut by_structural: Vec<&ExperimentReport> = reports.iter().collect();
    by_structural.sort_by(|a, b| b.total_structural_rules.cmp(&a.total_structural_rules));
    println!("top-10 experiments by structural rule count:");
    for r in by_structural.iter().take(10) {
        println!(
            "  {:<32}  {:>5} rules  {:>2}/∞ universal  {:.1}s",
            r.name, r.total_structural_rules, r.universal_count, r.elapsed_secs
        );
    }
}
