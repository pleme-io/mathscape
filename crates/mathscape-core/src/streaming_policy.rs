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
    /// Phase V.shed: per-weight activation counts. Incremented
    /// each time the corresponding feature dimension contributed
    /// non-trivially (|v_i| > 1e-9) to a streaming update.
    /// Basis for pruning decisions: weights whose counts stay
    /// near zero for a long window are candidates for shedding.
    activation_counts: RefCell<[u64; LibraryFeatures::WIDTH]>,
    /// Phase V.shed: cumulative |w_i × v_i| per weight — total
    /// contribution to the reward signal integrated over the
    /// stream. Complements activation_counts: a weight can fire
    /// often but contribute little, or fire rarely but
    /// contribute a lot. Both metrics together guide pruning.
    cumulative_contributions: RefCell<[f64; LibraryFeatures::WIDTH]>,
    /// Phase V.shed: which weights are currently pruned. Pruned
    /// weights are held at 0.0 and skipped during updates until
    /// rejuvenated.
    pruned: RefCell<[bool; LibraryFeatures::WIDTH]>,
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
            activation_counts: RefCell::new(
                [0u64; LibraryFeatures::WIDTH],
            ),
            cumulative_contributions: RefCell::new(
                [0.0f64; LibraryFeatures::WIDTH],
            ),
            pruned: RefCell::new([false; LibraryFeatures::WIDTH]),
        }
    }

    /// Phase V.shed: per-weight usage and contribution snapshot.
    /// Returns `(activation_counts, cumulative_contributions,
    /// pruned_flags)`. External optimizers can read this to decide
    /// pruning policy, or to detect "dead" dimensions that should
    /// be shed.
    pub fn weight_stats(
        &self,
    ) -> (
        [u64; LibraryFeatures::WIDTH],
        [f64; LibraryFeatures::WIDTH],
        [bool; LibraryFeatures::WIDTH],
    ) {
        (
            *self.activation_counts.borrow(),
            *self.cumulative_contributions.borrow(),
            *self.pruned.borrow(),
        )
    }

    /// Phase V.shed: prune weights whose absolute magnitude is
    /// below `magnitude_threshold` AND whose activation count is
    /// below `min_activations`. Zeros the weight; marks it as
    /// pruned so future updates skip it.
    ///
    /// Returns the indices that were pruned in this call.
    pub fn prune(
        &self,
        magnitude_threshold: f64,
        min_activations: u64,
    ) -> Vec<usize> {
        let mut pruned_now = Vec::new();
        let counts = *self.activation_counts.borrow();
        let mut policy = self.policy.borrow_mut();
        let mut pruned_flags = self.pruned.borrow_mut();
        for i in 0..LibraryFeatures::WIDTH {
            if !pruned_flags[i]
                && policy.weights[i].abs() < magnitude_threshold
                && counts[i] <= min_activations
            {
                policy.weights[i] = 0.0;
                pruned_flags[i] = true;
                pruned_now.push(i);
            }
        }
        pruned_now
    }

    /// Phase V.shed: rejuvenate a pruned weight — un-prune and
    /// re-initialize to a small value so subsequent updates can
    /// move it. Counterpart to `prune`: neuroplasticity in both
    /// directions.
    pub fn rejuvenate(&self, index: usize, initial_value: f64) -> bool {
        if index >= LibraryFeatures::WIDTH {
            return false;
        }
        let mut pruned = self.pruned.borrow_mut();
        if !pruned[index] {
            return false;
        }
        pruned[index] = false;
        self.policy.borrow_mut().weights[index] = initial_value;
        true
    }

    /// Number of currently-pruned weights.
    pub fn pruned_count(&self) -> usize {
        self.pruned.borrow().iter().filter(|p| **p).count()
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
    ///
    /// Phase V.shed: tracks per-weight activation counts and
    /// cumulative contributions; skips pruned weights entirely.
    fn apply_streaming_update(
        &self,
        features: &LibraryFeatures,
        reward: f64,
    ) {
        let v = features.as_vector();
        let lr = self.learning_rate.get();
        let mut p = self.policy.borrow_mut();
        let mut counts = self.activation_counts.borrow_mut();
        let mut contribs = self.cumulative_contributions.borrow_mut();
        let pruned = self.pruned.borrow();
        for i in 0..LibraryFeatures::WIDTH {
            if pruned[i] {
                continue;
            }
            let feature_val = v[i];
            let delta = lr * reward * feature_val;
            p.weights[i] += delta;
            if feature_val.abs() > 1e-9 {
                counts[i] += 1;
                contribs[i] += (p.weights[i] * feature_val).abs();
            }
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

    // ── Phase V.shed: neuroplasticity tests ──────────────────────

    #[test]
    fn prune_zeros_weight_and_marks_as_pruned() {
        let t = StreamingPolicyTrainer::new(0.1);
        // Inject a policy with one medium-sized weight and rest
        // zeros; then prune with magnitude threshold that catches
        // only the small weights.
        let mut p = t.snapshot();
        p.weights = [0.5, 0.5, 0.5, 0.001, 0.5, 0.5, 0.5, 0.5, 0.5];
        t.inject(p);
        let pruned = t.prune(0.01, 10);
        assert!(pruned.contains(&3), "weight[3] should be pruned");
        let after = t.snapshot();
        assert_eq!(after.weights[3], 0.0);
        assert!(
            t.pruned_count() >= 1,
            "at least weight[3] is pruned"
        );
        // Non-pruned weights are unchanged.
        assert_eq!(after.weights[0], 0.5);
    }

    #[test]
    fn pruned_weights_skip_subsequent_updates() {
        let t = StreamingPolicyTrainer::new(0.1);
        let pruned = t.prune(0.01, 0);
        // All 9 weights start at 0.0, so all below threshold → all pruned.
        assert_eq!(pruned.len(), LibraryFeatures::WIDTH);
        // An event that would otherwise update weights is a no-op
        // on pruned weights. Bias still moves, though.
        t.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });
        let snap = t.snapshot();
        assert!(snap.bias > 0.0); // bias moved
        // All weights stayed at 0.0 because pruned.
        assert!(snap.weights.iter().all(|w| *w == 0.0));
    }

    #[test]
    fn rejuvenate_un_prunes_and_sets_initial_value() {
        let t = StreamingPolicyTrainer::new(0.1);
        t.prune(1.0, 0);
        let before_count = t.pruned_count();
        assert!(before_count > 0);
        assert!(t.rejuvenate(3, 0.5));
        assert_eq!(t.pruned_count(), before_count - 1);
        let snap = t.snapshot();
        assert_eq!(snap.weights[3], 0.5);
    }

    #[test]
    fn rejuvenate_does_nothing_for_unpruned_or_out_of_range() {
        let t = StreamingPolicyTrainer::new(0.1);
        assert!(!t.rejuvenate(3, 0.5), "unpruned → false");
        assert!(!t.rejuvenate(999, 0.5), "out of range → false");
    }

    #[test]
    fn weight_stats_exposes_activation_and_contribution() {
        let t = StreamingPolicyTrainer::new(0.1);
        t.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });
        let (counts, contribs, pruned) = t.weight_stats();
        // At least one weight saw a non-trivial feature value (the
        // add_id rule produces non-zero features).
        assert!(counts.iter().any(|c| *c > 0));
        assert!(contribs.iter().any(|c| *c > 0.0));
        assert!(pruned.iter().all(|p| !*p), "no pruning yet");
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
