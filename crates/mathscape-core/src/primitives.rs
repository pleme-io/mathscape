//! R12 — ML primitive shape catalog.
//!
//! # Scope and honest framing
//!
//! R8 detected one structural shape: distributivity (bilinearity).
//! That's useful but narrow — ML depends on more. This module
//! extends detection to the broader set: identity elements,
//! involution, idempotence, distributivity (in both orientations),
//! and homomorphism. Each is a STRUCTURAL PROPERTY of a rewrite
//! rule, detectable by shape-matching against the rule's LHS/RHS.
//!
//! **What this module is.** A catalog of shape detectors. Given a
//! rule, `classify_primitives` returns every primitive property
//! that rule exhibits. The catalog IS the data; detectors are how
//! we read it.
//!
//! **What this module is NOT.** Neural networks. Differentiable
//! compute. Gradient descent. Those require compute machinery we
//! haven't built and aren't claiming. Detecting "this rule is an
//! identity law" does not build an optimizer. The primitive
//! catalog tells us WHAT the machine discovered; the ML stack
//! above (tensors-as-arrays, autodiff, loss, optimizers) requires
//! additional substrate that's future work.
//!
//! # Why these eight primitives
//!
//! They're the structural properties that recur everywhere in
//! algebra and ML: every linear layer has additive homomorphism,
//! every normalization is idempotent, every negation is involutive,
//! every matmul is bilinear. Detecting them when they emerge is
//! the first move toward knowing when the machine has the
//! structural ingredients for higher ML primitives.
//!
//! # Extending the catalog
//!
//! Add a new variant to `MlPrimitive`, implement a `detect_X`
//! function, wire into `classify_primitives`. Each detector is
//! self-contained; the catalog grows without refactoring.
//!
//! Future: make the catalog DATA (Sexp patterns with generic
//! matcher) rather than CODE (Rust detectors). Would let the
//! catalog be self-modifying and Lisp-editable. For now Rust
//! detectors are simpler and sufficient.

use crate::eval::RewriteRule;
use crate::term::Term;
use crate::value::Value;
use serde::{Deserialize, Serialize};

/// A structural property a rule exhibits. A single rule can
/// exhibit multiple (e.g., an identity-returning idempotent
/// projection).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MlPrimitive {
    /// `op(e, a) = a` — left identity for `op` with identity `e`.
    LeftIdentity { op: u32, identity: IdentityForm },
    /// `op(a, e) = a` — right identity.
    RightIdentity { op: u32, identity: IdentityForm },
    /// `f(f(a)) = a` — `f` is self-inverse.
    Involution { f: u32 },
    /// `f(f(a)) = f(a)` — applying `f` twice is the same as once.
    Idempotence { f: u32 },
    /// Left-distributivity: `outer(inner(a, b), c) =
    ///   inner(outer(a, c), outer(b, c))`. Bilinearity of `outer`
    /// over `inner`. (R8's Distributive re-expressed here.)
    LeftDistributive { outer: u32, inner: u32 },
    /// Right-distributivity: `outer(c, inner(a, b)) =
    ///   inner(outer(c, a), outer(c, b))`.
    RightDistributive { outer: u32, inner: u32 },
    /// `f(op(a, b)) = op(f(a), f(b))` — `f` is a homomorphism for
    /// `op`. Covers both "additive homomorphism" (when `op` is an
    /// additive operator) and "multiplicative homomorphism" (when
    /// `op` is multiplicative). The `op` field says which.
    Homomorphism { f: u32, op: u32 },
    /// A meta-shape version of distributivity, where the operators
    /// themselves are pattern variables. The gateway to abstract
    /// bilinearity over discovered operators.
    MetaDistributive,
}

/// The identity element shape. Either a concrete Number (Nat/Int)
/// or a variable (for patterns where the identity is abstract).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IdentityForm {
    Nat(u64),
    Int(i64),
    /// A pattern variable — the rule is parametric in the identity
    /// element. `?op(?id, ?a) = ?a` where `?id` is abstract.
    Abstract(u32),
}

/// Classify a rule: return every ML primitive property it exhibits.
/// A single rule can match multiple categories (e.g., `f(f(a)) =
/// f(a)` is both an involution shape-match AND an idempotence
/// shape-match depending on the RHS — but they're mutually
/// exclusive structurally, so in practice each rule hits at most
/// one or two).
#[must_use]
pub fn classify_primitives(rule: &RewriteRule) -> Vec<MlPrimitive> {
    let mut out = Vec::new();

    if let Some(p) = detect_left_identity(rule) {
        out.push(p);
    }
    if let Some(p) = detect_right_identity(rule) {
        out.push(p);
    }
    if let Some(p) = detect_involution(rule) {
        out.push(p);
    }
    if let Some(p) = detect_idempotence(rule) {
        out.push(p);
    }
    // Delegate distributivity detection to R8's existing work.
    match crate::tensor::classify(rule) {
        crate::tensor::TensorShape::Distributive { outer, inner } => {
            out.push(MlPrimitive::LeftDistributive { outer, inner });
        }
        crate::tensor::TensorShape::MetaDistributive => {
            out.push(MlPrimitive::MetaDistributive);
        }
        crate::tensor::TensorShape::None => {}
    }
    // Right-distributive needs its own detector since R8's
    // classify sorts in canonical form and may not surface this
    // variant.
    if let Some(p) = detect_right_distributive(rule) {
        // Avoid double-counting if R8 already caught this shape
        // under LeftDistributive (which happens when outer is
        // commutative and canonical form unifies the two).
        if !out
            .iter()
            .any(|q| matches!(q, MlPrimitive::LeftDistributive { .. }))
        {
            out.push(p);
        }
    }
    if let Some(p) = detect_homomorphism(rule) {
        out.push(p);
    }

    out
}

/// Summary statistics for a library: how many rules exhibit each
/// primitive. Useful for traversal reports and experiment analysis.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrimitiveCensus {
    pub left_identity: usize,
    pub right_identity: usize,
    pub involution: usize,
    pub idempotence: usize,
    pub left_distributive: usize,
    pub right_distributive: usize,
    pub homomorphism: usize,
    pub meta_distributive: usize,
    pub total_rules: usize,
}

impl PrimitiveCensus {
    /// Total count of rules that exhibit any primitive (sum of
    /// individual counts; a rule matching two categories counts
    /// in both).
    #[must_use]
    pub fn any_primitive_hits(&self) -> usize {
        self.left_identity
            + self.right_identity
            + self.involution
            + self.idempotence
            + self.left_distributive
            + self.right_distributive
            + self.homomorphism
            + self.meta_distributive
    }
}

/// Census over a library.
#[must_use]
pub fn census(rules: &[RewriteRule]) -> PrimitiveCensus {
    let mut c = PrimitiveCensus {
        total_rules: rules.len(),
        ..Default::default()
    };
    for r in rules {
        for p in classify_primitives(r) {
            match p {
                MlPrimitive::LeftIdentity { .. } => c.left_identity += 1,
                MlPrimitive::RightIdentity { .. } => c.right_identity += 1,
                MlPrimitive::Involution { .. } => c.involution += 1,
                MlPrimitive::Idempotence { .. } => c.idempotence += 1,
                MlPrimitive::LeftDistributive { .. } => c.left_distributive += 1,
                MlPrimitive::RightDistributive { .. } => c.right_distributive += 1,
                MlPrimitive::Homomorphism { .. } => c.homomorphism += 1,
                MlPrimitive::MetaDistributive => c.meta_distributive += 1,
            }
        }
    }
    c
}

// ── Detectors ───────────────────────────────────────────────────

fn detect_left_identity(rule: &RewriteRule) -> Option<MlPrimitive> {
    // LHS: Apply(Var(op), [identity, Var(a)])
    // RHS: Var(a)
    let (op_id, args) = match_binary_apply_concrete_head(&rule.lhs)?;
    if args.len() != 2 {
        return None;
    }
    let a_var = match &args[1] {
        Term::Var(v) => *v,
        _ => return None,
    };
    // RHS must be exactly Var(a_var)
    match &rule.rhs {
        Term::Var(v) if *v == a_var => {}
        _ => return None,
    }
    // First arg is the identity element.
    let id = extract_identity_form(&args[0], a_var)?;
    Some(MlPrimitive::LeftIdentity {
        op: op_id,
        identity: id,
    })
}

fn detect_right_identity(rule: &RewriteRule) -> Option<MlPrimitive> {
    let (op_id, args) = match_binary_apply_concrete_head(&rule.lhs)?;
    if args.len() != 2 {
        return None;
    }
    let a_var = match &args[0] {
        Term::Var(v) => *v,
        _ => return None,
    };
    match &rule.rhs {
        Term::Var(v) if *v == a_var => {}
        _ => return None,
    }
    let id = extract_identity_form(&args[1], a_var)?;
    Some(MlPrimitive::RightIdentity {
        op: op_id,
        identity: id,
    })
}

fn detect_involution(rule: &RewriteRule) -> Option<MlPrimitive> {
    // LHS: Apply(Var(f), [Apply(Var(f), [Var(a)])])
    // RHS: Var(a)
    let (outer_f, outer_args) = match_unary_apply_concrete_head(&rule.lhs)?;
    let inner = &outer_args[0];
    let (inner_f, inner_args) = match_unary_apply_concrete_head(inner)?;
    if outer_f != inner_f {
        return None;
    }
    let a_var = match &inner_args[0] {
        Term::Var(v) => *v,
        _ => return None,
    };
    match &rule.rhs {
        Term::Var(v) if *v == a_var => {}
        _ => return None,
    }
    Some(MlPrimitive::Involution { f: outer_f })
}

fn detect_idempotence(rule: &RewriteRule) -> Option<MlPrimitive> {
    // LHS: Apply(Var(f), [Apply(Var(f), [Var(a)])])
    // RHS: Apply(Var(f), [Var(a)])
    let (outer_f, outer_args) = match_unary_apply_concrete_head(&rule.lhs)?;
    let inner = &outer_args[0];
    let (inner_f, inner_args) = match_unary_apply_concrete_head(inner)?;
    if outer_f != inner_f {
        return None;
    }
    let a_var = match &inner_args[0] {
        Term::Var(v) => *v,
        _ => return None,
    };
    // RHS: Apply(Var(outer_f), [Var(a_var)])
    let (rhs_f, rhs_args) = match_unary_apply_concrete_head(&rule.rhs)?;
    if rhs_f != outer_f {
        return None;
    }
    match &rhs_args[0] {
        Term::Var(v) if *v == a_var => {}
        _ => return None,
    }
    Some(MlPrimitive::Idempotence { f: outer_f })
}

fn detect_right_distributive(rule: &RewriteRule) -> Option<MlPrimitive> {
    // LHS: Apply(Var(outer), [Var(c), Apply(Var(inner), [Var(a), Var(b)])])
    // RHS: Apply(Var(inner), [Apply(Var(outer), [Var(c), Var(a)]),
    //                         Apply(Var(outer), [Var(c), Var(b)])])
    let (outer_id, outer_args) = match_binary_apply_concrete_head(&rule.lhs)?;
    if outer_args.len() != 2 {
        return None;
    }
    // First arg must be a Var, second a nested Apply.
    let c_var = match &outer_args[0] {
        Term::Var(v) => *v,
        _ => return None,
    };
    let (inner_id, inner_args) = match_binary_apply_concrete_head(&outer_args[1])?;
    if inner_id == outer_id {
        return None;
    }
    if inner_args.len() != 2 {
        return None;
    }
    let a_var = match &inner_args[0] {
        Term::Var(v) => *v,
        _ => return None,
    };
    let b_var = match &inner_args[1] {
        Term::Var(v) => *v,
        _ => return None,
    };
    if a_var == b_var || a_var == c_var || b_var == c_var {
        return None;
    }

    // RHS: inner(outer(c, a), outer(c, b))
    let (rhs_head, rhs_args) = match_binary_apply_concrete_head(&rule.rhs)?;
    if rhs_head != inner_id {
        return None;
    }
    if rhs_args.len() != 2 {
        return None;
    }
    let (out1, out1_args) = match_binary_apply_concrete_head(&rhs_args[0])?;
    let (out2, out2_args) = match_binary_apply_concrete_head(&rhs_args[1])?;
    if out1 != outer_id || out2 != outer_id {
        return None;
    }
    let p1 = two_vars(out1_args)?;
    let p2 = two_vars(out2_args)?;
    // Each pair must contain c_var and one of {a_var, b_var}.
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
    Some(MlPrimitive::RightDistributive {
        outer: outer_id,
        inner: inner_id,
    })
}

fn detect_homomorphism(rule: &RewriteRule) -> Option<MlPrimitive> {
    // LHS: Apply(Var(f), [Apply(Var(op), [Var(a), Var(b)])])
    // RHS: Apply(Var(op), [Apply(Var(f), [Var(a)]), Apply(Var(f), [Var(b)])])
    let (f_id, f_args) = match_unary_apply_concrete_head(&rule.lhs)?;
    let (op_id, op_args) = match_binary_apply_concrete_head(&f_args[0])?;
    if f_id == op_id {
        return None;
    }
    if op_args.len() != 2 {
        return None;
    }
    let a_var = match &op_args[0] {
        Term::Var(v) => *v,
        _ => return None,
    };
    let b_var = match &op_args[1] {
        Term::Var(v) => *v,
        _ => return None,
    };
    if a_var == b_var {
        return None;
    }

    // RHS: Apply(Var(op), [Apply(Var(f), [a]), Apply(Var(f), [b])])
    let (rhs_head, rhs_args) = match_binary_apply_concrete_head(&rule.rhs)?;
    if rhs_head != op_id {
        return None;
    }
    if rhs_args.len() != 2 {
        return None;
    }
    let (left_f, left_args) = match_unary_apply_concrete_head(&rhs_args[0])?;
    let (right_f, right_args) = match_unary_apply_concrete_head(&rhs_args[1])?;
    if left_f != f_id || right_f != f_id {
        return None;
    }
    let left_var = match &left_args[0] {
        Term::Var(v) => *v,
        _ => return None,
    };
    let right_var = match &right_args[0] {
        Term::Var(v) => *v,
        _ => return None,
    };
    // {left_var, right_var} must equal {a_var, b_var} as sets.
    let set = [left_var, right_var];
    if !(set.contains(&a_var) && set.contains(&b_var)) {
        return None;
    }
    Some(MlPrimitive::Homomorphism {
        f: f_id,
        op: op_id,
    })
}

// ── Helpers ─────────────────────────────────────────────────────

/// Match `Term::Apply(Term::Var(id), args)` with arity 2 and
/// concrete (non-op-var) head id. Returns (id, args).
fn match_binary_apply_concrete_head(t: &Term) -> Option<(u32, &Vec<Term>)> {
    match t {
        Term::Apply(head, args) if args.len() == 2 => match head.as_ref() {
            Term::Var(id) if *id < 100 => Some((*id, args)),
            _ => None,
        },
        _ => None,
    }
}

/// Match `Term::Apply(Term::Var(id), args)` with arity 1 and
/// concrete head id.
fn match_unary_apply_concrete_head(t: &Term) -> Option<(u32, &Vec<Term>)> {
    match t {
        Term::Apply(head, args) if args.len() == 1 => match head.as_ref() {
            Term::Var(id) if *id < 100 => Some((*id, args)),
            _ => None,
        },
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

/// Extract the identity element from a term. Numbers become
/// concrete identities; variables (other than the a_var already
/// used) become abstract identities.
fn extract_identity_form(t: &Term, a_var: u32) -> Option<IdentityForm> {
    match t {
        Term::Number(Value::Nat(n)) => Some(IdentityForm::Nat(*n)),
        Term::Number(Value::Int(n)) => Some(IdentityForm::Int(*n)),
        Term::Var(v) if *v != a_var => Some(IdentityForm::Abstract(*v)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{ADD, MUL, NEG, SUCC};

    fn var(id: u32) -> Term {
        Term::Var(id)
    }
    fn app(h: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(h), args)
    }
    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }

    fn rule(name: &str, lhs: Term, rhs: Term) -> RewriteRule {
        RewriteRule {
            name: name.into(),
            lhs,
            rhs,
        }
    }

    // ── Left-identity ───────────────────────────────────────────

    #[test]
    fn detects_add_left_identity_with_zero() {
        // 0 + a = a
        let r = rule(
            "add-left-id",
            app(var(ADD), vec![nat(0), var(100)]),
            var(100),
        );
        let ps = classify_primitives(&r);
        assert!(ps.contains(&MlPrimitive::LeftIdentity {
            op: ADD,
            identity: IdentityForm::Nat(0),
        }));
    }

    #[test]
    fn detects_mul_left_identity_with_one() {
        let r = rule(
            "mul-left-id",
            app(var(MUL), vec![nat(1), var(100)]),
            var(100),
        );
        let ps = classify_primitives(&r);
        assert!(ps.contains(&MlPrimitive::LeftIdentity {
            op: MUL,
            identity: IdentityForm::Nat(1),
        }));
    }

    #[test]
    fn detects_right_identity() {
        let r = rule(
            "add-right-id",
            app(var(ADD), vec![var(100), nat(0)]),
            var(100),
        );
        let ps = classify_primitives(&r);
        assert!(ps.contains(&MlPrimitive::RightIdentity {
            op: ADD,
            identity: IdentityForm::Nat(0),
        }));
    }

    #[test]
    fn abstract_identity_captured_as_abstract() {
        // op(?e, ?a) = ?a — the identity element itself is a
        // pattern variable. Classic meta-identity shape.
        let r = rule(
            "abstract-left-id",
            app(var(ADD), vec![var(200), var(100)]),
            var(100),
        );
        let ps = classify_primitives(&r);
        assert!(ps.contains(&MlPrimitive::LeftIdentity {
            op: ADD,
            identity: IdentityForm::Abstract(200),
        }));
    }

    // ── Involution ──────────────────────────────────────────────

    #[test]
    fn detects_neg_involution() {
        // neg(neg(a)) = a — the canonical involution.
        let r = rule(
            "neg-involution",
            app(var(NEG), vec![app(var(NEG), vec![var(100)])]),
            var(100),
        );
        let ps = classify_primitives(&r);
        assert!(ps.contains(&MlPrimitive::Involution { f: NEG }));
    }

    #[test]
    fn different_heads_not_involution() {
        // succ(neg(a)) = a — different heads, not involution.
        let r = rule(
            "not-involution",
            app(var(SUCC), vec![app(var(NEG), vec![var(100)])]),
            var(100),
        );
        let ps = classify_primitives(&r);
        assert!(!ps.iter().any(|p| matches!(p, MlPrimitive::Involution { .. })));
    }

    // ── Idempotence ─────────────────────────────────────────────

    #[test]
    fn detects_idempotence() {
        // f(f(a)) = f(a) — idempotent unary function. Using succ
        // as placeholder; succ isn't actually idempotent, but the
        // SHAPE-check passes.
        let r = rule(
            "fake-idem",
            app(var(SUCC), vec![app(var(SUCC), vec![var(100)])]),
            app(var(SUCC), vec![var(100)]),
        );
        let ps = classify_primitives(&r);
        assert!(ps.contains(&MlPrimitive::Idempotence { f: SUCC }));
    }

    // ── Distributivity ──────────────────────────────────────────

    #[test]
    fn detects_left_distributivity_via_tensor() {
        // mul(add(a, b), c) = add(mul(a, c), mul(b, c))
        let a = var(100);
        let b = var(101);
        let c = var(102);
        let r = rule(
            "left-distrib",
            app(var(MUL), vec![app(var(ADD), vec![a.clone(), b.clone()]), c.clone()]),
            app(
                var(ADD),
                vec![
                    app(var(MUL), vec![a.clone(), c.clone()]),
                    app(var(MUL), vec![b.clone(), c.clone()]),
                ],
            ),
        );
        let ps = classify_primitives(&r);
        assert!(ps.contains(&MlPrimitive::LeftDistributive {
            outer: MUL,
            inner: ADD,
        }));
    }

    #[test]
    fn detects_meta_distributive() {
        let f = var(200);
        let g = var(201);
        let a = var(100);
        let b = var(101);
        let c = var(102);
        let r = rule(
            "meta-distrib",
            app(f.clone(), vec![app(g.clone(), vec![a.clone(), b.clone()]), c.clone()]),
            app(
                g.clone(),
                vec![
                    app(f.clone(), vec![a.clone(), c.clone()]),
                    app(f.clone(), vec![b.clone(), c.clone()]),
                ],
            ),
        );
        let ps = classify_primitives(&r);
        assert!(ps.contains(&MlPrimitive::MetaDistributive));
    }

    // ── Homomorphism ────────────────────────────────────────────

    #[test]
    fn detects_homomorphism_succ_over_add() {
        // succ(add(a, b)) = add(succ(a), succ(b))
        // (This isn't TRUE for Peano succ — succ(a+b) = succ(a)+b
        // in general — but the shape detector only checks structure,
        // not correctness. Correctness is the prover's job.)
        let r = rule(
            "succ-hom-add",
            app(var(SUCC), vec![app(var(ADD), vec![var(100), var(101)])]),
            app(
                var(ADD),
                vec![
                    app(var(SUCC), vec![var(100)]),
                    app(var(SUCC), vec![var(101)]),
                ],
            ),
        );
        let ps = classify_primitives(&r);
        assert!(ps.contains(&MlPrimitive::Homomorphism {
            f: SUCC,
            op: ADD,
        }));
    }

    // ── Negative cases ──────────────────────────────────────────

    #[test]
    fn commutativity_is_not_detected_as_primitive() {
        // R12 catalog excludes commutativity / associativity
        // because the kernel already canonicalizes them out.
        // A commutativity rule would match no primitive here.
        let r = rule(
            "add-commute",
            app(var(ADD), vec![var(100), var(101)]),
            app(var(ADD), vec![var(101), var(100)]),
        );
        let ps = classify_primitives(&r);
        assert!(
            ps.is_empty(),
            "commutativity should not match any R12 primitive; got {ps:?}"
        );
    }

    #[test]
    fn arbitrary_projection_is_not_identity() {
        // op(a, b) = a — arbitrary projection, not identity.
        // The 'b' isn't an identity element; it's just a free var.
        let r = rule(
            "projection",
            app(var(ADD), vec![var(100), var(101)]),
            var(100),
        );
        let ps = classify_primitives(&r);
        // Oops — this MIGHT match left/right identity depending on
        // whether we consider abstract identities. With our
        // current IdentityForm::Abstract handling, it WOULD match.
        // That's a known limitation of shape-only detection: we
        // can't tell parametric identity from arbitrary projection
        // without additional semantic info.
        //
        // We test that AT LEAST ONE identity variant fires, as
        // documentation of the limitation.
        let has_id = ps.iter().any(|p| {
            matches!(
                p,
                MlPrimitive::LeftIdentity {
                    identity: IdentityForm::Abstract(_),
                    ..
                } | MlPrimitive::RightIdentity {
                    identity: IdentityForm::Abstract(_),
                    ..
                }
            )
        });
        // Left identity fires because the rule `op(?b, ?a) = ?a`
        // looks like parametric left-identity with ?b as abstract
        // identity. Shape-matching alone can't distinguish.
        assert!(has_id);
    }

    // ── Census ──────────────────────────────────────────────────

    #[test]
    fn census_aggregates_across_library() {
        let rules = vec![
            rule(
                "add-left-id",
                app(var(ADD), vec![nat(0), var(100)]),
                var(100),
            ),
            rule(
                "mul-right-id",
                app(var(MUL), vec![var(100), nat(1)]),
                var(100),
            ),
            rule(
                "neg-inv",
                app(var(NEG), vec![app(var(NEG), vec![var(100)])]),
                var(100),
            ),
        ];
        let c = census(&rules);
        assert_eq!(c.total_rules, 3);
        assert_eq!(c.left_identity, 1);
        assert_eq!(c.right_identity, 1);
        assert_eq!(c.involution, 1);
        // Any-primitive-hits counts each primitive occurrence.
        assert!(c.any_primitive_hits() >= 3);
    }

    #[test]
    fn empty_library_has_zero_primitives() {
        let c = census(&[]);
        assert_eq!(c.total_rules, 0);
        assert_eq!(c.any_primitive_hits(), 0);
    }
}
