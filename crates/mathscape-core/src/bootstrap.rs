//! R26 — BootstrapCycle: the typescape entity for self-producing discovery.
//!
//! # What this closes
//!
//! R25 demonstrated the self-bootstrapping loop as a test: empty
//! library → discover tensor primitives → use primitives → discover
//! more → train a model. That test HAS the behavior but it's not
//! reusable — the logic lives in one test function.
//!
//! R26 enshrines the same behavior as a FIRST-CLASS TYPED ENTITY
//! with layered traits so each component can be independently
//! hijacked/optimized at runtime. The key insight: a self-producing
//! system must expose its internal interfaces cleanly so chaos can
//! be encapsulated inside implementations, not inside the caller's
//! understanding.
//!
//! # The layers
//!
//! ```text
//!   ┌─ BootstrapCycle ─────────────────────────────────────┐
//!   │                                                       │
//!   │    ┌─ CorpusGenerator  ─ makes corpora per iteration │
//!   │    ├─ LawExtractor     ─ derives laws from a corpus  │
//!   │    ├─ ModelUpdater     ─ trains policy on trajectory │
//!   │    └─ AttestationHook  ─ BLAKE3 at each boundary      │
//!   │                                                       │
//!   └──────────────────────────────────────────────────────┘
//! ```
//!
//! Each trait is a seam. Swap the implementation and the outer
//! cycle continues to work. Tests prove the hijack property.
//!
//! # Attestation
//!
//! Every BootstrapOutcome carries a BLAKE3 `attestation_hash`
//! that covers:
//!   - final library content
//!   - final policy weights + bias + generation
//!   - iteration count
//!   - trajectory step count
//!
//! Two cycles with identical inputs produce identical attestation.
//! Useful for:
//!   - deterministic_replay at the cycle level
//!   - audit trails across cycle generations
//!   - detecting drift when a layer implementation changes

use crate::eval::RewriteRule;
use crate::hash::TermRef;
use crate::policy::LinearPolicy;
use crate::term::Term;
use crate::trajectory::{ActionKind, LibraryFeatures, Trajectory, TrajectoryStep};
use serde::{Deserialize, Serialize};

// ── Layer traits ───────────────────────────────────────────────────

/// Layer 1: corpus generation. Given the iteration index and the
/// current library, produce the next corpus.
///
/// Hijack: swap this to feed different corpora shapes (R21 tensor
/// corpus, domain-specific corpora, adversarial corpora for
/// refutation experiments).
pub trait CorpusGenerator {
    fn generate(&self, iteration: usize, library: &[RewriteRule]) -> Vec<Term>;
}

/// Layer 2: law extraction. Given a corpus and the current library
/// (used as eval context), produce new candidate rules.
///
/// Hijack: swap this for different discovery strategies — R24's
/// paired-AU discovery, or a future e-graph-based extractor, or
/// a neural candidate generator. The BootstrapCycle doesn't care
/// how the laws were derived.
pub trait LawExtractor {
    fn extract(
        &self,
        corpus: &[Term],
        library: &[RewriteRule],
    ) -> Vec<RewriteRule>;
}

/// Layer 3: model update. Trains the policy given the current
/// trajectory. Default implementation calls `train_from_trajectory`.
///
/// Hijack: swap for different training strategies — bigger learning
/// rate, MLP instead of linear, actor-critic, etc.
pub trait ModelUpdater {
    fn update(&self, policy: &mut LinearPolicy, trajectory: &Trajectory);
}

// ── Default implementations ────────────────────────────────────────

/// Default corpus generator mirroring R25's seed strategy. Iteration
/// 0 feeds pure tensor-identity instances; later iterations add
/// compositional structure so discovered laws can surface.
#[derive(Debug, Clone, Default)]
pub struct DefaultCorpusGenerator;

impl CorpusGenerator for DefaultCorpusGenerator {
    fn generate(&self, iteration: usize, _library: &[RewriteRule]) -> Vec<Term> {
        use crate::builtin::{TENSOR_ADD, TENSOR_MUL};
        use crate::value::Value;

        let zeros = Term::Number(Value::tensor(vec![2], vec![0, 0]).unwrap());
        let ones = Term::Number(Value::tensor(vec![2], vec![1, 1]).unwrap());
        let operands: Vec<Term> = (2..=9)
            .map(|k| {
                Term::Number(
                    Value::tensor(vec![2], vec![k as i64, (k + 1) as i64])
                        .unwrap(),
                )
            })
            .collect();

        let mut corpus: Vec<Term> = Vec::new();
        match iteration {
            0 => {
                for op in &operands {
                    corpus.push(Term::Apply(
                        Box::new(Term::Var(TENSOR_ADD)),
                        vec![zeros.clone(), op.clone()],
                    ));
                    corpus.push(Term::Apply(
                        Box::new(Term::Var(TENSOR_MUL)),
                        vec![ones.clone(), op.clone()],
                    ));
                }
            }
            _ => {
                // Later iterations: nested compositions.
                for op in &operands {
                    let inner = Term::Apply(
                        Box::new(Term::Var(TENSOR_ADD)),
                        vec![zeros.clone(), op.clone()],
                    );
                    corpus.push(Term::Apply(
                        Box::new(Term::Var(TENSOR_ADD)),
                        vec![zeros.clone(), inner.clone()],
                    ));
                    let inner_mul = Term::Apply(
                        Box::new(Term::Var(TENSOR_MUL)),
                        vec![ones.clone(), op.clone()],
                    );
                    corpus.push(Term::Apply(
                        Box::new(Term::Var(TENSOR_MUL)),
                        vec![ones.clone(), inner_mul.clone()],
                    ));
                }
            }
        }
        corpus
    }
}

/// Default model updater — calls train_from_trajectory with a
/// fixed learning rate of 0.05. Swap this to adjust training
/// dynamics or use a non-linear model.
#[derive(Debug, Clone)]
pub struct DefaultModelUpdater {
    pub learning_rate: f64,
}

impl Default for DefaultModelUpdater {
    fn default() -> Self {
        Self {
            learning_rate: 0.05,
        }
    }
}

impl ModelUpdater for DefaultModelUpdater {
    fn update(&self, policy: &mut LinearPolicy, trajectory: &Trajectory) {
        policy.train_from_trajectory(trajectory, self.learning_rate);
    }
}

// ── BootstrapCycle ─────────────────────────────────────────────────

/// The layered, pluggable self-producing discovery cycle.
///
/// Compose three trait objects and call `run()` with a seed
/// library and seed policy. Get back a `BootstrapOutcome` with
/// the full trace and an attestation hash.
///
/// Generic over the three layers so each has zero-cost dispatch.
/// Use `BootstrapCycle::<DefaultCorpusGenerator, _, DefaultModelUpdater>::new(...)`
/// with a LawExtractor implementation (defined in mathscape-compress
/// via R24's `derive_laws_from_corpus`).
pub struct BootstrapCycle<C, E, M>
where
    C: CorpusGenerator,
    E: LawExtractor,
    M: ModelUpdater,
{
    pub corpus_gen: C,
    pub extractor: E,
    pub updater: M,
    pub n_iterations: usize,
    pub eval_step_limit: usize,
    pub min_law_support: usize,
}

impl<C, E, M> BootstrapCycle<C, E, M>
where
    C: CorpusGenerator,
    E: LawExtractor,
    M: ModelUpdater,
{
    pub fn new(
        corpus_gen: C,
        extractor: E,
        updater: M,
        n_iterations: usize,
    ) -> Self {
        Self {
            corpus_gen,
            extractor,
            updater,
            n_iterations,
            eval_step_limit: 300,
            min_law_support: 2,
        }
    }

    /// Execute the cycle: run N iterations, record trajectory,
    /// train the policy, emit BootstrapOutcome with attestation.
    pub fn run(
        &self,
        seed_library: Vec<RewriteRule>,
        seed_policy: LinearPolicy,
    ) -> BootstrapOutcome {
        let mut library = seed_library;
        let mut policy = seed_policy;
        let mut trajectory = Trajectory::new();
        let mut iterations: Vec<IterationSnapshot> = Vec::new();

        for iter in 0..self.n_iterations {
            let corpus = self.corpus_gen.generate(iter, &library);
            let library_size_before = library.len();
            let features_before = LibraryFeatures::extract(&library);

            let new_laws = self.extractor.extract(&corpus, &library);
            let accepted = !new_laws.is_empty();

            library.extend(new_laws.clone());
            let features_after = LibraryFeatures::extract(&library);

            trajectory.record(TrajectoryStep {
                epoch: iter,
                corpus_index: iter,
                pre_state: features_before,
                action: ActionKind::Discover,
                accepted,
                delta_dl: new_laws.len() as f64,
            });

            iterations.push(IterationSnapshot {
                iter,
                corpus_size: corpus.len(),
                library_size_before,
                new_law_count: new_laws.len(),
                features_after,
            });
        }

        trajectory.finalize(LibraryFeatures::extract(&library));
        self.updater.update(&mut policy, &trajectory);

        let attestation = compute_attestation(&library, &policy, &trajectory);

        BootstrapOutcome {
            iterations,
            final_library: library,
            final_policy: policy,
            trajectory,
            attestation,
        }
    }
}

// ── Outcome + attestation ──────────────────────────────────────────

/// Per-iteration summary captured during a cycle run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IterationSnapshot {
    pub iter: usize,
    pub corpus_size: usize,
    pub library_size_before: usize,
    pub new_law_count: usize,
    pub features_after: LibraryFeatures,
}

/// Full outcome of a BootstrapCycle run. Every field is
/// bincode-serializable; the attestation hash covers the whole.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BootstrapOutcome {
    pub iterations: Vec<IterationSnapshot>,
    pub final_library: Vec<RewriteRule>,
    pub final_policy: LinearPolicy,
    pub trajectory: Trajectory,
    /// BLAKE3 attestation — `compute_attestation(library, policy,
    /// trajectory)`. Covers the whole cycle result. Use this to
    /// detect drift when a layer implementation changes.
    pub attestation: TermRef,
}

/// Compute a BLAKE3 attestation hash for a cycle outcome. The
/// hash is of a canonical serialization of (library, policy,
/// trajectory). Two outcomes with identical content produce
/// identical attestations — the foundation of cycle-level
/// deterministic_replay.
pub fn compute_attestation(
    library: &[RewriteRule],
    policy: &LinearPolicy,
    trajectory: &Trajectory,
) -> TermRef {
    let payload = (
        library
            .iter()
            .map(|r| (r.name.clone(), r.lhs.clone(), r.rhs.clone()))
            .collect::<Vec<_>>(),
        policy.weights,
        policy.bias,
        policy.trained_steps,
        policy.generation,
        trajectory.steps.len(),
        trajectory.reached_tensor(),
    );
    let bytes = bincode::serialize(&payload).expect("serializable");
    TermRef::from_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::RewriteRule;
    use crate::value::Value;

    /// A trivial LawExtractor that emits one fixed law per run.
    /// Used in tests to verify the cycle mechanism without
    /// depending on mathscape-compress.
    struct FixedLawExtractor {
        law: RewriteRule,
    }

    impl LawExtractor for FixedLawExtractor {
        fn extract(
            &self,
            _corpus: &[Term],
            library: &[RewriteRule],
        ) -> Vec<RewriteRule> {
            // Emit the fixed law only on the first iteration where the
            // library doesn't already contain it. Otherwise, empty.
            if library.iter().any(|r| r.name == self.law.name) {
                Vec::new()
            } else {
                vec![self.law.clone()]
            }
        }
    }

    /// A null CorpusGenerator that produces empty corpora. Used
    /// for testing that the cycle's control flow is independent
    /// of what the corpus generator produces.
    struct NullCorpusGenerator;

    impl CorpusGenerator for NullCorpusGenerator {
        fn generate(&self, _iter: usize, _lib: &[RewriteRule]) -> Vec<Term> {
            Vec::new()
        }
    }

    /// A null ModelUpdater that does nothing. Proves the cycle
    /// doesn't require training to complete.
    struct NullModelUpdater;

    impl ModelUpdater for NullModelUpdater {
        fn update(&self, _p: &mut LinearPolicy, _t: &Trajectory) {}
    }

    fn dummy_law() -> RewriteRule {
        RewriteRule {
            name: "dummy".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![Term::Number(Value::Nat(0)), Term::Var(100)],
            ),
            rhs: Term::Var(100),
        }
    }

    #[test]
    fn cycle_runs_with_null_layers() {
        // Minimal cycle: null generator + fixed extractor + null
        // updater. Verifies the mechanism works end-to-end with
        // stub implementations.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            3,
        );
        let outcome = cycle.run(Vec::new(), LinearPolicy::new());
        // First iteration adds the law; subsequent iterations don't
        // re-add (FixedLawExtractor short-circuits once present).
        assert_eq!(outcome.final_library.len(), 1);
        assert_eq!(outcome.iterations.len(), 3);
    }

    #[test]
    fn attestation_is_deterministic() {
        // Two identical cycle runs ⇒ identical attestation hash.
        // This is cycle-level deterministic_replay.
        let cycle_a = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let cycle_b = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let out_a = cycle_a.run(Vec::new(), LinearPolicy::new());
        let out_b = cycle_b.run(Vec::new(), LinearPolicy::new());
        assert_eq!(out_a.attestation, out_b.attestation);
    }

    #[test]
    fn attestation_differs_when_content_differs() {
        // Different number of iterations ⇒ different attestation.
        let cycle_short = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            1,
        );
        let cycle_long = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            3,
        );
        let out_short = cycle_short.run(Vec::new(), LinearPolicy::new());
        let out_long = cycle_long.run(Vec::new(), LinearPolicy::new());
        assert_ne!(out_short.attestation, out_long.attestation);
    }

    #[test]
    fn hijack_corpus_generator_preserves_cycle_mechanism() {
        // "Hijack" the corpus generator: swap for NullCorpusGenerator.
        // Even with empty corpora, the mechanism completes because
        // the extractor still fires (FixedLawExtractor emits on
        // first call regardless of corpus).
        // This proves the CorpusGenerator layer is properly
        // encapsulated — the cycle doesn't leak assumptions about
        // what the generator returns.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let outcome = cycle.run(Vec::new(), LinearPolicy::new());
        assert!(outcome.attestation != TermRef::from_bytes(&[0u8; 32]));
        assert_eq!(outcome.iterations.len(), 2);
    }

    #[test]
    fn outcome_bincode_roundtrip() {
        // The typescape entity must be serde-roundtrippable for
        // persistence and cross-process attestation.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let outcome = cycle.run(Vec::new(), LinearPolicy::new());
        let bytes = bincode::serialize(&outcome).unwrap();
        let back: BootstrapOutcome = bincode::deserialize(&bytes).unwrap();
        assert_eq!(outcome, back);
    }

    // ── R27 invariant tests ──────────────────────────────────────

    #[test]
    fn zero_iteration_cycle_produces_empty_outcome() {
        // Edge case: N=0. Cycle completes without error; outcome
        // has no iterations, no trajectory steps. Library + policy
        // pass through unchanged from seeds.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            0,
        );
        let seed_lib = vec![dummy_law()];
        let seed_policy = LinearPolicy::tensor_seeking_prior();
        let outcome = cycle.run(seed_lib.clone(), seed_policy.clone());
        assert!(outcome.iterations.is_empty());
        assert!(outcome.trajectory.steps.is_empty());
        assert_eq!(outcome.final_library, seed_lib);
        // Null updater means policy unchanged; with seed weights
        // preserved.
        assert_eq!(outcome.final_policy.weights, seed_policy.weights);
    }

    #[test]
    fn cycle_library_is_monotonically_non_decreasing() {
        // Across iterations, library size never shrinks. Extracted
        // laws are only appended, never removed by the cycle
        // itself.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            5,
        );
        let outcome = cycle.run(Vec::new(), LinearPolicy::new());
        for pair in outcome.iterations.windows(2) {
            let prev = pair[0].features_after.rule_count;
            let curr = pair[1].features_after.rule_count;
            assert!(
                curr >= prev,
                "library must not shrink: {prev} → {curr}"
            );
        }
    }

    #[test]
    fn trajectory_step_count_matches_iteration_count() {
        // For any N iterations, the trajectory records exactly N
        // steps. Invariant: 1 step per iteration, no more no less.
        for n in [0, 1, 3, 7] {
            let cycle = BootstrapCycle::new(
                NullCorpusGenerator,
                FixedLawExtractor { law: dummy_law() },
                NullModelUpdater,
                n,
            );
            let outcome = cycle.run(Vec::new(), LinearPolicy::new());
            assert_eq!(
                outcome.trajectory.steps.len(),
                n,
                "trajectory step count must equal iteration count N={n}"
            );
        }
    }

    #[test]
    fn seed_library_passes_through_unchanged_with_null_extractor() {
        // A cycle whose extractor returns EMPTY for every call
        // must preserve the seed library exactly.
        struct EmptyExtractor;
        impl LawExtractor for EmptyExtractor {
            fn extract(&self, _c: &[Term], _l: &[RewriteRule]) -> Vec<RewriteRule> {
                Vec::new()
            }
        }
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            EmptyExtractor,
            NullModelUpdater,
            5,
        );
        let seed = vec![dummy_law()];
        let outcome = cycle.run(seed.clone(), LinearPolicy::new());
        assert_eq!(outcome.final_library, seed);
        // Each iteration recorded `accepted=false`.
        for step in &outcome.trajectory.steps {
            assert!(!step.accepted, "empty extractor should mark !accepted");
        }
    }

    #[test]
    fn attestation_covers_policy_changes() {
        // If the policy ends up different (different updater
        // trained it), attestation must differ — even if the
        // library is identical.
        let cycle_null_updater = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let cycle_default_updater = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            DefaultModelUpdater::default(),
            2,
        );
        let a = cycle_null_updater.run(Vec::new(), LinearPolicy::new());
        let b = cycle_default_updater.run(Vec::new(), LinearPolicy::new());
        assert_eq!(a.final_library, b.final_library);
        assert_ne!(a.attestation, b.attestation);
    }

    #[test]
    fn default_corpus_generator_is_deterministic() {
        // Two calls with same iter + library → identical corpus.
        let g = DefaultCorpusGenerator;
        let lib: Vec<RewriteRule> = Vec::new();
        assert_eq!(g.generate(0, &lib), g.generate(0, &lib));
        assert_eq!(g.generate(3, &lib), g.generate(3, &lib));
    }

    #[test]
    fn default_corpus_generator_iter_0_differs_from_later() {
        // The iteration-index dispatch must actually produce
        // different corpora for different iterations (the default
        // escalates complexity at iter ≥ 1).
        let g = DefaultCorpusGenerator;
        let lib: Vec<RewriteRule> = Vec::new();
        assert_ne!(g.generate(0, &lib), g.generate(1, &lib));
    }

    #[test]
    fn layer_boundaries_are_independent() {
        // Change the updater independently of the other layers.
        // Old output (null updater) and new output (swapped
        // updater) must DIFFER in policy but agree on library.
        //
        // This demonstrates the "hijack and optimize" property:
        // swap a layer and observe only that layer's effect.
        let cycle_null = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let cycle_default = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            DefaultModelUpdater::default(),
            2,
        );
        let out_null = cycle_null.run(Vec::new(), LinearPolicy::new());
        let out_default = cycle_default.run(Vec::new(), LinearPolicy::new());

        // Libraries identical (updater doesn't touch library).
        assert_eq!(out_null.final_library, out_default.final_library);
        // Policies differ: default trained, null didn't.
        // Generation is the marker.
        assert_ne!(
            out_null.final_policy.generation, out_default.final_policy.generation
        );
    }
}
