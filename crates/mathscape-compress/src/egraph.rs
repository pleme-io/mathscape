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
use mathscape_core::eval::{anonymize_term, RewriteRule};
use mathscape_core::term::Term;
use mathscape_core::value::Value;

/// Type alias so consumers (traversal tests, downstream crates)
/// can hold probe sets without depending on `egg` directly.
pub type MathscapeRewrite = Rewrite<MathscapeLang, ()>;

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
        // R7 (2026-04-18): Int encoded into egg by reinterpreting
        // the i64 bits as u64. Egg's Num node is u64; the kernel
        // round-trips Int via this reinterpretation. Equivalence
        // saturation doesn't care about semantic distinction — it
        // operates on syntactic structure — so Int vs Nat are
        // distinct at the egg level if the u64-bit encodings
        // differ. For small positive Ints (common case) the bit
        // pattern matches Nat, which is fine because the kernel
        // never inserts cross-domain candidates into the same
        // e-graph run.
        Term::Number(Value::Int(n)) => {
            expr.add(MathscapeLang::Num(*n as u64))
        }
        // R13: Tensors are opaque to the e-graph (shape + data
        // don't have e-node representation yet). Encode the
        // tensor's structural identity as a single hash-like u64
        // so the e-graph can at least distinguish equal vs
        // unequal tensors. Richer tensor reasoning — contraction
        // equivalence, reshape-invariance — is future work.
        Term::Number(Value::Tensor { shape, data }) => {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            shape.hash(&mut h);
            data.hash(&mut h);
            expr.add(MathscapeLang::Num(h.finish()))
        }
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

/// Test whether two *rules* are semantically equivalent under the
/// given probes. Rules are first anonymized (fresh-var ids
/// canonicalized) so that `add(?a, 0) → ?a` and `add(?b, 0) → ?b`
/// are recognized as the same rule despite different nominal ids.
/// Then both the LHS and RHS are checked for e-class merger under
/// the probe set.
///
/// Returns `Some(true)` iff BOTH the LHS pair and the RHS pair land
/// in the same e-class — i.e., the rules encode the same rewrite
/// modulo the probes. `Some(false)` is returned only if both halves
/// reach a definite "inequivalent" verdict; any uncertainty on
/// either half returns `None`.
///
/// This is strictly more powerful than `alpha_equivalent`: any
/// alpha-equivalent pair returns `Some(true)` here (empty probes
/// suffice for identical anonymized terms), and commutatively-
/// equivalent pairs that alpha_equivalent misses (`add(?a, ?b) →
/// ?c` vs `add(?b, ?a) → ?c`) surface when the commutativity probe
/// is supplied.
pub fn check_rule_equivalence(
    r1: &RewriteRule,
    r2: &RewriteRule,
    rewrites: &[Rewrite<MathscapeLang, ()>],
    step_limit: usize,
) -> Option<bool> {
    let r1_lhs = anonymize_term(&r1.lhs);
    let r1_rhs = anonymize_term(&r1.rhs);
    let r2_lhs = anonymize_term(&r2.lhs);
    let r2_rhs = anonymize_term(&r2.rhs);
    let lhs_verdict = check_equivalence(&r1_lhs, &r2_lhs, rewrites, step_limit);
    let rhs_verdict = check_equivalence(&r1_rhs, &r2_rhs, rewrites, step_limit);
    match (lhs_verdict, rhs_verdict) {
        (Some(true), Some(true)) => Some(true),
        (Some(false), _) | (_, Some(false)) => Some(false),
        _ => None,
    }
}

/// Is `rule` semantically NOVEL against a ledger of validated
/// theorems? Returns `true` when `rule` is not e-graph-equivalent
/// under the probe set to any entry in `ledger`. Returns `false`
/// as soon as one equivalent entry is found. Undetermined verdicts
/// (saturation limit) count as "not equivalent" — we err on the
/// side of accepting a rule the probe set can't confidently reject,
/// since the validator has already certified it empirically.
///
/// This is the phase K hook for tightening the Ledger's novelty
/// from STRUCTURAL (literal LHS+RHS dedup) to SEMANTIC (equivalence-
/// class dedup under commutativity/associativity). Pass
/// `commutativity_probe()` for cheap commute-only dedup, or
/// concatenate `commutativity_probe()` + `associativity_probe()`
/// for full AC-equivalence.
#[must_use]
pub fn is_semantically_novel(
    rule: &RewriteRule,
    ledger: &[RewriteRule],
    rewrites: &[Rewrite<MathscapeLang, ()>],
    step_limit: usize,
) -> bool {
    for existing in ledger {
        if matches!(
            check_rule_equivalence(rule, existing, rewrites, step_limit),
            Some(true)
        ) {
            return false;
        }
    }
    true
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

    #[test]
    fn rule_equivalence_catches_alpha_variants_with_no_probes() {
        // add(?a, ?b) → symbol_pair(?a, ?b)  vs
        // add(?c, ?d) → symbol_pair(?c, ?d) — identical modulo var renaming.
        let r1 = RewriteRule {
            name: "R1".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: Term::Symbol(42, vec![var(100), var(101)]),
        };
        let r2 = RewriteRule {
            name: "R2".into(),
            lhs: apply(var(2), vec![var(200), var(201)]),
            rhs: Term::Symbol(42, vec![var(200), var(201)]),
        };
        assert_eq!(
            check_rule_equivalence(&r1, &r2, &[], 5),
            Some(true),
            "alpha-renamed rules must be equivalent even without probes"
        );
    }

    #[test]
    fn rule_equivalence_under_commutativity_probe() {
        // add(?a, ?b) → S(?a, ?b)  vs  add(?b, ?a) → S(?b, ?a)
        // LHSs are commutatively equivalent; RHSs use the SAME
        // canonical var order post-anonymization, so RHS check
        // trivially passes. The probe is what unlocks the LHS merge.
        let r1 = RewriteRule {
            name: "R1".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: Term::Symbol(42, vec![var(100), var(101)]),
        };
        let r2 = RewriteRule {
            name: "R2".into(),
            lhs: apply(var(2), vec![var(101), var(100)]),
            rhs: Term::Symbol(42, vec![var(100), var(101)]),
        };
        let probes = commutativity_probe();
        let verdict = check_rule_equivalence(&r1, &r2, &probes, 30);
        assert_eq!(
            verdict, Some(true),
            "commutativity probe should merge arg-swapped rule variants; got {verdict:?}"
        );
    }

    #[test]
    fn rule_equivalence_rejects_distinct_rules() {
        // add(?a, 0) → ?a  vs  mul(?a, 1) → ?a — different operators
        // AND different constants. No probe set should merge.
        let r1 = RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        let r2 = RewriteRule {
            name: "mul-identity".into(),
            lhs: apply(var(3), vec![var(100), nat(1)]),
            rhs: var(100),
        };
        let probes = commutativity_probe();
        let verdict = check_rule_equivalence(&r1, &r2, &probes, 30);
        assert_ne!(
            verdict, Some(true),
            "distinct identities on different operators must not merge"
        );
    }

    #[test]
    fn semantic_novelty_against_empty_ledger_is_novel() {
        let rule = RewriteRule {
            name: "r".into(),
            lhs: apply(var(2), vec![nat(0), var(100)]),
            rhs: var(100),
        };
        assert!(is_semantically_novel(&rule, &[], &[], 5));
    }

    #[test]
    fn semantic_novelty_rejects_commutative_duplicate_with_probes() {
        // Ledger has `add(?a, ?b) → S(?a, ?b)`. A new rule
        // `add(?b, ?a) → S(?a, ?b)` is commutatively equivalent
        // under the commutativity probe — is_semantically_novel
        // must reject.
        let existing = RewriteRule {
            name: "existing".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: Term::Symbol(42, vec![Term::Var(100), Term::Var(101)]),
        };
        let new_rule = RewriteRule {
            name: "commute-variant".into(),
            lhs: apply(var(2), vec![var(101), var(100)]),
            rhs: Term::Symbol(42, vec![Term::Var(100), Term::Var(101)]),
        };
        let probes = commutativity_probe();
        assert!(
            !is_semantically_novel(&new_rule, &[existing], &probes, 30),
            "commutativity probe must collapse arg-swapped duplicate"
        );
    }

    #[test]
    fn semantic_novelty_against_structurally_different_lhs_is_novel() {
        // Different LHS shapes — these are genuinely different
        // rules, not merely re-expressions of one. Both should
        // survive dedup.
        let existing = RewriteRule {
            name: "existing".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: Term::Symbol(42, vec![Term::Var(100), Term::Var(101)]),
        };
        let new_rule = RewriteRule {
            name: "different-lhs".into(),
            lhs: apply(var(3), vec![var(100), var(101)]), // mul, not add
            rhs: Term::Symbol(42, vec![Term::Var(100), Term::Var(101)]),
        };
        let probes = commutativity_probe();
        assert!(
            is_semantically_novel(&new_rule, &[existing], &probes, 30),
            "mul rule must not collapse with add rule"
        );
    }

    #[test]
    fn rule_equivalence_rejects_structurally_different_rhs() {
        // Same LHS, but structurally different RHSs: one projects to
        // the first captured var, the other wraps both in a Symbol.
        // Anonymization canonicalizes Symbol ids, so only STRUCTURAL
        // divergence counts — that's the point.
        let r1 = RewriteRule {
            name: "project".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: var(100),
        };
        let r2 = RewriteRule {
            name: "wrap".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: Term::Symbol(42, vec![var(100), var(101)]),
        };
        assert_ne!(
            check_rule_equivalence(&r1, &r2, &[], 5),
            Some(true),
            "rules with structurally different RHSs must not merge"
        );
    }
}
