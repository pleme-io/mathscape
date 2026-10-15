use crate::law_generator::{derive_laws_from_corpus_instrumented, derive_laws_validated};
use mathscape_core::bootstrap::{
    BootstrapCycle, BootstrapCycleSpec, BootstrapOutcome, CanonicalDeduper, DefaultCorpusGenerator,
    DefaultModelUpdater, ExperimentOutcome, ExperimentScenario, LawExtractor, PhaseOutcome,
    SpecExecutionError, execute_scenario_core,
};
use mathscape_core::eval::RewriteRule;
use mathscape_core::hash::TermRef;
use mathscape_core::meta_loop::ScenarioExecutor;
use mathscape_core::policy::LinearPolicy;
use mathscape_core::term::{SymbolId, Term};
use mathscape_core::AdaptiveCorpusGenerator;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Extractor {
    DerivedLaws {
        step_limit: usize,
        min_support: usize,
    },
    Validated {
        step_limit: usize,
        min_support: usize,
        subterm_depth: usize,
        k_samples: usize,
        seed: u64,
    },
}

impl Extractor {
    #[must_use]
    pub const fn derived_laws() -> Self {
        Self::DerivedLaws {
            step_limit: 300,
            min_support: 2,
        }
    }

    #[must_use]
    pub const fn validated(seed: u64) -> Self {
        Self::Validated {
            step_limit: 300,
            min_support: 2,
            subterm_depth: 2,
            k_samples: 8,
            seed,
        }
    }
}

impl LawExtractor for Extractor {
    fn extract(&self, corpus: &[Term], library: &[RewriteRule]) -> Vec<RewriteRule> {
        let mut next_id = SymbolId::try_from(library.len() + 1).unwrap_or(SymbolId::MAX);
        match *self {
            Self::DerivedLaws {
                step_limit,
                min_support,
            } => {
                derive_laws_from_corpus_instrumented(corpus, library, step_limit, min_support, &mut next_id).0
            }
            Self::Validated {
                step_limit,
                min_support,
                subterm_depth,
                k_samples,
                seed,
            } => {
                derive_laws_validated(
                    corpus,
                    library,
                    step_limit,
                    min_support,
                    subterm_depth,
                    k_samples,
                    seed,
                    &mut next_id,
                )
                .0
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnknownCorpus {
    #[default]
    Refuse,
    Core,
    Skip,
}

const KNOWN_CORPORA: [&str; 2] = ["default", "adaptive"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Motor {
    pub extractor: Extractor,
    pub seed: u64,
    pub unknown_corpus: UnknownCorpus,
}

impl Default for Motor {
    fn default() -> Self {
        Self {
            extractor: Extractor::derived_laws(),
            seed: 0,
            unknown_corpus: UnknownCorpus::Refuse,
        }
    }
}

impl Motor {
    #[must_use]
    pub fn seeded(seed: u64) -> Self {
        Self {
            seed,
            unknown_corpus: UnknownCorpus::Core,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn on_unknown_corpus(mut self, unknown_corpus: UnknownCorpus) -> Self {
        self.unknown_corpus = unknown_corpus;
        self
    }

    #[must_use]
    pub fn with_extractor(mut self, extractor: Extractor) -> Self {
        self.extractor = extractor;
        self
    }

    pub fn run_spec(&self, spec: &BootstrapCycleSpec) -> Result<BootstrapOutcome, SpecExecutionError> {
        let n = spec.n_iterations;
        let lib = spec.seed_library.clone();
        let pol = spec.seed_policy.clone();
        let stable = spec.early_stop_after_stable;
        match spec.corpus_generator.as_str() {
            "default" => Ok(drive(
                BootstrapCycle::new(DefaultCorpusGenerator, self.extractor, DefaultModelUpdater::default(), n),
                lib,
                pol,
                stable,
            )),
            "adaptive" => Ok(drive(
                BootstrapCycle::new(
                    AdaptiveCorpusGenerator {
                        seed: self.seed,
                        ..AdaptiveCorpusGenerator::default()
                    },
                    self.extractor,
                    DefaultModelUpdater::default(),
                    n,
                ),
                lib,
                pol,
                stable,
            )),
            other => match self.unknown_corpus {
                UnknownCorpus::Refuse | UnknownCorpus::Skip => Err(SpecExecutionError::UnknownLayer {
                    role: "corpus_generator",
                    name: other.to_string(),
                }),
                UnknownCorpus::Core => {
                    let mini = ExperimentScenario {
                        name: "fallback".into(),
                        phases: vec![spec.clone()],
                    };
                    let inner = execute_scenario_core(&mini)?;
                    inner
                        .phases
                        .into_iter()
                        .next()
                        .map(|p| p.cycle_outcome)
                        .ok_or_else(|| SpecExecutionError::UnknownLayer {
                            role: "corpus_generator",
                            name: other.to_string(),
                        })
                }
            },
        }
    }
}

fn drive<C, M>(
    cycle: BootstrapCycle<C, Extractor, M>,
    lib: Vec<RewriteRule>,
    pol: LinearPolicy,
    stable: Option<usize>,
) -> BootstrapOutcome
where
    C: mathscape_core::bootstrap::CorpusGenerator,
    M: mathscape_core::bootstrap::ModelUpdater,
{
    match stable {
        Some(w) => cycle.run_until_stable(lib, pol, &CanonicalDeduper, w),
        None => cycle.run_with_dedup(lib, pol, &CanonicalDeduper),
    }
}

impl ScenarioExecutor for Motor {
    fn execute(&self, scenario: &ExperimentScenario) -> Result<ExperimentOutcome, SpecExecutionError> {
        let mut phases = Vec::with_capacity(scenario.phases.len());
        let mut carry_lib = scenario
            .phases
            .first()
            .map(|p| p.seed_library.clone())
            .unwrap_or_default();
        let mut carry_pol = scenario
            .phases
            .first()
            .map_or_else(LinearPolicy::tensor_seeking_prior, |p| p.seed_policy.clone());
        for (idx, base) in scenario.phases.iter().enumerate() {
            let mut spec = base.clone();
            if idx > 0 {
                spec.seed_library = carry_lib.clone();
                spec.seed_policy = carry_pol.clone();
            }
            if self.unknown_corpus == UnknownCorpus::Skip && !KNOWN_CORPORA.contains(&spec.corpus_generator.as_str()) {
                continue;
            }
            let outcome = self.run_spec(&spec)?;
            carry_lib = outcome.final_library.clone();
            carry_pol = outcome.final_policy.clone();
            phases.push(PhaseOutcome {
                phase_index: idx,
                spec_used: spec,
                cycle_outcome: outcome,
            });
        }
        let concat: Vec<u8> = phases
            .iter()
            .flat_map(|p| p.cycle_outcome.attestation.as_bytes().to_vec())
            .collect();
        Ok(ExperimentOutcome {
            phases,
            chain_attestation: TermRef::from_bytes(&concat),
            scenario_total_ns: 0,
        })
    }
}
