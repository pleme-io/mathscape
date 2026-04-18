//! R14 — Autograd: symbolic gradient via rewrite rules.
//!
//! # The compute step up from R13
//!
//! R13 gave us tensors and element-wise ops. R14 gives us
//! DERIVATIVES — the second ingredient of gradient-based learning.
//!
//! `symbolic_derivative(expr, var)` takes an expression tree and
//! a variable id, and produces another expression tree
//! representing `d(expr)/d(var)`. The rules are the standard
//! calculus ones:
//!
//! ```text
//!   d(c)/dx         = 0                (constant rule)
//!   d(x)/dx         = 1                (identity)
//!   d(y)/dx         = 0     (y ≠ x)    (independent variable)
//!   d(a + b)/dx     = d(a)/dx + d(b)/dx        (sum rule)
//!   d(a * b)/dx     = d(a)/dx * b + a * d(b)/dx (product rule)
//!   d(-a)/dx        = -d(a)/dx                  (linearity of neg)
//!   d(succ(a))/dx   = d(a)/dx                   (succ is offset)
//! ```
//!
//! Output is Int-valued (`Value::Int`): 0, 1, and the
//! compositional products/sums. The caller can evaluate or
//! canonicalize the result to get the numeric derivative at a
//! point.
//!
//! # Why symbolic, not numeric
//!
//! Numeric gradients (finite differences) require float arithmetic
//! (`(f(x+h) - f(x)) / h`). Our substrate is integer-only until
//! we add `Value::Float`. Symbolic gradients work with integers:
//! the derivative of an integer-valued expression is itself an
//! integer-valued expression. The result can be evaluated at any
//! point with the existing kernel.
//!
//! # What this enables
//!
//! - `grad(w*x + b, w) = x` — the gradient of a linear predictor
//!   w.r.t. the weight parameter is the input
//! - `grad(loss, param) = gradient` — the full ingredient for
//!   gradient descent
//! - Composability: `grad(grad(f, x), x)` gives the second
//!   derivative symbolically
//!
//! # What this doesn't yet do
//!
//! - Vector-valued derivatives (Jacobians). `grad` of a tensor-
//!   valued expression w.r.t. a vector variable is a rank-2
//!   tensor; we handle only scalar-valued expressions.
//! - Tensor ops in gradient (tensor_dot, etc.): those need
//!   vector-Jacobian product rules. Added incrementally.
//! - Learning rate / optimizer step (that's R15).
//!
//! # Discovery path
//!
//! The rules here are hard-coded for USE. The machine can
//! INDEPENDENTLY DISCOVER these patterns given enough corpus
//! examples: corpora rich in `grad(f, x)` expressions would let
//! anti-unification produce `grad(add(a, b), ?x) => add(grad(a,
//! ?x), grad(b, ?x))` as a meta-rule. R14.1 provides corpus
//! generators that exercise these patterns so discovery has
//! material to work with.

use crate::builtin::{
    ADD, FLOAT_ADD, FLOAT_MUL, FLOAT_NEG, FLOAT_SUB, INT_ADD, INT_MUL, INT_SUCC,
    INT_ZERO, MUL, NEG, SUCC, TENSOR_ADD, TENSOR_DOT, TENSOR_MUL, TENSOR_NEG,
    TENSOR_SCALE, TENSOR_SUM, ZERO,
};
use crate::term::Term;
use crate::value::Value;

/// Compute the symbolic derivative of `expr` with respect to the
/// variable `var_id`. Returns an Int-valued Term.
///
/// Applies chain/sum/product rules recursively. Basic
/// simplifications (mul by 0, add of 0) are performed at
/// construction to keep the output term tree shallow; the caller
/// can apply `.canonical()` for further reduction.
#[must_use]
pub fn symbolic_derivative(expr: &Term, var_id: u32) -> Term {
    match expr {
        // Constant: derivative is 0. All Nat / Int / Tensor
        // numbers are constants w.r.t. any variable.
        Term::Number(_) => Term::Number(Value::Int(0)),
        // Point: opaque atom; treat as constant.
        Term::Point(_) => Term::Number(Value::Int(0)),
        // Variable: derivative is 1 if it matches var_id, else 0.
        Term::Var(v) => {
            if *v == var_id {
                Term::Number(Value::Int(1))
            } else {
                Term::Number(Value::Int(0))
            }
        }
        // Apply: dispatch on head.
        Term::Apply(head, args) => {
            let head_id = match head.as_ref() {
                Term::Var(id) => *id,
                _ => {
                    // Non-builtin head: can't differentiate;
                    // return 0. Future: could return an abstract
                    // d-expression.
                    return Term::Number(Value::Int(0));
                }
            };
            derive_apply(head_id, args, var_id)
        }
        // Fn / Symbol: return 0 for now — these would require
        // beta-reduction or library expansion before
        // differentiation. Future work.
        Term::Fn(_, _) | Term::Symbol(_, _) => Term::Number(Value::Int(0)),
    }
}

fn derive_apply(head_id: u32, args: &[Term], var_id: u32) -> Term {
    match head_id {
        // Sum rule: d(add(a, b))/dx = add(da, db)
        // Works for both Nat ADD and Int INT_ADD.
        ADD | INT_ADD => {
            if args.len() != 2 {
                return Term::Number(Value::Int(0));
            }
            let da = symbolic_derivative(&args[0], var_id);
            let db = symbolic_derivative(&args[1], var_id);
            simplify_add(da, db)
        }
        // Product rule: d(mul(a, b))/dx = add(mul(da, b), mul(a, db))
        MUL | INT_MUL => {
            if args.len() != 2 {
                return Term::Number(Value::Int(0));
            }
            let da = symbolic_derivative(&args[0], var_id);
            let db = symbolic_derivative(&args[1], var_id);
            let term_1 = simplify_mul(da, args[1].clone());
            let term_2 = simplify_mul(args[0].clone(), db);
            simplify_add(term_1, term_2)
        }
        // R18: Float analogs. Build derivative expressions using
        // FLOAT ops so eval reduces in the correct domain.
        FLOAT_ADD => {
            if args.len() != 2 {
                return float_zero();
            }
            let da = symbolic_derivative(&args[0], var_id);
            let db = symbolic_derivative(&args[1], var_id);
            simplify_float_add(da, db)
        }
        FLOAT_SUB => {
            if args.len() != 2 {
                return float_zero();
            }
            let da = symbolic_derivative(&args[0], var_id);
            let db = symbolic_derivative(&args[1], var_id);
            // d(a - b)/dx = d(a)/dx - d(b)/dx. Express as
            // float_sub(da, db).
            simplify_float_sub(da, db)
        }
        FLOAT_MUL => {
            if args.len() != 2 {
                return float_zero();
            }
            let da = symbolic_derivative(&args[0], var_id);
            let db = symbolic_derivative(&args[1], var_id);
            let term_1 = simplify_float_mul(da, args[1].clone());
            let term_2 = simplify_float_mul(args[0].clone(), db);
            simplify_float_add(term_1, term_2)
        }
        FLOAT_NEG => {
            if args.len() != 1 {
                return float_zero();
            }
            let da = symbolic_derivative(&args[0], var_id);
            simplify_float_neg(da)
        }
        // Neg: d(-a)/dx = -(da)
        NEG => {
            if args.len() != 1 {
                return Term::Number(Value::Int(0));
            }
            let da = symbolic_derivative(&args[0], var_id);
            simplify_neg(da)
        }
        // Succ: d(succ(a))/dx = d(a)/dx (succ is a += 1 offset)
        SUCC | INT_SUCC => {
            if args.len() != 1 {
                return Term::Number(Value::Int(0));
            }
            symbolic_derivative(&args[0], var_id)
        }
        // Zero / int_zero: nullary constant.
        ZERO | INT_ZERO => Term::Number(Value::Int(0)),
        // R17: Gradient through tensor ops.
        // Tensor-add is linear: d(A+B)/dx = dA/dx + dB/dx.
        TENSOR_ADD => {
            if args.len() != 2 {
                return Term::Number(Value::Int(0));
            }
            let da = symbolic_derivative(&args[0], var_id);
            let db = symbolic_derivative(&args[1], var_id);
            // Wrap in tensor_add if both derivatives are tensors;
            // else use scalar add since the derivative of a
            // tensor expression w.r.t. a scalar var that doesn't
            // appear reduces to 0 (the Int zero we emit as
            // default for constants).
            simplify_tensor_add(da, db)
        }
        // Tensor-mul is element-wise product; each element uses
        // the product rule: d(A*B)/dx = dA/dx * B + A * dB/dx.
        // Only valid when d/dx produces a tensor-shaped
        // derivative; for scalar vars that don't appear in A or
        // B, this collapses to 0.
        TENSOR_MUL => {
            if args.len() != 2 {
                return Term::Number(Value::Int(0));
            }
            let da = symbolic_derivative(&args[0], var_id);
            let db = symbolic_derivative(&args[1], var_id);
            let term_1 = simplify_tensor_mul(da, args[1].clone());
            let term_2 = simplify_tensor_mul(args[0].clone(), db);
            simplify_tensor_add(term_1, term_2)
        }
        // Tensor-neg is linear: d(-T)/dx = -(dT/dx).
        TENSOR_NEG => {
            if args.len() != 1 {
                return Term::Number(Value::Int(0));
            }
            let da = symbolic_derivative(&args[0], var_id);
            simplify_tensor_neg(da)
        }
        // Tensor-scale: d(c*T)/dx = c * dT/dx + dc/dx * T.
        // When c is a scalar and doesn't depend on x, dc/dx=0
        // and the second term vanishes. When c IS x (the var),
        // d(c*T)/dx = T (treating T as constant w.r.t. x).
        TENSOR_SCALE => {
            if args.len() != 2 {
                return Term::Number(Value::Int(0));
            }
            let dc = symbolic_derivative(&args[0], var_id);
            let dt = symbolic_derivative(&args[1], var_id);
            // scale(c, dT) + scale(dc, T)
            //    where the first requires dT be tensor-shaped
            //    (skip if dt == 0)
            //    where the second requires dc be a scalar Int
            let first = if is_int_zero(&dt) {
                Term::Number(Value::Int(0))
            } else {
                Term::Apply(
                    Box::new(Term::Var(TENSOR_SCALE)),
                    vec![args[0].clone(), dt],
                )
            };
            let second = if is_int_zero(&dc) {
                Term::Number(Value::Int(0))
            } else {
                Term::Apply(
                    Box::new(Term::Var(TENSOR_SCALE)),
                    vec![dc, args[1].clone()],
                )
            };
            simplify_tensor_add(first, second)
        }
        // Tensor-sum is linear reduction: d(sum(T))/dx = sum(dT/dx).
        TENSOR_SUM => {
            if args.len() != 1 {
                return Term::Number(Value::Int(0));
            }
            let dt = symbolic_derivative(&args[0], var_id);
            if is_int_zero(&dt) {
                return Term::Number(Value::Int(0));
            }
            Term::Apply(Box::new(Term::Var(TENSOR_SUM)), vec![dt])
        }
        // Tensor-dot: d(a·b)/dx = (da/dx)·b + a·(db/dx)
        // — standard chain rule on inner product, returns
        // scalar. If both args are independent of x, collapses.
        TENSOR_DOT => {
            if args.len() != 2 {
                return Term::Number(Value::Int(0));
            }
            let da = symbolic_derivative(&args[0], var_id);
            let db = symbolic_derivative(&args[1], var_id);
            let first = if is_int_zero(&da) {
                Term::Number(Value::Int(0))
            } else {
                Term::Apply(
                    Box::new(Term::Var(TENSOR_DOT)),
                    vec![da, args[1].clone()],
                )
            };
            let second = if is_int_zero(&db) {
                Term::Number(Value::Int(0))
            } else {
                Term::Apply(
                    Box::new(Term::Var(TENSOR_DOT)),
                    vec![args[0].clone(), db],
                )
            };
            simplify_add(first, second)
        }
        // Unknown builtin or not-yet-supported. Return 0; future
        // work adds specific rules.
        _ => Term::Number(Value::Int(0)),
    }
}

/// Simplify `tensor_add(a, b)` — fall back to scalar add when both
/// derivatives are scalar 0, otherwise wrap in tensor_add.
fn simplify_tensor_add(a: Term, b: Term) -> Term {
    if is_int_zero(&a) {
        return b;
    }
    if is_int_zero(&b) {
        return a;
    }
    // Both non-zero: wrap as tensor_add. If the inputs aren't
    // tensors, evaluation will fail at runtime — same as with
    // the scalar path. This is fine: the caller must use
    // tensor-differentiating ops consistently.
    Term::Apply(Box::new(Term::Var(TENSOR_ADD)), vec![a, b])
}

fn simplify_tensor_mul(a: Term, b: Term) -> Term {
    if is_int_zero(&a) || is_int_zero(&b) {
        return Term::Number(Value::Int(0));
    }
    Term::Apply(Box::new(Term::Var(TENSOR_MUL)), vec![a, b])
}

fn simplify_tensor_neg(a: Term) -> Term {
    if is_int_zero(&a) {
        return Term::Number(Value::Int(0));
    }
    // Double-negation: tensor_neg(tensor_neg(x)) = x
    if let Term::Apply(head, inner) = &a {
        if let Term::Var(TENSOR_NEG) = head.as_ref() {
            if inner.len() == 1 {
                return inner[0].clone();
            }
        }
    }
    Term::Apply(Box::new(Term::Var(TENSOR_NEG)), vec![a])
}

/// Simplify `add(a, b)` during derivative construction:
/// - If either side is Int(0), return the other side.
/// - Otherwise, build an Apply(INT_ADD, [a, b]).
fn simplify_add(a: Term, b: Term) -> Term {
    if is_int_zero(&a) {
        return b;
    }
    if is_int_zero(&b) {
        return a;
    }
    Term::Apply(Box::new(Term::Var(INT_ADD)), vec![a, b])
}

/// Simplify `mul(a, b)` during derivative construction:
/// - If either side is Int(0), return Int(0).
/// - If either side is Int(1), return the other side.
/// - Otherwise, build an Apply(INT_MUL, [a, b]).
fn simplify_mul(a: Term, b: Term) -> Term {
    if is_int_zero(&a) || is_int_zero(&b) {
        return Term::Number(Value::Int(0));
    }
    if is_int_one(&a) {
        return b;
    }
    if is_int_one(&b) {
        return a;
    }
    Term::Apply(Box::new(Term::Var(INT_MUL)), vec![a, b])
}

/// Simplify `neg(a)`:
/// - If `a` is already `neg(b)`, return `b` (double-negation
///   cancels).
/// - If `a` is Int(0), return Int(0).
/// - Otherwise, wrap.
fn simplify_neg(a: Term) -> Term {
    if is_int_zero(&a) {
        return Term::Number(Value::Int(0));
    }
    // Double-negation unwrap.
    if let Term::Apply(head, inner) = &a {
        if let Term::Var(NEG) = head.as_ref() {
            if inner.len() == 1 {
                return inner[0].clone();
            }
        }
    }
    Term::Apply(Box::new(Term::Var(NEG)), vec![a])
}

fn is_int_zero(t: &Term) -> bool {
    matches!(t, Term::Number(Value::Int(0)) | Term::Number(Value::Nat(0)))
}

fn is_int_one(t: &Term) -> bool {
    matches!(t, Term::Number(Value::Int(1)) | Term::Number(Value::Nat(1)))
}

// ── R18: Float-domain derivative helpers ─────────────────────────

fn float_zero() -> Term {
    Term::Number(Value::zero_float())
}

fn float_one() -> Term {
    Term::Number(Value::from_f64(1.0).unwrap())
}

fn is_float_zero(t: &Term) -> bool {
    match t {
        Term::Number(Value::Float(bits)) => f64::from_bits(*bits) == 0.0,
        _ => false,
    }
}

fn is_float_one(t: &Term) -> bool {
    match t {
        Term::Number(Value::Float(bits)) => f64::from_bits(*bits) == 1.0,
        _ => false,
    }
}

fn simplify_float_add(a: Term, b: Term) -> Term {
    if is_float_zero(&a) || is_int_zero(&a) {
        return b;
    }
    if is_float_zero(&b) || is_int_zero(&b) {
        return a;
    }
    Term::Apply(Box::new(Term::Var(FLOAT_ADD)), vec![a, b])
}

fn simplify_float_sub(a: Term, b: Term) -> Term {
    if is_float_zero(&b) || is_int_zero(&b) {
        return a;
    }
    Term::Apply(Box::new(Term::Var(FLOAT_SUB)), vec![a, b])
}

fn simplify_float_mul(a: Term, b: Term) -> Term {
    if is_float_zero(&a) || is_float_zero(&b) || is_int_zero(&a) || is_int_zero(&b)
    {
        return float_zero();
    }
    if is_float_one(&a) || is_int_one(&a) {
        return b;
    }
    if is_float_one(&b) || is_int_one(&b) {
        return a;
    }
    Term::Apply(Box::new(Term::Var(FLOAT_MUL)), vec![a, b])
}

fn simplify_float_neg(a: Term) -> Term {
    if is_float_zero(&a) || is_int_zero(&a) {
        return float_zero();
    }
    if let Term::Apply(head, inner) = &a {
        if let Term::Var(FLOAT_NEG) = head.as_ref() {
            if inner.len() == 1 {
                return inner[0].clone();
            }
        }
    }
    Term::Apply(Box::new(Term::Var(FLOAT_NEG)), vec![a])
}

/// R18: Float-aware symbolic derivative.
///
/// `symbolic_derivative` was designed for Int/Nat expressions and
/// emits Int(0) / Int(1) constants throughout. For expressions
/// that use FLOAT_* operators, those Int constants don't mix
/// cleanly at eval time (cross-domain rejection).
///
/// This variant mirrors `symbolic_derivative` but emits Float
/// constants and uses FLOAT_* operators internally. The caller
/// chooses which to use based on the expression's domain.
#[must_use]
pub fn symbolic_derivative_float(expr: &Term, var_id: u32) -> Term {
    match expr {
        Term::Number(_) | Term::Point(_) => float_zero(),
        Term::Var(v) => {
            if *v == var_id {
                float_one()
            } else {
                float_zero()
            }
        }
        Term::Apply(head, args) => {
            let head_id = match head.as_ref() {
                Term::Var(id) => *id,
                _ => return float_zero(),
            };
            match head_id {
                FLOAT_ADD => {
                    if args.len() != 2 {
                        return float_zero();
                    }
                    let da = symbolic_derivative_float(&args[0], var_id);
                    let db = symbolic_derivative_float(&args[1], var_id);
                    simplify_float_add(da, db)
                }
                FLOAT_SUB => {
                    if args.len() != 2 {
                        return float_zero();
                    }
                    let da = symbolic_derivative_float(&args[0], var_id);
                    let db = symbolic_derivative_float(&args[1], var_id);
                    simplify_float_sub(da, db)
                }
                FLOAT_MUL => {
                    if args.len() != 2 {
                        return float_zero();
                    }
                    let da = symbolic_derivative_float(&args[0], var_id);
                    let db = symbolic_derivative_float(&args[1], var_id);
                    let t1 = simplify_float_mul(da, args[1].clone());
                    let t2 = simplify_float_mul(args[0].clone(), db);
                    simplify_float_add(t1, t2)
                }
                FLOAT_NEG => {
                    if args.len() != 1 {
                        return float_zero();
                    }
                    let da = symbolic_derivative_float(&args[0], var_id);
                    simplify_float_neg(da)
                }
                _ => float_zero(),
            }
        }
        Term::Fn(_, _) | Term::Symbol(_, _) => float_zero(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(id: u32) -> Term {
        Term::Var(id)
    }
    fn apply(head: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(head), args)
    }
    fn int_(n: i64) -> Term {
        Term::Number(Value::Int(n))
    }

    #[test]
    fn derivative_of_constant_is_zero() {
        assert_eq!(symbolic_derivative(&int_(5), 100), int_(0));
        assert_eq!(
            symbolic_derivative(&Term::Number(Value::Nat(7)), 100),
            int_(0)
        );
    }

    #[test]
    fn derivative_of_self_variable_is_one() {
        // d(x)/dx = 1
        assert_eq!(symbolic_derivative(&var(100), 100), int_(1));
    }

    #[test]
    fn derivative_of_other_variable_is_zero() {
        // d(y)/dx = 0
        assert_eq!(symbolic_derivative(&var(101), 100), int_(0));
    }

    #[test]
    fn sum_rule_d_add_ab() {
        // d(add(x, y))/dx = add(1, 0) = 1 (via simplify_add)
        let expr = apply(var(INT_ADD), vec![var(100), var(101)]);
        let d = symbolic_derivative(&expr, 100);
        assert_eq!(d, int_(1));
    }

    #[test]
    fn product_rule_d_mul_xy_dx_is_y() {
        // d(mul(x, y))/dx = add(mul(1, y), mul(x, 0))
        //                 = y  (via simplify)
        let expr = apply(var(INT_MUL), vec![var(100), var(101)]);
        let d = symbolic_derivative(&expr, 100);
        assert_eq!(d, var(101));
    }

    #[test]
    fn product_rule_d_mul_xy_dy_is_x() {
        // d(mul(x, y))/dy = x
        let expr = apply(var(INT_MUL), vec![var(100), var(101)]);
        let d = symbolic_derivative(&expr, 101);
        assert_eq!(d, var(100));
    }

    #[test]
    fn chain_rule_via_composition() {
        // d(mul(add(x, y), z))/dx
        //   = d(add(x,y))/dx * z + add(x,y) * d(z)/dx
        //   = 1 * z + add(x,y) * 0
        //   = z  (via simplify)
        let inner = apply(var(INT_ADD), vec![var(100), var(101)]);
        let outer = apply(var(INT_MUL), vec![inner, var(102)]);
        let d = symbolic_derivative(&outer, 100);
        assert_eq!(d, var(102));
    }

    #[test]
    fn negation_flips_derivative() {
        // d(-x)/dx = -1
        let expr = apply(var(NEG), vec![var(100)]);
        let d = symbolic_derivative(&expr, 100);
        assert_eq!(d, apply(var(NEG), vec![int_(1)]));
    }

    #[test]
    fn double_negation_cancels_in_derivative() {
        // d(-(-x))/dx = -(-(1)) → double-neg unwrap → 1
        let expr = apply(
            var(NEG),
            vec![apply(var(NEG), vec![var(100)])],
        );
        let d = symbolic_derivative(&expr, 100);
        assert_eq!(d, int_(1));
    }

    #[test]
    fn succ_is_transparent_under_derivative() {
        // d(succ(x))/dx = d(x)/dx = 1 (succ is just +1 offset)
        let expr = apply(var(SUCC), vec![var(100)]);
        let d = symbolic_derivative(&expr, 100);
        assert_eq!(d, int_(1));
    }

    // ── PROOF: gradient evaluated at a point ─────────────────────

    #[test]
    fn proof_linear_regression_gradient_at_point() {
        // Linear predictor: y = w*x + b.
        // Gradient w.r.t. w: d(y)/dw = x
        // Evaluate that symbolic gradient at x=7 → should be 7.
        //
        // Proves: symbolic gradient composes with concrete
        // evaluation. We derive the expression, then substitute
        // the concrete x, and the kernel eval machinery reduces
        // it to the correct gradient value.
        use crate::eval::eval;
        let w = var(100);
        let x = var(101);
        let b = var(102);
        let mul_wx = apply(var(INT_MUL), vec![w.clone(), x.clone()]);
        let y = apply(var(INT_ADD), vec![mul_wx, b]);

        // Symbolic gradient w.r.t. w.
        let dy_dw = symbolic_derivative(&y, 100);
        // It should be exactly `x` (Var(101)).
        assert_eq!(dy_dw, x);

        // Substitute x=7 and evaluate.
        let grounded = dy_dw.substitute(101, &int_(7));
        let v = eval(&grounded, &[], 100).unwrap();
        assert_eq!(v, int_(7));
    }

    #[test]
    fn proof_gradient_of_scalar_polynomial() {
        // f(x) = x*x + 3*x + 5
        // df/dx = 2x + 3
        //
        // Symbolically:
        //   d(x*x)/dx     = add(mul(1, x), mul(x, 1)) → add(x, x)
        //                   (no further simplification — we don't
        //                   collapse x+x to 2*x without library help)
        //   d(3*x)/dx     = add(mul(0, x), mul(3, 1)) → 3
        //   d(5)/dx       = 0
        //   total         = add(add(x, x), 3)
        //
        // Evaluate at x=4:
        //   (4 + 4) + 3 = 11
        //   Ground truth: 2*4 + 3 = 11 ✓
        use crate::eval::eval;
        let x_sq = apply(var(INT_MUL), vec![var(100), var(100)]);
        let three_x = apply(var(INT_MUL), vec![int_(3), var(100)]);
        let five = int_(5);
        let poly = apply(
            var(INT_ADD),
            vec![
                apply(var(INT_ADD), vec![x_sq, three_x]),
                five,
            ],
        );

        let dpoly = symbolic_derivative(&poly, 100);
        // Substitute x=4 and evaluate.
        let grounded = dpoly.substitute(100, &int_(4));
        let v = eval(&grounded, &[], 100).unwrap();
        assert_eq!(v, int_(11));
    }

    #[test]
    fn proof_second_derivative_is_recursive_grad() {
        // f(x) = x*x*x
        // df/dx = 3x²
        // d²f/dx² = 6x
        //
        // Compose symbolic_derivative twice:
        //   d/dx (x*x*x) = 3*x²  (symbolically, not reduced)
        //   d/dx (previous) = 6x
        // Evaluate at x=2:
        //   d²f/dx² = 12
        use crate::eval::eval;
        let x = var(100);
        // Build x*x*x as mul(mul(x, x), x).
        let x_sq = apply(var(INT_MUL), vec![x.clone(), x.clone()]);
        let x_cube = apply(var(INT_MUL), vec![x_sq, x.clone()]);

        let d1 = symbolic_derivative(&x_cube, 100);
        let d2 = symbolic_derivative(&d1, 100);
        let grounded = d2.substitute(100, &int_(2));
        let v = eval(&grounded, &[], 200).unwrap();
        assert_eq!(v, int_(12));
    }

    // ── R17: gradient flow through tensor ops ────────────────────

    fn t_val(shape: Vec<usize>, data: Vec<i64>) -> Term {
        Term::Number(Value::tensor(shape, data).unwrap())
    }

    #[test]
    fn tensor_add_constant_has_zero_gradient() {
        // d(tensor_add(T1, T2))/dx where x doesn't appear in
        // either tensor — gradient is 0.
        let expr = apply(
            Term::Var(crate::builtin::TENSOR_ADD),
            vec![
                t_val(vec![2], vec![1, 2]),
                t_val(vec![2], vec![3, 4]),
            ],
        );
        let d = symbolic_derivative(&expr, 100);
        assert_eq!(d, int_(0));
    }

    #[test]
    fn tensor_sum_is_linear_over_gradient() {
        // f(x) = tensor_sum(tensor_scale(x, T)) where T is constant.
        // df/dx = tensor_sum(T)  (gradient passes through sum linearly)
        //
        // For T = [1, 2, 3]:
        //   f(x) = x*1 + x*2 + x*3 = 6x
        //   df/dx = 6 = sum([1, 2, 3])
        use crate::eval::eval;
        let x = var(100);
        let t_const = t_val(vec![3], vec![1, 2, 3]);
        let scaled = apply(
            Term::Var(crate::builtin::TENSOR_SCALE),
            vec![x, t_const],
        );
        let summed = apply(
            Term::Var(crate::builtin::TENSOR_SUM),
            vec![scaled],
        );

        let d = symbolic_derivative(&summed, 100);
        // Evaluate the gradient expression.
        let v = eval(&d, &[], 100).unwrap();
        assert_eq!(v, int_(6));
    }

    #[test]
    fn tensor_dot_product_rule() {
        // f(x) = tensor_dot(tensor_scale(x, A), B)
        // With A=[1,2,3], B=[4,5,6]: f(x) = x*(1*4+2*5+3*6) = 32x
        // df/dx = 32
        use crate::eval::eval;
        let x = var(100);
        let a = t_val(vec![3], vec![1, 2, 3]);
        let b = t_val(vec![3], vec![4, 5, 6]);
        let scaled_a = apply(
            Term::Var(crate::builtin::TENSOR_SCALE),
            vec![x, a],
        );
        let dot_expr = apply(
            Term::Var(crate::builtin::TENSOR_DOT),
            vec![scaled_a, b],
        );

        let d = symbolic_derivative(&dot_expr, 100);
        let v = eval(&d, &[], 100).unwrap();
        assert_eq!(v, int_(32));
    }

    #[test]
    fn proof_linear_regression_gradient_via_tensor_dot() {
        // Model: y_hat = dot(w, x)  where w is a learnable
        // weight tensor, x is input, both 1D with length 3.
        //
        // Even though w is a tensor, we're taking gradient w.r.t.
        // the SCALAR elements we've modeled via composition.
        //
        // Actually for this test: use f(s) = dot(scale(s, w), x)
        // which is s * dot(w, x). With w=[1,2,3], x=[4,5,6]:
        //   f(s) = s * 32
        //   df/ds = 32
        //
        // Proves: gradient flows through both scale and dot.
        use crate::eval::eval;
        let s = var(100);
        let w = t_val(vec![3], vec![1, 2, 3]);
        let x = t_val(vec![3], vec![4, 5, 6]);
        let scaled_w = apply(
            Term::Var(crate::builtin::TENSOR_SCALE),
            vec![s, w],
        );
        let y_hat = apply(
            Term::Var(crate::builtin::TENSOR_DOT),
            vec![scaled_w, x],
        );

        let dy_ds = symbolic_derivative(&y_hat, 100);
        let v = eval(&dy_ds, &[], 100).unwrap();
        assert_eq!(v, int_(32));
    }

    #[test]
    fn proof_mse_gradient_through_tensor_ops() {
        // Loss function style: sum((a*v + b)²) where v is a
        // constant tensor and a, b are scalars.
        //
        // With v=[1,2,3], at a=0, b=1:
        //   a*v + b = [0+1, 0+1, 0+1] using broadcast... but we
        //   don't have scalar-tensor broadcast. So let's use a
        //   form that works with our primitives:
        //
        // f(a) = sum(scale(a, v) * scale(a, v))
        //      = sum(a² v²) = a² * sum(v²)
        //      = a² * (1 + 4 + 9) = 14 a²
        // df/da = 28a
        // At a=3: df/da = 84
        use crate::eval::eval;
        let a = var(100);
        let v = t_val(vec![3], vec![1, 2, 3]);
        let av = apply(
            Term::Var(crate::builtin::TENSOR_SCALE),
            vec![a.clone(), v.clone()],
        );
        let av2 = apply(
            Term::Var(crate::builtin::TENSOR_MUL),
            vec![av.clone(), av],
        );
        let loss = apply(
            Term::Var(crate::builtin::TENSOR_SUM),
            vec![av2],
        );

        // Symbolic gradient w.r.t. a.
        let grad = symbolic_derivative(&loss, 100);
        // Substitute a=3 and evaluate.
        let g = grad.substitute(100, &int_(3));
        let v_eval = eval(&g, &[], 200).unwrap();
        assert_eq!(v_eval, int_(84));
    }

    // ── R18: Float-domain autograd proofs ────────────────────────

    fn f(v: f64) -> Term {
        Term::Number(Value::from_f64(v).unwrap())
    }

    #[test]
    fn float_derivative_of_self_is_one() {
        assert_eq!(symbolic_derivative_float(&var(100), 100), f(1.0));
    }

    #[test]
    fn float_derivative_of_constant_is_zero() {
        assert_eq!(symbolic_derivative_float(&f(3.14), 100), f(0.0));
    }

    #[test]
    fn float_sum_rule() {
        // d(a + b)/da at a=100 = 1
        let expr = apply(
            Term::Var(crate::builtin::FLOAT_ADD),
            vec![var(100), var(101)],
        );
        assert_eq!(symbolic_derivative_float(&expr, 100), f(1.0));
    }

    #[test]
    fn float_product_rule() {
        // d(x * y)/dx = y
        let expr = apply(
            Term::Var(crate::builtin::FLOAT_MUL),
            vec![var(100), var(101)],
        );
        assert_eq!(symbolic_derivative_float(&expr, 100), var(101));
    }

    #[test]
    fn proof_float_autograd_sgd_end_to_end() {
        // Full training loop proof: use symbolic_derivative_float
        // to compute the gradient of a loss function, then apply
        // SGD. Verify convergence.
        //
        // Problem: minimize L(w) = (w - target)² via SGD.
        //   L(w)  = (w - 5.0)²
        //   L'(w) = 2(w - 5.0)
        //
        // We build the loss expression once, derive it once
        // symbolically, then in each step substitute the current
        // w and evaluate, apply SGD.
        use crate::eval::eval;
        let target_f = 5.0_f64;
        let lr_f = 0.1_f64;

        // Loss: float_mul(float_sub(w, target), float_sub(w, target))
        let w_var = var(100);
        let diff = apply(
            Term::Var(crate::builtin::FLOAT_SUB),
            vec![w_var.clone(), f(target_f)],
        );
        let loss = apply(
            Term::Var(crate::builtin::FLOAT_MUL),
            vec![diff.clone(), diff],
        );

        // Symbolic gradient.
        let dloss_dw = symbolic_derivative_float(&loss, 100);

        // Training loop: 50 iterations.
        let mut w: f64 = 0.0;
        for _ in 0..50 {
            // Substitute current w into gradient expression.
            let g_expr = dloss_dw.substitute(100, &f(w));
            let g_term = eval(&g_expr, &[], 500).unwrap();
            let g = g_term.as_float_val().unwrap();

            // SGD update: w = w - lr * g
            w -= lr_f * g;
        }

        // Should converge near target.
        assert!(
            (w - target_f).abs() < 0.001,
            "after 50 float-autograd SGD steps, w={w}, target={target_f}"
        );
    }

    #[test]
    fn proof_gradient_flow_through_mixed_expression() {
        // f(w, x, b) = (w * x + b) * x
        // df/dw = x * x
        //
        // At w=1 (doesn't matter since not in gradient), x=5, b=3:
        // df/dw should evaluate to 25.
        use crate::eval::eval;
        let w = var(100);
        let x = var(101);
        let b = var(102);
        let wx = apply(var(INT_MUL), vec![w, x.clone()]);
        let wx_b = apply(var(INT_ADD), vec![wx, b]);
        let y = apply(var(INT_MUL), vec![wx_b, x]);

        let df_dw = symbolic_derivative(&y, 100);
        // Substitute numeric values.
        let g = df_dw
            .substitute(101, &int_(5))
            .substitute(102, &int_(3));
        let v = eval(&g, &[], 200).unwrap();
        assert_eq!(v, int_(25));
    }
}
