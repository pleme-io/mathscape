//! Symbolic compression: anti-unification, library extraction, rewriting.
//!
//! E-graph integration (`egg`) lives in `egraph` — phase K bridges
//! mathscape's `Term` to egg's `Language` so future provers can
//! accept rules by semantic equivalence, not just syntactic match.

pub mod adapter;
pub mod antiunify;
pub mod egraph;
pub mod extract;
pub mod meta_gen;

pub use adapter::CompressionGenerator;
pub use meta_gen::{CompositeGenerator, MetaPatternGenerator};
