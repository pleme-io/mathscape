//! Events — the single primitive of mathscape motion.
//!
//! Every motion of the machine is a typed `Event`. Epochs, traces,
//! reward totals, regime detection — all derive from the event stream.
//! See `docs/arch/machine-synthesis.md`.
//!
//! Each event carries a `delta_dl: f64` — its contribution to the total
//! description length saved. The epoch's unified score
//! `V(epoch) = Σ event.delta_dl()` is the single scalar the allocator
//! reasons about.

use crate::eval::RewriteRule;
use crate::hash::TermRef;
use crate::lifecycle::{DemotionReason, ProofStatus};
use crate::promotion::{MigrationReport, PromotionSignal};
use serde::{Deserialize, Serialize};

use crate::epoch::{Artifact, Candidate, Rejection};

/// Which of the three passes this event belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventCategory {
    Discovery,
    Reinforce,
    Promote,
    /// Regime-transition or other cross-cutting events.
    Meta,
}

/// A status transition applied to an existing library entry by the
/// reinforcement pass (gates V / X / A).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusAdvance {
    pub artifact: TermRef,
    pub from: ProofStatus,
    pub to: ProofStatus,
    /// e-graph saturation id, Lean proof hash, canonicality window id —
    /// depends on the gate being crossed.
    pub evidence_hash: TermRef,
    pub delta_dl: f64,
}

/// Every motion the machine makes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    // ── Discovery pass ──────────────────────────────────────────────
    Proposal {
        candidate: Candidate,
        /// ΔDL for having produced a proposal at all — typically 0;
        /// positive only for proposals that beat a baseline.
        delta_dl: f64,
    },
    Reject {
        candidate_hash: TermRef,
        reasons: Vec<Rejection>,
    },
    Accept {
        artifact: Artifact,
        delta_dl: f64,
    },

    // ── Reinforcement pass ──────────────────────────────────────────
    StatusAdvance(StatusAdvance),
    Merge {
        kept: TermRef,
        merged: TermRef,
        delta_dl: f64,
    },
    Subsumption {
        absorbed: TermRef,
        subsumer: TermRef,
        delta_dl: f64,
    },
    Canonicalize {
        artifact: TermRef,
        rewritten: RewriteRule,
        delta_dl: f64,
    },

    // ── Promotion pass ──────────────────────────────────────────────
    Promote {
        signal: PromotionSignal,
        delta_dl: f64,
    },
    Migrate {
        report: MigrationReport,
        delta_dl: f64,
    },
    Demote {
        artifact: TermRef,
        reason: DemotionReason,
        delta_dl: f64,
    },

    // ── Meta ────────────────────────────────────────────────────────
    RegimeTransition {
        from: crate::control::Regime,
        to: crate::control::Regime,
        epoch_id: u64,
    },
}

impl Event {
    /// The ΔDL contributed by this event, in bits.
    #[must_use]
    pub fn delta_dl(&self) -> f64 {
        match self {
            Event::Proposal { delta_dl, .. }
            | Event::Accept { delta_dl, .. }
            | Event::Merge { delta_dl, .. }
            | Event::Subsumption { delta_dl, .. }
            | Event::Canonicalize { delta_dl, .. }
            | Event::Promote { delta_dl, .. }
            | Event::Migrate { delta_dl, .. }
            | Event::Demote { delta_dl, .. } => *delta_dl,
            Event::StatusAdvance(sa) => sa.delta_dl,
            Event::Reject { .. } | Event::RegimeTransition { .. } => 0.0,
        }
    }

    /// Which pass emitted this event.
    #[must_use]
    pub fn category(&self) -> EventCategory {
        match self {
            Event::Proposal { .. }
            | Event::Reject { .. }
            | Event::Accept { .. } => EventCategory::Discovery,
            Event::StatusAdvance(_)
            | Event::Merge { .. }
            | Event::Subsumption { .. }
            | Event::Canonicalize { .. } => EventCategory::Reinforce,
            Event::Promote { .. }
            | Event::Migrate { .. }
            | Event::Demote { .. } => EventCategory::Promote,
            Event::RegimeTransition { .. } => EventCategory::Meta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::Regime;
    use crate::lifecycle::ProofStatus;

    fn h(b: u8) -> TermRef {
        TermRef([b; 32])
    }

    #[test]
    fn delta_dl_reports_payload_for_scoring_variants() {
        let sa = StatusAdvance {
            artifact: h(1),
            from: ProofStatus::Conjectured,
            to: ProofStatus::Verified,
            evidence_hash: h(2),
            delta_dl: 0.75,
        };
        assert_eq!(Event::StatusAdvance(sa).delta_dl(), 0.75);
        assert_eq!(
            Event::Merge {
                kept: h(1),
                merged: h(2),
                delta_dl: 2.0,
            }
            .delta_dl(),
            2.0
        );
    }

    #[test]
    fn reject_and_regime_carry_zero_delta_dl() {
        assert_eq!(
            Event::Reject {
                candidate_hash: h(3),
                reasons: vec![],
            }
            .delta_dl(),
            0.0
        );
        assert_eq!(
            Event::RegimeTransition {
                from: Regime::Reductive,
                to: Regime::Explosive,
                epoch_id: 42,
            }
            .delta_dl(),
            0.0
        );
    }

    #[test]
    fn category_partitions_events_correctly() {
        assert_eq!(
            Event::Reject {
                candidate_hash: h(1),
                reasons: vec![]
            }
            .category(),
            EventCategory::Discovery
        );
        assert_eq!(
            Event::Merge {
                kept: h(1),
                merged: h(2),
                delta_dl: 1.0
            }
            .category(),
            EventCategory::Reinforce
        );
        assert_eq!(
            Event::Demote {
                artifact: h(1),
                reason: DemotionReason::StaleConjecture,
                delta_dl: 0.0,
            }
            .category(),
            EventCategory::Promote
        );
        assert_eq!(
            Event::RegimeTransition {
                from: Regime::Reductive,
                to: Regime::Explosive,
                epoch_id: 0,
            }
            .category(),
            EventCategory::Meta
        );
    }
}
