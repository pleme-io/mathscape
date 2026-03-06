//! Optional RL policy network for guided mutation (Phase 7+).
//!
//! When enabled, the policy replaces uniform random mutation selection
//! with a learned distribution over mutation operators, conditioned on
//! the current term structure and reward history.
//!
//! Currently provides a uniform policy (no learning). The trait is
//! designed so that a neural network policy can be swapped in later
//! without changing the evolve crate.

use mathscape_core::term::Term;
use rand::Rng;

/// Mutation operator index (maps to MutationOp enum in evolve crate).
pub type MutationOpIndex = usize;

/// Total number of available mutation operators.
pub const NUM_MUTATION_OPS: usize = 6;

/// A policy that selects which mutation operator to apply.
pub trait MutationPolicy: Send + Sync {
    /// Select a mutation operator for the given term.
    fn select_op(&self, term: &Term, rng: &mut dyn RngCore) -> MutationOpIndex;

    /// Update the policy with reward feedback (no-op for uniform policy).
    fn update(&mut self, _op: MutationOpIndex, _reward: f64) {}
}

/// Trait alias for rand::RngCore with Send.
pub use rand::RngCore;

/// Uniform random policy — selects each mutation operator with equal probability.
#[derive(Clone, Debug, Default)]
pub struct UniformPolicy;

impl MutationPolicy for UniformPolicy {
    fn select_op(&self, _term: &Term, rng: &mut dyn RngCore) -> MutationOpIndex {
        rng.gen_range(0..NUM_MUTATION_OPS)
    }
}

/// Tracking policy — uniform selection but records statistics.
#[derive(Clone, Debug)]
pub struct TrackingPolicy {
    pub counts: [u64; NUM_MUTATION_OPS],
    pub total_reward: [f64; NUM_MUTATION_OPS],
}

impl Default for TrackingPolicy {
    fn default() -> Self {
        TrackingPolicy {
            counts: [0; NUM_MUTATION_OPS],
            total_reward: [0.0; NUM_MUTATION_OPS],
        }
    }
}

impl TrackingPolicy {
    /// Average reward per operator.
    pub fn avg_reward(&self) -> [f64; NUM_MUTATION_OPS] {
        let mut avg = [0.0; NUM_MUTATION_OPS];
        for i in 0..NUM_MUTATION_OPS {
            if self.counts[i] > 0 {
                avg[i] = self.total_reward[i] / self.counts[i] as f64;
            }
        }
        avg
    }
}

impl MutationPolicy for TrackingPolicy {
    fn select_op(&self, _term: &Term, rng: &mut dyn RngCore) -> MutationOpIndex {
        rng.gen_range(0..NUM_MUTATION_OPS)
    }

    fn update(&mut self, op: MutationOpIndex, reward: f64) {
        if op < NUM_MUTATION_OPS {
            self.counts[op] += 1;
            self.total_reward[op] += reward;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::value::Value;
    use rand::SeedableRng;

    #[test]
    fn uniform_policy_selects_in_range() {
        let policy = UniformPolicy;
        let term = Term::Number(Value::Nat(42));
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        for _ in 0..100 {
            let op = policy.select_op(&term, &mut rng);
            assert!(op < NUM_MUTATION_OPS);
        }
    }

    #[test]
    fn tracking_policy_records_stats() {
        let mut policy = TrackingPolicy::default();
        let term = Term::Number(Value::Nat(0));
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        for _ in 0..100 {
            let op = policy.select_op(&term, &mut rng);
            policy.update(op, 1.0);
        }

        let total: u64 = policy.counts.iter().sum();
        assert_eq!(total, 100);

        let avg = policy.avg_reward();
        for a in &avg {
            assert!((*a - 1.0).abs() < f64::EPSILON || *a == 0.0);
        }
    }

    #[test]
    fn tracking_avg_reward_correct() {
        let mut policy = TrackingPolicy::default();
        policy.update(0, 2.0);
        policy.update(0, 4.0);
        policy.update(1, 3.0);

        let avg = policy.avg_reward();
        assert!((avg[0] - 3.0).abs() < f64::EPSILON);
        assert!((avg[1] - 3.0).abs() < f64::EPSILON);
        assert!((avg[2] - 0.0).abs() < f64::EPSILON);
    }
}
