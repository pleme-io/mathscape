//! Control plane — `Regime`, `RealizationPolicy`, `RewardEstimator`,
//! `Allocator`, `EpochAction`.
//!
//! See `docs/arch/reward-calculus.md` and
//! `docs/arch/machine-synthesis.md`. The allocator chooses the next
//! `EpochAction` based on expected ΔDL per unit compute over recent
//! event history; the policy parameterizes every gate threshold.

use crate::event::{Event, EventCategory};
use crate::hash::TermRef;
use crate::lifecycle::AxiomIdentity;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The three regimes, named by which event category dominates ΔDL in
/// recent epochs. Canonical names from
/// `docs/arch/machine-synthesis.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Regime {
    /// Reinforcement is paying off; discovery stays paused.
    Reductive,
    /// Reinforcement has plateaued; discovery is firing bursts.
    Explosive,
    /// An Artifact has cleared gates 4–5; promotion + migration are
    /// dominating ΔDL.
    Promotive,
}

impl Regime {
    /// Which event category this regime expects to dominate the epoch
    /// trace.
    #[must_use]
    pub fn dominant_category(&self) -> EventCategory {
        match self {
            Regime::Reductive => EventCategory::Reinforce,
            Regime::Explosive => EventCategory::Discovery,
            Regime::Promotive => EventCategory::Promote,
        }
    }
}

/// Per-regime weighting of the ΔDL axes. The prover consumes this to
/// compute its composite score.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RegimeWeights {
    /// Weight on corpus compression (`compression_ratio`).
    pub alpha: f64,
    /// Weight on novelty (`novelty`).
    pub beta: f64,
    /// Weight on library condensation / meta-compression.
    pub gamma: f64,
}

impl RegimeWeights {
    pub const fn reductive_default() -> Self {
        Self { alpha: 0.3, beta: 0.3, gamma: 0.4 }
    }
    pub const fn explosive_default() -> Self {
        Self { alpha: 0.7, beta: 0.25, gamma: 0.05 }
    }
    pub const fn promotive_default() -> Self {
        Self { alpha: 0.2, beta: 0.2, gamma: 0.6 }
    }
}

/// The five-knob control surface. Tunable at load time; later fit
/// adaptively (level 5 of the minimal-model ladder).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealizationPolicy {
    /// Gate 1 — minimum ΔDL for a proposal to be accepted (bits).
    pub epsilon_compression: f64,
    /// Gate 4 — minimum number of library entries a promotion candidate
    /// must subsume.
    pub k_condensation: usize,
    /// Gate 5 — minimum number of distinct corpora a promotion
    /// candidate must appear in.
    pub n_cross_corpus: usize,
    /// Demotion floor — usage tally below which a library entry is a
    /// demotion candidate.
    pub m_demotion_floor: usize,
    /// Sliding window over which usage is tallied (epochs).
    pub w_usage_window: u64,
    /// Per-regime weights (BTreeMap for deterministic serde order).
    pub regime_weights: BTreeMap<Regime, RegimeWeights>,
    /// Plateau threshold (bits) — below this ΔDL per reinforce pass we
    /// force-fire a discovery burst.
    pub epsilon_plateau: f64,
    /// Exploration bonus rate ρ for `expected_discover`.
    pub exploration_rho: f64,
}

impl Default for RealizationPolicy {
    fn default() -> Self {
        let mut regime_weights = BTreeMap::new();
        regime_weights.insert(Regime::Reductive, RegimeWeights::reductive_default());
        regime_weights.insert(Regime::Explosive, RegimeWeights::explosive_default());
        regime_weights.insert(Regime::Promotive, RegimeWeights::promotive_default());
        Self {
            epsilon_compression: 0.02,
            k_condensation: 3,
            n_cross_corpus: 2,
            m_demotion_floor: 1,
            w_usage_window: 100,
            regime_weights,
            epsilon_plateau: 0.5,
            exploration_rho: 0.1,
        }
    }
}

/// Rolling-window estimator of expected ΔDL by event category. v0 uses
/// a simple exponentially-weighted moving average; later phases swap
/// for EWMA + variance + confidence intervals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RewardEstimator {
    pub reinforce_mean: f64,
    pub discover_mean: f64,
    pub since_last_discovery: u64,
    /// EWMA smoothing factor in [0, 1]; 1.0 = no smoothing (last epoch
    /// only), 0.0 = never update.
    pub lambda: f64,
}

impl RewardEstimator {
    #[must_use]
    pub fn new(lambda: f64) -> Self {
        Self {
            reinforce_mean: 0.0,
            discover_mean: 0.0,
            since_last_discovery: 0,
            lambda: lambda.clamp(0.0, 1.0),
        }
    }

    /// Fold a batch of events from one epoch into the estimator.
    pub fn update(&mut self, events: &[Event]) {
        let mut reinforce_total = 0.0_f64;
        let mut discover_total = 0.0_f64;
        let mut saw_discovery = false;
        for ev in events {
            match ev.category() {
                EventCategory::Reinforce => reinforce_total += ev.delta_dl(),
                EventCategory::Discovery => {
                    discover_total += ev.delta_dl();
                    if matches!(ev, Event::Accept { .. }) {
                        saw_discovery = true;
                    }
                }
                _ => {}
            }
        }
        self.reinforce_mean =
            (1.0 - self.lambda) * self.reinforce_mean + self.lambda * reinforce_total;
        // Only update discover_mean on epochs that actually ran discovery.
        if discover_total != 0.0 || saw_discovery {
            self.discover_mean =
                (1.0 - self.lambda) * self.discover_mean + self.lambda * discover_total;
            self.since_last_discovery = 0;
        } else {
            self.since_last_discovery = self.since_last_discovery.saturating_add(1);
        }
    }

    /// Expected ΔDL from the next reinforcement pass.
    #[must_use]
    pub fn expected_reinforce(&self) -> f64 {
        self.reinforce_mean
    }

    /// Expected ΔDL from the next discovery burst, inflated by an
    /// exploration bonus that grows with epochs-since-last-discovery.
    #[must_use]
    pub fn expected_discover(&self, rho: f64) -> f64 {
        let bonus = 1.0 + rho * (1.0 + self.since_last_discovery as f64).ln();
        self.discover_mean * bonus
    }

    /// True if reinforcement has plateaued — expected reinforce is
    /// below the policy's `epsilon_plateau`.
    #[must_use]
    pub fn plateau_detected(&self, epsilon_plateau: f64) -> bool {
        self.expected_reinforce() < epsilon_plateau
    }
}

/// The next action to run. `Epoch::step` dispatches on this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EpochAction {
    /// Default: push every library entry one step further along the
    /// lifecycle. Emits Reinforcement events.
    Reinforce,
    /// Fire a discovery burst. Emits Discovery events.
    Discover,
    /// Hand a specific artifact to axiom-forge. Emits Promote events.
    Promote(TermRef),
    /// Perform library migration after a successful promotion. Emits
    /// Migrate events.
    Migrate(AxiomIdentity),
}

/// Allocator — given the current estimator + policy, choose the next
/// action. Level-4 of the minimal-model ladder, made concrete.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Allocator {
    pub policy: RealizationPolicy,
    pub estimator: RewardEstimator,
}

impl Allocator {
    #[must_use]
    pub fn new(policy: RealizationPolicy, estimator: RewardEstimator) -> Self {
        Self { policy, estimator }
    }

    /// Choose the next `EpochAction`.
    ///
    /// Decision rule:
    /// 1. If reinforcement plateau is detected, force-fire discovery.
    /// 2. Otherwise compare expected-ΔDL-per-cost. `c_r = |L|` (visit
    ///    each library entry once), `c_d = |C| * |L|` (anti-unify over
    ///    pairs). Switch iff
    ///    `expected_discover / |C| > expected_reinforce`.
    /// 3. If nothing to reinforce and nothing to discover, default to
    ///    Reinforce (cheaper no-op).
    ///
    /// Promote / Migrate actions are driven by `PromotionGate` events
    /// outside this allocator and inserted into the action queue
    /// directly.
    #[must_use]
    pub fn choose(&self, corpus_size: usize, library_size: usize) -> EpochAction {
        if library_size == 0 {
            // Nothing to reinforce; the only useful work is discovery.
            return EpochAction::Discover;
        }
        if self.estimator.plateau_detected(self.policy.epsilon_plateau) {
            return EpochAction::Discover;
        }
        let reinforce = self.estimator.expected_reinforce();
        let discover =
            self.estimator.expected_discover(self.policy.exploration_rho);
        let c_ratio = corpus_size.max(1) as f64;
        if discover / c_ratio > reinforce {
            EpochAction::Discover
        } else {
            EpochAction::Reinforce
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_has_three_regime_weight_entries() {
        let p = RealizationPolicy::default();
        assert_eq!(p.regime_weights.len(), 3);
        assert!(p.regime_weights.contains_key(&Regime::Reductive));
        assert!(p.regime_weights.contains_key(&Regime::Explosive));
        assert!(p.regime_weights.contains_key(&Regime::Promotive));
    }

    #[test]
    fn estimator_update_moves_mean_toward_input() {
        let mut est = RewardEstimator::new(0.5);
        // First reinforce-only epoch with 1.0 total ΔDL.
        let events = vec![Event::Merge {
            kept: TermRef([0; 32]),
            merged: TermRef([1; 32]),
            delta_dl: 1.0,
        }];
        est.update(&events);
        assert!((est.reinforce_mean - 0.5).abs() < 1e-9);
        assert_eq!(est.since_last_discovery, 1);
    }

    #[test]
    fn plateau_detected_below_threshold() {
        let mut est = RewardEstimator::new(1.0);
        est.reinforce_mean = 0.1;
        assert!(est.plateau_detected(0.5));
        est.reinforce_mean = 0.9;
        assert!(!est.plateau_detected(0.5));
    }

    #[test]
    fn exploration_bonus_grows_with_absence() {
        let mut est = RewardEstimator::new(1.0);
        est.discover_mean = 1.0;
        est.since_last_discovery = 0;
        let e0 = est.expected_discover(0.1);
        est.since_last_discovery = 100;
        let e100 = est.expected_discover(0.1);
        assert!(e100 > e0);
    }

    #[test]
    fn allocator_forces_discovery_on_plateau() {
        let mut policy = RealizationPolicy::default();
        policy.epsilon_plateau = 0.5;
        let mut est = RewardEstimator::new(1.0);
        est.reinforce_mean = 0.1; // well below plateau
        let alloc = Allocator::new(policy, est);
        assert_eq!(alloc.choose(10, 5), EpochAction::Discover);
    }

    #[test]
    fn allocator_picks_reinforce_when_discover_is_not_better_per_cost() {
        let mut est = RewardEstimator::new(1.0);
        est.reinforce_mean = 1.0;
        est.discover_mean = 0.5; // 0.5 / 10 < 1.0
        let alloc = Allocator::new(RealizationPolicy::default(), est);
        assert_eq!(alloc.choose(10, 5), EpochAction::Reinforce);
    }

    #[test]
    fn allocator_discovers_when_library_is_empty() {
        let alloc = Allocator::new(
            RealizationPolicy::default(),
            RewardEstimator::new(0.5),
        );
        assert_eq!(alloc.choose(10, 0), EpochAction::Discover);
    }

    #[test]
    fn policy_serde_round_trips() {
        let p = RealizationPolicy::default();
        let bytes = bincode::serialize(&p).unwrap();
        let decoded: RealizationPolicy = bincode::deserialize(&bytes).unwrap();
        assert_eq!(p, decoded);
    }
}
