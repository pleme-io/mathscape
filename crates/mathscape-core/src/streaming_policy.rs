//! Phase V.stream (2026-04-18): the never-destroy, never-retrain
//! model. The policy lives forever in memory and trains on the
//! event stream indefinitely.
//!
//! # The architectural property the user named
//!
//!   "Never destroy the model being trained and have to retrain
//!    it — we can just keep training it on a stream of data."
//!   "It could be running as it is taken apart and optimized by
//!    other things like other models or algorithms."
//!   "This should be entirely possible in pure memory and in lisp."
//!
//! # How this module delivers those
//!
//! `StreamingPolicyTrainer` owns a `LinearPolicy` behind a
//! `RefCell`. It is a `MapEventConsumer` — subscribing to the
//! map's event stream means every `MapEvent` flowing through
//! triggers a one-step SGD-like update. No batch cycles, no
//! retraining. The policy's state is the running integration
//! of the stream.
//!
//! Concurrent introspection: `snapshot()` returns a `LinearPolicy`
//! clone at any moment. External optimizers (hyperparameter
//! tuners, weight consolidators, ensemble voters) can:
//!   - read the policy's current weights
//!   - compute an external update
//!   - call `inject()` to write back a mutated policy
//!
//! All without stopping the stream.
//!
//! Pure Lisp path: `snapshot()` → R10's `policy_to_sexp` gives a
//! complete Sexp description. `inject()` takes a `LinearPolicy`
//! that can originate from `policy_from_sexp`. So external
//! optimizers running in Lisp (e.g. a future tatara-lisp WASM
//! module) can read the model, optimize, write back — all
//! through the existing Sexp bridge.
//!
//! Thread-safety: single-threaded today. For multi-consumer
//! concurrent read/write, wrap the RefCell in `Arc<RwLock<_>>`.
//! The public API (snapshot / inject / adjust_learning_rate)
//! stays identical — the change is purely internal.

use crate::bootstrap::LearningObservation;
use crate::mathscape_map::{MapEvent, MapEventConsumer};
use crate::policy::LinearPolicy;
use crate::trajectory::LibraryFeatures;
use std::cell::{Cell, RefCell};

/// Live policy trainer that consumes `MapEvent`s as streaming
/// supervision. The policy lives inside; snapshots + injections
/// are the only external surface.
#[derive(Debug)]
pub struct StreamingPolicyTrainer {
    policy: RefCell<LinearPolicy>,
    learning_rate: Cell<f64>,
    /// Count of map-events observed (for rate-limiting, back-off,
    /// etc.). Each on_event call increments regardless of whether
    /// the event triggered a policy update.
    events_seen: Cell<u64>,
    /// Count of events that produced a policy update (reward-
    /// bearing events).
    updates_applied: Cell<u64>,
}

impl StreamingPolicyTrainer {
    /// Start a fresh trainer with a zero-initialized policy.
    #[must_use]
    pub fn new(learning_rate: f64) -> Self {
        Self::from_policy(LinearPolicy::new(), learning_rate)
    }

    /// Start a trainer pre-loaded with an existing policy (e.g.
    /// from disk, from a prior session, from an external
    /// optimizer's output).
    #[must_use]
    pub fn from_policy(policy: LinearPolicy, learning_rate: f64) -> Self {
        Self {
            policy: RefCell::new(policy),
            learning_rate: Cell::new(learning_rate),
            events_seen: Cell::new(0),
            updates_applied: Cell::new(0),
        }
    }

    /// Snapshot the current policy state. Cloned — external
    /// readers don't hold any lock on the live policy.
    pub fn snapshot(&self) -> LinearPolicy {
        self.policy.borrow().clone()
    }

    /// Inject a new policy, replacing the current one. Intended
    /// for external optimizers that read via `snapshot()`, mutate
    /// in their own address space (or Lisp), and write back.
    pub fn inject(&self, policy: LinearPolicy) {
        *self.policy.borrow_mut() = policy;
    }

    /// Adjust the learning rate at runtime. No restart, no state
    /// loss — the existing weights stay; subsequent updates use
    /// the new rate.
    pub fn adjust_learning_rate(&self, new_rate: f64) {
        self.learning_rate.set(new_rate);
    }

    /// Current learning rate.
    pub fn learning_rate(&self) -> f64 {
        self.learning_rate.get()
    }

    /// Total events observed since this trainer started.
    pub fn events_seen(&self) -> u64 {
        self.events_seen.get()
    }

    /// Total policy updates applied (subset of events_seen —
    /// only reward-bearing events produce updates).
    pub fn updates_applied(&self) -> u64 {
        self.updates_applied.get()
    }

    /// Reward assignment: map an event onto a scalar signal to
    /// drive the policy update. The signs encode the architectural
    /// premise that the user named:
    ///
    ///   RuleCertified — STRONG positive: a rule reached the top
    ///                    of the state machine
    ///   CoreGrew      — moderate positive: invariant mathematics
    ///                    expanded
    ///   NovelRoot     — small positive: new territory visited
    ///   RootMutated   — sign of delta: growth good, shrinkage bad
    ///   StalenessCrossed — negative: the environment has stopped
    ///                    producing
    ///   RuleRejectedAtCertification — small negative: false
    ///                    signal at certification, policy should
    ///                    de-prefer states that yielded this
    fn reward_for(event: &MapEvent) -> f64 {
        match event {
            MapEvent::RuleCertified { .. } => 1.0,
            MapEvent::CoreGrew { .. } => 0.5,
            MapEvent::NovelRoot { .. } => 0.2,
            MapEvent::RootMutated { size_delta, .. } => {
                (*size_delta as f64).signum() * 0.3
            }
            MapEvent::StalenessCrossed { observed, threshold, .. } => {
                -((observed - threshold).max(0.0) as f64)
            }
            MapEvent::RuleRejectedAtCertification { .. } => -0.5,
            // Phase V.benchmark: LABELED reward. The canonical
            // problem set is human-known mathematics; the model
            // getting better at it is the strongest signal
            // available.
            //   absolute competence: solved_fraction ∈ [0, 1]
            //                        maps linearly to [0, 2.0]
            //   delta reward       : ±3.0 per unit improvement
            //                        (regressions hurt more than
            //                        equivalent improvements help)
            MapEvent::BenchmarkScored {
                solved_fraction,
                delta_from_prior,
                ..
            } => {
                let absolute = 2.0 * solved_fraction;
                let delta_signal = if delta_from_prior.is_nan() {
                    0.0
                } else if *delta_from_prior < 0.0 {
                    // Regressions get asymmetric penalty —
                    // "don't break what worked" signal.
                    *delta_from_prior * 5.0
                } else {
                    *delta_from_prior * 3.0
                };
                absolute + delta_signal
            }
        }
    }

    /// Features for the update: the library-state vector implied
    /// by the event. `CoreGrew` and `NovelRoot` and
    /// `RuleCertified` carry rule content we can hash into
    /// features; simpler events produce no feature vector and
    /// skip the update.
    fn features_for(event: &MapEvent) -> Option<LibraryFeatures> {
        use crate::eval::RewriteRule;
        fn from_rule(rule: &RewriteRule) -> LibraryFeatures {
            // Treat the rule as a 1-rule library for feature
            // extraction. Cheap projection.
            LibraryFeatures::extract(std::slice::from_ref(rule))
        }
        match event {
            MapEvent::RuleCertified { rule, .. } => Some(from_rule(rule)),
            MapEvent::CoreGrew { added_rule, .. } => Some(from_rule(added_rule)),
            MapEvent::RuleRejectedAtCertification { rule, .. } => {
                Some(from_rule(rule))
            }
            // Non-rule events don't carry feature content; policy
            // doesn't update on them.
            MapEvent::NovelRoot { .. }
            | MapEvent::RootMutated { .. }
            | MapEvent::StalenessCrossed { .. }
            | MapEvent::BenchmarkScored { .. } => None,
        }
    }

    /// Apply one streaming update: nudge weights toward (or away
    /// from) the event's implied feature vector by the reward
    /// signal × learning rate.
    fn apply_streaming_update(
        &self,
        features: &LibraryFeatures,
        reward: f64,
    ) {
        let v = features.as_vector();
        let lr = self.learning_rate.get();
        let mut p = self.policy.borrow_mut();
        for i in 0..LibraryFeatures::WIDTH {
            p.weights[i] += lr * reward * v[i];
        }
        p.bias += lr * reward;
        p.trained_steps += 1;
    }

    /// Streaming training from an observation directly (without
    /// going through the event channel). Useful when the caller
    /// wants to train on an arbitrary observation, not just those
    /// produced by the map.
    pub fn train_on_observation(&self, obs: &LearningObservation) {
        // Use observation staleness as a simple negative reward
        // (penalize states that produced no novelty).
        let reward = if obs.made_any_progress() {
            (obs.net_growth() as f64) * 0.3
        } else {
            -obs.staleness_score()
        };
        // Features: fake a library of the reported size. The
        // actual content isn't in the observation, but size +
        // metadata are enough for a streaming update.
        let lr = self.learning_rate.get();
        let mut p = self.policy.borrow_mut();
        // Simplest signal: adjust bias by reward × lr. Weight
        // updates would need real feature content.
        p.bias += lr * reward;
        p.trained_steps += 1;
        self.updates_applied.set(self.updates_applied.get() + 1);
    }
}

impl MapEventConsumer for StreamingPolicyTrainer {
    fn on_event(&self, event: &MapEvent) {
        self.events_seen.set(self.events_seen.get() + 1);
        let reward = Self::reward_for(event);
        if reward.abs() < 1e-9 {
            return;
        }
        if let Some(features) = Self::features_for(event) {
            self.apply_streaming_update(&features, reward);
            self.updates_applied.set(self.updates_applied.get() + 1);
        } else {
            // No feature content — just adjust bias.
            let lr = self.learning_rate.get();
            self.policy.borrow_mut().bias += lr * reward;
            self.updates_applied.set(self.updates_applied.get() + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::RewriteRule;
    use crate::hash::TermRef;
    use crate::term::Term;
    use crate::value::Value;

    fn add_id() -> RewriteRule {
        RewriteRule {
            name: "add-id".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![Term::Number(Value::Nat(0)), Term::Var(100)],
            ),
            rhs: Term::Var(100),
        }
    }

    #[test]
    fn trainer_starts_with_zero_policy() {
        let t = StreamingPolicyTrainer::new(0.01);
        let snap = t.snapshot();
        assert_eq!(snap.trained_steps, 0);
        assert!(snap.weights.iter().all(|w| *w == 0.0));
    }

    #[test]
    fn rule_certified_event_produces_positive_update() {
        let t = StreamingPolicyTrainer::new(0.1);
        t.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });
        let snap = t.snapshot();
        assert_eq!(snap.trained_steps, 1);
        // Bias moved positive (reward = 1.0, lr = 0.1 → +0.1).
        assert!(snap.bias > 0.0);
        assert_eq!(t.events_seen(), 1);
        assert_eq!(t.updates_applied(), 1);
    }

    #[test]
    fn staleness_event_produces_negative_update() {
        let t = StreamingPolicyTrainer::new(0.1);
        t.on_event(&MapEvent::StalenessCrossed {
            seed: 1,
            phase_index: 0,
            threshold: 0.6,
            observed: 0.9,
        });
        let snap = t.snapshot();
        assert!(snap.bias < 0.0, "staleness → negative bias update");
    }

    #[test]
    fn rejection_event_produces_negative_update() {
        let t = StreamingPolicyTrainer::new(0.1);
        t.on_event(&MapEvent::RuleRejectedAtCertification {
            rule: add_id(),
            reason: "fake".into(),
        });
        let snap = t.snapshot();
        assert!(snap.bias < 0.0);
    }

    #[test]
    fn inject_replaces_policy_without_stopping_stream() {
        let t = StreamingPolicyTrainer::new(0.1);
        // Train one event.
        t.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });
        let _ = t.snapshot();
        // Inject a modified policy from "external optimizer."
        let mut custom = LinearPolicy::new();
        custom.weights[0] = 5.0;
        custom.bias = 10.0;
        t.inject(custom);
        // Next event trains THIS policy, not the old one.
        t.on_event(&MapEvent::CoreGrew {
            prev_core_size: 0,
            new_core_size: 1,
            added_rule: add_id(),
        });
        let snap = t.snapshot();
        // weights[0] was 5.0 before the event; the event's reward
        // is positive so it should increase.
        assert!(snap.weights[0] >= 5.0);
        assert!(snap.bias >= 10.0);
    }

    #[test]
    fn adjust_learning_rate_takes_effect_immediately() {
        let t = StreamingPolicyTrainer::new(0.01);
        t.adjust_learning_rate(0.5);
        assert_eq!(t.learning_rate(), 0.5);
        // Training now uses the new rate.
        t.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });
        let snap = t.snapshot();
        // With lr=0.5 and reward=1.0, bias moves +0.5.
        assert!(snap.bias >= 0.4);
    }

    #[test]
    fn snapshot_does_not_freeze_the_trainer() {
        let t = StreamingPolicyTrainer::new(0.1);
        let _a = t.snapshot();
        t.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });
        let b = t.snapshot();
        assert!(b.trained_steps > 0, "training continues after snapshot");
    }

    #[test]
    fn trainer_never_resets_on_any_operation() {
        let t = StreamingPolicyTrainer::new(0.1);
        t.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });
        let steps_before = t.snapshot().trained_steps;
        // Snapshot, inject, adjust lr — none of these reset.
        let _ = t.snapshot();
        let current = t.snapshot();
        t.inject(current);
        t.adjust_learning_rate(0.5);
        t.on_event(&MapEvent::CoreGrew {
            prev_core_size: 0,
            new_core_size: 1,
            added_rule: add_id(),
        });
        let steps_after = t.snapshot().trained_steps;
        // Trained steps only INCREASES (never reset).
        assert!(steps_after > steps_before);
    }

    #[test]
    fn streaming_trainer_reacts_to_map_event_variety() {
        let t = StreamingPolicyTrainer::new(0.1);
        let novel_root_event = MapEvent::NovelRoot {
            seed: 1,
            phase_index: 0,
            root: TermRef::from_bytes(b"r"),
            library_size: 5,
        };
        let root_mutated_event = MapEvent::RootMutated {
            seed: 1,
            from_phase: 0,
            to_phase: 1,
            prev_root: TermRef::from_bytes(b"a"),
            next_root: TermRef::from_bytes(b"b"),
            size_delta: 2,
        };
        t.on_event(&novel_root_event);
        t.on_event(&root_mutated_event);
        t.on_event(&MapEvent::CoreGrew {
            prev_core_size: 0,
            new_core_size: 1,
            added_rule: add_id(),
        });
        assert_eq!(t.events_seen(), 3);
        // Three reward-bearing events → three updates.
        assert_eq!(t.updates_applied(), 3);
    }
}
