//! R8 — Tensor shape detector.
//!
//! # What "naturally develop the tensor" means
//!
//! Tensors are multi-linear maps: bilinear (rank 2), trilinear
//! (rank 3), and so on. In the mathscape substrate, tensor
//! structure emerges when the machine discovers rules that
//! express multi-linear relationships between operators.
//!
//! The simplest non-trivial tensor is a bilinear map — a function
//! linear in each of two arguments. `mul(a, b)` is bilinear in
//! (a, b) PROVIDED it distributes over `add`:
//!
//!   mul(add(a, b), c) = add(mul(a, c), mul(b, c))      [left-distrib]
//!   mul(a, add(b, c)) = add(mul(a, b), mul(a, c))      [right-distrib]
//!
//! Without distributivity, mul is just a binary operator. With it,
//! mul becomes the rank-2 tensor in the (add, mul) pair. The
//! machine's autonomous discovery of distributivity is PRECISELY
//! the moment tensor structure appears.
//!
//! # Detection strategy
//!
//! This module implements shape-based detection on rewrite rules.
//! Given a library, we classify each rule as one of:
//!
//!   - `TensorShape::None` — no tensor structure detected
//!   - `TensorShape::Distributive { outer, inner }` — left or right
//!     distributivity of `outer` over `inner`
//!   - `TensorShape::MetaDistributive` — a meta-rule that abstracts
//!     a distributivity pattern with operator variables
//!
//! The fraction of rules in the library with `≠ None` classification
//! is the **tensor density** — a scalar the traversal can report to
//! answer "has tensor structure emerged yet?"
//!
//! # What this does NOT do
//!
//! - Doesn't check that the rule actually holds numerically — that's
//!   the prover's job. The detector pattern-matches structure only.
//! - Doesn't distinguish rank-2 from rank-3. A future extension
//!   could pattern-match trilinear shapes (e.g., matrix product).
//! - Doesn't interpret the tensor — doesn't say "this is how
//!   matrix multiplication works." It says "this shape is present."

use crate::eval::RewriteRule;
use crate::term::Term;

/// The tensor shape of a rule, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TensorShape {
    /// No tensor structure detected.
    None,
    /// Distributivity: the rule expresses `outer(inner(a, b), c) =
    /// inner(outer(a, c), outer(b, c))` or the symmetric form.
    /// `outer` is the bilinear operator; `inner` is the additive
    /// operator it distributes over.
    Distributive { outer: u32, inner: u32 },
    /// Meta-distributive: a rule with operator-variables that
    /// abstracts distributivity across operators. Higher-order
    /// form — the machine's path to rank-3+ tensor structure.
    MetaDistributive,
}

/// Classify a single rule's tensor shape.
#[must_use]
pub fn classify(rule: &RewriteRule) -> TensorShape {
    // Canonicalize first so AC-equivalent rules get identified
    // regardless of arg ordering in the raw rule.
    let lhs = rule.lhs.canonical();
    let rhs = rule.rhs.canonical();

    if let Some((outer, inner)) = match_distributive(&lhs, &rhs) {
        return TensorShape::Distributive { outer, inner };
    }
    if match_meta_distributive(&lhs, &rhs) {
        return TensorShape::MetaDistributive;
    }
    TensorShape::None
}

/// Tensor density of a library: the fraction of rules with ≠ None
/// tensor shape. Scalar in [0, 1]. Zero means no tensor structure
/// has emerged; values near 1 mean the library is tensor-dense.
///
/// Empty libraries return 0.
#[must_use]
pub fn tensor_density(rules: &[RewriteRule]) -> f64 {
    if rules.is_empty() {
        return 0.0;
    }
    let hits = rules
        .iter()
        .filter(|r| !matches!(classify(r), TensorShape::None))
        .count();
    hits as f64 / rules.len() as f64
}

/// Count rules by tensor shape. Returns (distributive, meta, none).
#[must_use]
pub fn shape_counts(rules: &[RewriteRule]) -> (usize, usize, usize) {
    let mut distrib = 0;
    let mut meta = 0;
    let mut none = 0;
    for r in rules {
        match classify(r) {
            TensorShape::Distributive { .. } => distrib += 1,
            TensorShape::MetaDistributive => meta += 1,
            TensorShape::None => none += 1,
        }
    }
    (distrib, meta, none)
}

// ── Pattern matching ─────────────────────────────────────────────

/// Match distributivity at the concrete-operator level. Returns
/// (outer_id, inner_id) if the rule encodes
///   LHS = outer(inner(a, b), c)     or     outer(c, inner(a, b))
///   RHS = inner(outer(a, c), outer(b, c))
/// with all pattern variables distinct and concrete operator ids
/// in both outer and inner positions.
fn match_distributive(lhs: &Term, rhs: &Term) -> Option<(u32, u32)> {
    // LHS structure: Apply(Var(outer), [Apply(Var(inner), [Var(a), Var(b)]), Var(c)])
    // The canonicalizer sorts AC args, so after canonicalization
    // the Apply sub-term comes before the Var sub-term (Apply
    // ranks before Var in the derived Ord) — we account for both
    // orderings in case the rule involves non-AC operators.
    let (outer_id, outer_args) = match_binary_apply(lhs)?;
    let (inner_apply, other) = find_inner_apply_and_other(outer_args)?;
    let (inner_id, inner_args) = match_binary_apply(inner_apply)?;
    if outer_id == inner_id {
        return None; // need distinct ops for a meaningful distributivity
    }
    // Concrete-operator distributivity requires both heads to be
    // vocabulary ids (< 100 by the anonymize convention in
    // eval.rs). Heads ≥ 100 are pattern variables — treated as
    // meta-distributive by the caller.
    if outer_id >= 100 || inner_id >= 100 {
        return None;
    }
    let c_var = match other {
        Term::Var(v) => *v,
        _ => return None,
    };
    let a_var = match &inner_args[0] {
        Term::Var(v) => *v,
        _ => return None,
    };
    let b_var = match &inner_args[1] {
        Term::Var(v) => *v,
        _ => return None,
    };
    // a, b, c must be three distinct variables.
    if a_var == b_var || a_var == c_var || b_var == c_var {
        return None;
    }

    // RHS structure: Apply(Var(inner), [Apply(Var(outer), [?, ?]), Apply(Var(outer), [?, ?])])
    let (rhs_head, rhs_args) = match_binary_apply(rhs)?;
    if rhs_head != inner_id {
        return None;
    }
    let (out1_id, out1_args) = match_binary_apply(&rhs_args[0])?;
    let (out2_id, out2_args) = match_binary_apply(&rhs_args[1])?;
    if out1_id != outer_id || out2_id != outer_id {
        return None;
    }
    // Each outer(·, ·) on the RHS must contain the c_var. The
    // other arg of each must be a_var on one side and b_var on the
    // other (but order depends on canonical sort, so we check set
    // membership).
    let (p1, p2) = (two_vars(out1_args)?, two_vars(out2_args)?);
    let c_in_1 = p1.0 == c_var || p1.1 == c_var;
    let c_in_2 = p2.0 == c_var || p2.1 == c_var;
    if !(c_in_1 && c_in_2) {
        return None;
    }
    let other1 = if p1.0 == c_var { p1.1 } else { p1.0 };
    let other2 = if p2.0 == c_var { p2.1 } else { p2.0 };
    let set = [other1, other2];
    if !(set.contains(&a_var) && set.contains(&b_var)) {
        return None;
    }
    Some((outer_id, inner_id))
}

/// Match meta-distributive shape: at least one Var position is
/// used as a HEAD (operator variable) in both LHS and RHS. This
/// is the shape the machine uses to abstract over operators
/// (e.g., S_10000's `(?op ...)` pattern). Applied to
/// distributivity, a meta-distrib rule would say something like
/// `?f(?g(a, b), c) = ?g(?f(a, c), ?f(b, c))` — an abstract
/// tensor law.
fn match_meta_distributive(lhs: &Term, rhs: &Term) -> bool {
    // Must be operator-variable on LHS head AND nested operator-
    // variable inside (two op vars). Must preserve the
    // distributive SHAPE (outer over inner, outer appears twice
    // on RHS).
    let (outer_head, outer_args) = match lhs {
        Term::Apply(h, a) if a.len() == 2 => (h.as_ref(), a),
        _ => return false,
    };
    // Must be op-VARIABLE (id ≥ 100), not a concrete builtin
    // (id < 100). Concrete-op distributivity is handled by
    // match_distributive; this function catches the abstracted
    // form where the operators themselves are pattern variables.
    let outer_is_opvar = matches!(outer_head, Term::Var(id) if *id >= 100);
    if !outer_is_opvar {
        return false;
    }
    // Find an inner Apply whose head is also an op-variable.
    let inner_apply = outer_args.iter().find_map(|a| match a {
        Term::Apply(h, iargs)
            if iargs.len() == 2
                && matches!(h.as_ref(), Term::Var(id) if *id >= 100) =>
        {
            Some((h.as_ref(), iargs))
        }
        _ => None,
    });
    let Some((inner_head, _inner_args)) = inner_apply else {
        return false;
    };
    // RHS must use inner_head as outer and outer_head as inner,
    // with outer_head appearing as head of 2 sub-Applys.
    let (rhs_head, rhs_args) = match rhs {
        Term::Apply(h, a) if a.len() == 2 => (h.as_ref(), a),
        _ => return false,
    };
    // rhs_head should equal inner_head (from LHS)
    if rhs_head != inner_head {
        return false;
    }
    // both rhs args should be Apply whose head equals outer_head
    rhs_args.iter().all(|a| match a {
        Term::Apply(h, iargs) if iargs.len() == 2 => h.as_ref() == outer_head,
        _ => false,
    })
}

fn match_binary_apply(t: &Term) -> Option<(u32, &Vec<Term>)> {
    match t {
        Term::Apply(head, args) if args.len() == 2 => {
            if let Term::Var(id) = head.as_ref() {
                Some((*id, args))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn find_inner_apply_and_other(args: &[Term]) -> Option<(&Term, &Term)> {
    match (&args[0], &args[1]) {
        (Term::Apply(_, _), _) => Some((&args[0], &args[1])),
        (_, Term::Apply(_, _)) => Some((&args[1], &args[0])),
        _ => None,
    }
}

fn two_vars(args: &[Term]) -> Option<(u32, u32)> {
    if args.len() != 2 {
        return None;
    }
    let a = match &args[0] {
        Term::Var(v) => *v,
        _ => return None,
    };
    let b = match &args[1] {
        Term::Var(v) => *v,
        _ => return None,
    };
    Some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{ADD, MUL};

    fn var(id: u32) -> Term {
        Term::Var(id)
    }
    fn app(h: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(h), args)
    }

    #[test]
    fn distributive_mul_over_add_is_rank2_tensor() {
        // mul(add(?a, ?b), ?c) = add(mul(?a, ?c), mul(?b, ?c))
        let a = var(100);
        let b = var(101);
        let c = var(102);
        let rule = RewriteRule {
            name: "mul-distrib-add".into(),
            lhs: app(var(MUL), vec![app(var(ADD), vec![a.clone(), b.clone()]), c.clone()]),
            rhs: app(
                var(ADD),
                vec![
                    app(var(MUL), vec![a.clone(), c.clone()]),
                    app(var(MUL), vec![b.clone(), c.clone()]),
                ],
            ),
        };
        match classify(&rule) {
            TensorShape::Distributive { outer, inner } => {
                assert_eq!(outer, MUL);
                assert_eq!(inner, ADD);
            }
            other => panic!("expected Distributive, got {other:?}"),
        }
    }

    #[test]
    fn distributive_detects_symmetric_form_right() {
        // mul(?c, add(?a, ?b)) = add(mul(?c, ?a), mul(?c, ?b))
        // Right-distributive form. Canonical form sorts AC args,
        // so the LHS/RHS both normalize and the detector recognizes
        // whichever symmetric variant the machine discovered.
        let a = var(100);
        let b = var(101);
        let c = var(102);
        let rule = RewriteRule {
            name: "mul-distrib-add-right".into(),
            lhs: app(var(MUL), vec![c.clone(), app(var(ADD), vec![a.clone(), b.clone()])]),
            rhs: app(
                var(ADD),
                vec![
                    app(var(MUL), vec![c.clone(), a.clone()]),
                    app(var(MUL), vec![c.clone(), b.clone()]),
                ],
            ),
        };
        assert!(matches!(
            classify(&rule),
            TensorShape::Distributive { outer, inner } if outer == MUL && inner == ADD
        ));
    }

    #[test]
    fn non_distributive_rule_is_none() {
        // add-identity: add(?x, 0) = ?x — not tensor structure.
        let rule = RewriteRule {
            name: "add-identity".into(),
            lhs: app(var(ADD), vec![var(100), Term::Number(crate::value::Value::Nat(0))]),
            rhs: var(100),
        };
        assert_eq!(classify(&rule), TensorShape::None);
    }

    #[test]
    fn commutativity_is_not_tensor() {
        // add(?a, ?b) = add(?b, ?a) — commutativity, not tensor.
        let rule = RewriteRule {
            name: "add-commute".into(),
            lhs: app(var(ADD), vec![var(100), var(101)]),
            rhs: app(var(ADD), vec![var(101), var(100)]),
        };
        assert_eq!(classify(&rule), TensorShape::None);
    }

    #[test]
    fn same_op_on_both_sides_is_not_distributive() {
        // "add distributes over add" makes no sense — catch it.
        let a = var(100);
        let b = var(101);
        let c = var(102);
        let rule = RewriteRule {
            name: "add-over-add".into(),
            lhs: app(var(ADD), vec![app(var(ADD), vec![a.clone(), b.clone()]), c.clone()]),
            rhs: app(
                var(ADD),
                vec![
                    app(var(ADD), vec![a.clone(), c.clone()]),
                    app(var(ADD), vec![b.clone(), c.clone()]),
                ],
            ),
        };
        assert_eq!(classify(&rule), TensorShape::None);
    }

    #[test]
    fn meta_distributive_uses_op_vars() {
        // ?f(?g(?a, ?b), ?c) = ?g(?f(?a, ?c), ?f(?b, ?c))
        // An abstract distributivity pattern — gateway to rank-2
        // tensor generalization across operators.
        let f = var(200); // op var
        let g = var(201); // op var
        let a = var(100);
        let b = var(101);
        let c = var(102);
        let rule = RewriteRule {
            name: "meta-distrib".into(),
            lhs: app(f.clone(), vec![app(g.clone(), vec![a.clone(), b.clone()]), c.clone()]),
            rhs: app(
                g.clone(),
                vec![
                    app(f.clone(), vec![a.clone(), c.clone()]),
                    app(f.clone(), vec![b.clone(), c.clone()]),
                ],
            ),
        };
        assert_eq!(classify(&rule), TensorShape::MetaDistributive);
    }

    #[test]
    fn tensor_density_zero_for_empty_library() {
        assert_eq!(tensor_density(&[]), 0.0);
    }

    #[test]
    fn tensor_density_scales_with_tensor_rule_count() {
        let a = var(100);
        let b = var(101);
        let c = var(102);
        let distrib = RewriteRule {
            name: "distrib".into(),
            lhs: app(var(MUL), vec![app(var(ADD), vec![a.clone(), b.clone()]), c.clone()]),
            rhs: app(
                var(ADD),
                vec![
                    app(var(MUL), vec![a.clone(), c.clone()]),
                    app(var(MUL), vec![b.clone(), c.clone()]),
                ],
            ),
        };
        let identity = RewriteRule {
            name: "id".into(),
            lhs: app(var(ADD), vec![a.clone(), Term::Number(crate::value::Value::Nat(0))]),
            rhs: a.clone(),
        };
        // One tensor rule, one identity → 50% tensor density.
        let lib = vec![distrib.clone(), identity.clone()];
        let density = tensor_density(&lib);
        assert!((density - 0.5).abs() < 1e-9);
        // Zero tensor rules → 0%.
        assert_eq!(tensor_density(&[identity.clone()]), 0.0);
        // All tensor rules → 100%.
        assert!((tensor_density(&[distrib.clone()]) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn shape_counts_aggregates_all_three_categories() {
        let a = var(100);
        let b = var(101);
        let c = var(102);
        let distrib = RewriteRule {
            name: "distrib".into(),
            lhs: app(var(MUL), vec![app(var(ADD), vec![a.clone(), b.clone()]), c.clone()]),
            rhs: app(
                var(ADD),
                vec![
                    app(var(MUL), vec![a.clone(), c.clone()]),
                    app(var(MUL), vec![b.clone(), c.clone()]),
                ],
            ),
        };
        let f = var(200);
        let g = var(201);
        let meta = RewriteRule {
            name: "meta".into(),
            lhs: app(f.clone(), vec![app(g.clone(), vec![a.clone(), b.clone()]), c.clone()]),
            rhs: app(
                g.clone(),
                vec![
                    app(f.clone(), vec![a.clone(), c.clone()]),
                    app(f.clone(), vec![b.clone(), c.clone()]),
                ],
            ),
        };
        let identity = RewriteRule {
            name: "id".into(),
            lhs: app(var(ADD), vec![a.clone(), Term::Number(crate::value::Value::Nat(0))]),
            rhs: a.clone(),
        };
        let (d, m, n) = shape_counts(&[distrib, meta, identity]);
        assert_eq!(d, 1);
        assert_eq!(m, 1);
        assert_eq!(n, 1);
    }
}
