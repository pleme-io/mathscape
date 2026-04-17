//! Library migration — Phase I of the realization plan.
//!
//! After gates 6 + 7 accept a promotion, `migrate_library` updates
//! the mathscape registry to reflect the new primitive:
//!
//! 1. Every subsumed entry is marked `ProofStatus::Subsumed(promoted)`
//!    in the registry's overlay — append-only, status metadata only
//! 2. The promoted artifact's status advances to `Primitive(identity)`
//! 3. A `MigrationReport` is produced and returned for the caller to
//!    persist into the registry as an Artifact of its own
//!
//! v0 does not rewrite library rule rhs terms to use the new
//! primitive — that is Phase I+. The minimum useful migration is
//! enough to close the loop: subsumed entries become inactive; the
//! promoted artifact becomes a Primitive; the report is auditable.

use crate::epoch::Registry;
use crate::hash::TermRef;
use crate::lifecycle::{AxiomIdentity, ProofStatus};
use crate::promotion::{MigrationReport, PromotionSignal};

/// Apply a successful promotion to the library: mark subsumed
/// entries and advance the promoted artifact's status. Returns a
/// `MigrationReport` capturing the changes.
pub fn migrate_library<R: Registry + ?Sized>(
    registry: &mut R,
    signal: &PromotionSignal,
    primitive: AxiomIdentity,
    epoch_id: u64,
) -> MigrationReport {
    // 1. Mark each subsumed entry.
    for subsumed in &signal.subsumed_hashes {
        registry.mark_status(
            *subsumed,
            ProofStatus::Subsumed(signal.artifact_hash),
        );
    }
    // 2. Advance the promoted artifact's status.
    registry.mark_status(
        signal.artifact_hash,
        ProofStatus::Primitive(primitive.clone()),
    );
    // 3. Build the report. v0: `rewritten` is empty (no rhs term
    //    rewriting yet); `deduplicated` lists subsumed entries.
    MigrationReport::seal(
        primitive,
        Vec::new(),
        signal.subsumed_hashes.clone(),
        epoch_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epoch::{AcceptanceCertificate, Artifact, InMemoryRegistry};
    use crate::eval::RewriteRule;
    use crate::term::Term;

    fn mk_artifact(name: &str, sym: u32) -> Artifact {
        let rule = RewriteRule {
            name: name.into(),
            lhs: Term::Symbol(sym, vec![]),
            rhs: Term::Point(sym as u64),
        };
        Artifact::seal(
            rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        )
    }

    fn sample_identity() -> AxiomIdentity {
        AxiomIdentity {
            target: "mathscape_core::term::Term".into(),
            name: "Promoted".into(),
            proposal_hash: TermRef([0xaa; 32]),
        }
    }

    #[test]
    fn migrate_marks_subsumed_entries() {
        let mut reg = InMemoryRegistry::new();
        let promoted = mk_artifact("promoted", 1);
        let sub1 = mk_artifact("sub1", 2);
        let sub2 = mk_artifact("sub2", 3);
        reg.insert(promoted.clone());
        reg.insert(sub1.clone());
        reg.insert(sub2.clone());

        let signal = PromotionSignal {
            artifact_hash: promoted.content_hash,
            subsumed_hashes: vec![sub1.content_hash, sub2.content_hash],
            cross_corpus_support: vec!["arith".into(), "diff".into()],
            rationale: "test".into(),
            epoch_id: 5,
        };

        let report = migrate_library(&mut reg, &signal, sample_identity(), 5);

        // Subsumed entries carry the new status.
        assert!(matches!(
            reg.status_of(sub1.content_hash),
            Some(ProofStatus::Subsumed(h)) if h == promoted.content_hash
        ));
        assert!(matches!(
            reg.status_of(sub2.content_hash),
            Some(ProofStatus::Subsumed(h)) if h == promoted.content_hash
        ));
        // Promoted artifact advanced to Primitive.
        assert!(matches!(
            reg.status_of(promoted.content_hash),
            Some(ProofStatus::Primitive(_))
        ));
        // Report carries the expected deduplicated list.
        assert_eq!(report.deduplicated.len(), 2);
    }

    #[test]
    fn migrate_is_deterministic() {
        let mut reg_a = InMemoryRegistry::new();
        let mut reg_b = InMemoryRegistry::new();
        let promoted = mk_artifact("promoted", 1);
        let sub = mk_artifact("sub", 2);
        reg_a.insert(promoted.clone());
        reg_a.insert(sub.clone());
        reg_b.insert(promoted.clone());
        reg_b.insert(sub.clone());

        let signal = PromotionSignal {
            artifact_hash: promoted.content_hash,
            subsumed_hashes: vec![sub.content_hash],
            cross_corpus_support: vec!["arith".into()],
            rationale: "test".into(),
            epoch_id: 1,
        };
        let identity = sample_identity();

        let r1 = migrate_library(&mut reg_a, &signal, identity.clone(), 1);
        let r2 = migrate_library(&mut reg_b, &signal, identity, 1);
        assert_eq!(r1.content_hash, r2.content_hash);
    }

    #[test]
    fn migrate_preserves_registry_append_only_semantics() {
        let mut reg = InMemoryRegistry::new();
        let promoted = mk_artifact("p", 1);
        let sub = mk_artifact("s", 2);
        reg.insert(promoted.clone());
        reg.insert(sub.clone());
        let before_len = reg.len();

        let signal = PromotionSignal {
            artifact_hash: promoted.content_hash,
            subsumed_hashes: vec![sub.content_hash],
            cross_corpus_support: vec!["c".into()],
            rationale: "t".into(),
            epoch_id: 0,
        };
        migrate_library(&mut reg, &signal, sample_identity(), 0);
        // Registry size unchanged (append-only, overlay only).
        assert_eq!(reg.len(), before_len);
        // Both artifacts still accessible.
        assert!(reg.find(promoted.content_hash).is_some());
        assert!(reg.find(sub.content_hash).is_some());
    }
}
