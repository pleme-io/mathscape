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
    derive_laws_validated, derive_laws_with_cache,
    derive_laws_with_subterm_au, is_empirically_valid, is_rank2_shape,
    rank2_candidates_from_library, validate_candidates,
    validate_candidates_ext, Domain, LawGenStats, MemoizingAntiUnifier,
};
pub use antiunify::paired_subterm_anti_unify;
pub use meta_gen::{CompositeGenerator, MetaPatternGenerator};
