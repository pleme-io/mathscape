//! Reward computation: description length, compression ratio, novelty,
//! meta-compression, combined fitness scoring.

pub mod adapter;
pub mod compress_score;
pub mod novelty;
pub mod reward;

pub use adapter::StatisticalProver;
pub use reward::{RewardConfig, RewardResult, compute_reward};
