//! Shared test helpers for constructing Terms.
//!
//! Available in all crates via `mathscape_core::test_helpers::*`.

use crate::term::Term;
use crate::value::Value;

pub fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}

pub fn var(v: u32) -> Term {
    Term::Var(v)
}

pub fn apply(f: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(f), args)
}

pub fn point(id: u64) -> Term {
    Term::Point(id)
}

pub fn symbol(id: u32, args: Vec<Term>) -> Term {
    Term::Symbol(id, args)
}
