//! Reward computation: description length, compression ratio, novelty,
//! meta-compression, combined fitness scoring.
//!
//! Phase ML1 exposes a parallel `lisp_reward` module — the reward
//! formula as a homoiconic Sexp form evaluable against axis bindings.
//! The Rust `compute_reward` computes the axes; the Lisp layer
//! combines them. Future apparatus-mutation phases mutate the form
//! itself, not just the weight scalars.

pub mod adapter;
pub mod compress_score;
pub mod lisp_reward;
pub mod novelty;
pub mod reward;

pub use adapter::StatisticalProver;
pub use lisp_reward::{
    bindings_from_axes, evaluate_reward_sexp, parse_reward, CANONICAL_REWARD_SRC,
};
pub use reward::{RewardConfig, RewardResult, compute_reward};
