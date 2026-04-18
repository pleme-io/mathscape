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
}
