//! R11 — Environment: L8 in the inception ladder.
//!
//! See `docs/arch/inception.md` for the full layer stack. This
//! module provides the deployment unit — a first-class bundle of
//! the four things a model needs to run autonomously:
//!
//! 1. **Corpus source** — what terms the model traverses. Varies
//!    across environments to expose environment-specific vs
//!    universal discoveries.
//! 2. **Mechanism config** — discovery parameters (ML4's
//!    `MechanismConfig` lives in mathscape-proof, so this module
//!    stores a type-erased handle — string keys + values — that
//!    the axiom-bridge layer lifts into a concrete config).
//! 3. **Policy** — the scorer that guides candidate prioritization
//!    (R9/R10 LinearPolicy, or a future NN).
//! 4. **Seed** — RNG seed for deterministic replay. Two
//!    environments with identical corpus+mechanism+policy but
//!    different seeds explore different slices of the same
//!    distribution.
//!
//! # The deployment game
//!
//! ```text
//!   (policy converged at L6/L7)
//!                ↓
//!         deploy to N
//!         Environments
//!                ↓
//!     each runs autonomously:
//!     traverse → collect trajectory
//!              → update local policy
//!              → emit discovered rules
//!                ↓
//!     intersect discoveries across envs
//!                ↓
//!    A_universal = ∩ Aᵢ
//!    (the mathscape's invariants)
//! ```
//!
//! # Not in scope
//!
//! - Running the environment (that's the axiom-bridge layer —
//!   needs Generator/Registry/DiscoveryForest wiring).
//! - Intersection across environments (L9 — future work).
//! - Corpus generator DSL (tatara-lisp form for corpus spec).
//!
//! The Environment here is a DESCRIPTOR — enough structure to say
//! "run this", not yet the runtime that executes it.

use crate::policy::LinearPolicy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A corpus descriptor — what terms the model will traverse.
/// Tags the shape; the axiom-bridge layer knows how to instantiate
/// each variant into a concrete corpus. Keeping this here (in
/// core, not axiom-bridge) lets environments be first-class
/// serialized data independent of runtime wiring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CorpusShape {
    /// The canonical hand-crafted zoo — Peano successor chains,
    /// identity probes, etc. Current default for autonomous
    /// traverse.
    Zoo,
    /// Procedurally generated corpora over a domain. `budget`
    /// controls sweep size, `max_depth` controls complexity.
    Procedural {
        budget: usize,
        max_depth: usize,
    },
    /// A specific operator vocabulary to emphasize. For testing
    /// whether an environment weighted toward Int produces
    /// Int-specific discoveries (and vice versa for Nat / mixed).
    /// Operator ids match `crate::builtin` — 0..=3 Nat, 10..=14 Int.
    OperatorBiased {
        operators: Vec<u32>,
        budget: usize,
        max_depth: usize,
    },
    /// Custom named shape — deferred to the axiom-bridge layer to
    /// interpret. Lets external tools define corpora by name
    /// without extending this enum.
    Named(String),
}

impl CorpusShape {
    /// The default zoo shape used by autonomous_traverse_small.
    pub fn default_zoo() -> Self {
        Self::Zoo
    }

    /// Shape that exercises procedural diversity — the "stress"
    /// environment.
    pub fn stress() -> Self {
        Self::Procedural {
            budget: 40,
            max_depth: 5,
        }
    }

    /// Int-heavy environment. Requires the axiom-bridge layer to
    /// actually seed Int-valued corpora when it sees this shape;
    /// by itself the enum variant is just the intent tag.
    pub fn int_biased() -> Self {
        Self::OperatorBiased {
            operators: vec![
                crate::builtin::INT_ZERO,
                crate::builtin::INT_SUCC,
                crate::builtin::INT_ADD,
                crate::builtin::INT_MUL,
                crate::builtin::NEG,
            ],
            budget: 12,
            max_depth: 4,
        }
    }
}

/// A type-erased view of the mechanism config. The concrete
/// `MechanismConfig` (ML4) lives in mathscape-proof, so core
/// can't reference it directly without inverting the dep. We
/// store the configuration as a BTreeMap of parameter names to
/// integer values — string-typed but sufficient for an
/// Environment descriptor. The axiom-bridge layer lifts this
/// BTreeMap back into a typed `MechanismConfig`.
///
/// Using BTreeMap (not HashMap) for deterministic iteration —
/// same dependency-direction reason as the C2 fix in
/// `pattern_match`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MechanismSnapshot {
    pub params: BTreeMap<String, i64>,
}

impl MechanismSnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(mut self, key: impl Into<String>, value: i64) -> Self {
        self.params.insert(key.into(), value);
        self
    }

    /// Default snapshot mirroring the current traversal defaults.
    /// See mathscape-proof's MechanismConfig for canonical names.
    pub fn default_snapshot() -> Self {
        Self::new()
            .set("candidate-max-size", 4)
            .set("composition-cap", 30)
            .set("corpus-base-depth", 3)
            .set("corpus-max-value", 8)
            .set("extract-min-shared-size", 2)
            .set("extract-min-matches", 2)
            .set("extract-max-new-rules", 10)
            .set("validator-samples", 32)
            .set("validator-max-value", 16)
    }
}

/// The deployment unit. Bundle a corpus, a mechanism, a policy,
/// and a seed; hand to the runtime; get back a trained model
/// and a set of discovered rules. This is what a deployed
/// mathscape prospector looks like from outside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    /// Human-readable name. Useful when intersecting discoveries
    /// across environments — identifies which env each rule
    /// came from.
    pub name: String,
    pub corpus: CorpusShape,
    pub mechanism: MechanismSnapshot,
    pub policy: LinearPolicy,
    /// RNG seed for deterministic replay. Two environments with
    /// identical everything-else but different seed sample
    /// different slices of the same corpus distribution.
    pub seed: u64,
}

impl Environment {
    /// A sensible default — the zoo, default mechanism, fresh
    /// tensor-seeking policy, seed 1.
    pub fn default_zoo() -> Self {
        Self {
            name: "zoo-default".into(),
            corpus: CorpusShape::default_zoo(),
            mechanism: MechanismSnapshot::default_snapshot(),
            policy: LinearPolicy::tensor_seeking_prior(),
            seed: 1,
        }
    }

    /// Reseed — clone with a new seed. Multiple seeds over the
    /// same corpus+mechanism+policy = ensemble sampling.
    #[must_use]
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Rename — useful when creating environment variants.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Swap the corpus shape — one mechanism / one policy, many
    /// corpora. Measures how robust discoveries are to corpus
    /// shift.
    #[must_use]
    pub fn with_corpus(mut self, corpus: CorpusShape) -> Self {
        self.corpus = corpus;
        self
    }

    /// Swap the policy — same corpus / mechanism, different
    /// learner. Measures policy sensitivity.
    #[must_use]
    pub fn with_policy(mut self, policy: LinearPolicy) -> Self {
        self.policy = policy;
        self
    }
}

/// A canonical environment suite for post-convergence deployment.
/// Six environments that span the variation dimensions laid out
/// in `inception.md`: default, stress, Int-biased, and seed
/// variants.
///
/// When L9 intersection discovery runs, these are the initial
/// environments whose Axiomatized rule sets get intersected.
#[must_use]
pub fn canonical_deployment_suite() -> Vec<Environment> {
    let base = Environment::default_zoo();
    vec![
        base.clone(),
        base.clone()
            .named("stress")
            .with_corpus(CorpusShape::stress()),
        base.clone()
            .named("int-biased")
            .with_corpus(CorpusShape::int_biased()),
        base.clone().named("seed-7").with_seed(7),
        base.clone().named("seed-42").with_seed(42),
        base.named("seed-1000").with_seed(1000),
    ]
}

/// Convergence tracking across generations of a self-producing
/// policy loop. Each call to `record_generation` logs the
/// `fixed_point_distance` — the L2 distance between successive
/// policy weight vectors. When this drops below ε for K
/// consecutive generations, the generation has reached the
/// self-producing fixed point (per `inception.md`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConvergenceTracker {
    /// Per-generation L2 distance from the previous generation.
    /// Index 0 is gen 0 → gen 1, etc.
    pub distances: Vec<f64>,
    /// The ε threshold: distance below this counts as
    /// "unchanged".
    pub epsilon: f64,
    /// How many consecutive sub-ε generations required to declare
    /// convergence.
    pub required_stable_count: usize,
}

impl ConvergenceTracker {
    pub fn new(epsilon: f64, required_stable_count: usize) -> Self {
        Self {
            distances: Vec::new(),
            epsilon,
            required_stable_count,
        }
    }

    /// A sensible default: ε = 1e-4, K = 3 consecutive generations.
    pub fn default_tracker() -> Self {
        Self::new(1e-4, 3)
    }

    /// Record one generation's distance from the previous.
    pub fn record(&mut self, prev: &LinearPolicy, curr: &LinearPolicy) {
        let d = policy_distance(prev, curr);
        self.distances.push(d);
    }

    /// True if the last K generations all had distance < ε.
    /// False before K generations are recorded.
    #[must_use]
    pub fn converged(&self) -> bool {
        if self.distances.len() < self.required_stable_count {
            return false;
        }
        let tail = &self.distances[self.distances.len() - self.required_stable_count..];
        tail.iter().all(|d| *d < self.epsilon)
    }

    /// Number of generations recorded so far.
    pub fn generations(&self) -> usize {
        self.distances.len()
    }
}

/// L2 distance between two policies' weight vectors (plus bias).
/// Used by `ConvergenceTracker` to detect the self-producing
/// fixed point.
#[must_use]
pub fn policy_distance(a: &LinearPolicy, b: &LinearPolicy) -> f64 {
    let mut s = (a.bias - b.bias).powi(2);
    for i in 0..crate::trajectory::LibraryFeatures::WIDTH {
        s += (a.weights[i] - b.weights[i]).powi(2);
    }
    s.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_zoo_environment_is_named_and_sensible() {
        let e = Environment::default_zoo();
        assert_eq!(e.name, "zoo-default");
        assert!(matches!(e.corpus, CorpusShape::Zoo));
        assert!(!e.mechanism.params.is_empty());
        assert_eq!(e.seed, 1);
    }

    #[test]
    fn environment_seed_variants_are_distinct() {
        let a = Environment::default_zoo().with_seed(1);
        let b = Environment::default_zoo().with_seed(42);
        assert_ne!(a, b);
        // But everything else equal.
        assert_eq!(a.name, b.name);
        assert_eq!(a.corpus, b.corpus);
        assert_eq!(a.mechanism, b.mechanism);
    }

    #[test]
    fn corpus_shape_int_biased_uses_int_domain_ids() {
        let shape = CorpusShape::int_biased();
        match shape {
            CorpusShape::OperatorBiased { operators, .. } => {
                // Must include only Int-domain ids (10..=14).
                for op in operators {
                    assert!(
                        (10..=14).contains(&op),
                        "int_biased operator {op} must be in Int domain"
                    );
                }
            }
            _ => panic!("int_biased must produce OperatorBiased shape"),
        }
    }

    #[test]
    fn canonical_suite_has_diverse_environments() {
        let suite = canonical_deployment_suite();
        assert_eq!(suite.len(), 6);
        // Names all distinct.
        let names: std::collections::HashSet<_> =
            suite.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names.len(), 6, "environment names must be distinct");
        // Corpus shapes vary (at least 3 distinct).
        let shapes: std::collections::HashSet<_> =
            suite.iter().map(|e| format!("{:?}", e.corpus)).collect();
        assert!(shapes.len() >= 3, "suite must include diverse corpora");
    }

    #[test]
    fn mechanism_snapshot_roundtrips_via_bincode() {
        let m = MechanismSnapshot::default_snapshot();
        let bytes = bincode::serialize(&m).unwrap();
        let back: MechanismSnapshot = bincode::deserialize(&bytes).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn environment_roundtrips_via_bincode() {
        let e = Environment::default_zoo();
        let bytes = bincode::serialize(&e).unwrap();
        let back: Environment = bincode::deserialize(&bytes).unwrap();
        assert_eq!(e, back);
    }

    // ── Convergence tracking ─────────────────────────────────────

    #[test]
    fn policy_distance_zero_for_identical_policies() {
        let a = LinearPolicy::new();
        let b = LinearPolicy::new();
        assert_eq!(policy_distance(&a, &b), 0.0);
    }

    #[test]
    fn policy_distance_is_l2_norm() {
        let a = LinearPolicy::new();
        let mut b = LinearPolicy::new();
        b.bias = 3.0;
        b.weights[0] = 4.0;
        // L2 of (3, 4, 0, 0, ...) = 5.
        assert!((policy_distance(&a, &b) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn convergence_tracker_not_converged_before_k_generations() {
        let mut t = ConvergenceTracker::new(1e-3, 3);
        assert!(!t.converged());
        let p = LinearPolicy::new();
        t.record(&p, &p);
        assert!(!t.converged(), "1 generation is not enough");
        t.record(&p, &p);
        assert!(!t.converged(), "2 generations is not enough");
        t.record(&p, &p);
        assert!(t.converged(), "3 identical generations should converge");
    }

    #[test]
    fn convergence_tracker_resets_on_diverging_generation() {
        // A stable stretch followed by a jump must NOT report
        // converged — the tail-K-generations rule enforces that
        // convergence requires the MOST RECENT K generations to
        // all be sub-ε.
        let mut t = ConvergenceTracker::new(1e-3, 3);
        let p0 = LinearPolicy::new();
        let mut p1 = LinearPolicy::new();
        p1.bias = 1.0; // big distance from p0

        // Three "stable" (identical) gens.
        t.record(&p0, &p0);
        t.record(&p0, &p0);
        t.record(&p0, &p0);
        assert!(t.converged());

        // Then a divergence.
        t.record(&p0, &p1);
        assert!(!t.converged(), "recent jump must break convergence");
    }

    #[test]
    fn convergence_tracker_counts_generations() {
        let mut t = ConvergenceTracker::default_tracker();
        assert_eq!(t.generations(), 0);
        let p = LinearPolicy::new();
        t.record(&p, &p);
        t.record(&p, &p);
        assert_eq!(t.generations(), 2);
    }

    #[test]
    fn default_tracker_uses_architect_defaults() {
        let t = ConvergenceTracker::default_tracker();
        assert!((t.epsilon - 1e-4).abs() < 1e-12);
        assert_eq!(t.required_stable_count, 3);
    }
}
