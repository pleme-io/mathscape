//! Proof-status lifecycle — the monotone lattice every rule walks.
//!
//! See `docs/arch/machine-synthesis.md` for the canonical picture. The
//! lifecycle is:
//!
//! ```text
//!     Proposed
//!        │  gates 1–3
//!        ▼
//!    Conjectured ──── fails advance for W epochs ──► Demoted(stale)
//!        │  gate V (e-graph equivalence)
//!        ▼
//!     Verified ──────── subsumed by another rule ──► Subsumed(hash)
//!        │  gate X (Lean 4 proof exported)
//!        ▼
//!     Exported ─── usage drops below M for W ───► Demoted(unused)
//!        │  gate A (canonical form stable W epochs)
//!        ▼
//!    Axiomatized
//!        │  gates 4–5
//!        ▼
//!     Promoted
//!        │  gates 6–7 (axiom-forge + rustc)
//!        ▼
//!    Primitive ─── usage across corpora falls ────► Demoted(retired)
//! ```
//!
//! Only forward motion up this chain is legal. Subsumed / Demoted are
//! terminal in this lifecycle, though a new `Candidate` with the same
//! rule shape may be re-proposed in a later epoch and start again at
//! `Proposed`.

use crate::hash::TermRef;
use serde::{Deserialize, Serialize};

/// Identity of a primitive that has been promoted through axiom-forge
/// and accepted by rustc. Links a mathscape Artifact to the Rust enum
/// variant axiom-forge emitted.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AxiomIdentity {
    /// Target Rust path, e.g. `"mathscape_core::term::Term"`.
    pub target: String,
    /// PascalCase variant name, e.g. `"IdentityElement"`.
    pub name: String,
    /// axiom-forge's `Certificate::proposal_hash` at acceptance time.
    /// The chain back to the originating PromotionSignal is
    /// reconstructable from the registry.
    pub proposal_hash: TermRef,
}

/// Why a rule left the active lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DemotionReason {
    /// Conjectured for W epochs with no status advance.
    StaleConjecture,
    /// Exported or Axiomatized but usage fell below M for W epochs.
    UnusedExport,
    /// Primitive usage across all corpora fell below M.
    RetiredPrimitive,
    /// Replaced by a different primitive that does strictly more.
    Superseded(TermRef),
}

/// Full proof-status lifecycle.
///
/// The lattice in module-level docs is enforced at the API level by
/// `Registry::supersede`-style transitions. The enum is used in storage
/// + trace serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofStatus {
    /// Just proposed; has not yet run gates 1–3.
    Proposed,
    /// Cleared local gates (compression, coverage, irreducibility) but
    /// equivalence is only empirical.
    Conjectured,
    /// Equivalence confirmed by e-graph saturation (gate V).
    Verified,
    /// Formal proof emitted and accepted by Lean 4 (gate X).
    Exported,
    /// Canonical form stable across a W-epoch window (gate A).
    Axiomatized,
    /// Another rule in the library subsumes this one. Points at the
    /// subsumer's content hash.
    Subsumed(TermRef),
    /// Cleared gates 4–5 and has been handed to axiom-forge. Awaiting
    /// gates 6–7.
    Promoted,
    /// Accepted by axiom-forge (gate 6) and rustc (gate 7); lives as
    /// a first-class Rust type.
    Primitive(AxiomIdentity),
    /// Left the active lifecycle.
    Demoted(DemotionReason),
}

impl ProofStatus {
    /// Numeric rank used to enforce monotone advance. Subsumed / Demoted
    /// share a terminal rank.
    #[must_use]
    pub fn rank(&self) -> u8 {
        match self {
            ProofStatus::Proposed => 0,
            ProofStatus::Conjectured => 1,
            ProofStatus::Verified => 2,
            ProofStatus::Exported => 3,
            ProofStatus::Axiomatized => 4,
            ProofStatus::Promoted => 5,
            ProofStatus::Primitive(_) => 6,
            ProofStatus::Subsumed(_) | ProofStatus::Demoted(_) => u8::MAX,
        }
    }

    /// True if `next` is a legal transition from `self`. Demotion /
    /// subsumption are always legal. Forward advance must be strictly
    /// greater rank.
    #[must_use]
    pub fn can_advance_to(&self, next: &ProofStatus) -> bool {
        match next {
            ProofStatus::Subsumed(_) | ProofStatus::Demoted(_) => true,
            _ => self.rank() < next.rank(),
        }
    }

    /// Whether this status represents a rule that is actively
    /// contributing to the library's coverage.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !matches!(
            self,
            ProofStatus::Subsumed(_) | ProofStatus::Demoted(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_hash(byte: u8) -> TermRef {
        TermRef([byte; 32])
    }

    #[test]
    fn forward_ranks_are_strictly_increasing() {
        let chain = [
            ProofStatus::Proposed,
            ProofStatus::Conjectured,
            ProofStatus::Verified,
            ProofStatus::Exported,
            ProofStatus::Axiomatized,
            ProofStatus::Promoted,
            ProofStatus::Primitive(AxiomIdentity {
                target: "t::T".into(),
                name: "X".into(),
                proposal_hash: fake_hash(0),
            }),
        ];
        for w in chain.windows(2) {
            assert!(w[0].rank() < w[1].rank(), "rank not monotone at {:?}", w);
        }
    }

    #[test]
    fn subsumption_and_demotion_share_terminal_rank() {
        assert_eq!(
            ProofStatus::Subsumed(fake_hash(1)).rank(),
            u8::MAX,
        );
        assert_eq!(
            ProofStatus::Demoted(DemotionReason::StaleConjecture).rank(),
            u8::MAX,
        );
    }

    #[test]
    fn advance_forbids_regress() {
        assert!(ProofStatus::Conjectured
            .can_advance_to(&ProofStatus::Verified));
        assert!(!ProofStatus::Verified
            .can_advance_to(&ProofStatus::Conjectured));
        assert!(!ProofStatus::Verified
            .can_advance_to(&ProofStatus::Verified));
    }

    #[test]
    fn terminal_transitions_are_always_legal() {
        assert!(ProofStatus::Proposed
            .can_advance_to(&ProofStatus::Subsumed(fake_hash(2))));
        assert!(ProofStatus::Axiomatized
            .can_advance_to(&ProofStatus::Demoted(DemotionReason::UnusedExport)));
    }

    #[test]
    fn is_active_correct() {
        assert!(ProofStatus::Conjectured.is_active());
        assert!(!ProofStatus::Subsumed(fake_hash(3)).is_active());
        assert!(!ProofStatus::Demoted(DemotionReason::StaleConjecture).is_active());
    }

    #[test]
    fn status_serde_round_trips() {
        let original = ProofStatus::Primitive(AxiomIdentity {
            target: "mathscape_core::term::Term".into(),
            name: "IdentityElement".into(),
            proposal_hash: fake_hash(0xab),
        });
        let bytes = bincode::serialize(&original).unwrap();
        let decoded: ProofStatus = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }
}
