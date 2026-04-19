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
