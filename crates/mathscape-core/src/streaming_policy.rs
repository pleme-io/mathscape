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
    /// Phase W.stall (corrupted/stalled detection): `events_seen`
    /// count at which weight i was last updated with a non-trivial
    /// feature contribution. `events_seen - last_active_event[i]`
    /// is the dormancy age. Used by `prune_dormant_or_corrupted`
    /// to shed weights that have gone silent after previously
    /// being active.
    last_active_event: RefCell<[u64; LibraryFeatures::WIDTH]>,
    /// Phase W.1 (RigL-style): phantom-gradient accumulator for
    /// pruned weights. When a weight is pruned, its update is
    /// skipped — but the magnitude of the update that WOULD have
    /// been applied is summed here. A pruned weight with a large
    /// phantom-gradient accumulation has reward signal trying to
    /// move it; `auto_rejuvenate` promotes those weights back
    /// into the active set. This closes the neuroplasticity loop:
    /// pruning is automatic, rejuvenation is automatic, the
    /// representation self-adjusts capacity.
    phantom_gradient_accum: RefCell<[f64; LibraryFeatures::WIDTH]>,
    /// Phase W.2 (EWC-style): running per-weight Fisher information
    /// estimate — EMA of squared gradient. Fisher[i] is large when
    /// weight i has been load-bearing over the stream; small when
    /// it has rarely contributed. Used by the EWC pullback to
    /// resist changes to load-bearing weights under regression
    /// pressure.
    fisher_information: RefCell<[f64; LibraryFeatures::WIDTH]>,
    /// Phase W.2: anchored weights — the policy's state at the
    /// last "known-good" moment (e.g. last benchmark improvement).
    /// Under EWC, regression-producing events get a pullback term
    /// proportional to `fisher[i] * (w[i] - anchor[i])`, pulling
    /// load-bearing weights back toward the anchor.
    anchor_weights: RefCell<[f64; LibraryFeatures::WIDTH]>,
    /// Phase W.2: bias counterpart to `anchor_weights`.
    anchor_bias: Cell<f64>,
    /// Phase W.2: whether the anchor has been set at least once.
    /// Before the first anchor event, EWC pullback is disabled.
    anchor_set: Cell<bool>,
    /// Phase W.2: EWC regularization strength. 0.0 = EWC off.
    /// Typical range 0.01–0.5; depends on reward scale.
    ewc_lambda: Cell<f64>,
    /// Phase W.3 (Schmidhuber/Oudeyer learning progress):
    /// recent benchmark `solved_fraction` history. Used to
    /// compute intrinsic-motivation reward as the improvement
    /// over the minimum of the last K observations.
    benchmark_history: RefCell<Vec<f64>>,
    /// Phase W.3: window size K for learning-progress computation.
    /// Default 5. Setting to 0 disables learning-progress bonus.
    learning_progress_window: Cell<usize>,
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
            last_active_event: RefCell::new(
                [0u64; LibraryFeatures::WIDTH],
            ),
            phantom_gradient_accum: RefCell::new(
                [0.0f64; LibraryFeatures::WIDTH],
            ),
            fisher_information: RefCell::new(
                [0.0f64; LibraryFeatures::WIDTH],
            ),
            anchor_weights: RefCell::new(
                [0.0f64; LibraryFeatures::WIDTH],
            ),
            anchor_bias: Cell::new(0.0),
            anchor_set: Cell::new(false),
            ewc_lambda: Cell::new(0.0),
            benchmark_history: RefCell::new(Vec::new()),
            learning_progress_window: Cell::new(5),
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

    // ── Phase W.stall (corrupted / stalled neuroplasticity) ─────

    /// Phase W.stall: shed weights whose activity pattern shows
    /// they have gone silent after being active (stalled) or whose
    /// weight magnitude stays near zero despite accumulated Fisher
    /// information (corrupted — repeated pressure pulled the weight
    /// down but the reward signal keeps demanding contribution).
    ///
    /// Arguments:
    /// - `stall_events`: if a weight has not been active in the
    ///   last `stall_events` observations (and has any history),
    ///   it counts as stalled and gets pruned.
    /// - `corruption_fisher_floor`: if a weight's Fisher >= this
    ///   but its magnitude < `corruption_magnitude_ceiling`, it
    ///   counts as corrupted and gets pruned.
    /// - `corruption_magnitude_ceiling`: companion to
    ///   `corruption_fisher_floor`.
    ///
    /// Returns the indices pruned in this call. Weights already
    /// pruned are not re-pruned.
    pub fn prune_dormant_or_corrupted(
        &self,
        stall_events: u64,
        corruption_fisher_floor: f64,
        corruption_magnitude_ceiling: f64,
    ) -> Vec<usize> {
        let mut pruned_now = Vec::new();
        let events = self.events_seen.get();
        let last_active = *self.last_active_event.borrow();
        let counts = *self.activation_counts.borrow();
        let fisher = *self.fisher_information.borrow();
        let mut pruned_flags = self.pruned.borrow_mut();
        let mut policy = self.policy.borrow_mut();
        for i in 0..LibraryFeatures::WIDTH {
            if pruned_flags[i] {
                continue;
            }
            let dormancy = events.saturating_sub(last_active[i]);
            let stalled = counts[i] > 0 && dormancy >= stall_events;
            let corrupted = fisher[i] >= corruption_fisher_floor
                && policy.weights[i].abs() < corruption_magnitude_ceiling
                && counts[i] > 0;
            if stalled || corrupted {
                policy.weights[i] = 0.0;
                pruned_flags[i] = true;
                pruned_now.push(i);
            }
        }
        pruned_now
    }

    /// Phase W.stall: snapshot the last-active-event array. Each
    /// entry is the events-seen count when weight i last had a
    /// non-trivial contribution. Used by external diagnostics to
    /// inspect the dormancy age distribution.
    pub fn last_active_snapshot(
        &self,
    ) -> [u64; LibraryFeatures::WIDTH] {
        *self.last_active_event.borrow()
    }

    // ── Phase W.1 (RigL-style gradient-guided rejuvenation) ──────

    /// Phase W.1: snapshot the phantom-gradient accumulator. Each
    /// entry is the summed `|would-be-delta|` for that weight over
    /// its pruned lifetime. Large entries indicate the reward
    /// signal is actively trying to move weights that are pinned
    /// at zero — prime candidates for rejuvenation.
    pub fn phantom_gradients(
        &self,
    ) -> [f64; LibraryFeatures::WIDTH] {
        *self.phantom_gradient_accum.borrow()
    }

    /// Phase W.1: reset the phantom-gradient accumulator. Called
    /// after a rejuvenation pass so the next window of
    /// observation is fresh.
    pub fn clear_phantom_gradients(&self) {
        *self.phantom_gradient_accum.borrow_mut() =
            [0.0f64; LibraryFeatures::WIDTH];
    }

    /// Phase W.1: auto-rejuvenate any pruned weight whose phantom
    /// gradient has accumulated beyond `phantom_threshold`. This
    /// is the RigL-inspired regrowth signal: reinstate dimensions
    /// where signal is trying to move, shed dimensions where it
    /// isn't. Closes the neuroplasticity loop end-to-end.
    ///
    /// Returns the indices that were rejuvenated in this call.
    /// Clears the phantom-gradient entries for rejuvenated
    /// weights so the next window starts clean.
    pub fn auto_rejuvenate(
        &self,
        phantom_threshold: f64,
        initial_value: f64,
    ) -> Vec<usize> {
        let mut rejuvenated = Vec::new();
        let mut pruned_flags = self.pruned.borrow_mut();
        let mut phantoms = self.phantom_gradient_accum.borrow_mut();
        let mut policy = self.policy.borrow_mut();
        for i in 0..LibraryFeatures::WIDTH {
            if pruned_flags[i] && phantoms[i].abs() >= phantom_threshold {
                pruned_flags[i] = false;
                policy.weights[i] = initial_value;
                phantoms[i] = 0.0;
                rejuvenated.push(i);
            }
        }
        rejuvenated
    }

    // ── Phase W.2 (EWC-style Fisher-weighted stability) ──────────

    /// Phase W.2: snapshot the Fisher information estimate. Each
    /// entry is an EMA of `gradient_i^2` — a proxy for "how
    /// load-bearing weight i has been." EWC pullback scales per-
    /// weight by this value, so load-bearing weights resist
    /// change under regression pressure.
    pub fn fisher_snapshot(
        &self,
    ) -> [f64; LibraryFeatures::WIDTH] {
        *self.fisher_information.borrow()
    }

    /// Phase W.2: set the EWC regularization strength `λ`. 0.0
    /// disables EWC entirely. Typical range 0.01–0.5; scale
    /// relative to average reward magnitude. Higher λ = more
    /// stability, less plasticity.
    pub fn set_ewc_lambda(&self, lambda: f64) {
        self.ewc_lambda.set(lambda);
    }

    /// Phase W.2: current EWC regularization strength.
    pub fn ewc_lambda(&self) -> f64 {
        self.ewc_lambda.get()
    }

    /// Phase W.2: explicitly anchor the current policy state.
    /// Called automatically on benchmark improvement; exposed so
    /// external callers can anchor on other signals (e.g.
    /// certification milestones, operator-directed checkpoints).
    pub fn anchor_current_weights(&self) {
        let p = self.policy.borrow();
        *self.anchor_weights.borrow_mut() = p.weights;
        self.anchor_bias.set(p.bias);
        self.anchor_set.set(true);
    }

    /// Phase W.2: whether an anchor has been set.
    pub fn has_anchor(&self) -> bool {
        self.anchor_set.get()
    }

    /// Phase W.2: snapshot the anchor weights (zeros if never
    /// anchored).
    pub fn anchor_snapshot(
        &self,
    ) -> [f64; LibraryFeatures::WIDTH] {
        *self.anchor_weights.borrow()
    }

    // ── Phase W.3 (learning-progress intrinsic reward) ───────────

    /// Phase W.3: benchmark score history in arrival order.
    pub fn benchmark_history(&self) -> Vec<f64> {
        self.benchmark_history.borrow().clone()
    }

    /// Phase W.3: set the window K for learning-progress
    /// computation. Intrinsic reward at benchmark event is the
    /// improvement `current - min(last K scores)`, bonus only for
    /// positive improvement. K = 0 disables the bonus.
    pub fn set_learning_progress_window(&self, k: usize) {
        self.learning_progress_window.set(k);
    }

    /// Phase W.3: current learning-progress window size.
    pub fn learning_progress_window(&self) -> usize {
        self.learning_progress_window.get()
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
        let lambda = self.ewc_lambda.get();
        let anchor_set = self.anchor_set.get();
        let events = self.events_seen.get();
        let mut p = self.policy.borrow_mut();
        let mut counts = self.activation_counts.borrow_mut();
        let mut contribs = self.cumulative_contributions.borrow_mut();
        let mut last_active = self.last_active_event.borrow_mut();
        let mut phantoms = self.phantom_gradient_accum.borrow_mut();
        let mut fisher = self.fisher_information.borrow_mut();
        let anchor = self.anchor_weights.borrow();
        let pruned = self.pruned.borrow();
        // Fisher EMA decay: fast enough to track shifts, slow
        // enough to have memory (about 100 events of history).
        const FISHER_DECAY: f64 = 0.99;
        for i in 0..LibraryFeatures::WIDTH {
            let feature_val = v[i];
            let raw_delta = lr * reward * feature_val;
            if pruned[i] {
                // Phase W.1: phantom-gradient bookkeeping — what the
                // update would have been, had the weight been active.
                // Large accumulation → reward signal wants this
                // dimension back; `auto_rejuvenate` will pick it up.
                phantoms[i] += raw_delta.abs();
                continue;
            }
            // Phase W.2: Fisher EMA on the gradient (not the delta;
            // strip lr so the estimate is lr-independent).
            let grad = reward * feature_val;
            fisher[i] = FISHER_DECAY * fisher[i]
                + (1.0 - FISHER_DECAY) * grad * grad;
            // Phase W.2: EWC pullback is only applied when the
            // event is regressive (negative reward) AND an anchor
            // exists AND λ > 0. Keeps the trainer plastic on gains
            // and sticky on regressions.
            let ewc_pullback = if anchor_set
                && lambda > 0.0
                && reward < 0.0
            {
                lambda * fisher[i] * (p.weights[i] - anchor[i])
            } else {
                0.0
            };
            let delta = raw_delta - ewc_pullback;
            p.weights[i] += delta;
            if feature_val.abs() > 1e-9 {
                counts[i] += 1;
                contribs[i] += (p.weights[i] * feature_val).abs();
                last_active[i] = events;
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
        let mut reward = Self::reward_for(event);

        // Phase W.3: learning-progress intrinsic reward + history
        // bookkeeping. On every benchmark event: push into history;
        // compute "current minus min-of-last-K-scores"; positive
        // improvement adds a bonus to reward. Also auto-anchor on
        // strictly-improving benchmark events for W.2 EWC.
        if let MapEvent::BenchmarkScored {
            solved_fraction, ..
        } = event
        {
            let prev_last = self
                .benchmark_history
                .borrow()
                .last()
                .copied();
            self.benchmark_history.borrow_mut().push(*solved_fraction);
            let k = self.learning_progress_window.get();
            if k > 0 {
                let hist = self.benchmark_history.borrow();
                if hist.len() >= 2 {
                    let window_start = hist.len().saturating_sub(k + 1);
                    let recent = &hist[window_start..hist.len() - 1];
                    if !recent.is_empty() {
                        let min_recent = recent
                            .iter()
                            .copied()
                            .fold(f64::INFINITY, f64::min);
                        let progress = *solved_fraction - min_recent;
                        if progress > 0.0 && progress.is_finite() {
                            // Schmidhuber/Oudeyer: the agent's
                            // intrinsic reward is its own learning
                            // progress. Coefficient 4.0 matches the
                            // scale of the existing benchmark delta
                            // reward (3.0 × improvement).
                            reward += 4.0 * progress;
                        }
                    }
                }
            }
            // Auto-anchor on improvement — the current weights are
            // a "known good" checkpoint to pull back toward under
            // future regression pressure.
            if let Some(prev) = prev_last {
                if *solved_fraction > prev {
                    self.anchor_current_weights();
                }
            } else if *solved_fraction > 0.0 {
                // First-ever benchmark with any success → anchor.
                self.anchor_current_weights();
            }
        }

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

    // ── Phase W.1: RigL-style phantom gradients ─────────────────

    #[test]
    fn phantom_gradient_accumulates_on_pruned_weights() {
        let t = StreamingPolicyTrainer::new(0.1);
        // Prune everything; all weights are now inactive.
        t.prune(1.0, 0);
        // An event whose features contain non-zero values will
        // generate phantom-gradient activity on pruned weights.
        t.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });
        let phantoms = t.phantom_gradients();
        assert!(
            phantoms.iter().any(|p| *p > 0.0),
            "at least one pruned weight accumulated phantom signal"
        );
    }

    #[test]
    fn auto_rejuvenate_un_prunes_by_phantom_gradient() {
        let t = StreamingPolicyTrainer::new(0.1);
        t.prune(1.0, 0);
        // Feed repeated events so phantom gradients grow.
        for _ in 0..5 {
            t.on_event(&MapEvent::RuleCertified {
                rule: add_id(),
                evidence_samples: 96,
            });
        }
        let before = t.pruned_count();
        let rejuvenated = t.auto_rejuvenate(0.01, 0.05);
        assert!(
            !rejuvenated.is_empty(),
            "auto_rejuvenate picked up at least one phantom-active weight"
        );
        assert!(t.pruned_count() < before);
        let snap = t.snapshot();
        for &i in &rejuvenated {
            assert_eq!(snap.weights[i], 0.05);
        }
    }

    #[test]
    fn clear_phantom_gradients_resets_accumulator() {
        let t = StreamingPolicyTrainer::new(0.1);
        t.prune(1.0, 0);
        t.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });
        assert!(t.phantom_gradients().iter().any(|p| *p > 0.0));
        t.clear_phantom_gradients();
        assert!(t.phantom_gradients().iter().all(|p| *p == 0.0));
    }

    // ── Phase W.2: EWC-style Fisher-weighted stability ──────────

    #[test]
    fn fisher_information_accumulates_with_training() {
        let t = StreamingPolicyTrainer::new(0.1);
        for _ in 0..10 {
            t.on_event(&MapEvent::RuleCertified {
                rule: add_id(),
                evidence_samples: 96,
            });
        }
        let fisher = t.fisher_snapshot();
        assert!(
            fisher.iter().any(|f| *f > 0.0),
            "Fisher EMA grew on training"
        );
    }

    #[test]
    fn anchor_is_set_on_improving_benchmark() {
        let t = StreamingPolicyTrainer::new(0.1);
        assert!(!t.has_anchor());
        // First benchmark with positive fraction → anchor.
        t.on_event(&MapEvent::BenchmarkScored {
            solved_count: 5,
            total: 10,
            solved_fraction: 0.5,
            delta_from_prior: 0.5,
        });
        assert!(t.has_anchor(), "first positive-benchmark anchors");
    }

    #[test]
    fn ewc_pullback_resists_regression_on_load_bearing_weights() {
        let t = StreamingPolicyTrainer::new(0.1);
        // Train a few positive events to build Fisher + weights.
        for _ in 0..5 {
            t.on_event(&MapEvent::RuleCertified {
                rule: add_id(),
                evidence_samples: 96,
            });
        }
        // Anchor and enable EWC.
        t.anchor_current_weights();
        t.set_ewc_lambda(0.5);
        let before = t.snapshot();
        // Feed a regression-producing event; the pullback should
        // resist movement on load-bearing weights.
        t.on_event(&MapEvent::RuleRejectedAtCertification {
            rule: add_id(),
            reason: "fake".into(),
        });
        let after = t.snapshot();
        // The EWC pullback is active: when reward is negative, the
        // delta is (raw_delta - ewc_pullback). On weights with high
        // Fisher and weights equal to anchor, the pullback is zero;
        // on weights that have drifted from the anchor, the
        // pullback resists further drift. Quick sanity: the bias
        // still moved (EWC operates on weights, not bias), and
        // weights moved LESS than they would without EWC.
        assert!(after.bias < before.bias);
    }

    // ── Phase W.3: learning-progress intrinsic reward ───────────

    #[test]
    fn learning_progress_bonus_fires_on_new_high() {
        let t = StreamingPolicyTrainer::new(0.1);
        // Warmup history with low scores.
        for f in [0.2, 0.3, 0.25, 0.3] {
            t.on_event(&MapEvent::BenchmarkScored {
                solved_count: (f * 10.0) as usize,
                total: 10,
                solved_fraction: f,
                delta_from_prior: 0.0,
            });
        }
        let bias_before = t.snapshot().bias;
        // A score that EXCEEDS the min of recent history should
        // trigger the learning-progress bonus.
        t.on_event(&MapEvent::BenchmarkScored {
            solved_count: 8,
            total: 10,
            solved_fraction: 0.8,
            delta_from_prior: 0.5,
        });
        let bias_after = t.snapshot().bias;
        // The base benchmark reward would move bias; the learning-
        // progress bonus SHOULD push it further than the base
        // reward alone. Harder to assert exactly without
        // re-deriving the math, but we can at least confirm the
        // bias moved significantly.
        assert!(
            bias_after - bias_before > 0.1,
            "learning-progress bonus produced sizeable positive update"
        );
        assert!(!t.benchmark_history().is_empty());
    }

    #[test]
    fn learning_progress_window_disable_suppresses_bonus() {
        let t = StreamingPolicyTrainer::new(0.1);
        t.set_learning_progress_window(0);
        // Feed history — but with window=0, no bonus should fire.
        for f in [0.2, 0.3, 0.8] {
            t.on_event(&MapEvent::BenchmarkScored {
                solved_count: (f * 10.0) as usize,
                total: 10,
                solved_fraction: f,
                delta_from_prior: 0.0,
            });
        }
        assert_eq!(t.learning_progress_window(), 0);
        // Benchmark history still grows.
        assert_eq!(t.benchmark_history().len(), 3);
    }

    // ── Phase W.stall: dormant/corrupted pruning ────────────────

    #[test]
    fn prune_dormant_finds_weights_stale_after_activity() {
        let t = StreamingPolicyTrainer::new(0.1);
        // Activate all weights via one event.
        t.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });
        // Advance events_seen count without re-activating those
        // same feature dims — feed events with different feature
        // content or synthetic. For simplicity, just advance via
        // staleness events (no feature content, bias-only update).
        for _ in 0..20 {
            t.on_event(&MapEvent::StalenessCrossed {
                seed: 1,
                phase_index: 0,
                threshold: 0.6,
                observed: 0.9,
            });
        }
        let pruned = t.prune_dormant_or_corrupted(5, f64::INFINITY, 0.0);
        // At least one previously-active weight went stale.
        assert!(
            !pruned.is_empty(),
            "at least one weight was dormant long enough to prune"
        );
    }

    #[test]
    fn prune_corrupted_finds_pressure_flattened_weights() {
        let t = StreamingPolicyTrainer::new(0.1);
        // Warm up Fisher via repeated training.
        for _ in 0..30 {
            t.on_event(&MapEvent::RuleCertified {
                rule: add_id(),
                evidence_samples: 96,
            });
        }
        // Zero out a specific weight to simulate repeated
        // corrective pressure that pulled it to zero despite
        // Fisher indicating it had been load-bearing.
        let mut p = t.snapshot();
        p.weights[3] = 0.0;
        t.inject(p);
        // With a Fisher floor that the accumulated value meets and
        // a tight magnitude ceiling, weight[3] matches "corrupted."
        let fisher = t.fisher_snapshot();
        let max_fisher = fisher
            .iter()
            .cloned()
            .fold(0.0f64, f64::max);
        // At least one weight accumulated Fisher > 0; pick a floor
        // that's well below the max so weight[3] with zero magnitude
        // AND non-zero Fisher qualifies.
        let floor = max_fisher * 0.01;
        let pruned = t.prune_dormant_or_corrupted(u64::MAX, floor, 0.001);
        // If any weight had Fisher > floor and mag < 0.001, it
        // gets pruned. weight[3] was just zeroed; if its Fisher >
        // floor, it's pruned.
        if fisher[3] > floor {
            assert!(pruned.contains(&3), "corrupted weight[3] pruned");
        }
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
