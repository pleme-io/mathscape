//! Phase Y.0 (2026-04-19): live inference while training.
//!
//! # The user-framed concern
//!
//!   "If a model is built we should be able to inference it
//!    while it's up, we should consider that at some point."
//!
//! # The mechanism
//!
//! `LiveInferenceHandle` bundles an (externally shared) live
//! library and a live streaming trainer into ONE query surface.
//! External callers can:
//!
//! - `infer(&term)` — reduce a term using the CURRENT library;
//!   the library may change between calls as the motor
//!   continues to discover rules.
//! - `current_competency()` — run the mathematician's curriculum
//!   against the CURRENT library and get a per-subdomain
//!   report. Useful for dashboards.
//! - `policy_snapshot()` — clone the trainer's current policy
//!   without freezing the training stream.
//!
//! None of these block the motor. `snapshot()` returns a cheap
//! clone. `infer` takes a `&RefCell<Vec<RewriteRule>>` borrow
//! that's released as soon as eval finishes.
//!
//! # Single-threaded today
//!
//! Uses `Rc<RefCell<_>>`. When Phase W.7 lands the async hub,
//! this becomes `Arc<RwLock<_>>` and a query can run
//! concurrently with trainer updates on another thread. The
//! public API stays identical.

use crate::eval::{eval, EvalResult, RewriteRule};
use crate::math_problem::{
    mathematician_curriculum, run_curriculum, CurriculumReport,
};
use crate::policy::LinearPolicy;
use crate::streaming_policy::StreamingPolicyTrainer;
use crate::term::Term;
use std::cell::RefCell;
use std::rc::Rc;

/// Live inference surface over a shared library and a live
/// trainer. All methods are non-blocking: each reads a snapshot,
/// evaluates, and returns. The motor can continue mutating the
/// library between (or during release-reacquire cycles of) calls.
pub struct LiveInferenceHandle {
    library: Rc<RefCell<Vec<RewriteRule>>>,
    trainer: Rc<StreamingPolicyTrainer>,
}

impl LiveInferenceHandle {
    #[must_use]
    pub fn new(
        library: Rc<RefCell<Vec<RewriteRule>>>,
        trainer: Rc<StreamingPolicyTrainer>,
    ) -> Self {
        Self { library, trainer }
    }

    /// Reduce `input` using the CURRENT library. Each call
    /// re-reads the library, so results reflect any changes
    /// made between calls.
    pub fn infer(&self, input: &Term, step_limit: usize) -> EvalResult {
        let lib = self.library.borrow();
        eval(input, &lib, step_limit)
    }

    /// Current competency across the mathematician's curriculum.
    /// Each call re-runs the curriculum against the latest
    /// library — so external dashboards can poll this on any
    /// cadence they like.
    pub fn current_competency(&self) -> CurriculumReport {
        let lib = self.library.borrow();
        run_curriculum(&mathematician_curriculum(), &lib)
    }

    /// Clone of the trainer's current policy. Does not freeze
    /// the stream.
    pub fn policy_snapshot(&self) -> LinearPolicy {
        self.trainer.snapshot()
    }

    /// Count of rules currently in the library.
    pub fn library_size(&self) -> usize {
        self.library.borrow().len()
    }

    /// Clone of the library. Lets external observers archive a
    /// snapshot without holding the borrow.
    pub fn library_snapshot(&self) -> Vec<RewriteRule> {
        self.library.borrow().clone()
    }
}

impl std::fmt::Debug for LiveInferenceHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveInferenceHandle")
            .field("library_size", &self.library_size())
            .field("events_seen", &self.trainer.events_seen())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mathscape_map::{MapEvent, MapEventConsumer};
    use crate::value::Value;

    fn add_identity_rule() -> RewriteRule {
        RewriteRule {
            name: "add-id".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![
                    Term::Number(Value::Nat(0)),
                    Term::Var(100),
                ],
            ),
            rhs: Term::Var(100),
        }
    }

    #[test]
    fn inference_reflects_library_changes_live() {
        let library = Rc::new(RefCell::new(Vec::<RewriteRule>::new()));
        let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
        let handle = LiveInferenceHandle::new(library.clone(), trainer);

        // Initial library is empty — inferring add(0, x) with
        // x as a pattern variable does NOT reduce.
        let probe = Term::Apply(
            Box::new(Term::Var(2)), // ADD
            vec![
                Term::Number(Value::Nat(0)),
                Term::Var(100),
            ],
        );
        let before = handle.infer(&probe, 20).unwrap();
        // Should still be the apply form (pattern var blocks folding).
        assert!(matches!(before, Term::Apply(_, _)));

        // Add the identity rule to the live library.
        library.borrow_mut().push(add_identity_rule());

        // Now the SAME infer call reduces via the new rule.
        let after = handle.infer(&probe, 20).unwrap();
        assert_eq!(after, Term::Var(100));

        // Library size reflects the change.
        assert_eq!(handle.library_size(), 1);
    }

    #[test]
    fn competency_report_reflects_current_library() {
        let library = Rc::new(RefCell::new(Vec::<RewriteRule>::new()));
        let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
        let handle = LiveInferenceHandle::new(library.clone(), trainer);

        let baseline = handle.current_competency();
        // On an empty library, symbolic-nat scores 0.
        let baseline_sym = baseline
            .per_subdomain
            .get("symbolic-nat")
            .unwrap();
        assert_eq!(baseline_sym.solved_count, 0);

        // Add the identity rule.
        library.borrow_mut().push(add_identity_rule());

        // Competency jumped on symbolic-nat and
        // generalization-affected subdomains.
        let after = handle.current_competency();
        let after_sym = after.per_subdomain.get("symbolic-nat").unwrap();
        assert!(after_sym.solved_count > baseline_sym.solved_count);
    }

    #[test]
    fn policy_snapshot_is_non_blocking() {
        let library = Rc::new(RefCell::new(Vec::<RewriteRule>::new()));
        let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
        let handle = LiveInferenceHandle::new(library, trainer.clone());

        // Snapshot before any training.
        let s0 = handle.policy_snapshot();
        assert_eq!(s0.trained_steps, 0);

        // Push one training event.
        trainer.on_event(&MapEvent::RuleCertified {
            rule: add_identity_rule(),
            evidence_samples: 96,
        });

        // Snapshot AFTER shows new state — the handle doesn't
        // cache a stale clone.
        let s1 = handle.policy_snapshot();
        assert!(s1.trained_steps > s0.trained_steps);
    }

    #[test]
    fn library_snapshot_is_a_clone_not_a_reference() {
        let library = Rc::new(RefCell::new(Vec::<RewriteRule>::new()));
        let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
        let handle = LiveInferenceHandle::new(library.clone(), trainer);

        library.borrow_mut().push(add_identity_rule());
        let snap = handle.library_snapshot();
        assert_eq!(snap.len(), 1);

        // Mutating the snapshot doesn't affect the live library.
        let mut owned = snap;
        owned.push(add_identity_rule());
        assert_eq!(handle.library_size(), 1);
    }
}
