//! Demotion — the symmetric force that makes the system convergent
//! rather than just cumulative.
//!
//! Phase J of the realization plan. See `docs/arch/demotion-pipeline.md`.
//!
//! - Library demotion fires automatically (no operator review) when
//!   a Conjectured-or-higher entry has been idle for W epochs.
//! - Primitive demotion fires manually after an operator reviews the
//!   `DemotionCandidate` emitted by the gate.
//!
//! v0 implements both gates + the mark operation. The CLI-level
//! operator-approval workflow is Phase J+ UI.

use crate::epoch::Registry;
use crate::hash::TermRef;
use crate::lifecycle::{DemotionReason, ProofStatus};
use crate::promotion_gate::ArtifactHistory;
use serde::{Deserialize, Serialize};

/// Evidence presented to an operator (or an automatic pipeline) to
/// justify demoting an artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemotionCandidate {
    pub artifact_hash: TermRef,
    pub current_status: ProofStatus,
    pub proposed_reason: DemotionReason,
    pub rationale: String,
    pub epoch_id: u64,
    pub history: ArtifactHistory,
}

/// A DemotionGate evaluates (artifact_hash, current_status, history)
/// and decides whether to emit a `DemotionCandidate`.
pub trait DemotionGate {
    fn evaluate(
        &self,
        artifact_hash: TermRef,
        current_status: &ProofStatus,
        history: &ArtifactHistory,
        epoch_id: u64,
    ) -> Option<DemotionCandidate>;
}

/// Fires a demotion candidate when usage across the window falls
/// below floor M, provided the artifact has been alive for at least
/// W epochs (prevents premature demotion of freshly-proposed rules).
#[derive(Debug, Clone, Copy)]
pub struct UsageFloorGate {
    pub m_demotion_floor: u64,
    pub w_grace_period: u64,
}

impl UsageFloorGate {
    #[must_use]
    pub fn new(m_demotion_floor: u64, w_grace_period: u64) -> Self {
        Self { m_demotion_floor, w_grace_period }
    }
}

impl DemotionGate for UsageFloorGate {
    fn evaluate(
        &self,
        artifact_hash: TermRef,
        current_status: &ProofStatus,
        history: &ArtifactHistory,
        epoch_id: u64,
    ) -> Option<DemotionCandidate> {
        // Grace period: the artifact must have been alive long enough
        // to have accrued usage.
        if history.epochs_alive < self.w_grace_period {
            return None;
        }
        if history.usage_in_window >= self.m_demotion_floor {
            return None;
        }
        // Already demoted / subsumed — don't re-demote.
        if matches!(
            current_status,
            ProofStatus::Demoted(_) | ProofStatus::Subsumed(_)
        ) {
            return None;
        }
        let proposed_reason = match current_status {
            ProofStatus::Primitive(_) => DemotionReason::RetiredPrimitive,
            ProofStatus::Conjectured => DemotionReason::StaleConjecture,
            _ => DemotionReason::UnusedExport,
        };
        Some(DemotionCandidate {
            artifact_hash,
            current_status: current_status.clone(),
            proposed_reason,
            rationale: format!(
                "usage_in_window={} < floor {} after {} epochs alive",
                history.usage_in_window, self.m_demotion_floor, history.epochs_alive
            ),
            epoch_id,
            history: history.clone(),
        })
    }
}

/// Apply a demotion to the registry. This is append-only: the
/// artifact is not removed, only its overlay status is changed to
/// `Demoted(reason)`.
///
/// For primitive demotion, the caller additionally asks axiom-forge
/// to emit a `#[deprecated]` shim (via the bridge); that step is
/// outside this function.
pub fn demote_artifact<R: Registry + ?Sized>(
    registry: &mut R,
    candidate: &DemotionCandidate,
) {
    registry.mark_status(
        candidate.artifact_hash,
        ProofStatus::Demoted(candidate.proposed_reason.clone()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epoch::{AcceptanceCertificate, Artifact, InMemoryRegistry};
    use crate::eval::RewriteRule;
    use crate::promotion_gate::ArtifactHistory;
    use crate::term::Term;
    use std::collections::BTreeSet;

    fn mk_artifact(sym: u32) -> Artifact {
        let rule = RewriteRule {
            name: format!("r{sym}"),
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

    fn hist(epochs_alive: u64, usage: u64) -> ArtifactHistory {
        ArtifactHistory {
            corpus_matches: BTreeSet::new(),
            epochs_alive,
            usage_in_window: usage,
        }
    }

    #[test]
    fn grace_period_prevents_premature_demotion() {
        let gate = UsageFloorGate::new(1, 50);
        let h = hist(10, 0);
        let out = gate.evaluate(
            TermRef([0; 32]),
            &ProofStatus::Conjectured,
            &h,
            100,
        );
        assert!(out.is_none());
    }

    #[test]
    fn low_usage_past_grace_period_fires_candidate() {
        let gate = UsageFloorGate::new(5, 50);
        let h = hist(100, 2);
        let out = gate.evaluate(
            TermRef([0; 32]),
            &ProofStatus::Conjectured,
            &h,
            150,
        );
        assert!(out.is_some());
        let cand = out.unwrap();
        assert!(matches!(
            cand.proposed_reason,
            DemotionReason::StaleConjecture
        ));
    }

    #[test]
    fn primitive_status_yields_retired_primitive_reason() {
        use crate::lifecycle::AxiomIdentity;
        let gate = UsageFloorGate::new(5, 50);
        let h = hist(100, 0);
        let identity = AxiomIdentity {
            target: "t::T".into(),
            name: "X".into(),
            proposal_hash: TermRef([0; 32]),
            typescape_coord: crate::lifecycle::TypescapeCoord::precommit("t::T", "X"),
        };
        let out = gate
            .evaluate(
                TermRef([0; 32]),
                &ProofStatus::Primitive(identity),
                &h,
                150,
            )
            .unwrap();
        assert!(matches!(
            out.proposed_reason,
            DemotionReason::RetiredPrimitive
        ));
    }

    #[test]
    fn already_demoted_or_subsumed_is_not_re_demoted() {
        let gate = UsageFloorGate::new(5, 50);
        let h = hist(100, 0);
        assert!(gate
            .evaluate(
                TermRef([0; 32]),
                &ProofStatus::Demoted(DemotionReason::StaleConjecture),
                &h,
                150,
            )
            .is_none());
        assert!(gate
            .evaluate(
                TermRef([0; 32]),
                &ProofStatus::Subsumed(TermRef([1; 32])),
                &h,
                150,
            )
            .is_none());
    }

    #[test]
    fn sufficient_usage_never_fires() {
        let gate = UsageFloorGate::new(5, 50);
        let h = hist(100, 20);
        let out = gate.evaluate(
            TermRef([0; 32]),
            &ProofStatus::Exported,
            &h,
            150,
        );
        assert!(out.is_none());
    }

    #[test]
    fn demote_marks_status_in_registry() {
        let mut reg = InMemoryRegistry::new();
        let art = mk_artifact(1);
        let hash = art.content_hash;
        reg.insert(art);
        let cand = DemotionCandidate {
            artifact_hash: hash,
            current_status: ProofStatus::Conjectured,
            proposed_reason: DemotionReason::StaleConjecture,
            rationale: "test".into(),
            epoch_id: 0,
            history: hist(0, 0),
        };
        demote_artifact(&mut reg, &cand);
        assert!(matches!(
            reg.status_of(hash),
            Some(ProofStatus::Demoted(DemotionReason::StaleConjecture))
        ));
    }

    #[test]
    fn demotion_preserves_registry_length() {
        let mut reg = InMemoryRegistry::new();
        let art = mk_artifact(1);
        let hash = art.content_hash;
        reg.insert(art);
        let before = reg.len();
        let cand = DemotionCandidate {
            artifact_hash: hash,
            current_status: ProofStatus::Conjectured,
            proposed_reason: DemotionReason::StaleConjecture,
            rationale: "t".into(),
            epoch_id: 0,
            history: hist(0, 0),
        };
        demote_artifact(&mut reg, &cand);
        assert_eq!(reg.len(), before);
    }
}
