//! Promotion + migration types — the mathscape ↔ axiom-forge boundary.
//!
//! See `docs/arch/promotion-pipeline.md` and
//! `docs/arch/machine-synthesis.md`. A PromotionSignal is emitted when
//! an Artifact clears gates 4 and 5 (condensation K + cross-corpus N).
//! axiom-forge runs gates 6 + 7 and returns. On success mathscape emits
//! a MigrationReport capturing how the library contracted.

use crate::hash::TermRef;
use crate::lifecycle::AxiomIdentity;
use serde::{Deserialize, Serialize};

/// An opaque corpus identifier. A "corpus" is a source of terms
/// (arithmetic, combinator calculus, symbolic differentiation, etc.).
/// Cross-corpus support is tallied against this id. v0: `String`;
/// upgrade later to a strong type if needed.
pub type CorpusId = String;

/// Signal that an Artifact is ready for promotion to a Rust primitive.
/// Emitted by `PromotionGate` after gates 4 and 5 clear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionSignal {
    /// The Artifact being promoted.
    pub artifact_hash: TermRef,
    /// Library entries this artifact subsumes (gate 4 evidence).
    pub subsumed_hashes: Vec<TermRef>,
    /// Distinct corpora where this artifact has matched (gate 5 evidence).
    pub cross_corpus_support: Vec<CorpusId>,
    /// Human-readable "why this, why now" — becomes the doc string on
    /// the axiom-forge AxiomProposal.
    pub rationale: String,
    /// Epoch that emitted this signal.
    pub epoch_id: u64,
}

impl PromotionSignal {
    /// Canonical content hash of the signal itself. The signal is an
    /// event in the derivation DAG, and its hash is what axiom-forge
    /// stores as the upstream proposal identity.
    #[must_use]
    pub fn content_hash(&self) -> TermRef {
        let bytes = bincode::serialize(self)
            .expect("PromotionSignal::content_hash: bincode serialization infallible");
        TermRef::from_bytes(&bytes)
    }
}

/// Emitted on successful gate-7 acceptance. Records how the library
/// contracted around a newly-added primitive.
///
/// A MigrationReport is itself content-addressed and enters the Registry
/// as an Artifact. The Merkle DAG therefore records promotions as
/// first-class events, not metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationReport {
    /// The primitive that caused the migration.
    pub primitive: AxiomIdentity,
    /// Library entries whose rhs was rewritten to reference the new
    /// primitive (still present, now shorter).
    pub rewritten: Vec<TermRef>,
    /// Library entries removed as structurally redundant after rewrite.
    pub deduplicated: Vec<TermRef>,
    /// Epoch in which the migration was performed.
    pub epoch_id: u64,
    /// BLAKE3 over canonical bincode of the fields above.
    pub content_hash: TermRef,
}

impl MigrationReport {
    /// Build a report and compute its content hash.
    #[must_use]
    pub fn seal(
        primitive: AxiomIdentity,
        rewritten: Vec<TermRef>,
        deduplicated: Vec<TermRef>,
        epoch_id: u64,
    ) -> Self {
        let payload = (primitive.clone(), rewritten.clone(), deduplicated.clone(), epoch_id);
        let bytes = bincode::serialize(&payload)
            .expect("MigrationReport::seal: bincode serialization infallible");
        let content_hash = TermRef::from_bytes(&bytes);
        Self {
            primitive,
            rewritten,
            deduplicated,
            epoch_id,
            content_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_hash(byte: u8) -> TermRef {
        TermRef([byte; 32])
    }

    #[test]
    fn signal_content_hash_deterministic() {
        let a = PromotionSignal {
            artifact_hash: fake_hash(1),
            subsumed_hashes: vec![fake_hash(2), fake_hash(3)],
            cross_corpus_support: vec!["arith".into(), "diff".into()],
            rationale: "condenses three identity rules".into(),
            epoch_id: 17,
        };
        let b = a.clone();
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn signal_content_hash_distinguishes() {
        let base = PromotionSignal {
            artifact_hash: fake_hash(1),
            subsumed_hashes: vec![fake_hash(2)],
            cross_corpus_support: vec!["arith".into()],
            rationale: "x".into(),
            epoch_id: 0,
        };
        let mut changed = base.clone();
        changed.epoch_id = 1;
        assert_ne!(base.content_hash(), changed.content_hash());
    }

    #[test]
    fn migration_report_seal_hashes_inputs() {
        let primitive = AxiomIdentity {
            target: "mathscape_core::term::Term".into(),
            name: "IdentityElement".into(),
            proposal_hash: fake_hash(0xff),
        };
        let r1 = MigrationReport::seal(
            primitive.clone(),
            vec![fake_hash(10), fake_hash(11)],
            vec![fake_hash(12)],
            42,
        );
        let r2 = MigrationReport::seal(
            primitive,
            vec![fake_hash(10), fake_hash(11)],
            vec![fake_hash(12)],
            42,
        );
        assert_eq!(r1.content_hash, r2.content_hash);
    }

    #[test]
    fn migration_report_distinct_inputs_distinct_hashes() {
        let primitive = AxiomIdentity {
            target: "t::T".into(),
            name: "X".into(),
            proposal_hash: fake_hash(0),
        };
        let r1 = MigrationReport::seal(primitive.clone(), vec![fake_hash(1)], vec![], 0);
        let r2 = MigrationReport::seal(primitive, vec![fake_hash(2)], vec![], 0);
        assert_ne!(r1.content_hash, r2.content_hash);
    }

    #[test]
    fn signal_serde_round_trips() {
        let original = PromotionSignal {
            artifact_hash: fake_hash(5),
            subsumed_hashes: vec![fake_hash(6)],
            cross_corpus_support: vec!["c1".into()],
            rationale: "ok".into(),
            epoch_id: 3,
        };
        let bytes = bincode::serialize(&original).unwrap();
        let decoded: PromotionSignal = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded, original);
    }
}
