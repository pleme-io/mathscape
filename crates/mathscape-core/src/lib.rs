//! Core types: Point, Number, Fn, Term enum, hash-consing, evaluation,
//! substitution, s-expression parser/printer.

pub mod control;
pub mod corpus;
pub mod demotion;
pub mod epoch;
pub mod eval;
pub mod event;
pub mod hash;
pub mod lifecycle;
pub mod migration;
pub mod parse;
pub mod promotion;
pub mod promotion_gate;
pub mod reduction;
pub mod term;
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
pub use epoch::{
    AcceptanceCertificate, Artifact, Candidate, Emitter, Epoch, EpochTrace, Generator,
    InMemoryRegistry, Prover, Registry, Rejection, RuleEmitter, Verdict,
};
pub use event::{Event, EventCategory, StatusAdvance};
pub use hash::TermRef;
pub use lifecycle::{AxiomIdentity, DemotionReason, ProofStatus, TypescapeCoord};
pub use trap::{Trap, TrapDetector, TrapExitReason};
pub use migration::migrate_library;
pub use promotion::{CorpusId, MigrationReport, PromotionSignal};
pub use promotion_gate::{ArtifactHistory, PromotionGate, ThresholdGate};
pub use reduction::{
    check_maximally_reduced, check_reduction, detect_subsumption_pairs,
    reduction_pressure, ReductionBarrier, ReductionPolicy, ReductionSummary,
    ReductionVerdict,
};
pub use term::{StoredTerm, SymbolId, Term};
pub use value::Value;
