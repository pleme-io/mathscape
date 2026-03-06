//! Core types: Point, Number, Fn, Term enum, hash-consing, evaluation,
//! substitution, s-expression parser/printer.

pub mod eval;
pub mod hash;
pub mod parse;
pub mod term;
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;
pub mod value;

pub use hash::TermRef;
pub use term::{StoredTerm, SymbolId, Term};
pub use value::Value;
