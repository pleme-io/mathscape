//! Symbolic compression: anti-unification, library extraction, rewriting.
//!
//! E-graph integration (egg) is a future phase — this module starts with
//! direct anti-unification for pattern discovery.

pub mod adapter;
pub mod antiunify;
pub mod extract;
pub mod meta_gen;

pub use adapter::CompressionGenerator;
pub use meta_gen::{CompositeGenerator, MetaPatternGenerator};
