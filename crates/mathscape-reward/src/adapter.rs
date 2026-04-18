//! `StatisticalProver` — adapter impl bridging `compute_reward` to
//! `mathscape_core::Prover`.
//!
//! See `docs/arch/condensation-reward.md` and
//! `docs/arch/machine-synthesis.md`. v0 is a purely-statistical
//! prover: scores each candidate via `compute_reward` against the
//! current library and the proposed rule in isolation, then accepts
//! if the resulting composite score clears `policy.epsilon_compression`.
//!
//! Later phases add:
//!   - coverage_delta computation (requires match counting against
//!     pre/post snapshots)
//!   - condensation_ratio computation (requires library-shrinkage
//!     simulation)
//!   - equivalence verification (e-graph, via mathscape-proof)

use crate::lisp_reward::{bindings_from_axes, evaluate_reward_sexp};
use crate::reward::{compute_reward, RewardConfig};
use mathscape_core::{
    epoch::{
        AcceptanceCertificate, Artifact, Candidate, Prover, Rejection, Verdict,
    },
    eval::RewriteRule,
    lifecycle::ProofStatus,
    term::Term,
};
use tatara_lisp::ast::Sexp;

/// A [`Prover`] that scores each candidate via `compute_reward` and
/// accepts if the resulting composite score clears `min_score`.
#[derive(Debug, Clone)]
pub struct StatisticalProver {
    pub reward_config: RewardConfig,
    /// Minimum `reward` value for acceptance. Maps to
    /// `RealizationPolicy::epsilon_compression`.
    pub min_score: f64,
    /// Phase ML1: optional Lisp form for reward combination. When
    /// set, axes (cr, novelty, meta-compression, lhs-subsumption)
    /// are computed in Rust exactly as before, but their COMBINATION
    /// into a scalar score runs through the Lisp evaluator instead
    /// of the hardcoded `alpha*cr + beta*nov + gamma*meta +
    /// delta*sub` arithmetic. None = bit-identical to pre-ML1
    /// behavior. Set via `.with_reward_form(sexp)`.
    pub reward_form: Option<Sexp>,
}

impl StatisticalProver {
    #[must_use]
    pub fn new(reward_config: RewardConfig, min_score: f64) -> Self {
        Self {
            reward_config,
            min_score,
            reward_form: None,
        }
    }

    /// Phase ML1: builder that installs a Lisp combination rule.
    /// The form must be an arithmetic expression over the symbols
    /// `alpha`, `beta`, `gamma`, `delta`, `cr`, `novelty`,
    /// `meta-compression`, `lhs-subsumption` (see
    /// `lisp_reward::CANONICAL_REWARD_SRC` for the reference shape).
    #[must_use]
    pub fn with_reward_form(mut self, form: Sexp) -> Self {
        self.reward_form = Some(form);
        self
    }
}

impl Prover for StatisticalProver {
    fn prove(
        &self,
        candidate: &Candidate,
        corpus: &[Term],
        library: &[Artifact],
    ) -> Verdict {
        // Materialize rule view of the library.
        let existing: Vec<RewriteRule> =
            library.iter().map(|a| a.rule.clone()).collect();
        // Full library = existing + this candidate.
        let mut full_library = existing.clone();
        full_library.push(candidate.rule.clone());
        let new_rules = vec![candidate.rule.clone()];

        // Marginal reward: score against the baseline of `existing` alone
        // and subtract. Without this subtraction, once S_001 compresses
        // the corpus the absolute CR carries S_001's contribution into
        // every subsequent candidate's score — making the first rule a
        // gatekeeper that crowds out orthogonal patterns from other
        // families. By measuring ΔCR instead, each candidate is judged
        // only by what it adds, matching the ΔDL currency from
        // docs/arch/reward-calculus.md.
        let with_cand =
            compute_reward(corpus, &full_library, &new_rules, &self.reward_config);
        let baseline =
            compute_reward(corpus, &existing, &[], &self.reward_config);
        let marginal_cr = with_cand.compression_ratio - baseline.compression_ratio;
        let marginal_meta = with_cand.meta_compression - baseline.meta_compression;

        // Phase ML1: combine axes via Lisp form when one is
        // installed, else via the legacy Rust formula. Axes
        // themselves are computed above in Rust regardless.
        // Evaluator errors fall back to a score that forces
        // rejection — a malformed apparatus form must never smuggle
        // a candidate through.
        let marginal_reward = match &self.reward_form {
            Some(form) => {
                let bindings = bindings_from_axes(
                    self.reward_config.alpha,
                    self.reward_config.beta,
                    self.reward_config.gamma,
                    self.reward_config.delta,
                    marginal_cr,
                    with_cand.novelty_total,
                    marginal_meta,
                    with_cand.lhs_subsumption_count,
                );
                evaluate_reward_sexp(form, &bindings).unwrap_or(f64::MIN)
            }
            None => with_cand.reward - baseline.reward,
        };

        let result = crate::reward::RewardResult {
            reward: marginal_reward,
            compression_ratio: marginal_cr,
            meta_compression: marginal_meta,
            // Novelty, DL, and subsumption fields are already marginal-
            // by-construction in compute_reward (new_rules scopes each
            // to the candidate only).
            novelty_total: with_cand.novelty_total,
            description_length: with_cand.description_length,
            raw_length: with_cand.raw_length,
            lhs_subsumption_count: with_cand.lhs_subsumption_count,
        };

        if result.reward >= self.min_score {
            Verdict::Accept(AcceptanceCertificate {
                score: result.reward,
                compression_ratio: result.compression_ratio,
                condensation_ratio: 0.0, // gates 4 handle library shrinkage
                coverage_delta: 0,       // v1 addition — see MDL doc
                novelty: result.novelty_total,
                meta_compression: result.meta_compression,
                delta_dl: result.reward,
                status: ProofStatus::Conjectured,
                equivalence_hash: None,
            })
        } else {
            Verdict::Reject(vec![Rejection {
                reason: "score below epsilon_compression".into(),
                threshold: self.min_score,
                actual: result.reward,
            }])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::{
        epoch::Candidate,
        eval::RewriteRule,
        test_helpers::{apply, nat, var},
    };

    fn id_rule() -> RewriteRule {
        RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        }
    }

    #[test]
    fn accepts_useful_rule() {
        let corpus = vec![
            apply(var(2), vec![nat(5), nat(0)]),
            apply(var(2), vec![nat(3), nat(0)]),
            apply(var(2), vec![nat(7), nat(0)]),
        ];
        let prover = StatisticalProver::new(RewardConfig::default(), 0.0);
        let verdict = prover.prove(
            &Candidate {
                rule: id_rule(),
                origin: "t".into(),
            },
            &corpus,
            &[],
        );
        assert!(matches!(verdict, Verdict::Accept(_)));
    }

    #[test]
    fn rejects_below_threshold() {
        let corpus = vec![nat(1), nat(2)];
        let prover = StatisticalProver::new(RewardConfig::default(), 1000.0);
        let verdict = prover.prove(
            &Candidate {
                rule: id_rule(),
                origin: "t".into(),
            },
            &corpus,
            &[],
        );
        assert!(matches!(verdict, Verdict::Reject(_)));
    }

    #[test]
    fn accept_cert_reports_reward_axes() {
        let corpus = vec![
            apply(var(2), vec![nat(5), nat(0)]),
            apply(var(2), vec![nat(3), nat(0)]),
        ];
        let prover = StatisticalProver::new(RewardConfig::default(), 0.0);
        let Verdict::Accept(cert) = prover.prove(
            &Candidate {
                rule: id_rule(),
                origin: "t".into(),
            },
            &corpus,
            &[],
        ) else {
            panic!("expected accept");
        };
        assert!(cert.compression_ratio >= 0.0);
        assert!(cert.novelty >= 0.0);
        assert_eq!(cert.delta_dl, cert.score);
        assert_eq!(cert.status, ProofStatus::Conjectured);
    }

    #[test]
    fn rejection_carries_threshold_and_actual() {
        let corpus = vec![nat(1)];
        let prover = StatisticalProver::new(RewardConfig::default(), 999.0);
        let Verdict::Reject(rej) = prover.prove(
            &Candidate {
                rule: id_rule(),
                origin: "t".into(),
            },
            &corpus,
            &[],
        ) else {
            panic!("expected reject");
        };
        let r = &rej[0];
        assert_eq!(r.threshold, 999.0);
        assert!(r.actual < r.threshold);
    }

    #[test]
    fn orthogonal_rule_accepts_despite_existing_library() {
        // Regression for the machine-named blocker: once S_001
        // (add-identity) is in the library, a second orthogonal rule
        // over `mul` should still score positively on marginal ΔCR,
        // not be crowded out by the absolute CR already claimed by
        // S_001.
        use mathscape_core::epoch::{AcceptanceCertificate, Artifact};
        let corpus = vec![
            // add-family (already covered by S_001 below)
            apply(var(2), vec![nat(5), nat(0)]),
            apply(var(2), vec![nat(3), nat(0)]),
            // mul-family (to be covered by the new candidate)
            apply(var(3), vec![nat(7), nat(1)]),
            apply(var(3), vec![nat(4), nat(1)]),
            apply(var(3), vec![nat(9), nat(1)]),
        ];
        let existing = Artifact::seal(
            RewriteRule {
                name: "S_001".into(),
                lhs: apply(var(2), vec![var(100), nat(0)]),
                rhs: var(100),
            },
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        );
        let mul_id = RewriteRule {
            name: "S_002".into(),
            lhs: apply(var(3), vec![var(100), nat(1)]),
            rhs: var(100),
        };
        let prover = StatisticalProver::new(RewardConfig::default(), 0.0);
        let verdict = prover.prove(
            &Candidate { rule: mul_id, origin: "t".into() },
            &corpus,
            &[existing],
        );
        assert!(
            matches!(verdict, Verdict::Accept(_)),
            "orthogonal mul rule must accept on marginal ΔCR even when S_001 sits in the library: {verdict:?}"
        );
    }

    #[test]
    fn empty_corpus_yields_reject_with_default_threshold() {
        let prover = StatisticalProver::new(RewardConfig::default(), 0.1);
        let verdict = prover.prove(
            &Candidate {
                rule: id_rule(),
                origin: "t".into(),
            },
            &[],
            &[],
        );
        assert!(matches!(verdict, Verdict::Reject(_)));
    }
}
