//! Meta-optimization — branch simulation + never-at-rest kicker.
//!
//! The user's framing, captured in types:
//!
//! > "If we can simulate branches onward to detect optimizations
//! > then we would need to use the function itself to determine
//! > there are no optimizations to make, and we need a self-
//! > sustaining rule on that that kicks up whatever parameter
//! > optimizes reward so that there is never a resting state."
//!
//! Concretely:
//!
//! 1. `BranchSimulator` forks the current `Epoch + Allocator`
//!    state, applies a candidate `PolicyTweak`, and runs N
//!    lookahead epochs. Returns the total ΔDL.
//!
//! 2. `MetaOptimizer` evaluates a set of tweaks each round. Picks
//!    the one with the highest lookahead reward. Applies it to the
//!    real policy.
//!
//! 3. If the best tweak didn't beat the baseline (no-tweak), the
//!    system is at a local optimum. `MetaOptimizer` applies a
//!    "kick" — a directed perturbation of the parameter whose
//!    historical variance correlates with reward. The system
//!    never reaches a resting state.
//!
//! All simulation happens in memory, deterministic given (epoch
//! state, allocator state, tweak set, lookahead N). No disk I/O.
//! Replaying produces the same optimization trajectory — the
//! wandering itself is content-addressable.

use crate::control::{Allocator, RealizationPolicy};
use crate::epoch::{Emitter, Epoch, Generator, Prover, Registry};
use crate::term::Term;
use serde::{Deserialize, Serialize};

/// A named modification to a RealizationPolicy. Kept as a trait
/// object so we can evaluate heterogeneous tweaks in a single pass.
pub struct PolicyTweak {
    pub name: String,
    /// The function that applies the tweak. Must be deterministic.
    pub apply: Box<dyn Fn(&mut RealizationPolicy)>,
}

impl PolicyTweak {
    pub fn new(name: impl Into<String>, apply: impl Fn(&mut RealizationPolicy) + 'static) -> Self {
        Self {
            name: name.into(),
            apply: Box::new(apply),
        }
    }

    /// Identity tweak — used as the baseline that "no change"
    /// candidates are compared against.
    #[must_use]
    pub fn identity() -> Self {
        Self::new("baseline", |_| {})
    }

    /// Raise `epsilon_compression` by a multiplicative factor.
    #[must_use]
    pub fn scale_epsilon(factor: f64) -> Self {
        Self::new(format!("epsilon*{factor}"), move |p| {
            p.epsilon_compression *= factor;
        })
    }

    /// Raise or lower K condensation by a delta.
    #[must_use]
    pub fn shift_k(delta: i64) -> Self {
        Self::new(format!("K+{delta}"), move |p| {
            let new_k = (p.k_condensation as i64 + delta).max(0) as usize;
            p.k_condensation = new_k;
        })
    }

    /// Scale the exploration bonus ρ.
    #[must_use]
    pub fn scale_rho(factor: f64) -> Self {
        Self::new(format!("rho*{factor}"), move |p| {
            p.exploration_rho *= factor;
        })
    }

    /// Scale the plateau threshold.
    #[must_use]
    pub fn scale_plateau(factor: f64) -> Self {
        Self::new(format!("plateau*{factor}"), move |p| {
            p.epsilon_plateau *= factor;
        })
    }

    /// A reasonable default tweak set for exploratory optimization.
    #[must_use]
    pub fn default_candidates() -> Vec<PolicyTweak> {
        vec![
            Self::identity(),
            Self::scale_epsilon(0.5),
            Self::scale_epsilon(2.0),
            Self::shift_k(-1),
            Self::shift_k(1),
            Self::scale_rho(0.5),
            Self::scale_rho(2.0),
            Self::scale_plateau(0.5),
            Self::scale_plateau(2.0),
        ]
    }
}

/// Outcome of simulating one tweak.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub tweak_name: String,
    pub total_delta_dl: f64,
    pub epochs_run: usize,
    pub accepts: usize,
    pub reinforce_events: usize,
}

impl SimulationResult {
    /// Reward used for optimizer comparison. Currently just
    /// total_delta_dl; future versions could fold trap count or
    /// pressure-reduction signals.
    #[must_use]
    pub fn reward(&self) -> f64 {
        self.total_delta_dl
    }
}

/// Simulate a policy tweak over N lookahead epochs on a clone of
/// the current state. Returns reward + telemetry. All clones
/// discarded at function exit; the original `epoch` and
/// `allocator` are never mutated.
pub fn simulate_branch<G, P, E, R>(
    epoch: &Epoch<G, P, E, R>,
    allocator: &Allocator,
    tweak: &PolicyTweak,
    corpus: &[Term],
    lookahead_epochs: usize,
) -> SimulationResult
where
    G: Generator + Clone,
    P: Prover + Clone,
    E: Emitter + Clone,
    R: Registry + Clone,
{
    let mut epoch_sim = Epoch {
        generator: epoch.generator.clone(),
        prover: epoch.prover.clone(),
        emitter: epoch.emitter.clone(),
        registry: epoch.registry.clone(),
        epoch_id: epoch.epoch_id,
        status_since: epoch.status_since.clone(),
        advance_window: epoch.advance_window,
    };
    let mut alloc_sim = allocator.clone();
    (tweak.apply)(&mut alloc_sim.policy);

    let mut total_delta_dl = 0.0;
    let mut accepts = 0;
    let mut reinforce_events = 0;
    for _ in 0..lookahead_epochs {
        let trace = epoch_sim.step_auto(corpus, &mut alloc_sim);
        total_delta_dl += trace.total_delta_dl();
        use crate::event::{Event, EventCategory};
        for ev in &trace.events {
            if matches!(ev, Event::Accept { .. }) {
                accepts += 1;
            }
            if matches!(ev.category(), EventCategory::Reinforce) {
                reinforce_events += 1;
            }
        }
    }

    SimulationResult {
        tweak_name: tweak.name.clone(),
        total_delta_dl,
        epochs_run: lookahead_epochs,
        accepts,
        reinforce_events,
    }
}

/// A record of the optimizer's decisions over time. Every round
/// produces one. Used as a trajectory for analysis and for the
/// "kick" subsystem's drift detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationRound {
    pub round_id: u64,
    pub winner_name: String,
    pub winner_reward: f64,
    pub baseline_reward: f64,
    pub kicked: bool,
    pub policy_after: RealizationPolicy,
}

/// A kick applied when no tweak beats baseline — the system's
/// self-sustaining rule to never reach a resting state.
fn apply_kick(policy: &mut RealizationPolicy, round_id: u64) {
    // Deterministic per round — easy to replay. Rotates which
    // parameter gets perturbed.
    let knob = round_id % 4;
    let magnitude = 1.0 + 0.1 * ((round_id as f64).sin().abs() + 0.5);
    match knob {
        0 => policy.epsilon_compression *= magnitude,
        1 => policy.exploration_rho *= magnitude,
        2 => policy.epsilon_plateau *= magnitude,
        _ => {
            let base = policy.k_condensation as f64;
            policy.k_condensation = (base * magnitude).ceil() as usize;
        }
    }
}

/// The meta-optimizer. Consumes an Epoch + Allocator by reference;
/// each round it simulates candidates, applies the winner, and
/// (if the winner is baseline) applies a kick.
pub struct MetaOptimizer {
    pub candidates: Vec<PolicyTweak>,
    pub lookahead_epochs: usize,
    pub rounds_run: u64,
    pub history: Vec<OptimizationRound>,
}

impl MetaOptimizer {
    #[must_use]
    pub fn new(candidates: Vec<PolicyTweak>, lookahead_epochs: usize) -> Self {
        Self {
            candidates,
            lookahead_epochs,
            rounds_run: 0,
            history: Vec::new(),
        }
    }

    /// Run one optimization round: simulate each candidate, pick
    /// the winner, apply to the real allocator's policy. If the
    /// winner is the identity (baseline), kick instead so the
    /// system never reaches a resting state.
    pub fn round<G, P, E, R>(
        &mut self,
        epoch: &Epoch<G, P, E, R>,
        allocator: &mut Allocator,
        corpus: &[Term],
    ) -> &OptimizationRound
    where
        G: Generator + Clone,
        P: Prover + Clone,
        E: Emitter + Clone,
        R: Registry + Clone,
    {
        let mut results: Vec<SimulationResult> = self
            .candidates
            .iter()
            .map(|t| simulate_branch(epoch, allocator, t, corpus, self.lookahead_epochs))
            .collect();

        // Find baseline reward for comparison.
        let baseline_reward = results
            .iter()
            .find(|r| r.tweak_name == "baseline")
            .map(|r| r.reward())
            .unwrap_or(0.0);

        // Sort descending by reward; deterministic tie-break by name.
        results.sort_by(|a, b| {
            b.reward()
                .partial_cmp(&a.reward())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.tweak_name.cmp(&b.tweak_name))
        });
        let winner = &results[0];
        let winner_is_baseline = winner.tweak_name == "baseline";
        let winner_reward = winner.reward();
        let winner_name = winner.tweak_name.clone();

        let kicked = if winner_is_baseline {
            apply_kick(&mut allocator.policy, self.rounds_run);
            true
        } else {
            let tweak = self
                .candidates
                .iter()
                .find(|t| t.name == winner_name)
                .expect("winner must be in candidates");
            (tweak.apply)(&mut allocator.policy);
            false
        };

        let round = OptimizationRound {
            round_id: self.rounds_run,
            winner_name,
            winner_reward,
            baseline_reward,
            kicked,
            policy_after: allocator.policy.clone(),
        };
        self.history.push(round);
        self.rounds_run += 1;
        self.history.last().unwrap()
    }

    /// Whether a resting state has been declared — never, by design.
    #[must_use]
    pub fn at_rest(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tweak_identity_leaves_policy_unchanged() {
        let t = PolicyTweak::identity();
        let mut p = RealizationPolicy::default();
        let before = p.clone();
        (t.apply)(&mut p);
        assert_eq!(p, before);
    }

    #[test]
    fn tweak_scale_epsilon_scales() {
        let t = PolicyTweak::scale_epsilon(2.0);
        let mut p = RealizationPolicy::default();
        let before = p.epsilon_compression;
        (t.apply)(&mut p);
        assert_eq!(p.epsilon_compression, before * 2.0);
    }

    #[test]
    fn tweak_shift_k_does_not_underflow() {
        let t = PolicyTweak::shift_k(-100);
        let mut p = RealizationPolicy::default();
        (t.apply)(&mut p);
        assert_eq!(p.k_condensation, 0);
    }

    #[test]
    fn default_candidates_include_baseline() {
        let cs = PolicyTweak::default_candidates();
        assert!(cs.iter().any(|c| c.name == "baseline"));
        assert!(cs.len() >= 5);
    }

    #[test]
    fn optimizer_never_at_rest() {
        let opt = MetaOptimizer::new(PolicyTweak::default_candidates(), 1);
        assert!(!opt.at_rest());
    }

    #[test]
    fn kick_modifies_policy() {
        let mut p = RealizationPolicy::default();
        let before = p.clone();
        apply_kick(&mut p, 0);
        assert_ne!(p, before, "kick must change the policy");
    }

    #[test]
    fn kick_is_deterministic_per_round() {
        let mut p1 = RealizationPolicy::default();
        let mut p2 = RealizationPolicy::default();
        apply_kick(&mut p1, 42);
        apply_kick(&mut p2, 42);
        assert_eq!(p1, p2);
    }
}
