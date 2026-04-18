//! Core types: Point, Number, Fn, Term enum, hash-consing, evaluation,
//! substitution, s-expression parser/printer.

pub mod autograd;
pub mod bettyfine;
pub mod bootstrap;
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
pub mod lifecycle;
pub mod meta;
pub mod migration;
pub mod optimizer;
pub mod parse;
pub mod orchestrator;
pub mod promotion;
pub mod promotion_gate;
pub mod policy;
pub mod primitives;
pub mod reduction;
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
pub use bootstrap::{
    compute_attestation, AlphaDeduper, BootstrapCycle, BootstrapOutcome,
    CanonicalDeduper, CorpusGenerator, DefaultCorpusGenerator,
    DefaultModelUpdater, IterationSnapshot, LawExtractor, LibraryDeduper,
    ModelUpdater, NoDedup,
};
pub use optimizer::{sgd_step_int, sgd_step_tensor};
pub use bettyfine::{bettyfine_library, standard_bettyfine_cardinality, OperatorSpec};
pub use event::{Event, EventCategory, StatusAdvance};
pub use form_tree::{
    CheckPeriod, DiscoveryForest, FormNode, HitCount, IrreducibilityRate, Morphism,
};
pub use hash::TermRef;
pub use lifecycle::{AxiomIdentity, DemotionReason, ProofStatus, TypescapeCoord};
pub use trap::{Trap, TrapDetector, TrapExitReason};
pub use migration::migrate_library;
pub use promotion::{CorpusId, MigrationReport, PromotionSignal};
pub use promotion_gate::{ArtifactHistory, PromotionGate, ThresholdGate};
pub use orchestrator::{
    run_until_reduced, LayerEpochSnapshot, LayerTrajectory, MultiLayerReport,
    MultiLayerRunner, PromotionHook, PromotionOutcome,
};
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
