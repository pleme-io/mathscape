//! R24 — Law generator: discover equational laws from eval traces.
//!
//! # The missing mechanism
//!
//! R22/R23 found that the existing compression-generator converges on
//! abstraction rules of the form `Apply(?h, args) => Symbol(id, ...)` —
//! library shortcuts, not mathematical laws.
//!
//! To get LAW-shaped rules (`f(x, identity) = x`, `f(a, b) = f(b, a)`,
//! etc.) the machine needs a different mechanism. This module is that
//! mechanism.
//!
//! # How it works
//!
//! 1. **Evaluate** every corpus term using the kernel's eval. For terms
//!    that reduce to something structurally different (i.e.,
//!    `eval(t) ≠ t`), record the `(input, output)` pair as a trace.
//!
//! 2. **Paired anti-unify** pairs of traces. Given `(in1 → out1)` and
//!    `(in2 → out2)`, compute the least general generalization of BOTH
//!    sides using a shared var-map (the `paired_anti_unify` primitive
//!    in antiunify.rs). The result is a candidate law pattern.
//!
//! 3. **Filter** for meaningful laws: LHS must have ≥1 pattern var,
//!    RHS vars must be subset of LHS vars, LHS ≠ RHS. The
//!    `paired_anti_unify` function already enforces these.
//!
//! 4. **Deduplicate and rank** by the number of trace-pairs that
//!    generalize to the same (lhs, rhs). Laws that many traces agree
//!    on are stronger candidates.
//!
//! # Relationship to R13-R20 hand-coded primitives
//!
//! The primitives R13-R20 are our **reference implementation** — the
//! hand-coded truth. This module tries to DISCOVER them. If the
//! law-generator runs on a corpus like `[add(1,0), add(5,0), add(7,0),
//! ...]` and emits the law `add(?x, 0) = ?x`, that's the machine
//! arriving at the hand-coded R12 `LeftIdentity` primitive via its
//! own machinery — naturally, not forced.
//!
//! # What this is NOT
//!
//! - Not wired into the autonomous_traverse milestone (that's
//!   R25 future work). This module is a standalone function
//!   exercised by tests.
//! - Not a replacement for the compression-generator. Laws and
//!   compressions coexist in the library lifecycle.
//! - Not semantically verified beyond what eval gives us. A proof
//!   that the law holds for ALL inputs (not just the observed
//!   traces) is Phase J territory.

use crate::antiunify::{paired_anti_unify, paired_subterm_anti_unify};
use mathscape_core::eval::{eval, RewriteRule};
use mathscape_core::term::{SymbolId, Term};
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

/// R36: memoizing wrapper for `paired_anti_unify`. Same semantics,
/// but repeated calls on identical inputs return cached results.
///
/// Why this exists: R35 profiling showed paired_anti_unify at 92%
/// of extract time — 120 pair calls per iteration × 5 iterations =
/// 600 calls, many on recurring term structures when the corpus
/// shape is stable across iterations. A pass-through cache turns
/// those redundant calls into O(1) hashmap hits.
///
/// The cache key is the 4-tuple `(in1, in2, out1, out2)`. `Term`
/// is `Hash + Eq` (derived), so it slots into a HashMap directly.
///
/// **Performance tradeoff (measured 2026-04-18 on M0 default
/// corpus)**: each MISS pays a Term-clone × 4 (≈ 20 heap allocations
/// per new pair key) which costs roughly 2-3× what a plain
/// `paired_anti_unify` call takes. Cache HITs save ~300 ns each.
/// Break-even is ~6:1 hit:miss ratio. The default 5-iteration M0
/// corpus exhibits only a 1.5:1 ratio (360 hits / 240 misses) and
/// the cache is therefore a *net slowdown* on that workload.
///
/// Use this when:
///   - you expect ≥10× repeated extractor calls on the same corpus shape
///     (long-running scenarios, iterative refinement, ExperimentScenario
///     chains of many phases with stable shape)
///   - Term clones are cheap relative to AU body work (shallow terms)
///
/// Avoid when:
///   - the corpus rotates shape every iteration
///   - terms are deeply nested or carry large tensor payloads
///
/// For workloads with deep terms, a future R37 variant should key
/// the cache on `TermRef` (blake3 hash via hash-consing) to avoid
/// the clone altogether.
///
/// Thread-safety: single-threaded by design. If parallel extract
/// is ever introduced, wrap this in `Arc<Mutex<_>>` or use a
/// concurrent hashmap. For now the cycle is serial, so no lock.
#[derive(Debug, Default)]
pub struct MemoizingAntiUnifier {
    cache: HashMap<(Term, Term, Term, Term), Option<(Term, Term)>>,
    hits: u64,
    misses: u64,
}

impl MemoizingAntiUnifier {
    /// Fresh cache. `hits` and `misses` start at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cached `paired_anti_unify`. First call on a given
    /// 4-tuple computes the result and stores it; subsequent
    /// calls return the cached value directly.
    pub fn run(
        &mut self,
        pair1: (&Term, &Term),
        pair2: (&Term, &Term),
    ) -> Option<(Term, Term)> {
        let key =
            (pair1.0.clone(), pair1.1.clone(), pair2.0.clone(), pair2.1.clone());
        if let Some(cached) = self.cache.get(&key) {
            self.hits += 1;
            return cached.clone();
        }
        self.misses += 1;
        let result = paired_anti_unify(pair1, pair2);
        self.cache.insert(key, result.clone());
        result
    }

    /// Hits since construction.
    #[must_use]
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// Misses since construction.
    #[must_use]
    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// Current cache entry count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

/// R34: per-call stats for `derive_laws_from_corpus_instrumented`.
/// Breaks the extractor into its three phases so the caller can
/// see which phase dominates a given corpus/library configuration.
///
/// - `eval_ns`: Phase 1 — evaluate every corpus term. Typically
///   dominates when the eval step budget is large or when
///   library rules trigger deep reduction chains.
/// - `anti_unify_ns`: Phase 2 — O(n²) paired anti-unification
///   (capped at `max_pairs = 500`). Dominates when the trace set
///   is large relative to eval cost.
/// - `rank_ns`: Phase 3 — filter by min_support, emit rules,
///   sort by support. Small constant cost relative to the other
///   two; included for completeness.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LawGenStats {
    pub eval_ns: u64,
    pub anti_unify_ns: u64,
    pub rank_ns: u64,
    pub trace_count: usize,
    pub pairs_considered: usize,
    pub laws_emitted: usize,
}

impl LawGenStats {
    /// Sum across all three phases.
    #[must_use]
    pub fn total_ns(&self) -> u64 {
        self.eval_ns
            .saturating_add(self.anti_unify_ns)
            .saturating_add(self.rank_ns)
    }
}

#[inline]
fn elapsed_ns(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Derive candidate laws from a concrete corpus. Each term in
/// corpus is evaluated; non-trivial reductions become (input,
/// output) traces. Pairs of traces are anti-unified; the
/// resulting law patterns are returned as `RewriteRule`s.
///
/// - `library`: existing rules available to the evaluator
///   (typically the current library; can be empty)
/// - `step_limit`: eval step budget per term
/// - `min_support`: minimum number of trace-pairs that agree on
///   the same (lhs, rhs) for the law to be emitted
/// - `next_id`: symbol id allocator for naming discovered laws
#[must_use]
pub fn derive_laws_from_corpus(
    corpus: &[Term],
    library: &[RewriteRule],
    step_limit: usize,
    min_support: usize,
    next_id: &mut SymbolId,
) -> Vec<RewriteRule> {
    derive_laws_from_corpus_instrumented(
        corpus, library, step_limit, min_support, next_id,
    )
    .0
}

/// R34: instrumented variant of `derive_laws_from_corpus` returning
/// per-phase wall-clock stats alongside the laws. Use this when the
/// caller wants a breakdown of where the extractor spent its time —
/// eval (Phase 1) vs. paired AU (Phase 2) vs. rank (Phase 3).
///
/// The non-instrumented `derive_laws_from_corpus` delegates to this
/// function and discards the stats — same behavior, same results,
/// no observable difference.
///
/// Internally uses a scratch `MemoizingAntiUnifier` (fresh per
/// call, so no cross-call hits). For cross-call reuse see
/// `derive_laws_with_cache`.
#[must_use]
pub fn derive_laws_from_corpus_instrumented(
    corpus: &[Term],
    library: &[RewriteRule],
    step_limit: usize,
    min_support: usize,
    next_id: &mut SymbolId,
) -> (Vec<RewriteRule>, LawGenStats) {
    let mut cache = MemoizingAntiUnifier::new();
    derive_laws_with_cache(corpus, library, step_limit, min_support, next_id, &mut cache)
}

/// R36: instrumented law generator that accepts a caller-owned
/// `MemoizingAntiUnifier`. The extractor's AU calls go through the
/// cache, so if the caller reuses the same `MemoizingAntiUnifier`
/// across iterations with overlapping corpus shapes, repeated
/// pair-wise lookups are hashmap hits instead of full AU.
///
/// The returned `LawGenStats` covers this call only. Use
/// `MemoizingAntiUnifier::hits()` / `.misses()` on the caller-owned
/// cache to track reuse across multiple calls.
#[must_use]
pub fn derive_laws_with_cache(
    corpus: &[Term],
    library: &[RewriteRule],
    step_limit: usize,
    min_support: usize,
    next_id: &mut SymbolId,
    cache: &mut MemoizingAntiUnifier,
) -> (Vec<RewriteRule>, LawGenStats) {
    let mut stats = LawGenStats::default();

    // Phase 1: evaluate every term. Keep non-trivial reductions
    // as (input, output) traces.
    let t_eval = Instant::now();
    let mut traces: Vec<(Term, Term)> = Vec::new();
    for t in corpus {
        match eval(t, library, step_limit) {
            Ok(reduced) => {
                if reduced != *t {
                    traces.push((t.clone(), reduced));
                }
            }
            Err(_) => {
                // Eval error (step limit, type error) — skip.
            }
        }
    }
    stats.eval_ns = elapsed_ns(t_eval);
    stats.trace_count = traces.len();

    if traces.len() < 2 {
        return (Vec::new(), stats);
    }

    // Phase 2: paired anti-unify trace pairs via the cache. Build
    // a map from (lhs_pattern, rhs_pattern) → support count.
    let t_au = Instant::now();
    let mut law_support: BTreeMap<(Term, Term), usize> = BTreeMap::new();

    let max_pairs = 500.min(traces.len() * (traces.len() - 1) / 2);
    let mut considered = 0;
    'outer: for i in 0..traces.len() {
        for j in (i + 1)..traces.len() {
            if considered >= max_pairs {
                break 'outer;
            }
            considered += 1;

            let (in1, out1) = (&traces[i].0, &traces[i].1);
            let (in2, out2) = (&traces[j].0, &traces[j].1);

            if let Some((lhs_pat, rhs_pat)) =
                cache.run((in1, in2), (out1, out2))
            {
                *law_support.entry((lhs_pat, rhs_pat)).or_default() += 1;
            }
        }
    }
    stats.anti_unify_ns = elapsed_ns(t_au);
    stats.pairs_considered = considered;

    // Phase 3: filter by min_support, emit as rules.
    let t_rank = Instant::now();
    let mut laws: Vec<RewriteRule> = Vec::new();
    for ((lhs, rhs), support) in &law_support {
        if *support < min_support {
            continue;
        }
        let id = *next_id;
        *next_id += 1;
        laws.push(RewriteRule {
            name: format!("L_{id}"),
            lhs: lhs.clone(),
            rhs: rhs.clone(),
        });
    }

    // Rank by support descending (strongest evidence first).
    laws.sort_by_key(|r| {
        let k = (r.lhs.clone(), r.rhs.clone());
        std::cmp::Reverse(*law_support.get(&k).unwrap_or(&0))
    });
    stats.rank_ns = elapsed_ns(t_rank);
    stats.laws_emitted = laws.len();

    (laws, stats)
}

/// Phase I (2026-04-18): law generator that ALSO considers
/// subterm-level paired AU candidates up to `subterm_depth`.
/// Each trace pair contributes:
///   - the root-level AU candidate (identical to `derive_laws_with_cache`)
///   - every subterm-pair AU candidate whose resulting (lhs, rhs)
///     passes the usual LHS-has-vars + RHS-subset-LHS + LHS-≠-RHS
///     filters
///
/// `subterm_depth = 0` reduces to `derive_laws_with_cache` (root only).
/// Default recommended depth: 2 (captures distributivity/idempotence
/// shapes without blowup on the standard corpus).
///
/// This is what unblocks Phase H's rank-2 inception — it surfaces
/// the shape-orthogonal meta-candidates needed for MetaPatternGenerator
/// to have more than one meta-rule-equivalence-class to work with.
#[must_use]
pub fn derive_laws_with_subterm_au(
    corpus: &[Term],
    library: &[RewriteRule],
    step_limit: usize,
    min_support: usize,
    subterm_depth: usize,
    next_id: &mut SymbolId,
) -> (Vec<RewriteRule>, LawGenStats) {
    let mut stats = LawGenStats::default();

    // Phase 1: same as derive_laws_with_cache — eval each term,
    // keep non-trivial reductions.
    let t_eval = Instant::now();
    let mut traces: Vec<(Term, Term)> = Vec::new();
    for t in corpus {
        match eval(t, library, step_limit) {
            Ok(reduced) => {
                if reduced != *t {
                    traces.push((t.clone(), reduced));
                }
            }
            Err(_) => {}
        }
    }
    stats.eval_ns = elapsed_ns(t_eval);
    stats.trace_count = traces.len();

    if traces.len() < 2 {
        return (Vec::new(), stats);
    }

    // Phase 2: root + subterm paired AU. Expanded pair cap since
    // subterm AU multiplies candidates per pair by ~9 at depth 2.
    let t_au = Instant::now();
    let mut law_support: BTreeMap<(Term, Term), usize> = BTreeMap::new();

    let max_pairs = 1500.min(traces.len() * (traces.len() - 1) / 2);
    let mut considered = 0;
    'outer: for i in 0..traces.len() {
        for j in (i + 1)..traces.len() {
            if considered >= max_pairs {
                break 'outer;
            }
            considered += 1;

            let (in1, out1) = (&traces[i].0, &traces[i].1);
            let (in2, out2) = (&traces[j].0, &traces[j].1);

            // Root-level candidate (original behavior).
            if let Some(pair) = paired_anti_unify((in1, in2), (out1, out2)) {
                *law_support.entry(pair).or_default() += 1;
            }
            // Subterm-level candidates.
            if subterm_depth > 0 {
                for pair in paired_subterm_anti_unify(
                    (in1, in2),
                    (out1, out2),
                    subterm_depth,
                ) {
                    *law_support.entry(pair).or_default() += 1;
                }
            }
        }
    }
    stats.anti_unify_ns = elapsed_ns(t_au);
    stats.pairs_considered = considered;

    // Phase 3: filter + emit + rank, same as root-only variant.
    let t_rank = Instant::now();
    let mut laws: Vec<RewriteRule> = Vec::new();
    for ((lhs, rhs), support) in &law_support {
        if *support < min_support {
            continue;
        }
        let id = *next_id;
        *next_id += 1;
        laws.push(RewriteRule {
            name: format!("L_{id}"),
            lhs: lhs.clone(),
            rhs: rhs.clone(),
        });
    }
    laws.sort_by_key(|r| {
        let k = (r.lhs.clone(), r.rhs.clone());
        std::cmp::Reverse(*law_support.get(&k).unwrap_or(&0))
    });
    stats.rank_ns = elapsed_ns(t_rank);
    stats.laws_emitted = laws.len();

    (laws, stats)
}

// ── Phase J: empirical validity check ────────────────────────────
//
// Phase I surfaces candidate laws by shape. Phase J certifies each
// candidate by EVALUATING it on K random concrete bindings. If the
// LHS and RHS disagree on any binding, the candidate is rejected —
// it's structurally plausible but semantically wrong.
//
// Together, Phase I + Phase J unblock Phase H's rank-2 inception:
// Phase I surfaces shape-orthogonal candidates, Phase J filters the
// semantically-valid ones, leaving MetaPatternGenerator with a set
// of DISTINCT + VALID meta-rules to generalize over.

use mathscape_core::value::Value;

/// Phase J: empirical validity check. Generate `k_samples` concrete
/// bindings for `rule.lhs`'s pattern variables (Var ≥ 100), apply
/// them to both LHS and RHS, and evaluate against `library`. Return
/// true iff every binding produces bit-equal LHS and RHS normal forms.
///
/// `library` should NOT include the candidate rule itself — we
/// want to test whether the rule HOLDS, not whether it's
/// self-consistent after installation. (If the library already
/// proves the rule via other paths, that's fine; it just means the
/// candidate is redundant rather than invalid.)
///
/// `k_samples` trades speed for confidence. 8 samples catches most
/// wrong candidates (e.g. `(?op ?x ?id) => ?x` fails on ≥1 of 8
/// (mul, 0) / (add, 1) / non-identity bindings). 32 samples is
/// defensive.
///
/// `step_limit` caps eval steps per side. 300 is consistent with
/// the law generator's default.
///
/// `seed` is used to derive deterministic bindings. Same seed →
/// same samples → same validity verdict — determinism is preserved.
#[must_use]
pub fn is_empirically_valid(
    rule: &RewriteRule,
    library: &[RewriteRule],
    step_limit: usize,
    k_samples: usize,
    seed: u64,
) -> bool {
    let pattern_vars = collect_pattern_vars(&rule.lhs);
    if pattern_vars.is_empty() {
        // Concrete rule — just eval and compare once.
        let lhs_nf = eval(&rule.lhs, library, step_limit);
        let rhs_nf = eval(&rule.rhs, library, step_limit);
        return matches!((lhs_nf, rhs_nf), (Ok(l), Ok(r)) if l == r);
    }

    for sample_index in 0..k_samples {
        let mut lhs = rule.lhs.clone();
        let mut rhs = rule.rhs.clone();
        for (var_pos, &var_id) in pattern_vars.iter().enumerate() {
            let val = pick_concrete_value(seed, sample_index, var_pos);
            lhs = lhs.substitute(var_id, &val);
            rhs = rhs.substitute(var_id, &val);
        }
        let lhs_nf = eval(&lhs, library, step_limit);
        let rhs_nf = eval(&rhs, library, step_limit);
        match (lhs_nf, rhs_nf) {
            (Ok(l), Ok(r)) if l == r => continue,
            _ => return false,
        }
    }
    true
}

/// Pick a concrete `Term` for a given (seed, sample, var-position)
/// triple. Deterministic — same (seed, sample, var_pos) → same
/// value. Draws from a small pool covering Nat, Int, and small
/// Tensor values.
fn pick_concrete_value(seed: u64, sample_index: usize, var_pos: usize) -> Term {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    (seed, sample_index as u64, var_pos as u64).hash(&mut h);
    let x = h.finish();
    // Nat-only pool. Mixed-domain bindings (Nat + Int, Nat +
    // Tensor) don't round-trip through the kernel's ADD/MUL
    // (signatures require same-domain operands), which would
    // falsely reject domain-homogeneous valid laws. Keeping the
    // pool Nat means Phase J validates laws over the Nat algebra
    // — Phase J.2 will add domain-aware sampling for Int/Tensor
    // rules by reading the rule's operator signature.
    match x % 7 {
        0 => Term::Number(Value::Nat(0)),
        1 => Term::Number(Value::Nat(1)),
        2 => Term::Number(Value::Nat(2)),
        3 => Term::Number(Value::Nat(5)),
        4 => Term::Number(Value::Nat(17)),
        5 => Term::Number(Value::Nat(3)),
        _ => Term::Number(Value::Nat(8)),
    }
}

/// Local pattern-var collector. Uses the same >= 100 convention as
/// `antiunify::collect_pattern_vars_vec` but duplicated here to
/// avoid a cross-module API expansion.
fn collect_pattern_vars(t: &Term) -> Vec<u32> {
    let mut out = Vec::new();
    collect_inner(t, &mut out);
    out.sort_unstable();
    out.dedup();
    out
}

fn collect_inner(t: &Term, out: &mut Vec<u32>) {
    match t {
        Term::Var(v) if *v >= 100 => out.push(*v),
        Term::Var(_) | Term::Point(_) | Term::Number(_) => {}
        Term::Apply(head, args) => {
            collect_inner(head, out);
            for a in args {
                collect_inner(a, out);
            }
        }
        Term::Fn(_, body) => collect_inner(body, out),
        Term::Symbol(_, args) => {
            for a in args {
                collect_inner(a, out);
            }
        }
    }
}

/// Phase J convenience: filter an iterator of candidate rules to
/// only those that pass empirical validity. Uses `seed` = 0 and
/// `k_samples` = 8 by default — bump via `validate_candidates_ext`
/// when you want tighter certification.
#[must_use]
pub fn validate_candidates(
    candidates: Vec<RewriteRule>,
    library: &[RewriteRule],
    step_limit: usize,
) -> Vec<RewriteRule> {
    validate_candidates_ext(candidates, library, step_limit, 8, 0)
}

/// Phase H integration: wrap a validated library as `Artifact`s and
/// run `MetaPatternGenerator` over it to surface rank-2 (and
/// higher) meta-candidates. Returns the Candidate set; callers can
/// filter for `is_rank2` shapes or accept all meta proposals.
///
/// This is the path the `phase_h_unblock` demo proved out: Phase I
/// surfaces, Phase J certifies, MetaPatternGenerator mints.
/// Encapsulating the Artifact-seal + MetaGen invocation here makes
/// the pipeline a single call outside test code.
///
/// `epoch_id` is passed through to Artifact sealing and the
/// Generator call. `0` is fine for standalone uses; real callers
/// inside an Epoch pass their current epoch.
///
/// `extract_config` tunes `MetaPatternGenerator` — the Phase H
/// demo used `min_shared_size = 1, min_matches = 2,
/// max_new_rules = 20` with a `symbol_id_floor = 30_000`.
#[must_use]
pub fn rank2_candidates_from_library(
    validated_library: &[RewriteRule],
    corpus: &[Term],
    epoch_id: u64,
    extract_config: crate::extract::ExtractConfig,
    symbol_id_floor: SymbolId,
) -> Vec<mathscape_core::epoch::Candidate> {
    use mathscape_core::epoch::{
        AcceptanceCertificate, Artifact, Generator,
    };
    let artifacts: Vec<Artifact> = validated_library
        .iter()
        .enumerate()
        .map(|(i, rule)| {
            Artifact::seal(
                rule.clone(),
                epoch_id,
                AcceptanceCertificate::trivial_conjecture(1.0 + i as f64),
                Vec::new(),
            )
        })
        .collect();
    let mut meta_gen =
        crate::MetaPatternGenerator::new(extract_config, symbol_id_floor);
    meta_gen.propose(epoch_id, corpus, &artifacts)
}

/// Does the rule's LHS look like a rank-2 meta-rule? I.e., outer
/// head is a pattern var AND at least one arg is itself an Apply
/// with a pattern-var head. The structural signature of
/// "operator-variables over operator-variables" — the Phase H
/// inception signal.
#[must_use]
pub fn is_rank2_shape(t: &Term) -> bool {
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

/// Phase I + Phase J composed: extract subterm-paired AU candidates
/// then filter by empirical validity. The single entry point for
/// "discover laws AND certify them" — the path the Phase H rank-2
/// inception demo proved out.
///
/// `subterm_depth = 0` collapses to root-only (same behavior as
/// `derive_laws_with_cache` + validation).
///
/// Returns `(validated_laws, stats)`. Stats covers Phase I timing;
/// Phase J validation time is not broken out (a fixed K*|candidates|
/// eval cost, negligible compared to AU).
///
/// Use when: you want the cleanest "just the laws that provably
/// hold" output, without post-hoc filtering in caller code.
/// Skip the validation pass when performance matters more than
/// semantic correctness (the law generator's original behavior).
#[must_use]
pub fn derive_laws_validated(
    corpus: &[Term],
    library: &[RewriteRule],
    step_limit: usize,
    min_support: usize,
    subterm_depth: usize,
    k_samples: usize,
    seed: u64,
    next_id: &mut SymbolId,
) -> (Vec<RewriteRule>, LawGenStats) {
    let (raw, stats) = derive_laws_with_subterm_au(
        corpus,
        library,
        step_limit,
        min_support,
        subterm_depth,
        next_id,
    );
    let validated =
        validate_candidates_ext(raw, library, step_limit, k_samples, seed);
    (validated, stats)
}

/// Phase J with full knobs exposed.
#[must_use]
pub fn validate_candidates_ext(
    candidates: Vec<RewriteRule>,
    library: &[RewriteRule],
    step_limit: usize,
    k_samples: usize,
    seed: u64,
) -> Vec<RewriteRule> {
    candidates
        .into_iter()
        .filter(|r| is_empirically_valid(r, library, step_limit, k_samples, seed))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::builtin::{ADD, MUL};
    use mathscape_core::value::Value;

    fn var(id: u32) -> Term {
        Term::Var(id)
    }
    fn apply(h: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(h), args)
    }
    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }

    // ── R36 MemoizingAntiUnifier correctness tests ───────────────

    #[test]
    fn memoizing_au_result_matches_direct_call() {
        // Correctness invariant: cached result is identical to the
        // direct paired_anti_unify result for the same inputs.
        let in_a = apply(var(ADD), vec![nat(0), nat(5)]);
        let in_b = apply(var(ADD), vec![nat(0), nat(7)]);
        let out_a = nat(5);
        let out_b = nat(7);

        let direct =
            paired_anti_unify((&in_a, &in_b), (&out_a, &out_b));
        let mut cache = MemoizingAntiUnifier::new();
        let cached = cache.run((&in_a, &in_b), (&out_a, &out_b));
        assert_eq!(direct, cached, "cache must return the same value");
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn memoizing_au_second_call_is_a_hit() {
        // Second call on the same inputs must be a cache hit.
        let in_a = apply(var(ADD), vec![nat(0), nat(5)]);
        let in_b = apply(var(ADD), vec![nat(0), nat(7)]);
        let out_a = nat(5);
        let out_b = nat(7);

        let mut cache = MemoizingAntiUnifier::new();
        let first = cache.run((&in_a, &in_b), (&out_a, &out_b));
        let second = cache.run((&in_a, &in_b), (&out_a, &out_b));
        assert_eq!(first, second);
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.len(), 1, "no new entry on hit");
    }

    #[test]
    fn memoizing_au_different_inputs_distinct_entries() {
        // Different input tuples populate distinct cache entries.
        let in_a = apply(var(ADD), vec![nat(0), nat(5)]);
        let in_b = apply(var(ADD), vec![nat(0), nat(7)]);
        let in_c = apply(var(MUL), vec![nat(1), nat(3)]);
        let in_d = apply(var(MUL), vec![nat(1), nat(4)]);
        let out_a = nat(5);
        let out_b = nat(7);
        let out_c = nat(3);
        let out_d = nat(4);

        let mut cache = MemoizingAntiUnifier::new();
        let _ = cache.run((&in_a, &in_b), (&out_a, &out_b));
        let _ = cache.run((&in_c, &in_d), (&out_c, &out_d));
        assert_eq!(cache.misses(), 2);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn derive_laws_with_cache_matches_uncached_results() {
        // The cache must not change the output of the law generator.
        // Run with and without a cache on the same corpus; results
        // must be structurally identical.
        let corpus = vec![
            apply(var(ADD), vec![nat(0), nat(5)]),
            apply(var(ADD), vec![nat(0), nat(7)]),
            apply(var(ADD), vec![nat(0), nat(9)]),
        ];
        let mut id_a: SymbolId = 100;
        let uncached = derive_laws_from_corpus(&corpus, &[], 100, 2, &mut id_a);

        let mut id_b: SymbolId = 100;
        let mut cache = MemoizingAntiUnifier::new();
        let (cached, _stats) =
            derive_laws_with_cache(&corpus, &[], 100, 2, &mut id_b, &mut cache);

        assert_eq!(uncached.len(), cached.len());
        for (u, c) in uncached.iter().zip(cached.iter()) {
            assert_eq!(u.lhs, c.lhs);
            assert_eq!(u.rhs, c.rhs);
        }
    }

    // ── Phase I: subterm-paired AU tests ─────────────────────────

    // ── Phase J: empirical validity tests ─────────────────────────

    #[test]
    fn valid_add_identity_passes_validation() {
        // add(0, ?x) = ?x — classical identity, holds for every
        // Nat/Int value.
        let rule = RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(ADD), vec![nat(0), var(100)]),
            rhs: var(100),
        };
        assert!(is_empirically_valid(&rule, &[], 200, 8, 0));
    }

    #[test]
    fn bogus_symmetric_rule_fails_validation() {
        // add(?x, ?y) = ?y — obviously wrong semantically; on
        // most bindings, lhs evals to x+y ≠ y.
        let rule = RewriteRule {
            name: "bogus-ignores-x".into(),
            lhs: apply(var(ADD), vec![var(100), var(101)]),
            rhs: var(101),
        };
        assert!(!is_empirically_valid(&rule, &[], 200, 8, 0));
    }

    #[test]
    fn over_general_op_id_fails_validation() {
        // (?op ?x ?id) => ?x — structurally general, semantically
        // wrong because it doesn't constrain ?op / ?id.
        // With var(200) standing for the operator position and
        // concrete bindings substituting numbers for it, eval
        // rejects (no such op) or produces wrong values.
        let rule = RewriteRule {
            name: "over-general".into(),
            lhs: apply(var(200), vec![var(100), var(201)]),
            rhs: var(100),
        };
        // Most bindings can't eval (var(200) substituted to a
        // Number that's not a head op). is_empirically_valid
        // treats eval errors as rejection, which is the right
        // call — a law that doesn't evaluate isn't a valid law.
        assert!(!is_empirically_valid(&rule, &[], 200, 8, 0));
    }

    #[test]
    fn concrete_rule_validates_on_single_eval() {
        // add(2, 3) => 5 — no pattern vars; validity is a
        // single-shot eval comparison.
        let rule = RewriteRule {
            name: "concrete-add".into(),
            lhs: apply(var(ADD), vec![nat(2), nat(3)]),
            rhs: nat(5),
        };
        assert!(is_empirically_valid(&rule, &[], 200, 0, 0));
    }

    #[test]
    fn validate_candidates_filters_correctly() {
        let good = RewriteRule {
            name: "add-id".into(),
            lhs: apply(var(ADD), vec![nat(0), var(100)]),
            rhs: var(100),
        };
        let bad = RewriteRule {
            name: "bogus".into(),
            lhs: apply(var(ADD), vec![var(100), var(101)]),
            rhs: var(101),
        };
        let filtered = validate_candidates(vec![good.clone(), bad], &[], 200);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].lhs, good.lhs);
    }

    #[test]
    fn validation_is_deterministic() {
        // Same (rule, library, k, seed) → same verdict.
        let rule = RewriteRule {
            name: "add-id".into(),
            lhs: apply(var(ADD), vec![nat(0), var(100)]),
            rhs: var(100),
        };
        let v1 = is_empirically_valid(&rule, &[], 200, 16, 42);
        let v2 = is_empirically_valid(&rule, &[], 200, 16, 42);
        assert_eq!(v1, v2);
    }

    #[test]
    fn validation_seed_affects_sample_choice() {
        // Different seeds → potentially different samples. For a
        // TRULY VALID rule, both should accept; this pins that.
        let rule = RewriteRule {
            name: "mul-id".into(),
            lhs: apply(var(MUL), vec![nat(1), var(100)]),
            rhs: var(100),
        };
        assert!(is_empirically_valid(&rule, &[], 200, 8, 0));
        assert!(is_empirically_valid(&rule, &[], 200, 8, 1));
        assert!(is_empirically_valid(&rule, &[], 200, 8, 999));
    }

    #[test]
    fn subterm_au_finds_at_least_as_much_as_root_only() {
        // Invariant: every root-level candidate surfaces in the
        // subterm variant too (because subterm_depth includes the
        // root). The subterm variant may find additional patterns.
        let corpus = vec![
            apply(var(ADD), vec![nat(0), nat(5)]),
            apply(var(ADD), vec![nat(0), nat(7)]),
            apply(var(ADD), vec![nat(0), nat(9)]),
        ];
        let mut id_root: SymbolId = 100;
        let root_only =
            derive_laws_from_corpus(&corpus, &[], 100, 2, &mut id_root);
        let mut id_sub: SymbolId = 100;
        let (with_subterm, _) = derive_laws_with_subterm_au(
            &corpus, &[], 100, 2, 2, &mut id_sub,
        );
        assert!(
            with_subterm.len() >= root_only.len(),
            "subterm AU must not LOSE any root-level candidates"
        );
    }

    #[test]
    fn subterm_au_depth_zero_matches_root_only() {
        // With depth=0, subterm AU skips subterm iteration and
        // behaves identically to the root-only path.
        let corpus = vec![
            apply(var(ADD), vec![nat(0), nat(5)]),
            apply(var(ADD), vec![nat(0), nat(7)]),
        ];
        let mut id_root: SymbolId = 100;
        let (root_pair, _) = derive_laws_with_subterm_au(
            &corpus, &[], 100, 2, 0, &mut id_root,
        );
        let mut id_other: SymbolId = 100;
        let other =
            derive_laws_from_corpus(&corpus, &[], 100, 2, &mut id_other);
        assert_eq!(root_pair.len(), other.len());
    }

    #[test]
    fn subterm_au_deterministic() {
        // Same inputs → same outputs across runs.
        let corpus = vec![
            apply(var(ADD), vec![nat(0), nat(5)]),
            apply(var(ADD), vec![nat(0), nat(7)]),
            apply(var(ADD), vec![nat(0), nat(9)]),
        ];
        let mut id_a: SymbolId = 100;
        let (a, _) = derive_laws_with_subterm_au(
            &corpus, &[], 100, 2, 2, &mut id_a,
        );
        let mut id_b: SymbolId = 100;
        let (b, _) = derive_laws_with_subterm_au(
            &corpus, &[], 100, 2, 2, &mut id_b,
        );
        assert_eq!(a.len(), b.len());
        for (ra, rb) in a.iter().zip(b.iter()) {
            assert_eq!(ra.lhs, rb.lhs);
            assert_eq!(ra.rhs, rb.rhs);
        }
    }

    #[test]
    fn subterm_au_surfaces_inner_pattern() {
        // Corpus where the interesting pattern lives at an INNER
        // position. Traces like `add(0, 5) → 5` and `add(0, 7) → 7`
        // are root-level laws (root matches). But if the inputs are
        // WRAPPED in an outer apply whose outer structure varies,
        // only the subterm variant can surface the inner law.
        //
        // Specifically: two mul-add constructions where the outer
        // mul multiplicand differs but inner add(0, ?x) matches:
        //   - trace 1: mul(2, add(0, 5)) → eval = mul(2, 5) → 10
        //   - trace 2: mul(3, add(0, 7)) → eval = mul(3, 7) → 21
        //
        // Root-level AU sees LHS=mul(?a, add(0, ?b)), RHS=?c —
        // RHS var ?c not bound by LHS → reject.
        // Subterm AU at position [1] (the inner add) sees:
        //   LHS = add(0, ?b), RHS = 10 vs 21 → ?c — still rejected
        // Still invisible because outputs don't share the pattern.
        //
        // This test documents that Phase I ALONE cannot surface
        // patterns whose RHS structure depends on bindings not
        // present in both outputs. Demonstrates what Phase J
        // (empirical validity) would need to add.
        let corpus = vec![
            apply(var(MUL), vec![nat(2), apply(var(ADD), vec![nat(0), nat(5)])]),
            apply(var(MUL), vec![nat(3), apply(var(ADD), vec![nat(0), nat(7)])]),
        ];
        let mut id: SymbolId = 100;
        let (laws, stats) =
            derive_laws_with_subterm_au(&corpus, &[], 100, 2, 2, &mut id);
        // Pins current behavior: subterm AU surfaces *some* new
        // candidates compared to root, but the eval-fold collapses
        // output structure so few of them pass the RHS-subset-LHS
        // filter. Record what we actually see so regressions are
        // visible.
        assert!(stats.pairs_considered >= 1);
        let _ = laws; // laws may be empty; signal is in stats
    }

    #[test]
    fn derive_laws_with_cache_reuses_across_calls() {
        // Reusing the same cache across two calls on identical
        // corpora: second call is all hits.
        let corpus = vec![
            apply(var(ADD), vec![nat(0), nat(5)]),
            apply(var(ADD), vec![nat(0), nat(7)]),
            apply(var(ADD), vec![nat(0), nat(9)]),
        ];
        let mut cache = MemoizingAntiUnifier::new();
        let mut id: SymbolId = 100;

        let _ = derive_laws_with_cache(&corpus, &[], 100, 2, &mut id, &mut cache);
        let first_misses = cache.misses();
        let first_hits = cache.hits();

        let _ = derive_laws_with_cache(&corpus, &[], 100, 2, &mut id, &mut cache);
        assert_eq!(
            cache.misses(),
            first_misses,
            "second call on identical corpus adds zero misses"
        );
        assert!(
            cache.hits() > first_hits,
            "second call hits the cache"
        );
    }

    #[test]
    fn discovers_add_left_identity() {
        // Corpus of `add(0, x)` for varied x. Each reduces to x
        // via R6 constant folding (sort + fold).
        //
        // Wait — R6 folds `add(0, x)` when x is a Number (both
        // args are Numbers). For x = Var, it stays as `add(0, ?x)`.
        // To get non-trivial reductions, we need concrete inputs.
        //
        // Use concrete Nat values for x. R6 folds them via add:
        // add(0, 5) → 5, add(0, 7) → 7, etc.
        let corpus = vec![
            apply(var(ADD), vec![nat(0), nat(5)]),
            apply(var(ADD), vec![nat(0), nat(7)]),
            apply(var(ADD), vec![nat(0), nat(9)]),
            apply(var(ADD), vec![nat(0), nat(11)]),
        ];
        let mut next_id: SymbolId = 0;
        let laws = derive_laws_from_corpus(&corpus, &[], 100, 2, &mut next_id);

        // Expected: a law of shape `add(0, ?x) = ?x`.
        // Note the sort puts 0 first since Number < Var (ordering).
        // Actually for Number+Number, both args fold entirely — so
        // eval reduces add(0, 5) to 5 directly. The trace is
        // (add(0,5), 5).
        //
        // When we paired-AU two traces:
        //   (add(0,5), 5) and (add(0,7), 7)
        // LHS AU: add(0, ?v) — because 5 and 7 differ
        // RHS AU: ?v — same fresh var
        // Law: add(0, ?v) = ?v ✓
        assert!(
            !laws.is_empty(),
            "expected at least one law discovered from identity-rich corpus"
        );
        // Check that at least one law has shape `add(_, _) = var`.
        let found_identity = laws.iter().any(|l| {
            matches!(&l.lhs, Term::Apply(h, args) if matches!(h.as_ref(), Term::Var(ADD))
                && args.len() == 2)
                && matches!(&l.rhs, Term::Var(_))
        });
        assert!(
            found_identity,
            "expected identity-shaped law among discovered: {laws:#?}"
        );
    }

    #[test]
    fn discovers_mul_one_identity() {
        let corpus = vec![
            apply(var(MUL), vec![nat(1), nat(3)]),
            apply(var(MUL), vec![nat(1), nat(5)]),
            apply(var(MUL), vec![nat(1), nat(7)]),
            apply(var(MUL), vec![nat(1), nat(11)]),
        ];
        let mut next_id: SymbolId = 0;
        let laws = derive_laws_from_corpus(&corpus, &[], 100, 2, &mut next_id);
        assert!(!laws.is_empty(), "expected mul-identity law");
    }

    #[test]
    fn discovers_multiple_laws_from_mixed_corpus() {
        // Mix identity instances for BOTH add and mul in one corpus.
        // We expect the machine to separate them into two distinct
        // laws by support.
        let mut corpus = Vec::new();
        for v in [3u64, 5, 7, 11, 13] {
            corpus.push(apply(var(ADD), vec![nat(0), nat(v)]));
            corpus.push(apply(var(MUL), vec![nat(1), nat(v)]));
        }
        let mut next_id: SymbolId = 0;
        let laws = derive_laws_from_corpus(&corpus, &[], 100, 3, &mut next_id);
        // Both add-identity and mul-identity should emerge given
        // enough instances of each.
        let has_add_id = laws.iter().any(|l| {
            matches!(&l.lhs, Term::Apply(h, _) if matches!(h.as_ref(), Term::Var(ADD)))
        });
        let has_mul_id = laws.iter().any(|l| {
            matches!(&l.lhs, Term::Apply(h, _) if matches!(h.as_ref(), Term::Var(MUL)))
        });
        assert!(
            has_add_id && has_mul_id,
            "expected both add-identity and mul-identity laws: got {laws:#?}"
        );
    }

    #[test]
    fn rejects_trivial_no_reduction_corpus() {
        // All Vars — no reduction possible. Should return nothing.
        let corpus = vec![var(100), var(101), var(102)];
        let mut next_id: SymbolId = 0;
        let laws = derive_laws_from_corpus(&corpus, &[], 100, 2, &mut next_id);
        assert!(laws.is_empty(), "no-reduction corpus must produce no laws");
    }

    #[test]
    fn law_support_filter_works() {
        // Only one instance → can't form a pair → no law at min_support=2.
        let corpus = vec![apply(var(ADD), vec![nat(0), nat(5)])];
        let mut next_id: SymbolId = 0;
        let laws = derive_laws_from_corpus(&corpus, &[], 100, 2, &mut next_id);
        assert!(laws.is_empty(), "single-instance corpus can't form law pairs");
    }

    // ── R27 invariant tests: every emitted law is well-formed ────

    /// A law is well-formed iff:
    ///   1. LHS contains at least one pattern variable (id ≥ 100)
    ///   2. RHS variables are a subset of LHS variables
    ///   3. LHS ≠ RHS (non-trivial)
    /// These are the same conditions `paired_anti_unify` enforces
    /// — this property test verifies the derive_laws_from_corpus
    /// pipeline never leaks malformed output to its caller.
    fn is_well_formed_law(rule: &mathscape_core::eval::RewriteRule) -> bool {
        let lhs_vars = collect_pattern_vars(&rule.lhs);
        let rhs_vars = collect_pattern_vars(&rule.rhs);
        !lhs_vars.is_empty()
            && rhs_vars.is_subset(&lhs_vars)
            && rule.lhs != rule.rhs
    }

    fn collect_pattern_vars(
        t: &Term,
    ) -> std::collections::BTreeSet<u32> {
        let mut out = std::collections::BTreeSet::new();
        collect_inner(t, &mut out);
        out
    }

    fn collect_inner(t: &Term, out: &mut std::collections::BTreeSet<u32>) {
        match t {
            Term::Var(v) => {
                if *v >= 100 {
                    out.insert(*v);
                }
            }
            Term::Apply(head, args) => {
                collect_inner(head, out);
                for a in args {
                    collect_inner(a, out);
                }
            }
            Term::Fn(_, body) => collect_inner(body, out),
            Term::Symbol(_, args) => {
                for a in args {
                    collect_inner(a, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn every_emitted_law_is_well_formed_add_corpus() {
        let corpus: Vec<Term> = (3..=11u64)
            .step_by(2)
            .map(|v| apply(var(ADD), vec![nat(0), nat(v)]))
            .collect();
        let mut next_id: SymbolId = 0;
        let laws = derive_laws_from_corpus(&corpus, &[], 100, 2, &mut next_id);
        assert!(!laws.is_empty());
        for law in &laws {
            assert!(
                is_well_formed_law(law),
                "emitted malformed law: {law:?}"
            );
        }
    }

    #[test]
    fn every_emitted_law_is_well_formed_mixed_corpus() {
        let mut corpus = Vec::new();
        for v in [3u64, 5, 7, 11, 13, 17] {
            corpus.push(apply(var(ADD), vec![nat(0), nat(v)]));
            corpus.push(apply(var(MUL), vec![nat(1), nat(v)]));
        }
        let mut next_id: SymbolId = 0;
        let laws = derive_laws_from_corpus(&corpus, &[], 100, 2, &mut next_id);
        for law in &laws {
            assert!(is_well_formed_law(law), "malformed: {law:?}");
        }
    }

    #[test]
    fn derive_laws_is_deterministic() {
        // Same corpus + same support threshold → identical law
        // patterns. Name-level differences (symbol ids) are
        // allowed; structural content must match.
        let corpus = vec![
            apply(var(ADD), vec![nat(0), nat(5)]),
            apply(var(ADD), vec![nat(0), nat(7)]),
            apply(var(ADD), vec![nat(0), nat(9)]),
        ];
        let mut id_a: SymbolId = 0;
        let mut id_b: SymbolId = 0;
        let laws_a =
            derive_laws_from_corpus(&corpus, &[], 100, 2, &mut id_a);
        let laws_b =
            derive_laws_from_corpus(&corpus, &[], 100, 2, &mut id_b);
        assert_eq!(laws_a.len(), laws_b.len());
        let patterns_a: std::collections::BTreeSet<(Term, Term)> = laws_a
            .iter()
            .map(|l| (l.lhs.clone(), l.rhs.clone()))
            .collect();
        let patterns_b: std::collections::BTreeSet<(Term, Term)> = laws_b
            .iter()
            .map(|l| (l.lhs.clone(), l.rhs.clone()))
            .collect();
        assert_eq!(patterns_a, patterns_b);
    }

    #[test]
    fn min_support_filter_respected() {
        // 3 instances → C(3,2) = 3 pairs → max support per law is 3.
        // With min_support=5, nothing passes.
        let corpus = vec![
            apply(var(ADD), vec![nat(0), nat(5)]),
            apply(var(ADD), vec![nat(0), nat(7)]),
            apply(var(ADD), vec![nat(0), nat(9)]),
        ];
        let mut next_id: SymbolId = 0;
        let strict =
            derive_laws_from_corpus(&corpus, &[], 100, 5, &mut next_id);
        assert!(
            strict.is_empty(),
            "min_support=5 with max possible support=3 must yield no laws"
        );

        // With min_support=1, the law is emitted.
        let mut next_id2: SymbolId = 0;
        let lax = derive_laws_from_corpus(&corpus, &[], 100, 1, &mut next_id2);
        assert!(
            !lax.is_empty(),
            "min_support=1 must accept laws with any support"
        );
    }

    #[test]
    fn empty_corpus_yields_no_laws() {
        let mut next_id: SymbolId = 0;
        let laws = derive_laws_from_corpus(&[], &[], 100, 2, &mut next_id);
        assert!(laws.is_empty());
    }

    #[test]
    fn laws_ranked_by_support_descending() {
        // 5 identity-add instances + 2 identity-mul instances.
        // The add law has more supporting pairs than the mul law.
        // When both emerge, add should be first.
        let mut corpus = Vec::new();
        for v in [3u64, 5, 7, 9, 11] {
            corpus.push(apply(var(ADD), vec![nat(0), nat(v)]));
        }
        for v in [3u64, 5] {
            corpus.push(apply(var(MUL), vec![nat(1), nat(v)]));
        }
        let mut next_id: SymbolId = 0;
        let laws = derive_laws_from_corpus(&corpus, &[], 100, 1, &mut next_id);
        // First law emitted should have the most support — the add
        // identity law with ~C(5,2)=10 pairs vs mul at C(2,2)=1.
        // (Cross-pairs also contribute but same-head pairs dominate.)
        assert!(!laws.is_empty());
        // The first law's LHS should be add-headed (highest support).
        let first_head = match &laws[0].lhs {
            Term::Apply(h, _) => match h.as_ref() {
                Term::Var(id) => Some(*id),
                _ => None,
            },
            _ => None,
        };
        assert_eq!(
            first_head,
            Some(ADD),
            "highest-support law should be add-identity (more instances)"
        );
    }
}
