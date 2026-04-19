//! Anti-unification: find the most specific generalization of two terms.
//!
//! Given two terms t1 and t2, produce a pattern P with variables such that:
//! - P matches t1 with some substitution sigma1
//! - P matches t2 with some substitution sigma2
//! - P is the most specific such pattern (least general generalization)
//!
//! This is the core operation for discovering shared structure across
//! expressions in the population.

use mathscape_core::term::Term;
use std::collections::HashMap;

/// Result of anti-unification.
#[derive(Debug, Clone)]
pub struct AntiUnifyResult {
    /// The generalized pattern (with Var placeholders).
    pub pattern: Term,
    /// Number of shared structure nodes.
    pub shared_size: usize,
    /// Number of variable positions (divergence points).
    pub var_count: usize,
}

/// Phase I extension: subterm-aware anti-unification.
///
/// Classical `anti_unify` returns ONE pattern per (t1, t2) pair — the
/// root-level least general generalization. When roots differ, the
/// pattern is just a fresh variable at root, losing all inner
/// structure the terms might share at subterm positions.
///
/// `subterm_anti_unify` additionally tries anti-unifying at subterm
/// positions: each subterm of t1 against t2, each subterm of t2
/// against t1. The resulting set includes the root-level result plus
/// any subterm-level candidates whose `shared_size` exceeds a
/// threshold. This lets `extract_rules` surface patterns invisible
/// to root-only matching.
///
/// Example:
///   t1 = add(mul(x, 2), 0)
///   t2 = mul(x, 3)
///   Root anti-unify: fresh var at root, shared_size ≈ 1
///   Subterm anti-unify also tries: (mul(x, 2), mul(x, 3))
///     → shared: mul(?x, ?y), shared_size 3
///
/// The subterm variant is additive — it extends what `extract_rules`
/// can see without replacing classical AU.
pub fn subterm_anti_unify(t1: &Term, t2: &Term, min_shared_size: usize) -> Vec<AntiUnifyResult> {
    let mut results = Vec::new();
    // Always include the root-level result.
    results.push(anti_unify(t1, t2));

    // Each subterm of t1 paired with t2.
    for sub1 in collect_subterms(t1) {
        let r = anti_unify(&sub1, t2);
        if r.shared_size >= min_shared_size {
            results.push(r);
        }
    }
    // Each subterm of t2 paired with t1.
    for sub2 in collect_subterms(t2) {
        let r = anti_unify(t1, &sub2);
        if r.shared_size >= min_shared_size {
            results.push(r);
        }
    }

    // Dedup by pattern structural equality, keep max shared_size.
    results.sort_by(|a, b| b.shared_size.cmp(&a.shared_size));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut unique = Vec::new();
    for r in results {
        let key = format!("{}", r.pattern);
        if seen.insert(key) {
            unique.push(r);
        }
    }
    unique
}

/// Collect all non-leaf subterms of a term (excluding the term
/// itself). Used by subterm_anti_unify to enumerate positions where
/// subterm-level shared structure might live.
fn collect_subterms(t: &Term) -> Vec<Term> {
    let mut out = Vec::new();
    collect_subterms_inner(t, &mut out, true);
    out
}

fn collect_subterms_inner(t: &Term, out: &mut Vec<Term>, is_root: bool) {
    if !is_root {
        out.push(t.clone());
    }
    match t {
        Term::Apply(f, args) => {
            collect_subterms_inner(f, out, false);
            for a in args {
                collect_subterms_inner(a, out, false);
            }
        }
        Term::Fn(_, body) => collect_subterms_inner(body, out, false),
        Term::Symbol(_, args) => {
            for a in args {
                collect_subterms_inner(a, out, false);
            }
        }
        _ => {}
    }
}

/// Anti-unify two terms: find their most specific common generalization.
///
/// Fresh-var counter starts above the maximum var id present in either
/// input term, so the generated generalization variables don't collide
/// with pattern variables already inside the terms. Without this, anti-
/// unifying two rule LHSs from `CompressionGenerator` (which uses
/// Var(200) as its default pattern variable) would produce a pattern
/// where the fresh operator-variable *equals* the shared pattern
/// variable at Var(200) — a semantic bug that silently causes
/// pattern_match to fail on meta-level anti-unification.
pub fn anti_unify(t1: &Term, t2: &Term) -> AntiUnifyResult {
    let floor = 200u32;
    let max_in_inputs = max_var_id(t1).max(max_var_id(t2));
    let mut next_var = floor.max(max_in_inputs.saturating_add(1));
    let mut var_pairs: HashMap<(TermKey, TermKey), u32> = HashMap::new();
    let mut shared_size = 0;
    let mut var_count = 0;

    let pattern = au_inner(t1, t2, &mut next_var, &mut var_pairs, &mut shared_size, &mut var_count);

    AntiUnifyResult {
        pattern,
        shared_size,
        var_count,
    }
}

/// R24 (2026-04-18): paired anti-unification for law discovery.
///
/// Given two "before → after" pairs produced by evaluation
/// (e.g. `(add(5, 0), 5)` and `(add(7, 0), 7)`), compute the
/// least general generalization OF BOTH SIDES using a SHARED
/// var_pairs map. The same fresh variable is used wherever the
/// same corresponding subterm pair appears — across both the
/// LHS pattern and the RHS pattern.
///
/// Result: `(lhs_pattern, rhs_pattern)` — a candidate equational
/// law. If no generalization exists that respects both sides,
/// returns `None`.
///
/// Example:
///   Pair 1: add(5, 0) → 5
///   Pair 2: add(7, 0) → 7
///   Paired AU: lhs = add(?v200, 0), rhs = ?v200 ⇒ law
///              `add(?x, 0) = ?x`
///
/// This is the primitive the law-generator uses to extract laws
/// from evaluation traces. Currently in antiunify rather than its
/// own module because it shares the inner machinery.
pub fn paired_anti_unify(
    inputs: (&Term, &Term),
    outputs: (&Term, &Term),
) -> Option<(Term, Term)> {
    // `inputs` = (in_a, in_b) — the two inputs to anti-unify against
    // each other, producing the LHS pattern.
    // `outputs` = (out_a, out_b) — the two outputs, producing the
    // RHS pattern.
    let (in_a, in_b) = inputs;
    let (out_a, out_b) = outputs;

    let floor = 200u32;
    let max_in = max_var_id(in_a)
        .max(max_var_id(in_b))
        .max(max_var_id(out_a))
        .max(max_var_id(out_b));
    let mut next_var = floor.max(max_in.saturating_add(1));
    let mut var_pairs: HashMap<(TermKey, TermKey), u32> = HashMap::new();
    let mut shared_in = 0;
    let mut var_count_in = 0;
    let mut shared_out = 0;
    let mut var_count_out = 0;

    // Both AUs share var_pairs — same (subterm_t1, subterm_t2) pair
    // assigns the SAME fresh variable, even across LHS/RHS.
    let lhs_pattern = au_inner(
        in_a,
        in_b,
        &mut next_var,
        &mut var_pairs,
        &mut shared_in,
        &mut var_count_in,
    );
    let rhs_pattern = au_inner(
        out_a,
        out_b,
        &mut next_var,
        &mut var_pairs,
        &mut shared_out,
        &mut var_count_out,
    );

    // Validity check: the law is useful only if LHS has at least
    // one pattern variable (otherwise it's a concrete equation,
    // not a law). And RHS patterns must either be concrete or use
    // vars that the LHS also uses (otherwise RHS has free vars
    // the law can't bind).
    if var_count_in == 0 {
        return None;
    }
    let lhs_vars: std::collections::BTreeSet<u32> =
        collect_pattern_vars(&lhs_pattern);
    let rhs_vars: std::collections::BTreeSet<u32> =
        collect_pattern_vars(&rhs_pattern);
    // Every var in RHS must be bound by LHS (otherwise the law is
    // underdetermined — the RHS has a free variable the machine
    // can't fill in).
    if !rhs_vars.is_subset(&lhs_vars) {
        return None;
    }
    // And LHS ≠ RHS — a vacuous law (LHS = LHS) carries no info.
    if lhs_pattern == rhs_pattern {
        return None;
    }

    Some((lhs_pattern, rhs_pattern))
}

fn collect_pattern_vars(t: &Term) -> std::collections::BTreeSet<u32> {
    let mut out = std::collections::BTreeSet::new();
    collect_pattern_vars_inner(t, &mut out);
    out
}

fn collect_pattern_vars_inner(t: &Term, out: &mut std::collections::BTreeSet<u32>) {
    match t {
        Term::Var(v) => {
            // Only pattern vars (>= 100 by anonymize convention);
            // vocabulary (< 100) isn't a binding.
            if *v >= 100 {
                out.insert(*v);
            }
        }
        Term::Apply(head, args) => {
            collect_pattern_vars_inner(head, out);
            for a in args {
                collect_pattern_vars_inner(a, out);
            }
        }
        Term::Fn(_, body) => collect_pattern_vars_inner(body, out),
        Term::Symbol(_, args) => {
            for a in args {
                collect_pattern_vars_inner(a, out);
            }
        }
        _ => {}
    }
}

/// Maximum var id appearing anywhere in a term (0 if none).
fn max_var_id(t: &Term) -> u32 {
    match t {
        Term::Var(v) => *v,
        Term::Apply(f, args) => {
            let mut m = max_var_id(f);
            for a in args {
                m = m.max(max_var_id(a));
            }
            m
        }
        Term::Fn(_, body) => max_var_id(body),
        Term::Symbol(_, args) => {
            let mut m = 0u32;
            for a in args {
                m = m.max(max_var_id(a));
            }
            m
        }
        Term::Point(_) | Term::Number(_) => 0,
    }
}

/// Simplified term key for memoizing variable assignments inside
/// `au_inner`'s divergence-to-var map.
///
/// R38: leaves hold `Term` directly rather than a `Vec<u8>` produced
/// by `format!("{t:?}").into_bytes()`. The old path round-tripped
/// every leaf divergence through the Debug formatter + a heap
/// allocation; `Term` already derives `Hash + Eq`, so using it as
/// the key is both faster and more direct. Complex (non-leaf)
/// divergences still collapse to a single bucket — not changed here
/// because doing so would alter AU semantics (siblings that were
/// previously treated as the same divergence would now be
/// distinguished, altering which patterns get generated).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum TermKey {
    Leaf(Term),
    Complex,
}

fn term_key(t: &Term) -> TermKey {
    match t {
        Term::Point(_) | Term::Number(_) | Term::Var(_) => {
            TermKey::Leaf(t.clone())
        }
        _ => TermKey::Complex,
    }
}

fn au_inner(
    t1: &Term,
    t2: &Term,
    next_var: &mut u32,
    var_pairs: &mut HashMap<(TermKey, TermKey), u32>,
    shared_size: &mut usize,
    var_count: &mut usize,
) -> Term {
    // If structurally identical, return as-is
    if t1 == t2 {
        *shared_size += t1.size();
        return t1.clone();
    }

    // Try to match structure
    match (t1, t2) {
        (Term::Apply(f1, a1), Term::Apply(f2, a2)) if a1.len() == a2.len() => {
            let f = au_inner(f1, f2, next_var, var_pairs, shared_size, var_count);
            let args: Vec<Term> = a1
                .iter()
                .zip(a2.iter())
                .map(|(a, b)| au_inner(a, b, next_var, var_pairs, shared_size, var_count))
                .collect();
            *shared_size += 1; // the Apply node itself
            Term::Apply(Box::new(f), args)
        }

        (Term::Fn(p1, b1), Term::Fn(p2, b2)) if p1.len() == p2.len() && p1 == p2 => {
            let body = au_inner(b1, b2, next_var, var_pairs, shared_size, var_count);
            *shared_size += 1;
            Term::Fn(p1.clone(), Box::new(body))
        }

        (Term::Symbol(id1, a1), Term::Symbol(id2, a2))
            if id1 == id2 && a1.len() == a2.len() =>
        {
            let args: Vec<Term> = a1
                .iter()
                .zip(a2.iter())
                .map(|(a, b)| au_inner(a, b, next_var, var_pairs, shared_size, var_count))
                .collect();
            *shared_size += 1;
            Term::Symbol(*id1, args)
        }

        // Different structure: introduce a variable
        _ => {
            let k1 = term_key(t1);
            let k2 = term_key(t2);
            let key = (k1, k2);

            let var_id = if let Some(&existing) = var_pairs.get(&key) {
                existing
            } else {
                let v = *next_var;
                *next_var += 1;
                *var_count += 1;
                var_pairs.insert(key, v);
                v
            };

            Term::Var(var_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn identical_terms() {
        let t = apply(var(2), vec![nat(1), nat(2)]);
        let result = anti_unify(&t, &t);
        assert_eq!(result.pattern, t);
        assert_eq!(result.var_count, 0);
    }

    #[test]
    fn different_constants() {
        // add(1, 2) vs add(3, 4) => add(?a, ?b)
        let t1 = apply(var(2), vec![nat(1), nat(2)]);
        let t2 = apply(var(2), vec![nat(3), nat(4)]);
        let result = anti_unify(&t1, &t2);

        // Should share the Apply+Var(2) structure, with 2 variable positions
        assert_eq!(result.var_count, 2);
        assert!(result.shared_size >= 2); // Apply + func
    }

    #[test]
    fn shared_substructure() {
        // add(x, 0) vs add(y, 0) => add(?a, 0)
        let t1 = apply(var(2), vec![nat(5), nat(0)]);
        let t2 = apply(var(2), vec![nat(9), nat(0)]);
        let result = anti_unify(&t1, &t2);

        // The 0 should be shared, only one variable position
        assert_eq!(result.var_count, 1);
    }

    #[test]
    fn completely_different_structure_produces_single_variable() {
        // A leaf (nat) vs an Apply — totally different structure
        let t1 = nat(42);
        let t2 = apply(var(2), vec![nat(1), nat(2)]);
        let result = anti_unify(&t1, &t2);

        // Should produce a single fresh variable since nothing is shared
        assert_eq!(result.var_count, 1);
        assert!(matches!(result.pattern, Term::Var(_)));
    }

    #[test]
    fn empty_args_match() {
        // Both Apply with 0 args: Apply(var(2), []) vs Apply(var(3), [])
        let t1 = apply(var(2), vec![]);
        let t2 = apply(var(3), vec![]);
        let result = anti_unify(&t1, &t2);

        // The Apply structure is shared but the function differs
        assert!(result.shared_size >= 1, "Apply node itself should be shared");
        assert_eq!(result.var_count, 1, "function position should be a variable");
    }

    #[test]
    fn subterm_anti_unify_finds_shared_subterm_across_different_roots() {
        // Phase I test: t1 and t2 have different root operators but
        // share an inner subterm. Classical AU gives a fresh var at
        // root, shared_size ≈ 1. Subterm AU should find the shared
        // inner structure.
        let t1 = apply(var(2), vec![apply(var(3), vec![nat(5), nat(6)]), nat(0)]);
        // = add(mul(5, 6), 0)
        let t2 = apply(var(3), vec![nat(5), nat(6)]);
        // = mul(5, 6)  (exact subterm of t1)
        let results = subterm_anti_unify(&t1, &t2, 2);
        assert!(
            results.iter().any(|r| r.shared_size >= 3),
            "subterm AU should find the shared mul(5, 6) subterm with \
             shared_size >= 3; got results: {:?}",
            results.iter().map(|r| r.shared_size).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn subterm_anti_unify_includes_root_result() {
        // Even when subterm results are better, the root-level
        // result should still appear in the output.
        let t1 = apply(var(2), vec![nat(1), nat(2)]);
        let t2 = apply(var(2), vec![nat(3), nat(4)]);
        let results = subterm_anti_unify(&t1, &t2, 1);
        assert!(
            !results.is_empty(),
            "subterm AU should always return at least the root result"
        );
    }

    #[test]
    fn subterm_anti_unify_dedups_equivalent_patterns() {
        // If multiple subterm pairs anti-unify to the same pattern
        // (e.g., repeated structure), results should be deduped.
        let t1 = apply(var(2), vec![nat(1), nat(1)]);
        let t2 = apply(var(2), vec![nat(2), nat(2)]);
        let results = subterm_anti_unify(&t1, &t2, 1);
        // At minimum the root pattern. Any additional entries should
        // be structurally distinct from each other.
        let mut patterns: std::collections::HashSet<String> = std::collections::HashSet::new();
        for r in &results {
            assert!(
                patterns.insert(format!("{}", r.pattern)),
                "duplicate pattern in results: {}",
                r.pattern
            );
        }
    }

    #[test]
    fn deep_nested_terms_shared_size_grows() {
        // Build deeply nested identical structure with one leaf difference
        // add(add(add(1, 0), 0), 0) vs add(add(add(2, 0), 0), 0)
        // Depth 3 of shared Apply+var(2) + shared 0 constants
        let inner1 = apply(var(2), vec![nat(1), nat(0)]);
        let mid1 = apply(var(2), vec![inner1, nat(0)]);
        let outer1 = apply(var(2), vec![mid1, nat(0)]);

        let inner2 = apply(var(2), vec![nat(2), nat(0)]);
        let mid2 = apply(var(2), vec![inner2, nat(0)]);
        let outer2 = apply(var(2), vec![mid2, nat(0)]);

        let result = anti_unify(&outer1, &outer2);

        // Only one variable position (the differing leaf: 1 vs 2)
        assert_eq!(result.var_count, 1);
        // Shared structure should include all the Apply nodes, var(2) functions, and 0 constants
        // 3 Apply nodes + 3 var(2) + 3 nat(0) = at minimum several shared nodes
        assert!(
            result.shared_size >= 6,
            "deep nesting should yield large shared_size, got {}",
            result.shared_size
        );
    }
}
