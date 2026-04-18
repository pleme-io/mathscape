//! Combined reward function: alpha * CR + beta * novelty + gamma * meta_compression
//! + delta * lhs_subsumption.

use crate::compress_score;
use crate::novelty;
use mathscape_core::eval::{subsumes, RewriteRule};
use mathscape_core::term::Term;

/// Configuration for the reward function weights.
#[derive(Clone, Debug)]
pub struct RewardConfig {
    /// Weight for compression ratio (exploitation).
    pub alpha: f64,
    /// Weight for novelty (exploration).
    pub beta: f64,
    /// Weight for meta-compression (library reduces itself).
    pub gamma: f64,
    /// Weight for library-LHS subsumption: each existing library
    /// rule whose LHS the candidate subsumes earns `delta` bits.
    /// This is the signal that drives dimensional discovery — a
    /// meta-rule that generalizes multiple concrete rules gets
    /// scored for the library shortening it enables, even when its
    /// marginal ΔCR over the corpus is zero.
    pub delta: f64,
}

impl Default for RewardConfig {
    fn default() -> Self {
        RewardConfig {
            alpha: 0.6,
            beta: 0.3,
            gamma: 0.1,
            delta: 0.5,
        }
    }
}

/// Result of reward computation for one epoch.
#[derive(Clone, Debug)]
pub struct RewardResult {
    /// Overall reward score.
    pub reward: f64,
    /// Compression ratio.
    pub compression_ratio: f64,
    /// Description length.
    pub description_length: usize,
    /// Raw description length (without library).
    pub raw_length: usize,
    /// Total novelty from new symbols.
    pub novelty_total: f64,
    /// Meta-compression score (compression of the library RHSs).
    pub meta_compression: f64,
    /// Count of existing library rules whose LHS the new rules
    /// subsume. Multiplied by `delta` in the composite reward.
    /// This captures dimensional-discovery value: a meta-rule that
    /// generalizes many concrete rules gets rewarded even if its
    /// marginal ΔCR on the corpus is zero.
    pub lhs_subsumption_count: f64,
}

/// Compute the reward for an epoch.
///
/// - `corpus`: the population's expression trees
/// - `full_library`: all library rules including new ones
/// - `new_rules`: rules discovered this epoch
/// - `config`: weight configuration
pub fn compute_reward(
    corpus: &[Term],
    full_library: &[RewriteRule],
    new_rules: &[RewriteRule],
    config: &RewardConfig,
) -> RewardResult {
    let cr = compress_score::compression_ratio(corpus, full_library);
    let dl = compress_score::description_length(corpus, full_library);
    let raw = compress_score::description_length(corpus, &[]);

    // Sum novelty of new rules against the existing library (excluding the new rules)
    let existing: Vec<_> = full_library
        .iter()
        .filter(|r| !new_rules.iter().any(|nr| nr.name == r.name))
        .cloned()
        .collect();

    let novelty_total: f64 = new_rules
        .iter()
        .map(|r| novelty::novelty_score(r, corpus, &existing))
        .sum();

    // Meta-compression: how compressible is the library itself?
    // Treat library RHS expressions as a mini-corpus and compute CR.
    //
    // Clamped to [0, ∞): meta_compression is a reward for genuine
    // library shrinkage, never a penalty. With small libraries the
    // full-library cost (`|L|`) dwarfs the tiny rhs-as-corpus, which
    // drives raw CR strongly negative — that's an accounting
    // artifact, not a real regression. The canonical definition in
    // docs/arch/machine-synthesis.md (`1 - |L_new|/|L_expanded|`) is
    // bounded below by 0; we enforce the same floor here so this
    // term doesn't crowd out accepted ΔCR + novelty for orthogonal
    // candidates arriving when the library already holds one rule.
    let lib_corpus: Vec<Term> = full_library.iter().map(|r| r.rhs.clone()).collect();
    let meta_compression = if lib_corpus.len() > 1 {
        compress_score::compression_ratio(&lib_corpus, full_library).max(0.0)
    } else {
        0.0
    };

    // LHS subsumption: how many existing library rules does each
    // new rule subsume? Sum across new rules. The reinforcement pass
    // will collapse each subsumed rule, so this is a direct measure
    // of library shortening.
    let lhs_subsumption_count: f64 = new_rules
        .iter()
        .map(|nr| {
            existing
                .iter()
                .filter(|e| subsumes(&nr.lhs, &e.lhs))
                .count() as f64
        })
        .sum();

    let reward = config.alpha * cr
        + config.beta * novelty_total
        + config.gamma * meta_compression
        + config.delta * lhs_subsumption_count;

    RewardResult {
        reward,
        compression_ratio: cr,
        description_length: dl,
        raw_length: raw,
        novelty_total,
        meta_compression,
        lhs_subsumption_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn reward_with_useful_library() {
        let corpus = vec![
            apply(var(2), vec![nat(5), nat(0)]),
            apply(var(2), vec![nat(3), nat(0)]),
            apply(var(2), vec![nat(7), nat(0)]),
        ];

        let rule = RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };

        let config = RewardConfig::default();
        let result = compute_reward(&corpus, &[rule.clone()], &[rule], &config);

        assert!(result.compression_ratio > 0.0);
        assert!(result.novelty_total > 0.0);
        assert!(result.reward > 0.0);
        assert!(result.description_length < result.raw_length);
    }

    #[test]
    fn empty_corpus_produces_reward_zero() {
        let rule = RewriteRule {
            name: "r".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        let config = RewardConfig::default();
        let result = compute_reward(&[], &[rule.clone()], &[rule], &config);
        // Empty corpus: CR = 0.0, novelty generality = 0.0 (empty corpus),
        // meta_compression = 0.0 (only 1 lib entry), so reward = 0.0
        assert_eq!(result.reward, 0.0);
        assert_eq!(result.compression_ratio, 0.0);
    }

    #[test]
    fn custom_weight_config_produces_different_reward() {
        let corpus = vec![
            apply(var(2), vec![nat(5), nat(0)]),
            apply(var(2), vec![nat(3), nat(0)]),
        ];
        let rule = RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };

        let default_config = RewardConfig::default();
        let custom_config = RewardConfig {
            alpha: 0.1,
            beta: 0.8,
            gamma: 0.1,
        };

        let r_default = compute_reward(&corpus, &[rule.clone()], &[rule.clone()], &default_config);
        let r_custom = compute_reward(&corpus, &[rule.clone()], &[rule], &custom_config);

        // Same underlying scores but different weights => different reward
        assert!(
            (r_default.reward - r_custom.reward).abs() > 1e-9,
            "different weights should produce different rewards"
        );
        // Underlying metrics should be the same
        assert_eq!(r_default.compression_ratio, r_custom.compression_ratio);
        assert_eq!(r_default.novelty_total, r_custom.novelty_total);
    }

    #[test]
    fn no_new_rules_novelty_is_zero() {
        let corpus = vec![
            apply(var(2), vec![nat(1), nat(2)]),
            apply(var(2), vec![nat(3), nat(4)]),
        ];
        let rule = RewriteRule {
            name: "some-existing".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        let config = RewardConfig::default();
        // full_library has the rule, but new_rules is empty
        let result = compute_reward(&corpus, &[rule], &[], &config);
        assert_eq!(result.novelty_total, 0.0);
    }

    #[test]
    fn large_library_no_matching_rules_cr_not_positive() {
        // Corpus terms don't match any library rule patterns
        let corpus = vec![
            apply(var(2), vec![nat(1), nat(2)]),  // add(1, 2) — no rule matches
            apply(var(2), vec![nat(3), nat(4)]),   // add(3, 4) — no rule matches
        ];
        // Rules that use var(3) (mul) — won't match var(2) (add) corpus
        let rules: Vec<RewriteRule> = (0..10)
            .map(|i| RewriteRule {
                name: format!("mul-rule-{i}"),
                lhs: apply(var(3), vec![var(100), nat(i)]),
                rhs: var(100),
            })
            .collect();

        let cr = compress_score::compression_ratio(&corpus, &rules);
        // Library cost is added but corpus doesn't shrink, so CR should be negative
        assert!(
            cr <= 0.0,
            "CR should be zero or negative when library adds cost but doesn't compress: {cr}"
        );
    }
}
