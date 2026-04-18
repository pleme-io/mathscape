//! Phase K — e-graph equality saturation via the `egg` crate.
//!
//! Brings semantic-equivalence reasoning to mathscape's prover.
//! Classical pattern matching (our current `subsumes` /
//! `pattern_match`) only sees SYNTACTIC equality — two rules that
//! commute at the arithmetic level (`add(a,b)` vs `add(b,a)`)
//! appear as distinct patterns and both survive as library entries.
//! The e-graph approach merges them into one equivalence class:
//! they're *the same rule up to commutativity*.
//!
//! This module is scoped narrowly:
//!
//! 1. Bridge mathscape's `Term` to egg's `Language` trait via a
//!    `MathscapeLang` node type.
//! 2. Expose `check_equivalence(a, b, rewrites, step_limit)` —
//!    given two terms and a ruleset, saturate and ask whether both
//!    terms land in the same e-class.
//! 3. Expose a curated set of canonical algebraic rewrites
//!    (commutativity, associativity, identity) that the machine can
//!    INVOKE to detect equivalences beyond syntactic match. These
//!    are NOT mathematical axioms the machine has proven — they're
//!    probes that test "if these laws held, would these two terms
//!    be the same?"
//!
//! Designed as a pure additive module. Nothing outside this file
//! imports egg. Consumers (a future `EGraphProver`) will layer on
//! top without touching existing pattern-matching paths.

use egg::{define_language, Id, RecExpr, Rewrite, Runner, Symbol};
use mathscape_core::term::Term;
use mathscape_core::value::Value;

// ── The e-graph language ────────────────────────────────────────
//
// MathscapeLang mirrors mathscape_core::Term in the form egg wants:
// a flat list of nodes where children are Ids into the same RecExpr.
// egg's define_language! macro generates the parser + discriminant
// plumbing for us.

define_language! {
    pub enum MathscapeLang {
        // A natural number constant. egg requires a value type that
        // impls FromStr / Display / Eq / Hash; u64 fits.
        Num(u64),
        // A pattern-level variable (mathscape's Var) — keyed by id.
        // Encoded as a Symbol so egg's pattern parser can bind it.
        "var" = Var([Id; 1]),
        // A function-application application — the mathscape Apply.
        // First Id is the head (function), remaining Ids are args.
        // Use a variadic form by convention via egg's Vec child.
        "apply" = Apply(Box<[Id]>),
        // A named symbol (mathscape Symbol) — first Id holds the
        // symbol id as a Num; remaining are children.
        "sym" = Sym(Box<[Id]>),
        // Opaque Point nodes — hash-consed by id as Num.
        "point" = Point([Id; 1]),
        // Fn node — params list then body. Params encoded as a tuple
        // list via nested applies; kept minimal since mathscape's
        // Fn is rare in live corpora.
        "fn" = Fn(Box<[Id]>),
        // Fallback: egg's Symbol for anything external (e.g., a
        // variable name in a pattern like "?x"). Allows pattern
        // parsing via egg's built-in Var::parse.
        Other(Symbol),
    }
}

/// Push a `Term` into an egg `RecExpr`, returning the root `Id`.
///
/// Walks bottom-up; children are added before parents so egg's
/// internal hash-cons has the child Ids ready. This preserves the
/// structural sharing mathscape's Term already has.
pub fn term_to_recexpr(term: &Term, expr: &mut RecExpr<MathscapeLang>) -> Id {
    match term {
        Term::Number(Value::Nat(n)) => expr.add(MathscapeLang::Num(*n)),
        Term::Var(v) => {
            let id_node = expr.add(MathscapeLang::Num(*v as u64));
            expr.add(MathscapeLang::Var([id_node]))
        }
        Term::Point(p) => {
            let id_node = expr.add(MathscapeLang::Num(*p));
            expr.add(MathscapeLang::Point([id_node]))
        }
        Term::Apply(f, args) => {
            let mut ids = Vec::with_capacity(args.len() + 1);
            ids.push(term_to_recexpr(f, expr));
            for a in args {
                ids.push(term_to_recexpr(a, expr));
            }
            expr.add(MathscapeLang::Apply(ids.into_boxed_slice()))
        }
        Term::Symbol(sid, args) => {
            let mut ids = Vec::with_capacity(args.len() + 1);
            let id_node = expr.add(MathscapeLang::Num(*sid as u64));
            ids.push(id_node);
            for a in args {
                ids.push(term_to_recexpr(a, expr));
            }
            expr.add(MathscapeLang::Sym(ids.into_boxed_slice()))
        }
        Term::Fn(params, body) => {
            let mut ids = Vec::with_capacity(params.len() + 1);
            for p in params {
                ids.push(expr.add(MathscapeLang::Num(*p as u64)));
            }
            ids.push(term_to_recexpr(body, expr));
            expr.add(MathscapeLang::Fn(ids.into_boxed_slice()))
        }
    }
}

/// Build a standalone RecExpr from a Term.
#[must_use]
pub fn term_to_expr(term: &Term) -> RecExpr<MathscapeLang> {
    let mut expr = RecExpr::default();
    term_to_recexpr(term, &mut expr);
    expr
}

// ── Curated rewrites ────────────────────────────────────────────
//
// These are the "what if" laws the machine uses to ask whether two
// terms are semantically equivalent. Each rewrite is a hypothesis:
// if this law held, would the terms merge? egg's saturation runs
// all rewrites to fixed-point and reports the resulting e-classes.
//
// The rewrites are selected to be operator-agnostic via egg Pattern
// variables (?op, ?x, ?y). They test commutativity and
// associativity for binary operators in the `apply` shape.

/// Commutativity probe: `apply(?op, ?a, ?b)` ↔ `apply(?op, ?b, ?a)`.
/// Active for any binary operator; doesn't assume which one is
/// commutative. Use with care — applying this blindly makes e-graph
/// saturation treat add and mul as commutative even though only
/// some operators are. The probe's VALUE is not "are these
/// commutative" but "under the hypothesis of commutativity, do
/// these terms merge?"
#[must_use]
pub fn commutativity_probe() -> Vec<Rewrite<MathscapeLang, ()>> {
    vec![
        egg::rewrite!("commute-binary"; "(apply ?op ?a ?b)" => "(apply ?op ?b ?a)"),
    ]
}

/// Associativity probe: `apply(?op, apply(?op, ?a, ?b), ?c)` ↔
/// `apply(?op, ?a, apply(?op, ?b, ?c))`.
/// Same caveat as commutativity — it's a probe, not an assertion.
#[must_use]
pub fn associativity_probe() -> Vec<Rewrite<MathscapeLang, ()>> {
    vec![
        egg::rewrite!(
            "assoc-left-right";
            "(apply ?op (apply ?op ?a ?b) ?c)" => "(apply ?op ?a (apply ?op ?b ?c))"
        ),
        egg::rewrite!(
            "assoc-right-left";
            "(apply ?op ?a (apply ?op ?b ?c))" => "(apply ?op (apply ?op ?a ?b) ?c)"
        ),
    ]
}

/// Test whether `lhs` and `rhs` are equivalent under the given
/// probe rewrites, saturating up to `step_limit` iterations.
///
/// Returns:
///   Some(true)  — the two terms landed in the same e-class
///                 (equivalent under the probes)
///   Some(false) — saturation completed and they're in different
///                 e-classes (proven inequivalent under the probes)
///   None        — saturation hit the step limit without converging;
///                 equivalence is unknown at this probe-budget
pub fn check_equivalence(
    lhs: &Term,
    rhs: &Term,
    rewrites: &[Rewrite<MathscapeLang, ()>],
    step_limit: usize,
) -> Option<bool> {
    let lhs_expr = term_to_expr(lhs);
    let rhs_expr = term_to_expr(rhs);
    let runner = Runner::default()
        .with_iter_limit(step_limit)
        .with_expr(&lhs_expr)
        .with_expr(&rhs_expr)
        .run(rewrites);
    let stop_reason = runner.stop_reason.clone();
    let lhs_id = runner.egraph.lookup_expr(&lhs_expr);
    let rhs_id = runner.egraph.lookup_expr(&rhs_expr);
    match (lhs_id, rhs_id, stop_reason) {
        (Some(a), Some(b), Some(egg::StopReason::Saturated)) => Some(a == b),
        (Some(a), Some(b), _) if a == b => Some(true),
        (Some(_), Some(_), _) => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn term_to_expr_preserves_leaf_constants() {
        let t = nat(42);
        let expr = term_to_expr(&t);
        assert!(!expr.as_ref().is_empty());
    }

    #[test]
    fn term_to_expr_preserves_apply_structure() {
        // add(1, 2)
        let t = apply(var(2), vec![nat(1), nat(2)]);
        let expr = term_to_expr(&t);
        // Should have nodes for Num(1), Num(2), Num(2 as var_id),
        // Var([..]), and Apply([..]).
        assert!(expr.as_ref().len() >= 5);
    }

    #[test]
    fn identical_terms_are_equivalent() {
        // No rewrites needed — structural equality lands in the
        // same e-class trivially.
        let t = apply(var(2), vec![nat(1), nat(2)]);
        assert_eq!(
            check_equivalence(&t, &t, &[], 5),
            Some(true),
            "identical terms must land in the same e-class"
        );
    }

    #[test]
    fn distinct_terms_are_inequivalent_without_rewrites() {
        let t1 = apply(var(2), vec![nat(1), nat(2)]);
        let t2 = apply(var(2), vec![nat(3), nat(4)]);
        assert_eq!(
            check_equivalence(&t1, &t2, &[], 5),
            Some(false),
            "structurally distinct terms are not equivalent under empty rewrites"
        );
    }

    #[test]
    fn commutativity_probe_merges_swapped_args() {
        // add(1, 2) and add(2, 1) — equivalent under commutativity.
        let t1 = apply(var(2), vec![nat(1), nat(2)]);
        let t2 = apply(var(2), vec![nat(2), nat(1)]);
        let rewrites = commutativity_probe();
        let verdict = check_equivalence(&t1, &t2, &rewrites, 30);
        assert_eq!(
            verdict, Some(true),
            "commutativity probe should merge swapped-arg pairs; got {verdict:?}"
        );
    }

    #[test]
    fn associativity_probe_merges_regrouping() {
        // add(add(1, 2), 3) and add(1, add(2, 3))
        let left = apply(
            var(2),
            vec![apply(var(2), vec![nat(1), nat(2)]), nat(3)],
        );
        let right = apply(
            var(2),
            vec![nat(1), apply(var(2), vec![nat(2), nat(3)])],
        );
        let rewrites = associativity_probe();
        let verdict = check_equivalence(&left, &right, &rewrites, 30);
        assert_eq!(
            verdict, Some(true),
            "associativity probe should merge regrouped pairs; got {verdict:?}"
        );
    }

    #[test]
    fn commutativity_does_not_merge_different_ops() {
        // add(1, 2) and mul(1, 2) — different operators. Even under
        // commutativity, they must NOT merge.
        let t1 = apply(var(2), vec![nat(1), nat(2)]);
        let t2 = apply(var(3), vec![nat(1), nat(2)]);
        let rewrites = commutativity_probe();
        let verdict = check_equivalence(&t1, &t2, &rewrites, 30);
        assert_ne!(
            verdict, Some(true),
            "commutativity must not merge add(1,2) with mul(1,2)"
        );
    }
}
