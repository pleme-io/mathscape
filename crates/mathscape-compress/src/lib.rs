//! Symbolic compression: anti-unification, library extraction, rewriting.
//!
//! E-graph integration (`egg`) lives in `egraph` — phase K bridges
//! mathscape's `Term` to egg's `Language` so future provers can
//! accept rules by semantic equivalence, not just syntactic match.

pub mod adapter;
pub mod antiunify;
pub mod egraph;
pub mod extract;
pub mod law_generator;
pub mod meta_gen;

pub use adapter::CompressionGenerator;
pub use law_generator::{
    derive_laws_from_corpus, derive_laws_from_corpus_instrumented,
    derive_laws_with_cache, LawGenStats, MemoizingAntiUnifier,
};
pub use meta_gen::{CompositeGenerator, MetaPatternGenerator};
