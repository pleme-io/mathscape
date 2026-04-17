//! Core types: Point, Number, Fn, Term enum, hash-consing, evaluation,
//! substitution, s-expression parser/printer.

pub mod control;
pub mod epoch;
pub mod eval;
pub mod event;
pub mod hash;
pub mod lifecycle;
pub mod parse;
pub mod promotion;
pub mod term;
#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;
pub mod value;

pub use control::{
    Allocator, EpochAction, Regime, RegimeWeights, RealizationPolicy, RewardEstimator,
};
pub use epoch::{
    AcceptanceCertificate, Artifact, Candidate, Emitter, Epoch, EpochTrace, Generator,
    InMemoryRegistry, Prover, Registry, Rejection, Verdict,
};
pub use event::{Event, EventCategory, StatusAdvance};
pub use hash::TermRef;
pub use lifecycle::{AxiomIdentity, DemotionReason, ProofStatus};
pub use promotion::{CorpusId, MigrationReport, PromotionSignal};
pub use term::{StoredTerm, SymbolId, Term};
pub use value::Value;
