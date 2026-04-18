//! R10 — Discovery scorer / policy model.
//!
//! # The self-producing system
//!
//! A `PolicyModel` is a function from (library_state, candidate_rule)
//! → priority score. Bigger score = more promising candidate.
//!
//! The model's weights ARE the system's learned knowledge about
//! what kinds of discoveries lead to tensor emergence. When we
//! persist those weights and load them into the next traversal,
//! the NEW system is produced by the OLD system — self-producing.
//!
//! ```text
//!  Traversal_n  →  Trajectory_n
//!                   │
//!                   ▼
//!                 train
//!                   │
//!                   ▼
//!  PolicyModel_n+1  ──→  seeds Traversal_{n+1}
//! ```
//!
//! The model is the output of each generation and the seed of the
//! next. The substrate never changes — only the weights that steer
//! it. Over many generations, the weights encode an increasingly
//! sophisticated prior over discovery trajectories.
//!
//! # This module
//!
//! - `PolicyModel` trait: abstract scorer interface
//! - `LinearPolicy`: baseline linear-combination scorer, fully
//!   interpretable, trainable via gradient descent on trajectory
//!   data
//! - `LinearPolicy::train_from_trajectory`: supervised updates —
//!   "rewarded" trajectory steps nudge weights toward features
//!   that led to acceptance
//! - `LinearPolicy::serialize` / `deserialize`: the model IS a
//!   bytestring that seeds the next system. Self-producing loop
//!   is literally `bincode::serialize(model) → bincode::deserialize
//!   → model`.
//!
//! # Not in scope (next steps, listed here as a sketch)
//!
//! - Candidate-level features (currently score is a function only
//!   of library state; the candidate is implicit). A richer scorer
//!   would take `(state_feat, candidate_feat)` with candidate_feat
//!   = features of the rule being considered (its shape, size,
//!   tensor classification, etc.)
//! - Nonlinear models (MLP, then a proper NN with learned
//!   embeddings). The `PolicyModel` trait is the abstraction seam.
//! - Reinforcement learning: policy gradient from trajectory
//!   returns, not just supervised from acceptance.
//! - Actor-critic: a critic model estimates value of a state, the
//!   actor chooses candidates.
//!
//! # Why "tensor" as the target
//!
//! The R8 tensor detector gives a scalar `tensor_density` that's
//! nonzero when the machine has discovered bilinear structure.
//! This scalar is in the state features (`LibraryFeatures.
//! tensor_density`), so the scorer can learn to increase it. A
//! supervisor reward function that heavily weights trajectories
//! that REACH tensor trains the scorer to prioritize candidates
//! that push toward distributivity-shaped rules.

use crate::trajectory::{LibraryFeatures, Trajectory, TrajectoryStep};
use serde::{Deserialize, Serialize};

/// A policy model: function from library state to a scalar score
/// representing how PROMISING that state is for producing
/// tensor-reaching trajectories. Higher = better.
///
/// The interface is deliberately minimal so multiple models
/// (linear, MLP, NN) share it. The trainer works against the
/// trait, not concrete types.
pub trait PolicyModel {
    /// Score a library state. Higher scores predict states that
    /// are closer to or more productive for tensor emergence.
    fn score(&self, state: &LibraryFeatures) -> f64;
}

/// A linear model over the feature vector. `score = dot(weights,
/// features) + bias`. Interpretable — each weight says how much
/// that feature contributes to promising-ness. Trainable via
/// gradient descent.
///
/// Weight layout matches `LibraryFeatures::as_vector()` exactly.
/// Changes to that ordering require matching changes here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinearPolicy {
    pub weights: [f64; LibraryFeatures::WIDTH],
    pub bias: f64,
    /// Training metadata: how many trajectory steps this model
    /// has been trained on. Generational counter — a model of
    /// generation N was trained on trajectories from generations
    /// 1..=N-1.
    pub trained_steps: u64,
    /// The generation number. Incremented each time the model is
    /// trained. The self-producing loop emits model of gen N+1 as
    /// the output of training on gen N's trajectory.
    pub generation: u64,
}

impl LinearPolicy {
    /// Create a zero-initialized model. All weights start at zero,
    /// so every state scores equally — the un-trained prior is
    /// uniform. Training breaks the symmetry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            weights: [0.0; LibraryFeatures::WIDTH],
            bias: 0.0,
            trained_steps: 0,
            generation: 0,
        }
    }

    /// A "tensor-seeking" initial model: weight tensor_density
    /// positively, so even without training the scorer prefers
    /// library states with tensor structure. Useful baseline
    /// that encodes the architect's prior before any trajectory
    /// data exists.
    ///
    /// This is the "model that produces its own system" starting
    /// point — once a trajectory reaches tensor, the trained
    /// model takes over, but the seed is explicit.
    #[must_use]
    pub fn tensor_seeking_prior() -> Self {
        let mut p = Self::new();
        // tensor_density is at index 4 in as_vector. Weight it
        // strongly positive.
        p.weights[4] = 1.0;
        // tensor_distributive_count at index 5.
        p.weights[5] = 0.5;
        // tensor_meta_count at index 6.
        p.weights[6] = 0.5;
        p
    }

    /// Train on one trajectory. Simple supervised update:
    /// for each step where a candidate was ACCEPTED, nudge weights
    /// toward that state's features (the state was "worth
    /// proposing from"). For rejected candidates, nudge weights
    /// AWAY from that state's features. ΔDL amplifies the
    /// update magnitude — bigger rewards move weights more.
    ///
    /// This is the simplest possible gradient-like update. A real
    /// scorer would use a proper loss function and
    /// differentiability. For now, this is the skeleton that
    /// demonstrates the self-producing loop: trajectory in,
    /// updated weights out.
    pub fn train_from_trajectory(&mut self, trajectory: &Trajectory, learning_rate: f64) {
        for step in &trajectory.steps {
            self.train_one_step(step, learning_rate);
        }
        // Bonus: if trajectory reached tensor, boost weights of
        // the final state's features — the "successful terminal"
        // signal. This is what makes the model learn to VALUE
        // tensor-dense states, not just acceptance.
        if trajectory.reached_tensor() {
            if let Some(terminal) = &trajectory.terminal_state {
                let v = terminal.as_vector();
                let boost = learning_rate * 2.0; // reward terminal tensor state
                for i in 0..LibraryFeatures::WIDTH {
                    self.weights[i] += boost * v[i];
                }
            }
        }
        self.generation += 1;
    }

    fn train_one_step(&mut self, step: &TrajectoryStep, learning_rate: f64) {
        let v = step.pre_state.as_vector();
        // Sign of the update: accepted candidates push weights
        // toward this feature vector; rejected push away. ΔDL
        // amplifies the magnitude — more reward, more movement.
        let sign = if step.accepted { 1.0 } else { -1.0 };
        let magnitude = learning_rate * sign * step.delta_dl.abs().max(0.01);
        for i in 0..LibraryFeatures::WIDTH {
            self.weights[i] += magnitude * v[i];
        }
        self.bias += magnitude;
        self.trained_steps += 1;
    }

    /// Serialize the model to bytes — the "self-producing output."
    /// Write this to disk between runs; the next run loads it via
    /// `deserialize` and uses it as the seed policy.
    pub fn serialize(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize a model from bytes. The inverse of `serialize`.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}

impl Default for LinearPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyModel for LinearPolicy {
    fn score(&self, state: &LibraryFeatures) -> f64 {
        let v = state.as_vector();
        let mut s = self.bias;
        for i in 0..LibraryFeatures::WIDTH {
            s += self.weights[i] * v[i];
        }
        s
    }
}

/// Rank a set of library states by the model's score. Returns
/// indices sorted highest-score-first. Deterministic on ties
/// (stable sort preserves insertion order).
#[must_use]
pub fn rank_states<P: PolicyModel>(model: &P, states: &[LibraryFeatures]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..states.len()).collect();
    idx.sort_by(|a, b| {
        let sa = model.score(&states[*a]);
        let sb = model.score(&states[*b]);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{ADD, MUL};
    use crate::eval::RewriteRule;
    use crate::term::Term;
    use crate::trajectory::ActionKind;
    use crate::value::Value;

    fn var(id: u32) -> Term {
        Term::Var(id)
    }
    fn app(h: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(h), args)
    }
    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }

    fn identity_rule() -> RewriteRule {
        RewriteRule {
            name: "id".into(),
            lhs: app(var(ADD), vec![var(100), nat(0)]),
            rhs: var(100),
        }
    }

    fn distributive_rule() -> RewriteRule {
        let a = var(100);
        let b = var(101);
        let c = var(102);
        RewriteRule {
            name: "distrib".into(),
            lhs: app(
                var(MUL),
                vec![app(var(ADD), vec![a.clone(), b.clone()]), c.clone()],
            ),
            rhs: app(
                var(ADD),
                vec![
                    app(var(MUL), vec![a.clone(), c.clone()]),
                    app(var(MUL), vec![b.clone(), c.clone()]),
                ],
            ),
        }
    }

    #[test]
    fn fresh_model_scores_everything_zero() {
        let m = LinearPolicy::new();
        let empty = LibraryFeatures::extract(&[]);
        let with_id = LibraryFeatures::extract(&[identity_rule()]);
        assert_eq!(m.score(&empty), 0.0);
        assert_eq!(m.score(&with_id), 0.0);
    }

    #[test]
    fn tensor_seeking_prior_prefers_tensor_states() {
        let m = LinearPolicy::tensor_seeking_prior();
        let no_tensor = LibraryFeatures::extract(&[identity_rule()]);
        let with_tensor = LibraryFeatures::extract(&[distributive_rule()]);
        assert!(
            m.score(&with_tensor) > m.score(&no_tensor),
            "tensor-seeking model must prefer tensor states — \
             no_tensor={} vs with_tensor={}",
            m.score(&no_tensor),
            m.score(&with_tensor),
        );
    }

    #[test]
    fn training_on_accepted_step_moves_weights_toward_features() {
        let mut m = LinearPolicy::new();
        let feat = LibraryFeatures::extract(&[identity_rule()]);

        let mut traj = Trajectory::new();
        traj.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: feat.clone(),
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 1.0,
        });

        m.train_from_trajectory(&traj, 0.1);
        // After training on one accepted step, score of this
        // exact state must be > 0.
        assert!(
            m.score(&feat) > 0.0,
            "training on accepted step should raise score of the trained state"
        );
        assert_eq!(m.trained_steps, 1);
        assert_eq!(m.generation, 1);
    }

    #[test]
    fn training_on_rejected_step_moves_weights_away() {
        let mut m = LinearPolicy::new();
        let feat = LibraryFeatures::extract(&[identity_rule()]);

        let mut traj = Trajectory::new();
        traj.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: feat.clone(),
            action: ActionKind::Discover,
            accepted: false,
            delta_dl: 0.5,
        });

        m.train_from_trajectory(&traj, 0.1);
        assert!(
            m.score(&feat) < 0.0,
            "training on rejected step should lower score of the trained state"
        );
    }

    #[test]
    fn tensor_reaching_trajectory_boosts_final_state_features() {
        let mut m = LinearPolicy::new();
        let no_tensor = LibraryFeatures::extract(&[identity_rule()]);
        let with_tensor = LibraryFeatures::extract(&[distributive_rule()]);

        let mut traj = Trajectory::new();
        traj.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: no_tensor.clone(),
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 1.0,
        });
        traj.finalize(with_tensor.clone());
        assert!(traj.reached_tensor());

        m.train_from_trajectory(&traj, 0.1);
        // Trained model should now score the tensor-reaching
        // terminal state higher than an unrelated no-tensor state.
        assert!(
            m.score(&with_tensor) > m.score(&no_tensor),
            "tensor-reaching training should elevate tensor-state score above no-tensor"
        );
    }

    #[test]
    fn model_serialize_deserialize_roundtrip() {
        // The self-producing invariant: a model's bytes faithfully
        // reconstruct the model. Training generation N,
        // serializing, deserializing at generation N+1, and
        // continuing training must preserve the accumulated
        // learning.
        let mut m = LinearPolicy::tensor_seeking_prior();
        let feat = LibraryFeatures::extract(&[distributive_rule()]);
        let mut traj = Trajectory::new();
        traj.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: feat.clone(),
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 2.0,
        });
        traj.finalize(feat.clone());
        m.train_from_trajectory(&traj, 0.1);

        let bytes = m.serialize().unwrap();
        let back = LinearPolicy::deserialize(&bytes).unwrap();
        assert_eq!(m, back);
        // Scores must match after roundtrip.
        let unrelated = LibraryFeatures::extract(&[identity_rule()]);
        assert!(
            (m.score(&unrelated) - back.score(&unrelated)).abs() < 1e-12,
            "serialized model must produce bit-identical scores"
        );
    }

    #[test]
    fn self_producing_loop_roundtrip_preserves_learning() {
        // Proves the self-producing loop mechanically: a model
        // trained to gen-1 state, serialized, deserialized in a
        // "new run", continues training to gen-2, and its state
        // reflects accumulated learning from BOTH generations.
        let mut gen1 = LinearPolicy::new();
        let feat = LibraryFeatures::extract(&[distributive_rule()]);
        let mut traj1 = Trajectory::new();
        traj1.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: feat.clone(),
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 1.0,
        });
        traj1.finalize(feat.clone());
        gen1.train_from_trajectory(&traj1, 0.1);
        assert_eq!(gen1.generation, 1);

        // Simulate "next run": persist, reload, continue training.
        let bytes = gen1.serialize().unwrap();
        let mut gen2 = LinearPolicy::deserialize(&bytes).unwrap();
        // Gen-2 inherits everything gen-1 learned.
        assert_eq!(gen2.trained_steps, 1);
        assert_eq!(gen2.generation, 1);

        let mut traj2 = Trajectory::new();
        traj2.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: feat.clone(),
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 1.0,
        });
        traj2.finalize(feat.clone());
        gen2.train_from_trajectory(&traj2, 0.1);

        assert_eq!(gen2.generation, 2);
        assert_eq!(gen2.trained_steps, 2);
        // Score of the trained state grows across generations.
        assert!(
            gen2.score(&feat) > gen1.score(&feat),
            "gen-2 should score the trained state higher than gen-1 after additional training"
        );
    }

    #[test]
    fn rank_states_orders_by_score_descending() {
        let m = LinearPolicy::tensor_seeking_prior();
        let no_tensor = LibraryFeatures::extract(&[identity_rule()]);
        let with_tensor = LibraryFeatures::extract(&[distributive_rule()]);
        let mixed = LibraryFeatures::extract(&[identity_rule(), distributive_rule()]);

        let states = vec![no_tensor.clone(), with_tensor.clone(), mixed.clone()];
        let ranked = rank_states(&m, &states);
        // with_tensor (density 1.0) should rank above mixed (0.5)
        // which should rank above no_tensor (0.0).
        assert_eq!(ranked[0], 1, "tensor-only state first");
        assert_eq!(ranked[1], 2, "mixed state second");
        assert_eq!(ranked[2], 0, "no-tensor state last");
    }
}
