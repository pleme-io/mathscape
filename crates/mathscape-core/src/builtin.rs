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
pub const ZERO: u32 = 0;
pub const SUCC: u32 = 1;
pub const ADD: u32 = 2;
pub const MUL: u32 = 3;

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
}
