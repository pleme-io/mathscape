//! Trap — a content-addressed fixed point of the mathscape machine.
//!
//! See `docs/arch/fixed-point-convergence.md`. A Trap is a registry
//! state that has been stable for W epochs: the dispatcher saw no
//! new accepts, no reinforce advances, no promotions, no demotions.
//! Traps are first-class artifacts — content-hashed, shareable,
//! replayable.

use crate::hash::TermRef;
use crate::promotion::CorpusId;
use serde::{Deserialize, Serialize};

/// Why the machine left a trap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrapExitReason {
    /// Reinforcement plateaued and the allocator fired a discovery burst.
    ReinforcementPlateau,
    /// A PromotionSignal cleared gates 4–5; migration imminent.
    PromotionFired(TermRef),
    /// A DemotionCandidate fired; reverse migration imminent.
    DemotionFired(TermRef),
    /// Corpus rotation provided new terms; gate-5 re-evaluations pending.
    CorpusRotation(CorpusId),
    /// Operator changed the policy; trajectory branches.
    PolicyChange(TermRef),
}

/// A fixed point of the mathscape machine — a registry root that
/// stayed stable for at least `observation_window` epochs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trap {
    /// The registry's Merkle root at the trap.
    pub registry_root: TermRef,
    /// Epoch at which the registry first reached this root.
    pub epoch_id_entered: u64,
    /// Epoch at which the registry changed (None if still in trap).
    pub epoch_id_left: Option<u64>,
    /// Content hash of the active `RealizationPolicy`.
    pub policy_hash: TermRef,
    /// Sequence of CorpusSnapshot hashes that fed into this trap.
    pub corpus_hash_sequence: Vec<TermRef>,
    /// Why the machine left the trap (if it did).
    pub exit_reason: Option<TrapExitReason>,
    /// BLAKE3 over the above fields — the trap's own identity.
    pub content_hash: TermRef,
}

impl Trap {
    /// Seal a trap: compute its content hash from the other fields.
    #[must_use]
    pub fn seal(
        registry_root: TermRef,
        epoch_id_entered: u64,
        epoch_id_left: Option<u64>,
        policy_hash: TermRef,
        corpus_hash_sequence: Vec<TermRef>,
        exit_reason: Option<TrapExitReason>,
    ) -> Self {
        let payload = (
            registry_root,
            epoch_id_entered,
            epoch_id_left,
            policy_hash,
            corpus_hash_sequence.clone(),
            exit_reason.clone(),
        );
        let bytes =
            bincode::serialize(&payload).expect("Trap::seal: bincode infallible");
        let content_hash = TermRef::from_bytes(&bytes);
        Self {
            registry_root,
            epoch_id_entered,
            epoch_id_left,
            policy_hash,
            corpus_hash_sequence,
            exit_reason,
            content_hash,
        }
    }

    /// Whether the machine is still inside this trap.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.epoch_id_left.is_none()
    }
}

/// A detector that watches registry roots across epochs and emits a
/// Trap when the root has been stable for `window` epochs. Simple FSM;
/// no trained params.
#[derive(Debug, Clone)]
pub struct TrapDetector {
    pub window: u64,
    pub current_root: Option<TermRef>,
    pub stable_since_epoch: u64,
    pub stable_for: u64,
}

impl TrapDetector {
    #[must_use]
    pub fn new(window: u64) -> Self {
        Self {
            window: window.max(1),
            current_root: None,
            stable_since_epoch: 0,
            stable_for: 0,
        }
    }

    /// Observe the post-epoch registry root. Returns `Some(Trap)` the
    /// first time the root has been stable for `window` epochs.
    pub fn observe(&mut self, registry_root: TermRef, epoch_id: u64) -> Option<Trap> {
        match self.current_root {
            None => {
                self.current_root = Some(registry_root);
                self.stable_since_epoch = epoch_id;
                self.stable_for = 1;
                None
            }
            Some(prev) if prev == registry_root => {
                self.stable_for += 1;
                if self.stable_for == self.window {
                    Some(Trap::seal(
                        registry_root,
                        self.stable_since_epoch,
                        None,
                        TermRef([0; 32]),
                        vec![],
                        None,
                    ))
                } else {
                    None
                }
            }
            Some(_) => {
                // Root changed — reset the window.
                self.current_root = Some(registry_root);
                self.stable_since_epoch = epoch_id;
                self.stable_for = 1;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(b: u8) -> TermRef {
        TermRef([b; 32])
    }

    #[test]
    fn trap_seal_is_deterministic() {
        let a = Trap::seal(h(1), 0, None, h(2), vec![h(3)], None);
        let b = Trap::seal(h(1), 0, None, h(2), vec![h(3)], None);
        assert_eq!(a.content_hash, b.content_hash);
    }

    #[test]
    fn trap_distinct_inputs_distinct_hashes() {
        let a = Trap::seal(h(1), 0, None, h(2), vec![], None);
        let b = Trap::seal(h(1), 1, None, h(2), vec![], None);
        assert_ne!(a.content_hash, b.content_hash);
    }

    #[test]
    fn active_trap_has_no_exit_epoch() {
        let t = Trap::seal(h(1), 0, None, h(2), vec![], None);
        assert!(t.is_active());
    }

    #[test]
    fn detector_fires_after_window_stable_epochs() {
        let mut d = TrapDetector::new(3);
        assert!(d.observe(h(1), 0).is_none());
        assert!(d.observe(h(1), 1).is_none());
        let trap = d.observe(h(1), 2).expect("should fire on the 3rd stable observation");
        assert_eq!(trap.registry_root, h(1));
        assert_eq!(trap.epoch_id_entered, 0);
    }

    #[test]
    fn detector_resets_on_root_change() {
        let mut d = TrapDetector::new(3);
        d.observe(h(1), 0);
        d.observe(h(1), 1);
        // Root changes at epoch 2 — window resets.
        d.observe(h(2), 2);
        assert!(d.observe(h(2), 3).is_none());
        assert!(d.observe(h(2), 4).is_some());
    }

    #[test]
    fn serde_round_trip() {
        let t = Trap::seal(
            h(9),
            10,
            Some(20),
            h(8),
            vec![h(7), h(6)],
            Some(TrapExitReason::ReinforcementPlateau),
        );
        let bytes = bincode::serialize(&t).unwrap();
        let decoded: Trap = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded, t);
    }
}
