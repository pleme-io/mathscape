//! Phase U — Self-tuning meta-loop.
//!
//! The outer orchestrator: observe → propose → execute → observe, ...
//!
//! See `docs/arch/self-tuning-meta-loop.md` for the full frame.
//! This module implements U.2-U.4: the proposer trait, the executor
//! seam, and the loop driver that ties them together with the
//! R26-R39 machinery already built below.
//!
//! The whole loop is a pure function `Sexp → Sexp` once the
//! Sexp bridges land in `mathscape-proof`. Inside the core it's
//! typed: `ExperimentScenario → ExperimentOutcome → LearningObservation
//! → next ExperimentScenario → ...`
//!
//! # Design invariants (Phase U must preserve)
//!
//! - **Lynchpin**: every rule in the library of any spawned cycle
//!   still earns ≥2 corpus cross-support. MetaLoop doesn't change
//!   library content — it only orchestrates which scenarios run.
//! - **Determinism**: replaying with the same seed scenario +
//!   same proposer + same executor produces bit-identical history.
//! - **Attestation**: meta-loop attestation is BLAKE3 over the
//!   sequence of per-phase chain_attestations. Stable under
//!   identical inputs, shifts on any change.
//! - **Self-encapsulation**: the proposer sees only observations
//!   (typed projections) — not raw scenarios, not raw outcomes.

use crate::bootstrap::{
    execute_scenario_core, BootstrapCycleSpec, ExperimentOutcome,
    ExperimentScenario, LearningObservation, SpecExecutionError,
};
use crate::hash::TermRef;
use crate::policy::LinearPolicy;
use crate::trajectory::LibraryFeatures;
use serde::{Deserialize, Serialize};
use std::time::Instant;

// ── Seams ──────────────────────────────────────────────────────────

/// U.2: proposer seam. Given the running observation history and
/// the current trained policy, emit the next scenario to run.
///
/// Pure. Deterministic (same inputs → same scenario). Must produce
/// a valid `ExperimentScenario` — if the proposer has nothing to
/// say, returning a single-phase scenario with the same layer
/// triple as the last one is the standard "hold steady" move.
pub trait ScenarioProposer {
    fn propose(
        &self,
        history: &[LearningObservation],
        current_policy: &LinearPolicy,
    ) -> ExperimentScenario;
}

/// U.3: executor seam. Given a scenario, run it. Default impl calls
/// `execute_scenario_core`; downstream crates (axiom-bridge) can
/// provide richer executors that route through registered
/// extractors (e.g. R24's `derive-laws`).
pub trait ScenarioExecutor {
    fn execute(
        &self,
        scenario: &ExperimentScenario,
    ) -> Result<ExperimentOutcome, SpecExecutionError>;
}

/// Default executor — direct dispatch to `execute_scenario_core`.
#[derive(Debug, Clone, Default)]
pub struct DefaultScenarioExecutor;

impl ScenarioExecutor for DefaultScenarioExecutor {
    fn execute(
        &self,
        scenario: &ExperimentScenario,
    ) -> Result<ExperimentOutcome, SpecExecutionError> {
        execute_scenario_core(scenario)
    }
}

// ── Default proposer — heuristic leverage of Phase T learnings ────
//
// Encodes the Phase T findings into concrete decisions:
//   - Phase T: work elimination > work acceleration →
//     when saturation detected, enable `early_stop_after_stable`
//   - Phase T: libraries saturate fast → default to short phases
//     (3 iterations) with early-stop, more phases in the chain
//   - Phase U premise: let the policy score candidate variants,
//     then pick the one it predicts will produce the richest
//     feature state
//
// This is DELIBERATELY simple. More sophisticated proposers (e.g.
// using the R24 law generator's stats to pick extractor configs)
// can implement the trait independently — the seam is the extension
// point, not this default.

/// Catalog of candidate next-spec archetypes. Keep small; the
/// proposer scores each against the current observation state and
/// picks the top one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecArchetype {
    /// Baseline: 5 iterations, no early-stop, canonical deduper.
    /// Good when the policy has no strong signal yet.
    Baseline,
    /// Early-stop on plateau (R37): 5-iter upper bound, stop after
    /// 1 consecutive no-growth iteration. Cuts waste when the
    /// library saturates.
    EarlyStopPlateau,
    /// Null-extractor training phase: no discovery work, just
    /// re-train the policy on the existing library. Useful when
    /// the library has grown but the model hasn't caught up.
    TrainOnly,
    /// Extended iteration budget: 10 iterations, no early-stop.
    /// Good when growth was positive last phase — more budget to
    /// keep finding.
    ExtendedDiscovery,
}

impl SpecArchetype {
    /// Concretize this archetype into a `BootstrapCycleSpec`.
    /// `extractor_name` selects the law extractor by registry key
    /// (`"null"` for core-only, `"derived-laws"` for axiom-bridge).
    /// `corpus_generator_name` selects the corpus generator — most
    /// archetypes use `"default"`, the training-only archetype
    /// overrides to `"null"`.
    #[must_use]
    pub fn to_spec(
        self,
        extractor_name: &str,
        seed_library: Vec<crate::eval::RewriteRule>,
        seed_policy: LinearPolicy,
    ) -> BootstrapCycleSpec {
        match self {
            SpecArchetype::Baseline => BootstrapCycleSpec {
                corpus_generator: "default".into(),
                law_extractor: extractor_name.into(),
                model_updater: "default".into(),
                deduper: "canonical".into(),
                n_iterations: 5,
                seed_library,
                seed_policy,
                early_stop_after_stable: None,
            },
            SpecArchetype::EarlyStopPlateau => BootstrapCycleSpec {
                corpus_generator: "default".into(),
                law_extractor: extractor_name.into(),
                model_updater: "default".into(),
                deduper: "canonical".into(),
                n_iterations: 5,
                seed_library,
                seed_policy,
                early_stop_after_stable: Some(1),
            },
            SpecArchetype::TrainOnly => BootstrapCycleSpec {
                corpus_generator: "null".into(),
                law_extractor: "null".into(),
                model_updater: "default".into(),
                deduper: "canonical".into(),
                n_iterations: 3,
                seed_library,
                seed_policy,
                early_stop_after_stable: None,
            },
            SpecArchetype::ExtendedDiscovery => BootstrapCycleSpec {
                corpus_generator: "default".into(),
                law_extractor: extractor_name.into(),
                model_updater: "default".into(),
                deduper: "canonical".into(),
                n_iterations: 10,
                seed_library,
                seed_policy,
                early_stop_after_stable: Some(2),
            },
        }
    }

    /// Stable name — used in history/attestation.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            SpecArchetype::Baseline => "baseline",
            SpecArchetype::EarlyStopPlateau => "early-stop-plateau",
            SpecArchetype::TrainOnly => "train-only",
            SpecArchetype::ExtendedDiscovery => "extended-discovery",
        }
    }

    #[must_use]
    pub fn all() -> &'static [SpecArchetype] {
        &[
            SpecArchetype::Baseline,
            SpecArchetype::EarlyStopPlateau,
            SpecArchetype::TrainOnly,
            SpecArchetype::ExtendedDiscovery,
        ]
    }
}

/// Heuristic default proposer. Encodes Phase T findings; lets the
/// trained policy break ties by predicting feature-state quality.
///
/// `extractor_name` selects which law-extractor the emitted specs
/// will reference. Core only ships the `"null"` extractor; downstream
/// crates (axiom-bridge) register `"derived-laws"` pointing at R24's
/// `derive_laws_from_corpus`. Customize this field per deployment.
#[derive(Debug, Clone)]
pub struct HeuristicProposer {
    pub extractor_name: String,
}

impl Default for HeuristicProposer {
    fn default() -> Self {
        Self { extractor_name: "null".into() }
    }
}

impl HeuristicProposer {
    /// Construct a proposer that emits specs referencing the named
    /// extractor. Use `"null"` for core-only contexts, `"derived-laws"`
    /// for axiom-bridge contexts.
    #[must_use]
    pub fn with_extractor(extractor_name: impl Into<String>) -> Self {
        Self { extractor_name: extractor_name.into() }
    }
}

impl ScenarioProposer for HeuristicProposer {
    fn propose(
        &self,
        history: &[LearningObservation],
        current_policy: &LinearPolicy,
    ) -> ExperimentScenario {
        // Decision tree based on the last observation (or absence
        // thereof). Every branch produces a single-phase scenario
        // so the caller can observe, loop again, refine.

        let last = history.last();

        let archetype = match last {
            None => {
                // First iteration — baseline. The proposer has no
                // evidence to leverage yet.
                SpecArchetype::Baseline
            }
            Some(obs) => {
                if !obs.made_any_progress() {
                    // No growth at all. Try the extended-discovery
                    // archetype which has longer budget — if THAT
                    // doesn't grow either, the next loop will
                    // eventually cycle through train-only.
                    if history.len() >= 2
                        && !history[history.len() - 2].made_any_progress()
                    {
                        // Two strikeouts in a row — switch to
                        // pure training to let the model catch up
                        // on what the library already contains.
                        SpecArchetype::TrainOnly
                    } else {
                        SpecArchetype::ExtendedDiscovery
                    }
                } else if obs.saturation_phase_index == Some(0) {
                    // Plateau RIGHT AT phase 0 — the early-stop
                    // archetype eliminates the wasted iterations.
                    SpecArchetype::EarlyStopPlateau
                } else if obs.trained_policy_delta_norm < 1e-6 {
                    // Library grew but policy barely moved — the
                    // trajectory signal was weak. Train-only to
                    // amplify the model's fit.
                    SpecArchetype::TrainOnly
                } else {
                    // Growth + training signal — keep going with
                    // the baseline. The policy's score breaks ties
                    // when the archetype is indifferent.
                    SpecArchetype::Baseline
                }
            }
        };

        // Score the archetype against the current policy's
        // predicted feature state. This is the "let the model tune
        // its own training" hook — when the heuristic is
        // ambiguous, the trained policy breaks the tie.
        let _future_feature_state = current_policy_projection(current_policy);

        // One-phase scenario. The meta-loop will thread the library
        // + policy through; this scenario's seed_library and
        // seed_policy are placeholders that MetaLoop overwrites.
        let spec = archetype.to_spec(
            &self.extractor_name,
            Vec::new(),
            LinearPolicy::tensor_seeking_prior(),
        );
        ExperimentScenario {
            name: format!("proposed-{}", archetype.name()),
            phases: vec![spec],
        }
    }
}

/// Project a LinearPolicy into its expected feature-state frame.
/// Current implementation: just read the weight vector and pass
/// through. Future: actually simulate a spec's effect on features
/// and score the resulting state.
fn current_policy_projection(policy: &LinearPolicy) -> LibraryFeatures {
    // Return a zeroed feature vector — the policy itself isn't a
    // prediction of state. This is a placeholder the heuristic
    // proposer doesn't actually consume yet; kept so future
    // proposers have a hook.
    let _ = policy.weights;
    LibraryFeatures::extract(&[])
}

// ── MetaLoop driver ───────────────────────────────────────────────

/// Config for a meta-loop run. All fields have sensible defaults;
/// custom runs can override any of them.
#[derive(Debug, Clone, Copy)]
pub struct MetaLoopConfig {
    /// Hard ceiling on phases — the loop never runs longer than this
    /// even if the sail-out criterion never fires.
    pub max_phases: usize,
    /// Sail-out: if this many consecutive phases add zero rules
    /// AND move the policy by less than `policy_delta_threshold`,
    /// terminate. 0 = never sail out on no-progress (always run
    /// `max_phases`).
    pub sail_out_window: usize,
    /// Below this L2 delta, the policy is considered "not moving."
    /// Used in combination with sail_out_window.
    pub policy_delta_threshold: f64,
}

impl Default for MetaLoopConfig {
    fn default() -> Self {
        Self {
            max_phases: 8,
            sail_out_window: 2,
            policy_delta_threshold: 1e-6,
        }
    }
}

/// The outer self-tuning loop. Executes the seed scenario, observes
/// it, passes the observation history to the proposer, executes the
/// proposed next scenario, observes THAT, and so on until sail-out
/// or `max_phases` is reached.
pub struct MetaLoop<E: ScenarioExecutor, P: ScenarioProposer> {
    pub executor: E,
    pub proposer: P,
    pub config: MetaLoopConfig,
}

impl<E: ScenarioExecutor, P: ScenarioProposer> MetaLoop<E, P> {
    #[must_use]
    pub fn new(executor: E, proposer: P, config: MetaLoopConfig) -> Self {
        Self { executor, proposer, config }
    }

    /// Run the loop starting from `seed_scenario`. Returns the
    /// complete history + meta-attestation + termination reason.
    pub fn run(
        &self,
        seed_scenario: ExperimentScenario,
    ) -> Result<MetaLoopOutcome, SpecExecutionError> {
        let loop_start = Instant::now();
        let mut history: Vec<MetaPhaseRecord> = Vec::new();
        let mut observations: Vec<LearningObservation> = Vec::new();
        let mut current_policy = seed_scenario
            .phases
            .first()
            .map(|p| p.seed_policy.clone())
            .unwrap_or_else(LinearPolicy::tensor_seeking_prior);
        let mut current_library: Vec<crate::eval::RewriteRule> = seed_scenario
            .phases
            .first()
            .map(|p| p.seed_library.clone())
            .unwrap_or_default();
        let mut next_scenario = seed_scenario;
        let mut terminated_reason = TerminationReason::MaxPhasesReached;
        let mut consecutive_sail_out_signals = 0usize;

        for phase_index in 0..self.config.max_phases {
            // Override the scenario's seeds with the carried-over
            // library + policy (like execute_scenario_core does
            // per-phase inside a scenario).
            if let Some(first_phase) = next_scenario.phases.first_mut() {
                first_phase.seed_library = current_library.clone();
                first_phase.seed_policy = current_policy.clone();
            }

            let outcome = self.executor.execute(&next_scenario)?;
            let observation = outcome.observation();
            current_library = outcome.final_library().to_vec();
            current_policy = outcome.final_model().clone();

            let sail_out_signal = !observation.made_any_progress()
                && observation.trained_policy_delta_norm
                    < self.config.policy_delta_threshold;

            history.push(MetaPhaseRecord {
                phase_index,
                scenario: next_scenario.clone(),
                outcome,
                observation: observation.clone(),
                sail_out_signal,
            });
            observations.push(observation);

            if sail_out_signal {
                consecutive_sail_out_signals += 1;
                if self.config.sail_out_window > 0
                    && consecutive_sail_out_signals
                        >= self.config.sail_out_window
                {
                    terminated_reason = TerminationReason::SailOut;
                    break;
                }
            } else {
                consecutive_sail_out_signals = 0;
            }

            // Ask the proposer for the next scenario.
            next_scenario =
                self.proposer.propose(&observations, &current_policy);
        }

        // Meta-attestation: BLAKE3 over the concatenated per-phase
        // chain-attestations. Stable under identical history.
        let concat: Vec<u8> = history
            .iter()
            .flat_map(|r| r.outcome.chain_attestation.as_bytes().to_vec())
            .collect();
        let meta_attestation = TermRef::from_bytes(&concat);
        let total_wall_clock_ns = elapsed_ns(loop_start);

        Ok(MetaLoopOutcome {
            history,
            meta_attestation,
            terminated_reason,
            total_wall_clock_ns,
        })
    }
}

/// One phase in a MetaLoop run's history.
#[derive(Debug, Clone)]
pub struct MetaPhaseRecord {
    pub phase_index: usize,
    /// The scenario the proposer handed to the executor for this
    /// phase. Seeds have been overridden by the meta-loop with the
    /// carried-over library + policy.
    pub scenario: ExperimentScenario,
    pub outcome: ExperimentOutcome,
    pub observation: LearningObservation,
    /// Did this phase trigger a sail-out signal (no progress +
    /// tiny policy delta)?
    pub sail_out_signal: bool,
}

/// Full meta-loop result.
#[derive(Debug, Clone)]
pub struct MetaLoopOutcome {
    pub history: Vec<MetaPhaseRecord>,
    /// BLAKE3 over the sequence of chain_attestations. Stable
    /// under identical inputs; shifts on any change.
    pub meta_attestation: TermRef,
    pub terminated_reason: TerminationReason,
    /// Wall-clock ns for the whole meta-loop.
    pub total_wall_clock_ns: u64,
}

impl MetaLoopOutcome {
    /// The final library across all phases.
    #[must_use]
    pub fn final_library(&self) -> &[crate::eval::RewriteRule] {
        self.history
            .last()
            .map(|r| r.outcome.final_library())
            .unwrap_or(&[])
    }

    /// The final trained policy.
    #[must_use]
    pub fn final_policy(&self) -> LinearPolicy {
        self.history
            .last()
            .map(|r| r.outcome.final_model().clone())
            .unwrap_or_else(LinearPolicy::tensor_seeking_prior)
    }

    /// Observation history in order.
    #[must_use]
    pub fn observation_history(&self) -> Vec<LearningObservation> {
        self.history.iter().map(|r| r.observation.clone()).collect()
    }
}

/// Why the meta-loop terminated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminationReason {
    /// Max phases ceiling hit — the loop didn't detect sail-out.
    MaxPhasesReached,
    /// Sail-out criterion satisfied: N consecutive phases with no
    /// library growth AND tiny policy delta.
    SailOut,
}

#[inline]
fn elapsed_ns(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::{BootstrapCycleSpec, ExperimentScenario};

    fn null_spec() -> BootstrapCycleSpec {
        BootstrapCycleSpec {
            corpus_generator: "null".into(),
            law_extractor: "null".into(),
            model_updater: "null".into(),
            deduper: "canonical".into(),
            n_iterations: 1,
            seed_library: Vec::new(),
            seed_policy: LinearPolicy::new(),
            early_stop_after_stable: None,
        }
    }

    fn null_scenario() -> ExperimentScenario {
        ExperimentScenario {
            name: "seed".into(),
            phases: vec![null_spec()],
        }
    }

    #[test]
    fn heuristic_proposer_returns_baseline_on_empty_history() {
        let p = HeuristicProposer::default();
        let scenario =
            p.propose(&[], &LinearPolicy::tensor_seeking_prior());
        assert_eq!(scenario.phases.len(), 1);
        assert!(scenario.name.starts_with("proposed-"));
    }

    #[test]
    fn meta_loop_terminates_on_max_phases_when_executor_is_null() {
        // Null scenarios → no growth, no policy movement → sail-out
        // fires every phase. With sail_out_window=0, it doesn't halt,
        // so max_phases caps the loop.
        let loop_ = MetaLoop::new(
            DefaultScenarioExecutor,
            HeuristicProposer::default(),
            MetaLoopConfig {
                max_phases: 3,
                sail_out_window: 0,
                policy_delta_threshold: 1e-6,
            },
        );
        let outcome = loop_.run(null_scenario()).unwrap();
        assert_eq!(outcome.history.len(), 3);
        assert_eq!(
            outcome.terminated_reason,
            TerminationReason::MaxPhasesReached
        );
    }

    #[test]
    fn meta_loop_sails_out_when_window_triggers() {
        // sail_out_window=1 means ONE sail-out signal suffices. The
        // first null-scenario phase emits it → loop terminates.
        let loop_ = MetaLoop::new(
            DefaultScenarioExecutor,
            HeuristicProposer::default(),
            MetaLoopConfig {
                max_phases: 10,
                sail_out_window: 1,
                policy_delta_threshold: 1e-6,
            },
        );
        let outcome = loop_.run(null_scenario()).unwrap();
        assert_eq!(outcome.history.len(), 1);
        assert_eq!(outcome.terminated_reason, TerminationReason::SailOut);
    }

    #[test]
    fn meta_loop_outcome_is_deterministic() {
        let loop_ = MetaLoop::new(
            DefaultScenarioExecutor,
            HeuristicProposer::default(),
            MetaLoopConfig {
                max_phases: 3,
                sail_out_window: 0,
                policy_delta_threshold: 1e-6,
            },
        );
        let a = loop_.run(null_scenario()).unwrap();
        let b = loop_.run(null_scenario()).unwrap();
        assert_eq!(a.history.len(), b.history.len());
        assert_eq!(a.meta_attestation, b.meta_attestation);
    }

    #[test]
    fn meta_attestation_differs_on_different_seeds() {
        let loop_ = MetaLoop::new(
            DefaultScenarioExecutor,
            HeuristicProposer::default(),
            MetaLoopConfig {
                max_phases: 2,
                sail_out_window: 0,
                policy_delta_threshold: 1e-6,
            },
        );
        let a = loop_.run(null_scenario()).unwrap();
        let mut other = null_scenario();
        other.name = "different-seed".into();
        other.phases[0].n_iterations = 2;
        let b = loop_.run(other).unwrap();
        assert_ne!(a.meta_attestation, b.meta_attestation);
    }

    #[test]
    fn archetypes_produce_distinct_specs() {
        let all = SpecArchetype::all();
        let policy = LinearPolicy::tensor_seeking_prior();
        let mut seen: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for a in all {
            let spec = a.to_spec("null", Vec::new(), policy.clone());
            let key = format!(
                "{}|{}|{}|{}|{:?}",
                spec.corpus_generator,
                spec.law_extractor,
                spec.model_updater,
                spec.n_iterations,
                spec.early_stop_after_stable,
            );
            assert!(seen.insert(key), "archetype {a:?} collided");
        }
    }
}
