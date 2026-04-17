//! `PrimitiveGrowth` — the generic trait surface shared by every
//! substrate-extending convergence system in the pleme-io platform.
//!
//! This crate intentionally has **zero dependencies**. Domain-specific
//! crates (`mathscape-core`, a future `ml-forge::primitive`, a future
//! `iac-forge::primitive`) implement this trait with their own
//! concrete types. The trait is the *shared pattern*; concrete types
//! stay in their domain crate so each domain keeps its own proofs,
//! serde choices, and storage contracts.
//!
//! See `mathscape/docs/arch/rust-lisp-duality.md` and
//! `mathscape/docs/arch/in-memory-convergence-layering.md` for the
//! theoretical motivation.
//!
//! # The four roles and the five associated types
//!
//! ```text
//!        Generator  ──► Candidate ──► Prover  ──► Verdict(Cert)
//!                                                     │
//!                                                     ▼
//!                                                   Emitter
//!                                                     │
//!                                                     ▼
//!                                                  Artifact ──► Registry (append-only)
//! ```
//!
//! A domain implements `PrimitiveGrowth` by choosing:
//!
//!   - `type Proposal`       — the candidate it proposes
//!   - `type Certificate`    — the evidence of acceptance
//!   - `type Violations`     — the failure evidence
//!   - `type Artifact`       — the content-addressed output
//!   - `type CommittedId`    — the id returned on successful insert
//!
//! # Why not include types here
//!
//! Every domain's Certificate / Artifact / Registry has domain-
//! specific fields (mathscape's `condensation_ratio`, ml-forge's
//! `shape_rule`, iac-forge's `compliance_lattice`). Generic types in
//! this crate would either be too abstract to be useful, or would
//! bake in assumptions no domain can honor. Trait only is the right
//! level.
//!
//! # Minimal scope
//!
//! This crate defines:
//!
//!  - `PrimitiveGrowth` trait (4 required methods + 5 associated types)
//!  - `Step` enum parameterizing the loop semantics (Propose / Prove
//!    / Emit / Register)
//!  - `GrowthOutcome` enum for per-step results
//!
//! That's intentionally everything. Later upgrades may add a
//! `RegistryRoot` trait + generic `Merkle` helpers, but only if
//! concrete domains agree on them.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;
use alloc::vec::Vec;

/// The generic trait every substrate-extending convergence system
/// implements. A single instance of `PrimitiveGrowth` advances the
/// loop by one step when `step()` is called.
pub trait PrimitiveGrowth {
    /// The candidate shape this domain proposes.
    type Proposal;
    /// Evidence of acceptance (domain-specific reward axes + status).
    type Certificate;
    /// Evidence of rejection (one or more constraint violations).
    type Violations;
    /// The content-addressed accepted output.
    type Artifact;
    /// The id returned on successful registry insert — usually the
    /// artifact's content hash.
    type CommittedId;

    /// Generator-side: propose candidates from the current context.
    /// The context is implicit in `self` (corpus, state, etc.).
    fn propose(&mut self) -> Vec<Self::Proposal>;

    /// Prover-side: decide whether a proposal is acceptable.
    fn prove(&self, proposal: &Self::Proposal) -> GrowthOutcome<Self::Certificate, Self::Violations>;

    /// Emitter-side: materialize an accepted proposal as an artifact.
    /// Returns `None` if the domain's emitter decides not to emit
    /// despite the certificate (rare; e.g., duplicate detection).
    fn emit(&self, proposal: &Self::Proposal, certificate: &Self::Certificate) -> Option<Self::Artifact>;

    /// Registry-side: append the artifact and return its identity.
    fn register(&mut self, artifact: Self::Artifact) -> Self::CommittedId;

    /// Orchestration default: run propose → prove → emit → register
    /// for one candidate. Overridable when domains need custom
    /// batching or per-proposal tracing.
    fn step(&mut self) -> Vec<StepResult<Self::CommittedId, Self::Violations>> {
        let proposals = self.propose();
        let mut results = Vec::with_capacity(proposals.len());
        for proposal in &proposals {
            match self.prove(proposal) {
                GrowthOutcome::Accept(cert) => match self.emit(proposal, &cert) {
                    Some(artifact) => {
                        let id = self.register(artifact);
                        results.push(StepResult::Registered(id));
                    }
                    None => results.push(StepResult::EmittedNone),
                },
                GrowthOutcome::Reject(violations) => {
                    results.push(StepResult::Rejected(violations));
                }
            }
        }
        results
    }
}

/// The prover's verdict on a proposal.
pub enum GrowthOutcome<Cert, Viol> {
    Accept(Cert),
    Reject(Viol),
}

/// What happened to a proposal during `step()`.
pub enum StepResult<Id, Viol> {
    /// Accepted + emitted + registered with the returned id.
    Registered(Id),
    /// Accepted but the emitter chose not to emit (e.g., duplicate).
    EmittedNone,
    /// Rejected by the prover.
    Rejected(Viol),
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec;

    /// Trivial in-memory implementation to prove the trait is
    /// instantiable. Accepts every positive integer, emits its square,
    /// registers by index.
    struct Squares {
        queue: Vec<i32>,
        emitted: Vec<i32>,
    }

    impl PrimitiveGrowth for Squares {
        type Proposal = i32;
        type Certificate = i32;
        type Violations = &'static str;
        type Artifact = i32;
        type CommittedId = usize;

        fn propose(&mut self) -> Vec<Self::Proposal> {
            core::mem::take(&mut self.queue)
        }
        fn prove(&self, p: &Self::Proposal) -> GrowthOutcome<Self::Certificate, Self::Violations> {
            if *p > 0 {
                GrowthOutcome::Accept(*p)
            } else {
                GrowthOutcome::Reject("non-positive")
            }
        }
        fn emit(&self, p: &Self::Proposal, _c: &Self::Certificate) -> Option<Self::Artifact> {
            Some(*p * *p)
        }
        fn register(&mut self, a: Self::Artifact) -> Self::CommittedId {
            self.emitted.push(a);
            self.emitted.len() - 1
        }
    }

    #[test]
    fn default_step_runs_full_loop() {
        let mut s = Squares {
            queue: vec![1, -2, 3],
            emitted: Vec::new(),
        };
        let results = s.step();
        assert_eq!(results.len(), 3);
        assert!(matches!(results[0], StepResult::Registered(0)));
        assert!(matches!(results[1], StepResult::Rejected("non-positive")));
        assert!(matches!(results[2], StepResult::Registered(1)));
        assert_eq!(s.emitted, vec![1, 9]);
    }

    #[test]
    fn empty_queue_produces_empty_results() {
        let mut s = Squares {
            queue: vec![],
            emitted: vec![],
        };
        assert!(s.step().is_empty());
    }
}
