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

use crate::antiunify::paired_anti_unify;
use mathscape_core::eval::{eval, RewriteRule};
use mathscape_core::term::{SymbolId, Term};
use std::collections::BTreeMap;

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
    // Phase 1: evaluate every term. Keep non-trivial reductions
    // as (input, output) traces.
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

    if traces.len() < 2 {
        return Vec::new();
    }

    // Phase 2: paired anti-unify trace pairs. Build a map from
    // (lhs_pattern, rhs_pattern) → support count.
    let mut law_support: BTreeMap<(Term, Term), usize> = BTreeMap::new();

    let max_pairs = 500.min(traces.len() * (traces.len() - 1) / 2);
    let mut considered = 0;
    'outer: for i in 0..traces.len() {
        for j in (i + 1)..traces.len() {
            if considered >= max_pairs {
                break 'outer;
            }
            considered += 1;

            // paired_anti_unify takes TWO traces. Each trace is
            // an (input, output) pair. The shared fresh-var map
            // across the two anti-unifications (one for inputs,
            // one for outputs) makes the SAME position get the
            // SAME pattern variable in both.
            //
            // Subtle: paired_anti_unify's signature is
            //   (pair1: (&Term, &Term), pair2: (&Term, &Term))
            // where pair1 = (in1, in2) — the two INPUTS we anti-
            // unify against each other — and pair2 = (out1, out2)
            // — the two OUTPUTS. So we pass trace fields as:
            //   ((in1, in2), (out1, out2))
            // which is trace-i's input vs trace-j's input, and
            // trace-i's output vs trace-j's output.
            let (in1, out1) = (&traces[i].0, &traces[i].1);
            let (in2, out2) = (&traces[j].0, &traces[j].1);

            if let Some((lhs_pat, rhs_pat)) =
                paired_anti_unify((in1, in2), (out1, out2))
            {
                *law_support.entry((lhs_pat, rhs_pat)).or_default() += 1;
            }
        }
    }

    // Phase 3: filter by min_support, emit as rules.
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

    laws
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
