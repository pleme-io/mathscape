//! # mathscape-acquisition — the garimpo acquisition-space typescape
//!
//! Realizes [`theory/GARIMPO.md §3.5`](https://github.com/pleme-io/theory/blob/main/GARIMPO.md)
//! as a real **TYPED-SPEC + INTERPRETER TRIPLET**: a typed Rust border
//! (`types`), a `(defacquisition …)` tatara-lisp authoring surface
//! (`spec` + `spec_lisp`), and a working interpreter behind a mockable
//! environment (`interp`).
//!
//! ## The one thesis: "fully known" = "fully typed"
//!
//! The acquisition space is **fully known because it is fully typed** —
//! you know exactly what a valid acquisition is, and every invalid one
//! has **no constructor**:
//!
//! - a [`Layer`](types::Layer) is substrate-ground **only if sealed**
//!   (INV-1 — no `Layer` without a [`SealWitness`](types::SealWitness));
//! - a [`FoldBack`](types::FoldBack) is lossless **only if witnessed**
//!   (INV-2 — no `FoldBack` without a
//!   [`SemanticsPreservingWitness`](types::SemanticsPreservingWitness));
//! - an [`AbsorbDecision`](types::AbsorbDecision) constructs **only** on
//!   net-negative ΔDL (§1.5 significance-as-a-type);
//! - a [`Frontier`](types::Frontier) has **no `Closed` variant** —
//!   incompleteness (Chaitin, §1.5) as a type;
//! - an [`AcquisitionDAG`](types::AcquisitionDAG) is content-addressed +
//!   parent-linked with total `restore`/`fork` — a lost state is
//!   unrepresentable.
//!
//! ## What is a real type today vs a typed gap
//!
//! **Real types today** (buildable, tested): `AbsorbDecision::try_new`
//! (DL-gated), `Frontier` (no `Closed`), `AcquisitionDAG` (reversible),
//! and the `Sample`/`TrustedPrimitive` **floor** constructors of the
//! two witnesses.
//!
//! **Typed gaps** (the destination, per the no-silent-stub rule):
//! [`SealWitness::Proof`](types::SealWitness) (saturation/Lean) and
//! [`SemanticsPreservingWitness::EquivalenceChecked`](types::SemanticsPreservingWitness)
//! (per-fold-back `check_equivalence`) are reached only through the
//! interpreter, which returns
//! [`SpecError::Interp { phase }`](types::SpecError) — a **typed error
//! naming the gap**, never a fabricated pass (`lean_export.rs` is a
//! `sorry`-stub). The honest floor is thus *legible in the type*: both
//! constructors exist, so the weak tier is visible, never rounded up.
//!
//! ## Reuse, not re-implementation
//!
//! Wraps `mathscape_core::eval::RewriteRule`; the DAG reuses the BLAKE3
//! `mathscape_core::hash::TermRef` content-addressing shape the shipped
//! `Artifact` `parent_hashes` DAG uses (`epoch.rs`); the MDL currency is
//! the same `AcceptanceCertificate` ratios (`epoch.rs`). This crate adds
//! the *typed acquisition invariants* on top; it forks nothing.
//!
//! ## Honest limit on the Lisp surface
//!
//! `spec_lisp` is acquisition's own reader over `tatara_lisp::ast::Sexp`
//! (the same `to_sexp`/`from_sexp` shape the mechanism/proof layers use),
//! not the shared `#[derive(TataraDomain)]` runtime registration
//! machinery. Swapping it in once that ships is a named follow-up, not a
//! claim it is used here.

pub mod catalog;
pub mod interp;
pub mod spec;
pub mod spec_lisp;
pub mod types;

pub use catalog::{catalog, AcquisitionPrimitive};
pub use interp::{apply, AcquisitionEnvironment, AcquisitionOutcome, StepReport};
pub use spec::{AcquisitionOp, AcquisitionSpec};
pub use types::{
    AbsorbDecision, AcquisitionDAG, AcquisitionState, Complexity, Dl, FoldBack, Frontier, Layer,
    ProofKind, SealWitness, SemanticsPreservingWitness, SpecError, Tier,
};
