//! R5 — Builtin operator registry.
//!
//! The kernel reduction that eliminates magic-number operator ids.
//! Every property of a builtin (name, arity, commutativity,
//! associativity, evaluation rule) lives in ONE place — the
//! BUILTINS table. Before R5, these properties were scattered:
//! eval.rs had match arms on `BUILTIN_ADD = 2`, term.rs had
//! `is_commutative_op` with its own `AC_BUILTIN_ADD = 2`, and
//! downstream crates had their own magic-number matches. The
//! single source of truth eliminates drift between these
//! representations.
//!
//! # The Builtin contract
//!
//! Each builtin declares:
//!   - `id`: the u32 `Var(id)` that invokes it in a Term
//!   - `name`: human-readable name (for pretty-printing + Lisp)
//!   - `arity`: number of arguments required
//!   - `commutative`: args can be sorted without changing meaning
//!   - `associative`: nested same-op calls can be flattened
//!   - `eval`: a pure function from args to reduced Term, or
//!             None if args aren't yet reduced enough
//!
//! # Adding a new builtin
//!
//! Append a `Builtin` struct to the `BUILTINS` constant. Every
//! call site that consults the registry picks up the new op
//! automatically — evaluator, canonicalizer, any future property
//! lookup. No magic-number edits required.
//!
//! # Future: machine-proposed builtins
//!
//! When the machine proposes a new operator (e.g., `sub`) via
//! Lisp, the proposal is a builtin spec in Sexp form. If
//! validated, it's instantiated as a Builtin entry in a runtime
//! extension of the registry. The static BUILTINS table stays as
//! the bootstrap; the runtime builtins grow via the machine's own
//! discovery. R5 is the infrastructure; the discovery mechanism
//! for new builtins is ML5+.

use crate::term::Term;
use crate::value::Value;

// Human-readable aliases for the canonical builtin ids. These are
// re-exported so tests and user code don't litter magic numbers.
// If you're constructing a term like `apply(Var(2), ...)`, prefer
// `apply(Var(ADD), ...)`.
//
// Nat domain (0..=3):
pub const ZERO: u32 = 0;
pub const SUCC: u32 = 1;
pub const ADD: u32 = 2;
pub const MUL: u32 = 3;

// Int domain (10..=14) — R7 (2026-04-18). Disjoint from Nat:
// INT_ADD rejects Nat args, ADD rejects Int args. The gap
// 4..=9 is reserved for future Nat-domain extensions (e.g. pow,
// sub-with-saturation).
pub const INT_ZERO: u32 = 10;
pub const INT_SUCC: u32 = 11;
pub const INT_ADD: u32 = 12;
pub const INT_MUL: u32 = 13;
pub const NEG: u32 = 14;

// Tensor domain (20..=26) — R13 (2026-04-18). The compute-layer
// primitives: element-wise ops over Value::Tensor, reductions
// (sum, dot). All reject non-Tensor args — no implicit
// broadcasting between scalars and tensors at the kernel level.
//
// The gap 15..=19 is reserved for future unary ops (abs, square,
// etc.) that might belong to either Int or Tensor domain.
pub const TENSOR_ADD: u32 = 20;
pub const TENSOR_MUL: u32 = 21;
pub const TENSOR_SUM: u32 = 22;
pub const TENSOR_DOT: u32 = 23;
pub const TENSOR_NEG: u32 = 24;
pub const TENSOR_SCALE: u32 = 25;

// R16 (2026-04-18): 2D tensor ops. Complete the compute-layer
// width with the primitives linear-algebra depends on.
pub const TENSOR_MATMUL: u32 = 26;
pub const TENSOR_TRANSPOSE: u32 = 27;
pub const TENSOR_RESHAPE: u32 = 28;

// Float domain (30..=36) — R18 (2026-04-18). IEEE 754 doubles
// enable real gradient descent convergence. All eval rules
// reject non-Float args; overflow/underflow to non-finite
// returns None (preserves kernel "true" invariant — non-finite
// is not a valid value).
pub const FLOAT_ZERO: u32 = 30;
pub const FLOAT_ADD: u32 = 31;
pub const FLOAT_MUL: u32 = 32;
pub const FLOAT_NEG: u32 = 33;
pub const FLOAT_DIV: u32 = 34;
pub const FLOAT_FROM_INT: u32 = 35;
pub const FLOAT_SUB: u32 = 36;

// Float Tensor domain (40..=48) — R19 (2026-04-18). Enables
// real-valued parametric models. Elementwise ops mirror the
// integer Tensor domain (R13) but preserve float precision.
pub const FT_ADD: u32 = 40;
pub const FT_MUL: u32 = 41;
pub const FT_NEG: u32 = 42;
pub const FT_SUM: u32 = 43;
pub const FT_DOT: u32 = 44;
pub const FT_SCALE: u32 = 45;
pub const FT_SUB: u32 = 46;
pub const FT_MATMUL: u32 = 47;

/// A builtin operator — the atomic primitives the evaluator knows
/// how to reduce directly. Fields are the declaration; `eval` is
/// the reduction rule.
#[derive(Clone, Copy, Debug)]
pub struct Builtin {
    pub id: u32,
    pub name: &'static str,
    pub arity: usize,
    pub commutative: bool,
    pub associative: bool,
    /// Reduction: given args that may or may not be fully
    /// reduced, produce the next-step result. Return None when
    /// args aren't yet in the shape the builtin needs (e.g., not
    /// yet reduced to Numbers).
    pub eval: fn(&[Term]) -> Option<Term>,
}

fn eval_zero(_args: &[Term]) -> Option<Term> {
    Some(Term::Number(Value::Nat(0)))
}

fn eval_succ(args: &[Term]) -> Option<Term> {
    if args.len() != 1 {
        return None;
    }
    if let Term::Number(v) = &args[0] {
        Some(Term::Number(v.succ()))
    } else {
        None
    }
}

fn eval_add(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    if let (Term::Number(Value::Nat(a)), Term::Number(Value::Nat(b))) =
        (&args[0], &args[1])
    {
        Some(Term::Number(Value::Nat(a + b)))
    } else {
        None
    }
}

fn eval_mul(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    // Pure-arithmetic kernel only. Symbolic simplification (zero-
    // absorber, identity) remains the motor's job to DISCOVER as
    // rules — not something the kernel short-circuits. The
    // `simplify_mul_of` machinery in autograd.rs stays used by
    // the derivative path; wiring it into eval would let the
    // kernel solve what the motor should be solving.
    if let (Term::Number(Value::Nat(a)), Term::Number(Value::Nat(b))) =
        (&args[0], &args[1])
    {
        Some(Term::Number(Value::Nat(a * b)))
    } else {
        None
    }
}

// ── R7 (2026-04-18): Int-domain eval rules ────────────────────────
//
// Int builtins are strictly domain-disjoint from Nat: each rejects
// cross-domain args. The machine can't silently promote Nat to Int
// — if a theorem involves both domains it must discover an explicit
// promotion operator itself.

fn eval_int_zero(_args: &[Term]) -> Option<Term> {
    Some(Term::Number(Value::Int(0)))
}

fn eval_int_succ(args: &[Term]) -> Option<Term> {
    if args.len() != 1 {
        return None;
    }
    if let Term::Number(Value::Int(n)) = &args[0] {
        Some(Term::Number(Value::Int(n + 1)))
    } else {
        None
    }
}

fn eval_int_add(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    if let (Term::Number(Value::Int(a)), Term::Number(Value::Int(b))) =
        (&args[0], &args[1])
    {
        // Int is signed i64; use checked_add so overflow produces
        // None rather than panicking in debug / wrapping in release.
        // The kernel invariant "true" demands correctness: an
        // overflow is NOT the right answer.
        a.checked_add(*b).map(|v| Term::Number(Value::Int(v)))
    } else {
        None
    }
}

fn eval_int_mul(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    if let (Term::Number(Value::Int(a)), Term::Number(Value::Int(b))) =
        (&args[0], &args[1])
    {
        a.checked_mul(*b).map(|v| Term::Number(Value::Int(v)))
    } else {
        None
    }
}

fn eval_neg(args: &[Term]) -> Option<Term> {
    if args.len() != 1 {
        return None;
    }
    if let Term::Number(Value::Int(n)) = &args[0] {
        n.checked_neg().map(|v| Term::Number(Value::Int(v)))
    } else {
        None
    }
}

// ── R13 (2026-04-18): Tensor-domain eval rules ────────────────────
//
// Tensor builtins operate on Value::Tensor exclusively. Shape
// mismatch returns None. Element-wise ops check both args are
// tensors with identical shape. Reductions consume a tensor
// and produce a scalar. Overflow uses checked arithmetic — same
// truthfulness invariant as R7's Int ops.

fn eval_tensor_add(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    let (sa, da) = args[0].as_tensor_val()?;
    let (sb, db) = args[1].as_tensor_val()?;
    if sa != sb {
        return None;
    }
    let mut out = Vec::with_capacity(da.len());
    for (a, b) in da.iter().zip(db.iter()) {
        out.push(a.checked_add(*b)?);
    }
    Some(Term::Number(Value::Tensor {
        shape: sa.to_vec(),
        data: out,
    }))
}

fn eval_tensor_mul(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    let (sa, da) = args[0].as_tensor_val()?;
    let (sb, db) = args[1].as_tensor_val()?;
    if sa != sb {
        return None;
    }
    let mut out = Vec::with_capacity(da.len());
    for (a, b) in da.iter().zip(db.iter()) {
        out.push(a.checked_mul(*b)?);
    }
    Some(Term::Number(Value::Tensor {
        shape: sa.to_vec(),
        data: out,
    }))
}

fn eval_tensor_sum(args: &[Term]) -> Option<Term> {
    if args.len() != 1 {
        return None;
    }
    let (_shape, data) = args[0].as_tensor_val()?;
    let mut acc: i64 = 0;
    for d in data {
        acc = acc.checked_add(*d)?;
    }
    Some(Term::Number(Value::Int(acc)))
}

fn eval_tensor_dot(args: &[Term]) -> Option<Term> {
    // 1D tensors only. `dot([a_1 ... a_n], [b_1 ... b_n]) =
    // sum_i(a_i * b_i)`. Returns Int scalar.
    if args.len() != 2 {
        return None;
    }
    let (sa, da) = args[0].as_tensor_val()?;
    let (sb, db) = args[1].as_tensor_val()?;
    // Must be 1D with matching length.
    if sa.len() != 1 || sb.len() != 1 || sa != sb {
        return None;
    }
    let mut acc: i64 = 0;
    for (a, b) in da.iter().zip(db.iter()) {
        let p = a.checked_mul(*b)?;
        acc = acc.checked_add(p)?;
    }
    Some(Term::Number(Value::Int(acc)))
}

fn eval_tensor_neg(args: &[Term]) -> Option<Term> {
    if args.len() != 1 {
        return None;
    }
    let (shape, data) = args[0].as_tensor_val()?;
    let mut out = Vec::with_capacity(data.len());
    for d in data {
        out.push(d.checked_neg()?);
    }
    Some(Term::Number(Value::Tensor {
        shape: shape.to_vec(),
        data: out,
    }))
}

// ── R16 (2026-04-18): 2D tensor ops ───────────────────────────────

fn eval_tensor_matmul(args: &[Term]) -> Option<Term> {
    // 2D matrix multiplication: A[m,n] @ B[n,p] = C[m,p].
    // Returns None on non-2D shapes or mismatched inner dim.
    if args.len() != 2 {
        return None;
    }
    let (sa, da) = args[0].as_tensor_val()?;
    let (sb, db) = args[1].as_tensor_val()?;
    if sa.len() != 2 || sb.len() != 2 {
        return None;
    }
    let (m, n_a) = (sa[0], sa[1]);
    let (n_b, p) = (sb[0], sb[1]);
    if n_a != n_b {
        return None;
    }
    let n = n_a;
    let mut out = vec![0i64; m * p];
    // C[i, j] = sum_k A[i, k] * B[k, j]
    for i in 0..m {
        for j in 0..p {
            let mut acc: i64 = 0;
            for k in 0..n {
                let a_ik = da[i * n + k];
                let b_kj = db[k * p + j];
                let prod = a_ik.checked_mul(b_kj)?;
                acc = acc.checked_add(prod)?;
            }
            out[i * p + j] = acc;
        }
    }
    Some(Term::Number(Value::Tensor {
        shape: vec![m, p],
        data: out,
    }))
}

fn eval_tensor_transpose(args: &[Term]) -> Option<Term> {
    // 2D transpose: T[i, j] → T^T[j, i]. Shape [m, n] → [n, m].
    if args.len() != 1 {
        return None;
    }
    let (s, d) = args[0].as_tensor_val()?;
    if s.len() != 2 {
        return None;
    }
    let (m, n) = (s[0], s[1]);
    let mut out = vec![0i64; m * n];
    for i in 0..m {
        for j in 0..n {
            // T^T[j, i] = T[i, j]
            out[j * m + i] = d[i * n + j];
        }
    }
    Some(Term::Number(Value::Tensor {
        shape: vec![n, m],
        data: out,
    }))
}

// ── R18 (2026-04-18): Float-domain eval rules ─────────────────────

fn eval_float_zero(_args: &[Term]) -> Option<Term> {
    Some(Term::Number(Value::zero_float()))
}

fn eval_float_add(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    let a = args[0].as_float_val()?;
    let b = args[1].as_float_val()?;
    Value::from_f64(a + b).map(Term::Number)
}

fn eval_float_mul(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    let a = args[0].as_float_val()?;
    let b = args[1].as_float_val()?;
    Value::from_f64(a * b).map(Term::Number)
}

fn eval_float_sub(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    let a = args[0].as_float_val()?;
    let b = args[1].as_float_val()?;
    Value::from_f64(a - b).map(Term::Number)
}

fn eval_float_neg(args: &[Term]) -> Option<Term> {
    if args.len() != 1 {
        return None;
    }
    let a = args[0].as_float_val()?;
    Value::from_f64(-a).map(Term::Number)
}

fn eval_float_div(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    let a = args[0].as_float_val()?;
    let b = args[1].as_float_val()?;
    // Division by zero → non-finite → None (kernel "true"
    // invariant). This is the right behavior: the caller must
    // discover a "reciprocal of non-zero" precondition rule
    // rather than the kernel silently producing Inf.
    if b == 0.0 {
        return None;
    }
    Value::from_f64(a / b).map(Term::Number)
}

fn eval_float_from_int(args: &[Term]) -> Option<Term> {
    // Promote Int → Float. Distinct operator rather than
    // implicit so cross-domain conversion is always visible.
    if args.len() != 1 {
        return None;
    }
    let i = match &args[0] {
        Term::Number(Value::Int(n)) => *n,
        _ => return None,
    };
    Value::from_f64(i as f64).map(Term::Number)
}

// ── R19 (2026-04-18): FloatTensor eval rules ──────────────────────

fn as_ft(t: &Term) -> Option<(Vec<usize>, Vec<f64>)> {
    match t {
        Term::Number(v) => v.as_float_tensor().map(|(s, d)| (s.to_vec(), d)),
        _ => None,
    }
}

fn eval_ft_add(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    let (sa, da) = as_ft(&args[0])?;
    let (sb, db) = as_ft(&args[1])?;
    if sa != sb {
        return None;
    }
    let mut out: Vec<f64> = Vec::with_capacity(da.len());
    for (a, b) in da.iter().zip(db.iter()) {
        let s = a + b;
        if !s.is_finite() {
            return None;
        }
        out.push(s);
    }
    Value::float_tensor(sa, out).map(Term::Number)
}

fn eval_ft_mul(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    let (sa, da) = as_ft(&args[0])?;
    let (sb, db) = as_ft(&args[1])?;
    if sa != sb {
        return None;
    }
    let mut out: Vec<f64> = Vec::with_capacity(da.len());
    for (a, b) in da.iter().zip(db.iter()) {
        let p = a * b;
        if !p.is_finite() {
            return None;
        }
        out.push(p);
    }
    Value::float_tensor(sa, out).map(Term::Number)
}

fn eval_ft_sub(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    let (sa, da) = as_ft(&args[0])?;
    let (sb, db) = as_ft(&args[1])?;
    if sa != sb {
        return None;
    }
    let mut out: Vec<f64> = Vec::with_capacity(da.len());
    for (a, b) in da.iter().zip(db.iter()) {
        let s = a - b;
        if !s.is_finite() {
            return None;
        }
        out.push(s);
    }
    Value::float_tensor(sa, out).map(Term::Number)
}

fn eval_ft_neg(args: &[Term]) -> Option<Term> {
    if args.len() != 1 {
        return None;
    }
    let (s, d) = as_ft(&args[0])?;
    let out: Vec<f64> = d.iter().map(|x| -x).collect();
    Value::float_tensor(s, out).map(Term::Number)
}

fn eval_ft_sum(args: &[Term]) -> Option<Term> {
    if args.len() != 1 {
        return None;
    }
    let (_s, d) = as_ft(&args[0])?;
    let mut acc: f64 = 0.0;
    for v in d {
        acc += v;
        if !acc.is_finite() {
            return None;
        }
    }
    Value::from_f64(acc).map(Term::Number)
}

fn eval_ft_dot(args: &[Term]) -> Option<Term> {
    if args.len() != 2 {
        return None;
    }
    let (sa, da) = as_ft(&args[0])?;
    let (sb, db) = as_ft(&args[1])?;
    if sa.len() != 1 || sb.len() != 1 || sa != sb {
        return None;
    }
    let mut acc: f64 = 0.0;
    for (a, b) in da.iter().zip(db.iter()) {
        acc += a * b;
        if !acc.is_finite() {
            return None;
        }
    }
    Value::from_f64(acc).map(Term::Number)
}

fn eval_ft_scale(args: &[Term]) -> Option<Term> {
    // scale(c, T) where c is Float scalar, T is FloatTensor.
    if args.len() != 2 {
        return None;
    }
    let c = args[0].as_float_val()?;
    let (s, d) = as_ft(&args[1])?;
    let out: Vec<f64> = d.iter().map(|x| c * x).collect();
    // Guard non-finite in output (e.g., overflow from large c).
    for v in &out {
        if !v.is_finite() {
            return None;
        }
    }
    Value::float_tensor(s, out).map(Term::Number)
}

fn eval_ft_matmul(args: &[Term]) -> Option<Term> {
    // 2D float matrix multiplication: A[m,n] @ B[n,p] = C[m,p].
    if args.len() != 2 {
        return None;
    }
    let (sa, da) = as_ft(&args[0])?;
    let (sb, db) = as_ft(&args[1])?;
    if sa.len() != 2 || sb.len() != 2 {
        return None;
    }
    let (m, n_a) = (sa[0], sa[1]);
    let (n_b, p) = (sb[0], sb[1]);
    if n_a != n_b {
        return None;
    }
    let n = n_a;
    let mut out = vec![0.0_f64; m * p];
    for i in 0..m {
        for j in 0..p {
            let mut acc = 0.0_f64;
            for k in 0..n {
                acc += da[i * n + k] * db[k * p + j];
                if !acc.is_finite() {
                    return None;
                }
            }
            out[i * p + j] = acc;
        }
    }
    Value::float_tensor(vec![m, p], out).map(Term::Number)
}

fn eval_tensor_reshape(args: &[Term]) -> Option<Term> {
    // Reshape T to a new shape. Second arg is a 1D tensor whose
    // elements (as usize) are the new dims. Rejects on numel
    // mismatch between old and new shape.
    if args.len() != 2 {
        return None;
    }
    let (_old_shape, data) = args[0].as_tensor_val()?;
    let (shape_spec_shape, shape_spec_data) = args[1].as_tensor_val()?;
    if shape_spec_shape.len() != 1 {
        return None;
    }
    // All dim values must be non-negative; convert to usize.
    let mut new_shape: Vec<usize> = Vec::with_capacity(shape_spec_data.len());
    for v in shape_spec_data {
        if *v < 0 {
            return None;
        }
        new_shape.push(*v as usize);
    }
    let expected: usize = new_shape.iter().product();
    if expected != data.len() {
        return None;
    }
    Some(Term::Number(Value::Tensor {
        shape: new_shape,
        data: data.to_vec(),
    }))
}

fn eval_tensor_scale(args: &[Term]) -> Option<Term> {
    // `scale(c, T) = c * T` element-wise. `c` is an Int scalar,
    // `T` is a tensor. The Int ↔ Tensor boundary is explicit —
    // the machine would need to discover a promotion rule if it
    // wanted to scale a Nat into a tensor.
    if args.len() != 2 {
        return None;
    }
    let c = match &args[0] {
        Term::Number(Value::Int(n)) => *n,
        _ => return None,
    };
    let (shape, data) = args[1].as_tensor_val()?;
    let mut out = Vec::with_capacity(data.len());
    for d in data {
        out.push(c.checked_mul(*d)?);
    }
    Some(Term::Number(Value::Tensor {
        shape: shape.to_vec(),
        data: out,
    }))
}

/// The canonical builtin set. Mirror of previous BUILTIN_*
/// constants in eval.rs, now with all properties bundled and the
/// eval rules inline. Adding a new builtin means appending a
/// `Builtin` here.
///
/// Ids chosen to match the pre-R5 magic constants so existing
/// test corpora and terms continue to work without re-indexing:
///   0 = zero (nullary constant)
///   1 = succ (unary)
///   2 = add  (binary, AC)
///   3 = mul  (binary, AC)
pub const BUILTINS: &[Builtin] = &[
    Builtin {
        id: 0,
        name: "zero",
        arity: 0,
        commutative: false,
        associative: false,
        eval: eval_zero,
    },
    Builtin {
        id: 1,
        name: "succ",
        arity: 1,
        commutative: false,
        associative: false,
        eval: eval_succ,
    },
    Builtin {
        id: 2,
        name: "add",
        arity: 2,
        commutative: true,
        associative: true,
        eval: eval_add,
    },
    Builtin {
        id: 3,
        name: "mul",
        arity: 2,
        commutative: true,
        associative: true,
        eval: eval_mul,
    },
    // R7: Int domain. Disjoint from Nat — each eval rejects Nat args.
    Builtin {
        id: 10,
        name: "int_zero",
        arity: 0,
        commutative: false,
        associative: false,
        eval: eval_int_zero,
    },
    Builtin {
        id: 11,
        name: "int_succ",
        arity: 1,
        commutative: false,
        associative: false,
        eval: eval_int_succ,
    },
    Builtin {
        id: 12,
        name: "int_add",
        arity: 2,
        commutative: true,
        associative: true,
        eval: eval_int_add,
    },
    Builtin {
        id: 13,
        name: "int_mul",
        arity: 2,
        commutative: true,
        associative: true,
        eval: eval_int_mul,
    },
    Builtin {
        id: 14,
        name: "neg",
        arity: 1,
        commutative: false,
        associative: false,
        eval: eval_neg,
    },
    // R13: Tensor domain. Element-wise ops are AC (commutative +
    // associative within a consistent shape). Reductions (sum,
    // dot) are non-AC.
    Builtin {
        id: 20,
        name: "tensor_add",
        arity: 2,
        commutative: true,
        associative: true,
        eval: eval_tensor_add,
    },
    Builtin {
        id: 21,
        name: "tensor_mul",
        arity: 2,
        commutative: true,
        associative: true,
        eval: eval_tensor_mul,
    },
    Builtin {
        id: 22,
        name: "tensor_sum",
        arity: 1,
        commutative: false,
        associative: false,
        eval: eval_tensor_sum,
    },
    Builtin {
        id: 23,
        name: "tensor_dot",
        arity: 2,
        // Dot product IS commutative for 1D inner products.
        // Associativity doesn't apply (it's binary, not
        // self-composing).
        commutative: true,
        associative: false,
        eval: eval_tensor_dot,
    },
    Builtin {
        id: 24,
        name: "tensor_neg",
        arity: 1,
        commutative: false,
        associative: false,
        eval: eval_tensor_neg,
    },
    Builtin {
        id: 25,
        name: "tensor_scale",
        arity: 2,
        // Scalar · tensor is not commutative as-written (first arg
        // must be Int, second must be Tensor — they're typed
        // asymmetrically), though `scale(c, T) = scale(T', c)`
        // semantically if you flip. We keep it non-commutative to
        // avoid canonical-sort swapping the args.
        commutative: false,
        associative: false,
        eval: eval_tensor_scale,
    },
    // R16: 2D tensor operations. Matmul is bilinear but
    // NOT commutative (A·B ≠ B·A in general) and NOT
    // associative as declared here because associativity only
    // works when shapes match up. Transpose and reshape are
    // unary.
    Builtin {
        id: 26,
        name: "tensor_matmul",
        arity: 2,
        commutative: false,
        associative: false,
        eval: eval_tensor_matmul,
    },
    Builtin {
        id: 27,
        name: "tensor_transpose",
        arity: 1,
        commutative: false,
        associative: false,
        eval: eval_tensor_transpose,
    },
    Builtin {
        id: 28,
        name: "tensor_reshape",
        arity: 2,
        commutative: false,
        associative: false,
        eval: eval_tensor_reshape,
    },
    // R18: Float domain (ids 30..=36). Enables real-valued
    // gradient descent. All operations are element-disjoint from
    // Nat/Int/Tensor — cross-domain eval rejects.
    Builtin {
        id: 30,
        name: "float_zero",
        arity: 0,
        commutative: false,
        associative: false,
        eval: eval_float_zero,
    },
    Builtin {
        id: 31,
        name: "float_add",
        arity: 2,
        commutative: true,
        associative: true,
        eval: eval_float_add,
    },
    Builtin {
        id: 32,
        name: "float_mul",
        arity: 2,
        commutative: true,
        associative: true,
        eval: eval_float_mul,
    },
    Builtin {
        id: 33,
        name: "float_neg",
        arity: 1,
        commutative: false,
        associative: false,
        eval: eval_float_neg,
    },
    Builtin {
        id: 34,
        name: "float_div",
        arity: 2,
        // Division is NOT commutative (a/b != b/a in general).
        commutative: false,
        associative: false,
        eval: eval_float_div,
    },
    Builtin {
        id: 35,
        name: "float_from_int",
        arity: 1,
        commutative: false,
        associative: false,
        eval: eval_float_from_int,
    },
    Builtin {
        id: 36,
        name: "float_sub",
        arity: 2,
        commutative: false,
        associative: false,
        eval: eval_float_sub,
    },
    // R19: Float tensor domain (ids 40..=47). Real-valued
    // parametric models use these. Same AC flags as integer
    // tensor; semantic domain-disjoint from both integer tensor
    // and scalar Float.
    Builtin {
        id: 40,
        name: "ft_add",
        arity: 2,
        commutative: true,
        associative: true,
        eval: eval_ft_add,
    },
    Builtin {
        id: 41,
        name: "ft_mul",
        arity: 2,
        commutative: true,
        associative: true,
        eval: eval_ft_mul,
    },
    Builtin {
        id: 42,
        name: "ft_neg",
        arity: 1,
        commutative: false,
        associative: false,
        eval: eval_ft_neg,
    },
    Builtin {
        id: 43,
        name: "ft_sum",
        arity: 1,
        commutative: false,
        associative: false,
        eval: eval_ft_sum,
    },
    Builtin {
        id: 44,
        name: "ft_dot",
        arity: 2,
        commutative: true,
        associative: false,
        eval: eval_ft_dot,
    },
    Builtin {
        id: 45,
        name: "ft_scale",
        arity: 2,
        commutative: false,
        associative: false,
        eval: eval_ft_scale,
    },
    Builtin {
        id: 46,
        name: "ft_sub",
        arity: 2,
        commutative: false,
        associative: false,
        eval: eval_ft_sub,
    },
    Builtin {
        id: 47,
        name: "ft_matmul",
        arity: 2,
        commutative: false,
        associative: false,
        eval: eval_ft_matmul,
    },
];

/// Look up a builtin by its operator id. Returns None if the id
/// isn't a known builtin.
#[must_use]
pub fn lookup(id: u32) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|b| b.id == id)
}

/// Is `head` a known-commutative builtin operator?
#[must_use]
pub fn is_commutative(head: &Term) -> bool {
    match head {
        Term::Var(id) => lookup(*id).map_or(false, |b| b.commutative),
        _ => false,
    }
}

/// Is `head` a known-associative builtin operator?
#[must_use]
pub fn is_associative(head: &Term) -> bool {
    match head {
        Term::Var(id) => lookup(*id).map_or(false, |b| b.associative),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_finds_each_builtin_by_id() {
        assert_eq!(lookup(0).unwrap().name, "zero");
        assert_eq!(lookup(1).unwrap().name, "succ");
        assert_eq!(lookup(2).unwrap().name, "add");
        assert_eq!(lookup(3).unwrap().name, "mul");
    }

    #[test]
    fn lookup_returns_none_for_unknown_id() {
        assert!(lookup(99).is_none());
    }

    #[test]
    fn add_and_mul_are_commutative_and_associative() {
        let add = lookup(2).unwrap();
        let mul = lookup(3).unwrap();
        assert!(add.commutative && add.associative);
        assert!(mul.commutative && mul.associative);
    }

    #[test]
    fn succ_is_unary_and_non_ac() {
        let succ = lookup(1).unwrap();
        assert_eq!(succ.arity, 1);
        assert!(!succ.commutative);
        assert!(!succ.associative);
    }

    #[test]
    fn eval_succ_increments() {
        let result = eval_succ(&[Term::Number(Value::Nat(7))]).unwrap();
        assert_eq!(result, Term::Number(Value::Nat(8)));
    }

    #[test]
    fn eval_add_computes_sum() {
        let result = eval_add(&[
            Term::Number(Value::Nat(3)),
            Term::Number(Value::Nat(5)),
        ])
        .unwrap();
        assert_eq!(result, Term::Number(Value::Nat(8)));
    }

    #[test]
    fn eval_returns_none_on_non_reduced_args() {
        // Builtin eval fires only when args are Numbers. If an
        // arg is still an Apply (not fully reduced), eval
        // returns None so the caller can continue reducing.
        let not_yet = Term::Apply(
            Box::new(Term::Var(2)),
            vec![Term::Number(Value::Nat(1)), Term::Number(Value::Nat(2))],
        );
        assert!(eval_add(&[not_yet, Term::Number(Value::Nat(5))]).is_none());
    }

    #[test]
    fn is_commutative_uses_registry() {
        assert!(is_commutative(&Term::Var(2))); // add
        assert!(is_commutative(&Term::Var(3))); // mul
        assert!(!is_commutative(&Term::Var(1))); // succ
        assert!(!is_commutative(&Term::Var(99))); // unknown
    }

    // ── R7: Int-domain builtin gold tests ─────────────────────────

    #[test]
    fn int_builtins_registered() {
        assert_eq!(lookup(INT_ZERO).unwrap().name, "int_zero");
        assert_eq!(lookup(INT_SUCC).unwrap().name, "int_succ");
        assert_eq!(lookup(INT_ADD).unwrap().name, "int_add");
        assert_eq!(lookup(INT_MUL).unwrap().name, "int_mul");
        assert_eq!(lookup(NEG).unwrap().name, "neg");
    }

    #[test]
    fn int_add_and_mul_are_ac() {
        let int_add = lookup(INT_ADD).unwrap();
        let int_mul = lookup(INT_MUL).unwrap();
        assert!(int_add.commutative && int_add.associative);
        assert!(int_mul.commutative && int_mul.associative);
    }

    #[test]
    fn neg_is_unary_non_ac() {
        let neg = lookup(NEG).unwrap();
        assert_eq!(neg.arity, 1);
        assert!(!neg.commutative && !neg.associative);
    }

    #[test]
    fn eval_int_zero_returns_int_not_nat() {
        // Domain distinction: int_zero yields Int(0), not Nat(0).
        let result = eval_int_zero(&[]).unwrap();
        assert_eq!(result, Term::Number(Value::Int(0)));
        assert_ne!(result, Term::Number(Value::Nat(0)));
    }

    #[test]
    fn eval_int_succ_increments_int() {
        let result = eval_int_succ(&[Term::Number(Value::Int(-3))]).unwrap();
        assert_eq!(result, Term::Number(Value::Int(-2)));
    }

    #[test]
    fn eval_int_succ_rejects_nat_input() {
        // Cross-domain rejection: int_succ must refuse Nat args.
        // Returns None, so the evaluator falls through.
        let result = eval_int_succ(&[Term::Number(Value::Nat(5))]);
        assert!(result.is_none(), "int_succ must reject Nat input");
    }

    #[test]
    fn eval_int_add_computes_signed_sum() {
        let result = eval_int_add(&[
            Term::Number(Value::Int(-7)),
            Term::Number(Value::Int(5)),
        ])
        .unwrap();
        assert_eq!(result, Term::Number(Value::Int(-2)));
    }

    #[test]
    fn eval_int_add_rejects_mixed_domain() {
        // (Int, Nat) must fail — no silent promotion.
        let result = eval_int_add(&[
            Term::Number(Value::Int(3)),
            Term::Number(Value::Nat(5)),
        ]);
        assert!(result.is_none(), "mixed Int+Nat must fail");
    }

    #[test]
    fn eval_neg_flips_sign() {
        let result = eval_neg(&[Term::Number(Value::Int(42))]).unwrap();
        assert_eq!(result, Term::Number(Value::Int(-42)));
        // And applying neg twice is identity.
        let twice = eval_neg(&[result]).unwrap();
        assert_eq!(twice, Term::Number(Value::Int(42)));
    }

    #[test]
    fn eval_neg_rejects_nat() {
        // Nat has no negation; neg must refuse Nat input. The
        // machine would have to discover an explicit
        // Nat→Int promotion before negation became possible.
        let result = eval_neg(&[Term::Number(Value::Nat(5))]);
        assert!(result.is_none(), "neg must reject Nat input");
    }

    #[test]
    fn eval_int_add_overflow_returns_none() {
        // Correctness > wrapping: i64::MAX + 1 is not a truthful
        // answer; return None instead. The kernel invariant
        // "true" demands this.
        let result = eval_int_add(&[
            Term::Number(Value::Int(i64::MAX)),
            Term::Number(Value::Int(1)),
        ]);
        assert!(result.is_none(), "overflow must not silently wrap");
    }

    #[test]
    fn nat_builtins_reject_int_input() {
        // Symmetric cross-domain guard: Nat add refuses Int args.
        let result = eval_add(&[
            Term::Number(Value::Int(3)),
            Term::Number(Value::Int(5)),
        ]);
        assert!(result.is_none(), "Nat add must reject Int input");
    }

    #[test]
    fn canonical_folds_int_expressions_via_r6() {
        // R6 constant folding now flows through Int builtins
        // uniformly — the registry-driven fold trusts the eval
        // rule, so Int(3) + Int(5) folds to Int(8) just like the
        // Nat version.
        let t = Term::Apply(
            Box::new(Term::Var(INT_ADD)),
            vec![
                Term::Number(Value::Int(3)),
                Term::Number(Value::Int(5)),
            ],
        );
        let canon = t.canonical();
        assert_eq!(canon, Term::Number(Value::Int(8)));
    }

    #[test]
    fn canonical_folds_nested_int_neg_and_add() {
        // neg(int_add(3, 5)) = neg(8) = -8 via R6 bottom-up.
        let inner = Term::Apply(
            Box::new(Term::Var(INT_ADD)),
            vec![
                Term::Number(Value::Int(3)),
                Term::Number(Value::Int(5)),
            ],
        );
        let neg_it = Term::Apply(Box::new(Term::Var(NEG)), vec![inner]);
        let canon = neg_it.canonical();
        assert_eq!(canon, Term::Number(Value::Int(-8)));
    }

    // ── R13: Tensor-domain gold tests ────────────────────────────

    fn t(shape: Vec<usize>, data: Vec<i64>) -> Term {
        Term::Number(Value::tensor(shape, data).unwrap())
    }

    #[test]
    fn tensor_builtins_registered() {
        assert_eq!(lookup(TENSOR_ADD).unwrap().name, "tensor_add");
        assert_eq!(lookup(TENSOR_MUL).unwrap().name, "tensor_mul");
        assert_eq!(lookup(TENSOR_SUM).unwrap().name, "tensor_sum");
        assert_eq!(lookup(TENSOR_DOT).unwrap().name, "tensor_dot");
        assert_eq!(lookup(TENSOR_NEG).unwrap().name, "tensor_neg");
        assert_eq!(lookup(TENSOR_SCALE).unwrap().name, "tensor_scale");
    }

    #[test]
    fn tensor_add_elementwise() {
        // [1 2 3] + [10 20 30] = [11 22 33]
        let a = t(vec![3], vec![1, 2, 3]);
        let b = t(vec![3], vec![10, 20, 30]);
        let sum = eval_tensor_add(&[a, b]).unwrap();
        assert_eq!(sum, t(vec![3], vec![11, 22, 33]));
    }

    #[test]
    fn tensor_add_rejects_shape_mismatch() {
        // [3] + [4] — different shapes, must fail.
        let a = t(vec![3], vec![1, 2, 3]);
        let b = t(vec![4], vec![1, 2, 3, 4]);
        assert!(eval_tensor_add(&[a, b]).is_none());
    }

    #[test]
    fn tensor_mul_elementwise() {
        let a = t(vec![4], vec![1, 2, 3, 4]);
        let b = t(vec![4], vec![2, 2, 2, 2]);
        let prod = eval_tensor_mul(&[a, b]).unwrap();
        assert_eq!(prod, t(vec![4], vec![2, 4, 6, 8]));
    }

    #[test]
    fn tensor_sum_reduces_to_scalar_int() {
        let v = t(vec![5], vec![1, 2, 3, 4, 5]);
        let s = eval_tensor_sum(&[v]).unwrap();
        assert_eq!(s, Term::Number(Value::Int(15)));
    }

    #[test]
    fn tensor_dot_1d_computes_inner_product() {
        // dot([1 2 3], [4 5 6]) = 1*4 + 2*5 + 3*6 = 32
        let a = t(vec![3], vec![1, 2, 3]);
        let b = t(vec![3], vec![4, 5, 6]);
        let d = eval_tensor_dot(&[a, b]).unwrap();
        assert_eq!(d, Term::Number(Value::Int(32)));
    }

    #[test]
    fn tensor_neg_flips_all_elements() {
        let v = t(vec![3], vec![1, -2, 3]);
        let n = eval_tensor_neg(&[v]).unwrap();
        assert_eq!(n, t(vec![3], vec![-1, 2, -3]));
    }

    #[test]
    fn tensor_scale_broadcasts_scalar() {
        // scale(3, [1 2 3]) = [3 6 9]
        let s = eval_tensor_scale(&[
            Term::Number(Value::Int(3)),
            t(vec![3], vec![1, 2, 3]),
        ])
        .unwrap();
        assert_eq!(s, t(vec![3], vec![3, 6, 9]));
    }

    #[test]
    fn tensor_ops_reject_scalar_input() {
        let int_a = Term::Number(Value::Int(5));
        let int_b = Term::Number(Value::Int(7));
        assert!(eval_tensor_add(&[int_a.clone(), int_b.clone()]).is_none());
        assert!(eval_tensor_mul(&[int_a.clone(), int_b.clone()]).is_none());
        assert!(eval_tensor_sum(&[int_a.clone()]).is_none());
    }

    // ── PROOF: end-to-end compute pipeline ───────────────────────
    //
    // Not just "types exist" — the full chain of tensor
    // construction, element-wise ops, and reduction runs through
    // the kernel's eval pipeline and produces the expected
    // numerical result. This is the "prove it" demand from the
    // user: a working compute layer, not a stub.

    #[test]
    fn proof_linear_evaluation_end_to_end() {
        // y = w · x + b   where w, x are vectors and b is a scalar.
        // With w=[2, 3, 5], x=[1, 10, 100], b=7:
        //   w·x = 2*1 + 3*10 + 5*100 = 2 + 30 + 500 = 532
        //   y   = 532 + 7 = 539
        //
        // Build this as Terms, evaluate through the kernel, verify
        // the scalar result. This proves:
        //  - Tensor values pass through the kernel's Term wrapper
        //  - Tensor builtins compose (dot → add)
        //  - The bilinear op (dot) reduces to Int for scalar add
        //  - R6 constant folding applies throughout: the entire
        //    Apply tree collapses to a single Int via .canonical()
        use crate::eval::eval;
        let w = t(vec![3], vec![2, 3, 5]);
        let x = t(vec![3], vec![1, 10, 100]);
        let b = Term::Number(Value::Int(7));
        let dot = Term::Apply(
            Box::new(Term::Var(TENSOR_DOT)),
            vec![w, x],
        );
        let y = Term::Apply(
            Box::new(Term::Var(INT_ADD)),
            vec![dot, b],
        );

        // Evaluate through the kernel.
        let result = eval(&y, &[], 100).unwrap();
        assert_eq!(result, Term::Number(Value::Int(539)));

        // Same via canonical (R6 fold) — must agree. The compute
        // layer composes cleanly with the existing folding.
        let canon = y.canonical();
        assert_eq!(canon, Term::Number(Value::Int(539)));
    }

    #[test]
    fn proof_mse_loss_end_to_end() {
        // Mean-squared-error loss on 3 samples, integer math.
        //   pred   = [10, 20, 30]
        //   target = [8,  22, 31]
        //   diff   = pred - target = [2, -2, -1]
        //      (compute as pred + neg(target))
        //   sq     = diff * diff = [4, 4, 1]
        //   loss   = sum(sq) = 9
        //
        // This proves: element-wise chain (add ∘ neg ∘ mul ∘ sum)
        // composes, reduces, and yields the correct loss scalar.
        // End-to-end through the kernel — no hand computation.
        use crate::eval::eval;
        let pred = t(vec![3], vec![10, 20, 30]);
        let target = t(vec![3], vec![8, 22, 31]);

        let neg_target = Term::Apply(
            Box::new(Term::Var(TENSOR_NEG)),
            vec![target],
        );
        let diff = Term::Apply(
            Box::new(Term::Var(TENSOR_ADD)),
            vec![pred, neg_target],
        );
        let sq = Term::Apply(
            Box::new(Term::Var(TENSOR_MUL)),
            vec![diff.clone(), diff],
        );
        let loss = Term::Apply(
            Box::new(Term::Var(TENSOR_SUM)),
            vec![sq],
        );

        let result = eval(&loss, &[], 200).unwrap();
        assert_eq!(result, Term::Number(Value::Int(9)));
    }

    #[test]
    fn proof_linear_composition_folds_via_canonical() {
        // Nested linear compute: tensor_sum(tensor_mul(tensor_add(x, y), z))
        // for x=[1,2], y=[3,4], z=[10,20]:
        //   x+y = [4, 6]
        //   (x+y) * z = [40, 120]
        //   sum = 160
        //
        // Entirely via canonical() — R6 folding navigates the
        // mixed tensor+int chain. Proves the compute layer's
        // constant folding really works at the substrate level,
        // not just through eval().
        let x = t(vec![2], vec![1, 2]);
        let y = t(vec![2], vec![3, 4]);
        let z = t(vec![2], vec![10, 20]);
        let xy = Term::Apply(
            Box::new(Term::Var(TENSOR_ADD)),
            vec![x, y],
        );
        let xy_z = Term::Apply(
            Box::new(Term::Var(TENSOR_MUL)),
            vec![xy, z],
        );
        let expr = Term::Apply(
            Box::new(Term::Var(TENSOR_SUM)),
            vec![xy_z],
        );
        let canon = expr.canonical();
        assert_eq!(canon, Term::Number(Value::Int(160)));
    }

    #[test]
    fn proof_scale_and_sum_fold_via_canonical() {
        // scale(-2, [5, 10, 15]) = [-10, -20, -30]
        // sum of that = -60
        let v = t(vec![3], vec![5, 10, 15]);
        let scaled = Term::Apply(
            Box::new(Term::Var(TENSOR_SCALE)),
            vec![Term::Number(Value::Int(-2)), v],
        );
        let s = Term::Apply(
            Box::new(Term::Var(TENSOR_SUM)),
            vec![scaled],
        );
        assert_eq!(s.canonical(), Term::Number(Value::Int(-60)));
    }

    #[test]
    fn proof_tensor_ops_associativity_via_canonical() {
        // (a + b) + c == a + (b + c) for tensors.
        // Values: a=[1,2], b=[3,4], c=[5,6]
        // Both groupings → [9, 12]
        let a = t(vec![2], vec![1, 2]);
        let b = t(vec![2], vec![3, 4]);
        let c = t(vec![2], vec![5, 6]);
        let left_grouped = Term::Apply(
            Box::new(Term::Var(TENSOR_ADD)),
            vec![
                Term::Apply(
                    Box::new(Term::Var(TENSOR_ADD)),
                    vec![a.clone(), b.clone()],
                ),
                c.clone(),
            ],
        );
        let right_grouped = Term::Apply(
            Box::new(Term::Var(TENSOR_ADD)),
            vec![
                a.clone(),
                Term::Apply(
                    Box::new(Term::Var(TENSOR_ADD)),
                    vec![b.clone(), c.clone()],
                ),
            ],
        );
        let expected = t(vec![2], vec![9, 12]);
        assert_eq!(left_grouped.canonical(), expected);
        assert_eq!(right_grouped.canonical(), expected);
        // Critically: both groupings canonicalize to the SAME
        // term. R3/R4 (AC canonicalization) works for tensor_add
        // because the registry declared it AC.
        assert_eq!(left_grouped.canonical(), right_grouped.canonical());
    }

    #[test]
    fn proof_tensor_overflow_is_truthful() {
        // Kernel invariant "true": overflow must not silently
        // wrap. tensor_add of two near-max i64 tensors must
        // return None (rejected, not a wrong answer).
        let a = t(vec![1], vec![i64::MAX]);
        let b = t(vec![1], vec![1]);
        let result = eval_tensor_add(&[a, b]);
        assert!(result.is_none(), "overflow must not wrap");
    }

    // ── R16: 2D tensor op tests ──────────────────────────────────

    #[test]
    fn matmul_registered_and_not_ac() {
        let m = lookup(TENSOR_MATMUL).unwrap();
        assert_eq!(m.name, "tensor_matmul");
        assert_eq!(m.arity, 2);
        assert!(!m.commutative, "A@B != B@A in general");
    }

    #[test]
    fn matmul_2x3_by_3x2() {
        // A = [[1, 2, 3], [4, 5, 6]] — shape [2, 3]
        // B = [[7, 8], [9, 10], [11, 12]] — shape [3, 2]
        // C = A @ B — shape [2, 2]
        //   C[0,0] = 1*7 + 2*9 + 3*11 = 7 + 18 + 33 = 58
        //   C[0,1] = 1*8 + 2*10 + 3*12 = 8 + 20 + 36 = 64
        //   C[1,0] = 4*7 + 5*9 + 6*11 = 28 + 45 + 66 = 139
        //   C[1,1] = 4*8 + 5*10 + 6*12 = 32 + 50 + 72 = 154
        let a = t(vec![2, 3], vec![1, 2, 3, 4, 5, 6]);
        let b = t(vec![3, 2], vec![7, 8, 9, 10, 11, 12]);
        let c = eval_tensor_matmul(&[a, b]).unwrap();
        assert_eq!(c, t(vec![2, 2], vec![58, 64, 139, 154]));
    }

    #[test]
    fn matmul_rejects_mismatched_inner_dim() {
        // [2, 3] @ [4, 2] — inner dims 3 vs 4 mismatch.
        let a = t(vec![2, 3], vec![1, 2, 3, 4, 5, 6]);
        let b = t(vec![4, 2], vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(eval_tensor_matmul(&[a, b]).is_none());
    }

    #[test]
    fn matmul_rejects_non_2d_shapes() {
        let a = t(vec![3], vec![1, 2, 3]); // 1D
        let b = t(vec![3, 2], vec![1, 2, 3, 4, 5, 6]);
        assert!(eval_tensor_matmul(&[a, b]).is_none());
    }

    #[test]
    fn transpose_2x3_yields_3x2() {
        // T = [[1, 2, 3], [4, 5, 6]] shape [2,3]
        // T^T = [[1, 4], [2, 5], [3, 6]] shape [3,2]
        let t_in = t(vec![2, 3], vec![1, 2, 3, 4, 5, 6]);
        let t_out = eval_tensor_transpose(&[t_in]).unwrap();
        assert_eq!(t_out, t(vec![3, 2], vec![1, 4, 2, 5, 3, 6]));
    }

    #[test]
    fn transpose_is_involutive() {
        // (T^T)^T = T
        let t_in = t(vec![3, 4], vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        let once = eval_tensor_transpose(&[t_in.clone()]).unwrap();
        let twice = eval_tensor_transpose(&[once]).unwrap();
        assert_eq!(twice, t_in);
    }

    #[test]
    fn reshape_preserves_numel() {
        // 2x3 → 3x2. Same 6 elements, different shape.
        let t_in = t(vec![2, 3], vec![1, 2, 3, 4, 5, 6]);
        let shape_spec = t(vec![2], vec![3, 2]);
        let t_out = eval_tensor_reshape(&[t_in, shape_spec]).unwrap();
        assert_eq!(t_out, t(vec![3, 2], vec![1, 2, 3, 4, 5, 6]));
    }

    #[test]
    fn reshape_rejects_numel_mismatch() {
        let t_in = t(vec![2, 3], vec![1, 2, 3, 4, 5, 6]);
        let bad_shape = t(vec![2], vec![3, 3]); // 9 != 6
        assert!(eval_tensor_reshape(&[t_in, bad_shape]).is_none());
    }

    #[test]
    fn reshape_flattens_to_1d() {
        // 2x3 → 6 (1D)
        let t_in = t(vec![2, 3], vec![1, 2, 3, 4, 5, 6]);
        let flat_spec = t(vec![1], vec![6]);
        let flat = eval_tensor_reshape(&[t_in, flat_spec]).unwrap();
        assert_eq!(flat, t(vec![6], vec![1, 2, 3, 4, 5, 6]));
    }

    // ── PROOF: linear layer via matmul ───────────────────────────

    #[test]
    fn proof_linear_layer_forward_pass() {
        // A linear layer: y = W @ x + b
        // Input x = [1, 2, 3] (shape [3])
        // But matmul is 2D; we treat x as column vector [3, 1].
        //   x_col = [[1], [2], [3]]
        // Weights W: shape [2, 3]
        //   W = [[1, 2, 3], [4, 5, 6]]
        // Bias b: shape [2, 1]
        //   b = [[10], [20]]
        // y = W @ x + b
        //   W @ x = [[1+4+9], [4+10+18]] = [[14], [32]]
        //   y = [[24], [52]]
        use crate::eval::eval;
        let w = t(vec![2, 3], vec![1, 2, 3, 4, 5, 6]);
        let x = t(vec![3, 1], vec![1, 2, 3]);
        let b = t(vec![2, 1], vec![10, 20]);

        let matmul = Term::Apply(
            Box::new(Term::Var(TENSOR_MATMUL)),
            vec![w, x],
        );
        let layer_out = Term::Apply(
            Box::new(Term::Var(TENSOR_ADD)),
            vec![matmul, b],
        );
        let result = eval(&layer_out, &[], 100).unwrap();
        assert_eq!(result, t(vec![2, 1], vec![24, 52]));
    }

    // ── R18: Float-domain tests ──────────────────────────────────

    fn f(v: f64) -> Term {
        Term::Number(Value::from_f64(v).unwrap())
    }

    #[test]
    fn float_builtins_registered() {
        assert_eq!(lookup(FLOAT_ADD).unwrap().name, "float_add");
        assert_eq!(lookup(FLOAT_MUL).unwrap().name, "float_mul");
        assert_eq!(lookup(FLOAT_DIV).unwrap().name, "float_div");
        assert_eq!(lookup(FLOAT_SUB).unwrap().name, "float_sub");
    }

    #[test]
    fn float_add_basic() {
        let r = eval_float_add(&[f(1.5), f(2.5)]).unwrap();
        assert_eq!(r, f(4.0));
    }

    #[test]
    fn float_mul_basic() {
        let r = eval_float_mul(&[f(2.5), f(4.0)]).unwrap();
        assert_eq!(r, f(10.0));
    }

    #[test]
    fn float_sub_basic() {
        let r = eval_float_sub(&[f(10.0), f(3.5)]).unwrap();
        assert_eq!(r, f(6.5));
    }

    #[test]
    fn float_div_basic() {
        let r = eval_float_div(&[f(10.0), f(4.0)]).unwrap();
        assert_eq!(r, f(2.5));
    }

    #[test]
    fn float_div_by_zero_returns_none() {
        let r = eval_float_div(&[f(1.0), f(0.0)]);
        assert!(r.is_none(), "division by zero must not silently produce Inf");
    }

    #[test]
    fn float_overflow_rejects() {
        // Max × 2 overflows to Inf — must reject.
        let r = eval_float_mul(&[f(f64::MAX), f(2.0)]);
        assert!(r.is_none(), "overflow must not silently produce Inf");
    }

    #[test]
    fn float_from_int_promotes() {
        let r = eval_float_from_int(&[Term::Number(Value::Int(42))]).unwrap();
        assert_eq!(r, f(42.0));
    }

    #[test]
    fn float_neg_flips_sign() {
        let r = eval_float_neg(&[f(3.14)]).unwrap();
        assert_eq!(r, f(-3.14));
    }

    #[test]
    fn float_ops_reject_int_input() {
        // Cross-domain guard: Float builtins must reject Int args.
        assert!(eval_float_add(&[Term::Number(Value::Int(1)), Term::Number(Value::Int(2))])
            .is_none());
    }

    #[test]
    fn nat_add_rejects_float_input() {
        // Symmetric: Nat add rejects Float.
        assert!(eval_add(&[f(1.0), f(2.0)]).is_none());
    }

    // ── PROOF: gradient descent converges with floats ────────────

    #[test]
    fn proof_float_gradient_descent_converges() {
        // Problem: find w such that w = target = 7.0.
        // Loss: L(w) = (w - target)². dL/dw = 2(w - target).
        // With lr = 0.1, update: w_new = w - lr * 2 * (w - target).
        //
        // Starting w = 0.0, target = 7.0.
        // After 1 step: w = 0 - 0.1 * 2 * (-7) = 1.4
        // After 2 steps: w = 1.4 - 0.1 * 2 * (-5.6) = 1.4 + 1.12 = 2.52
        // ... convergence toward 7.
        //
        // Run 50 steps and verify we're close to 7.
        use crate::eval::eval;
        let target: f64 = 7.0;
        let lr: f64 = 0.1;
        let mut w: f64 = 0.0;
        for _ in 0..50 {
            // Construct gradient expression: 2 * (w - target).
            // Actually easier to compute directly since w is just
            // a value — we're proving the KERNEL eval does it
            // right, not the autograd.
            let grad_expr = Term::Apply(
                Box::new(Term::Var(FLOAT_MUL)),
                vec![
                    f(2.0),
                    Term::Apply(
                        Box::new(Term::Var(FLOAT_SUB)),
                        vec![f(w), f(target)],
                    ),
                ],
            );
            let grad_term = eval(&grad_expr, &[], 100).unwrap();
            let grad = grad_term.as_float_val().unwrap();

            // Apply SGD step: w = w - lr * grad
            let step_expr = Term::Apply(
                Box::new(Term::Var(FLOAT_SUB)),
                vec![
                    f(w),
                    Term::Apply(
                        Box::new(Term::Var(FLOAT_MUL)),
                        vec![f(lr), f(grad)],
                    ),
                ],
            );
            let step_term = eval(&step_expr, &[], 100).unwrap();
            w = step_term.as_float_val().unwrap();
        }
        // After 50 steps with lr=0.1, the convergence rate
        // (1 - 0.2)^50 ~ 1.4e-5, so we're within ~0.0001 of target.
        assert!(
            (w - target).abs() < 0.001,
            "after 50 SGD steps, w={w}, target={target}, |diff|={}",
            (w - target).abs()
        );
    }

    // ── R19: FloatTensor tests ───────────────────────────────────

    fn ft(shape: Vec<usize>, data: Vec<f64>) -> Term {
        Term::Number(Value::float_tensor(shape, data).unwrap())
    }

    #[test]
    fn ft_builtins_registered() {
        assert_eq!(lookup(FT_ADD).unwrap().name, "ft_add");
        assert_eq!(lookup(FT_MUL).unwrap().name, "ft_mul");
        assert_eq!(lookup(FT_SUM).unwrap().name, "ft_sum");
        assert_eq!(lookup(FT_DOT).unwrap().name, "ft_dot");
        assert_eq!(lookup(FT_MATMUL).unwrap().name, "ft_matmul");
    }

    #[test]
    fn ft_add_elementwise() {
        let a = ft(vec![3], vec![1.0, 2.0, 3.0]);
        let b = ft(vec![3], vec![0.5, 1.5, 2.5]);
        let r = eval_ft_add(&[a, b]).unwrap();
        assert_eq!(r, ft(vec![3], vec![1.5, 3.5, 5.5]));
    }

    #[test]
    fn ft_mul_elementwise() {
        let a = ft(vec![2], vec![2.0, 3.0]);
        let b = ft(vec![2], vec![4.0, 5.0]);
        let r = eval_ft_mul(&[a, b]).unwrap();
        assert_eq!(r, ft(vec![2], vec![8.0, 15.0]));
    }

    #[test]
    fn ft_sum_reduces_to_scalar_float() {
        let v = ft(vec![4], vec![0.5, 1.5, 2.5, 3.5]);
        let s = eval_ft_sum(&[v]).unwrap();
        assert_eq!(s, f(8.0));
    }

    #[test]
    fn ft_dot_computes_inner_product() {
        // dot([1, 2], [3, 4]) = 3 + 8 = 11
        let a = ft(vec![2], vec![1.0, 2.0]);
        let b = ft(vec![2], vec![3.0, 4.0]);
        let r = eval_ft_dot(&[a, b]).unwrap();
        assert_eq!(r, f(11.0));
    }

    #[test]
    fn ft_scale_multiplies_all_elements() {
        let r = eval_ft_scale(&[f(2.5), ft(vec![3], vec![1.0, 2.0, 3.0])]).unwrap();
        assert_eq!(r, ft(vec![3], vec![2.5, 5.0, 7.5]));
    }

    #[test]
    fn ft_matmul_2x3_by_3x2() {
        // [[1, 2, 3], [4, 5, 6]] @ [[0.5, 1.0], [1.5, 2.0], [2.5, 3.0]]
        //   C[0,0] = 1*0.5 + 2*1.5 + 3*2.5 = 0.5 + 3 + 7.5 = 11.0
        //   C[0,1] = 1*1.0 + 2*2.0 + 3*3.0 = 1 + 4 + 9 = 14.0
        //   C[1,0] = 4*0.5 + 5*1.5 + 6*2.5 = 2 + 7.5 + 15 = 24.5
        //   C[1,1] = 4*1.0 + 5*2.0 + 6*3.0 = 4 + 10 + 18 = 32.0
        let a = ft(vec![2, 3], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let b = ft(vec![3, 2], vec![0.5, 1.0, 1.5, 2.0, 2.5, 3.0]);
        let c = eval_ft_matmul(&[a, b]).unwrap();
        assert_eq!(
            c,
            ft(vec![2, 2], vec![11.0, 14.0, 24.5, 32.0])
        );
    }

    #[test]
    fn ft_ops_reject_integer_tensor() {
        // Cross-domain guard: ft_add rejects integer Tensor args.
        let int_t = Term::Number(Value::tensor(vec![2], vec![1, 2]).unwrap());
        assert!(eval_ft_add(&[int_t.clone(), int_t]).is_none());
    }

    // ── PROOF: linear regression on FloatTensor ──────────────────

    #[test]
    fn proof_float_linear_regression_forward_and_loss() {
        // Model: y_hat = dot(w, x) + b  where w, x are 1D float
        // tensors and b is a Float scalar.
        //   w = [1.0, 0.5, 2.0]
        //   x = [4.0, 6.0, 1.0]
        //   b = 0.5
        //   y_hat = 1*4 + 0.5*6 + 2*1 + 0.5 = 4 + 3 + 2 + 0.5 = 9.5
        //
        // Loss: (y_hat - target)² where target = 10.0
        //   loss = (9.5 - 10.0)² = (-0.5)² = 0.25
        use crate::eval::eval;
        let w = ft(vec![3], vec![1.0, 0.5, 2.0]);
        let x = ft(vec![3], vec![4.0, 6.0, 1.0]);
        let b = f(0.5);
        let target = f(10.0);

        let dot = Term::Apply(
            Box::new(Term::Var(FT_DOT)),
            vec![w, x],
        );
        let y_hat = Term::Apply(
            Box::new(Term::Var(FLOAT_ADD)),
            vec![dot, b],
        );
        // Check y_hat directly.
        let y_hat_val = eval(&y_hat, &[], 200).unwrap();
        assert_eq!(y_hat_val, f(9.5));

        // Compute loss: (y_hat - target)²
        let diff = Term::Apply(
            Box::new(Term::Var(FLOAT_SUB)),
            vec![y_hat, target],
        );
        let loss = Term::Apply(
            Box::new(Term::Var(FLOAT_MUL)),
            vec![diff.clone(), diff],
        );
        let loss_val = eval(&loss, &[], 200).unwrap();
        assert_eq!(loss_val, f(0.25));
    }

    #[test]
    fn proof_matmul_composes_with_transpose() {
        // Identity: (A @ B)^T = B^T @ A^T
        // A = 2x3, B = 3x2, A@B = 2x2
        //   A = [[1, 2, 3], [4, 5, 6]]
        //   B = [[7, 8], [9, 10], [11, 12]]
        //   AB = [[58, 64], [139, 154]]
        //   (AB)^T = [[58, 139], [64, 154]]
        //   A^T = [[1, 4], [2, 5], [3, 6]]  shape 3x2
        //   B^T = [[7, 9, 11], [8, 10, 12]] shape 2x3
        //   B^T @ A^T = shape [2, 2]
        //     [0,0] = 7*1 + 9*2 + 11*3 = 7 + 18 + 33 = 58
        //     [0,1] = 7*4 + 9*5 + 11*6 = 28 + 45 + 66 = 139
        //     [1,0] = 8*1 + 10*2 + 12*3 = 8 + 20 + 36 = 64
        //     [1,1] = 8*4 + 10*5 + 12*6 = 32 + 50 + 72 = 154
        //   → [[58, 139], [64, 154]]  matches (AB)^T ✓
        use crate::eval::eval;
        let a = t(vec![2, 3], vec![1, 2, 3, 4, 5, 6]);
        let b = t(vec![3, 2], vec![7, 8, 9, 10, 11, 12]);

        // Left: (A @ B)^T
        let ab = Term::Apply(
            Box::new(Term::Var(TENSOR_MATMUL)),
            vec![a.clone(), b.clone()],
        );
        let ab_t = Term::Apply(
            Box::new(Term::Var(TENSOR_TRANSPOSE)),
            vec![ab],
        );
        let left = eval(&ab_t, &[], 100).unwrap();

        // Right: B^T @ A^T
        let bt = Term::Apply(
            Box::new(Term::Var(TENSOR_TRANSPOSE)),
            vec![b],
        );
        let at = Term::Apply(
            Box::new(Term::Var(TENSOR_TRANSPOSE)),
            vec![a],
        );
        let bt_at = Term::Apply(
            Box::new(Term::Var(TENSOR_MATMUL)),
            vec![bt, at],
        );
        let right = eval(&bt_at, &[], 100).unwrap();

        assert_eq!(left, right);
        assert_eq!(left, t(vec![2, 2], vec![58, 139, 64, 154]));
    }
}
