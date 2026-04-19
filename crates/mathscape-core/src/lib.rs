//! Core types: Point, Number, Fn, Term enum, hash-consing, evaluation,
//! substitution, s-expression parser/printer.

pub mod adaptive_corpus;
pub mod autograd;
pub mod bandit_probe;
pub mod bettyfine;
pub mod coach;
pub mod bootstrap;
pub mod certification;
pub mod builtin;
pub mod control;
pub mod corpus;
pub mod demotion;
pub mod environment;
pub mod epoch;
pub mod eval;
pub mod event;
pub mod form_tree;
pub mod hash;
pub mod inference;
pub mod lifecycle;
pub mod math_problem;
pub mod mathscape_map;
pub mod meta;
pub mod meta_loop;
pub mod model_testing;
pub mod snapshot;
pub mod streaming_policy;
pub mod task;

pub mod migration;
pub mod optimizer;
pub mod parse;
pub mod orchestrator;
pub mod plasticity;
pub mod promotion;
pub mod promotion_gate;
pub mod policy;
pub mod primitives;
pub mod reduction;
pub mod rich_corpus;
pub mod tensor;
pub mod term;
pub mod trajectory;
pub mod trap;
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;
pub mod value;

pub use control::{
    Allocator, EpochAction, Regime, RegimeDetector, RegimeWeights, RealizationPolicy,
    RewardEstimator,
};
pub use corpus::{CorpusLog, CorpusSnapshot};
pub use demotion::{demote_artifact, DemotionCandidate, DemotionGate, UsageFloorGate};
pub use environment::{
    canonical_deployment_suite, policy_distance, ConvergenceTracker, CorpusShape,
    Environment, MechanismSnapshot,
};
pub use epoch::{
    AcceptanceCertificate, Artifact, Candidate, Emitter, Epoch, EpochTrace, Generator,
    InMemoryRegistry, Prover, Registry, Rejection, RuleEmitter, Verdict,
};
pub use autograd::{
    simplify_add_of, simplify_mul_of, simplify_neg_of, symbolic_derivative,
    symbolic_derivative_float, DomainOps, FloatOps, IntOps, TensorOps,
};
pub use adaptive_corpus::AdaptiveCorpusGenerator;
pub use bandit_probe::BanditProbe;
pub use coach::{
    CoachObservation, CoachPolicy, CurriculumCoach, LearnedCoachPolicy,
    RuleBasedPolicy, TuningAction,
};
pub use bootstrap::{
    compute_attestation, deduplicate_library, execute_scenario_core,
    execute_spec_core, library_merkle_root, AlphaDeduper, BootstrapCycle,
    BootstrapCycleSpec, BootstrapOutcome, CanonicalDeduper, CorpusGenerator,
    CycleTimings, DefaultCorpusGenerator, DefaultModelUpdater,
    ExperimentOutcome, ExperimentScenario, IterationSnapshot,
    IterationTimings, LawExtractor, LearningObservation, LibraryDeduper,
    ModelUpdater, NoDedup, PhaseOutcome, SpecExecutionError,
    SubsumptionDeduper,
};
pub use certification::{
    run_certification_step, CertificationLevel, CertificationStepReport,
    CertificationVerdict, CertifiedRule, Certifier, CertifyingConsumer,
    DefaultCertifier,
};
pub use mathscape_map::{
    BufferedConsumer, EventHub, MapEvent, MapEventConsumer, MapSnapshot,
    MapSummary, MathscapeMap,
};
pub use optimizer::{sgd_step_int, sgd_step_tensor};
pub use meta_loop::{
    publish_outcome_events, AdaptiveProposer, DefaultScenarioExecutor,
    HeuristicProposer, MetaLoop, MetaLoopConfig, MetaLoopOutcome,
    MetaPhaseRecord, ScenarioExecutor, ScenarioProposer, SpecArchetype,
    TerminationReason,
};
pub use math_problem::{
    canonical_problem_set, harder_problem_set, mathematician_curriculum,
    run_benchmark, run_curriculum, solve_problem, BenchmarkConsumer,
    BenchmarkReport, CurriculumProblem, CurriculumReport, MathProblem,
    ProblemResult,
};
pub use plasticity::{
    ComponentTick, Plastic, PlasticityController, PlasticityReport,
};
pub use streaming_policy::StreamingPolicyTrainer;
pub use task::{
    as_math_tasks, run_benchmark as run_task_benchmark,
    GenericBenchmarkConsumer, LanguageContext, LanguageDomain, MathDomain,
    Task, TaskDomain, TaskReport, TaskResult,
};
pub use bettyfine::{bettyfine_library, standard_bettyfine_cardinality, OperatorSpec};
pub use event::{Event, EventCategory, StatusAdvance};
pub use form_tree::{
    CheckPeriod, DiscoveryForest, FormNode, HitCount, IrreducibilityRate, Morphism,
};
pub use hash::TermRef;
pub use inference::LiveInferenceHandle;
pub use model_testing::{
    certify_snapshot, check_invariants, compare_snapshots,
    verify_serialization_roundtrip, CertificationReport, InvariantResult,
    SnapshotDiff,
};
pub use snapshot::{
    analyze_snapshot, attach_analysis, deep_analyze, fork_from_snapshot,
    format_analysis, snapshot_handle, trainer_from_snapshot, trainer_snapshot,
    ModelAnalysis, ModelSnapshot, SnapshotError, TrainerSnapshot,
    SNAPSHOT_VERSION,
};
pub use lifecycle::{AxiomIdentity, DemotionReason, ProofStatus, TypescapeCoord};
pub use trap::{Trap, TrapDetector, TrapExitReason};
pub use migration::migrate_library;
pub use promotion::{CorpusId, MigrationReport, PromotionSignal};
pub use promotion_gate::{ArtifactHistory, PromotionGate, ThresholdGate};
pub use orchestrator::{
    run_until_reduced, LayerEpochSnapshot, LayerTrajectory, MultiLayerReport,
    MultiLayerRunner, PromotionHook, PromotionOutcome,
};
pub use rich_corpus::RichCorpusGenerator;
pub use reduction::{
    check_maximally_reduced, check_reduction, detect_subsumption_pairs,
    reduction_pressure, ReductionBarrier, ReductionPolicy, ReductionSummary,
    ReductionVerdict,
};
pub use policy::{rank_states, LinearPolicy, PolicyModel};
pub use primitives::{
    census, classify_primitives, collect_primitive_labels, primitive_label,
    IdentityForm, MlPrimitive, PrimitiveCensus,
};
pub use tensor::{classify, shape_counts, tensor_density, TensorShape};
pub use term::{StoredTerm, SymbolId, Term};
pub use trajectory::{ActionKind, LibraryFeatures, Trajectory, TrajectoryStep};
pub use value::Value;
