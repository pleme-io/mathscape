//! `PromotionGate` — gates 4 and 5 of the realization lattice.
//!
//! See `docs/arch/promotion-pipeline.md`. A PromotionGate is consulted
//! each epoch with (artifact, library, corpus_history). It emits a
//! `PromotionSignal` when both the condensation threshold K and the
//! cross-corpus threshold N are cleared.

use crate::{
    epoch::Artifact,
    eval::{pattern_match, RewriteRule},
    promotion::{CorpusId, PromotionSignal},
    term::Term,
};
use std::collections::BTreeSet;

/// Per-artifact history needed to evaluate gates 4 and 5.
#[derive(Debug, Clone, Default)]
pub struct ArtifactHistory {
    /// Distinct corpora in which the artifact's pattern has matched
    /// at least once.
    pub corpus_matches: BTreeSet<CorpusId>,
    /// Total number of epochs the artifact has been in the registry.
    pub epochs_alive: u64,
    /// Usage tally over the sliding window (set by the reinforcement
    /// pass).
    pub usage_in_window: u64,
}

/// A PromotionGate evaluates (artifact, library, history) and decides
/// whether to emit a `PromotionSignal`.
pub trait PromotionGate {
    fn evaluate(
        &self,
        artifact: &Artifact,
        library: &[Artifact],
        history: &ArtifactHistory,
        epoch_id: u64,
    ) -> Option<PromotionSignal>;
}

/// A simple threshold gate: emit a signal iff the artifact subsumes
/// ≥ `k_condensation` other library entries AND appears across
/// ≥ `n_cross_corpus` distinct corpora.
#[derive(Debug, Clone, Copy)]
pub struct ThresholdGate {
    pub k_condensation: usize,
    pub n_cross_corpus: usize,
}

impl ThresholdGate {
    #[must_use]
    pub fn new(k_condensation: usize, n_cross_corpus: usize) -> Self {
        Self { k_condensation, n_cross_corpus }
    }
}

impl PromotionGate for ThresholdGate {
    fn evaluate(
        &self,
        artifact: &Artifact,
        library: &[Artifact],
        history: &ArtifactHistory,
        epoch_id: u64,
    ) -> Option<PromotionSignal> {
        // Gate 4: condensation. Collect library entries whose lhs is
        // matched as a subterm of `artifact.rule.lhs` (heuristic — real
        // subsumption requires e-graph; this is the cheap
        // approximation). We also consider an entry "subsumed" if its
        // rhs appears inside artifact.rule.rhs as a structural
        // subterm — the pattern rewrites the old rule.
        let subsumed: Vec<_> = library
            .iter()
            .filter(|a| a.content_hash != artifact.content_hash)
            .filter(|a| subsumes_structurally(&artifact.rule, &a.rule))
            .map(|a| a.content_hash)
            .collect();
        if subsumed.len() < self.k_condensation {
            return None;
        }
        // Gate 5: cross-corpus.
        let corpora: Vec<CorpusId> = history.corpus_matches.iter().cloned().collect();
        if corpora.len() < self.n_cross_corpus {
            return None;
        }
        Some(PromotionSignal {
            artifact_hash: artifact.content_hash,
            subsumed_hashes: subsumed,
            cross_corpus_support: corpora,
            rationale: format!(
                "subsumes {} library entries across {} corpora (epochs_alive={})",
                history.corpus_matches.len(),
                history.corpus_matches.len(),
                history.epochs_alive
            ),
            epoch_id,
        })
    }
}

/// Heuristic structural subsumption: `candidate` subsumes `other`
/// when either:
/// 1. `other.rhs` appears as a subterm of `candidate.rhs` (the new
///    rule's expansion references the old), OR
/// 2. `candidate.lhs` matches a subterm of `other.rhs` (the new
///    rule's pattern fits inside the old's expansion, suggesting the
///    old could be rewritten using it).
///
/// This is cheap and approximate. A real promotion gate would run
/// e-graph saturation to prove subsumption. v0 uses this to enable
/// end-to-end flow; Phase F/G upgrades replace it.
fn subsumes_structurally(candidate: &RewriteRule, other: &RewriteRule) -> bool {
    contains_subterm(&candidate.rhs, &other.rhs)
        || subterm_matches_pattern(&other.rhs, &candidate.lhs)
}

fn contains_subterm(haystack: &Term, needle: &Term) -> bool {
    if haystack == needle {
        return true;
    }
    match haystack {
        Term::Fn(_, body) => contains_subterm(body, needle),
        Term::Apply(f, args) => {
            contains_subterm(f, needle) || args.iter().any(|a| contains_subterm(a, needle))
        }
        Term::Symbol(_, args) => args.iter().any(|a| contains_subterm(a, needle)),
        _ => false,
    }
}

fn subterm_matches_pattern(term: &Term, pattern: &Term) -> bool {
    if pattern_match(pattern, term).is_some() {
        return true;
    }
    match term {
        Term::Fn(_, body) => subterm_matches_pattern(body, pattern),
        Term::Apply(f, args) => {
            subterm_matches_pattern(f, pattern) || args.iter().any(|a| subterm_matches_pattern(a, pattern))
        }
        Term::Symbol(_, args) => args.iter().any(|a| subterm_matches_pattern(a, pattern)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epoch::AcceptanceCertificate;
    use crate::test_helpers::{apply, nat, var};
    use crate::term::Term;

    fn art(id: u32, rule: RewriteRule) -> Artifact {
        Artifact::seal(
            rule,
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        )
    }

    #[test]
    fn threshold_gate_rejects_insufficient_condensation() {
        let gate = ThresholdGate::new(3, 1);
        let candidate = art(
            1,
            RewriteRule {
                name: "c".into(),
                lhs: Term::Symbol(1, vec![]),
                rhs: apply(var(2), vec![nat(0)]),
            },
        );
        let hist = ArtifactHistory {
            corpus_matches: [CorpusId::from("arith")].into_iter().collect(),
            epochs_alive: 10,
            usage_in_window: 5,
        };
        // No library entries at all — subsumed count is 0.
        assert!(gate.evaluate(&candidate, &[], &hist, 11).is_none());
    }

    #[test]
    fn threshold_gate_rejects_insufficient_cross_corpus() {
        // Library has entries candidate subsumes, but only one corpus.
        let gate = ThresholdGate::new(0, 2); // k=0 so condensation auto-clears
        let candidate = art(
            1,
            RewriteRule {
                name: "c".into(),
                lhs: Term::Symbol(1, vec![]),
                rhs: Term::Number(crate::value::Value::Nat(1)),
            },
        );
        let hist = ArtifactHistory {
            corpus_matches: [CorpusId::from("arith")].into_iter().collect(),
            epochs_alive: 10,
            usage_in_window: 5,
        };
        assert!(gate.evaluate(&candidate, &[], &hist, 11).is_none());
    }

    #[test]
    fn threshold_gate_accepts_when_both_thresholds_cleared() {
        // Candidate's rhs contains N(1); other's rhs is N(1) — so
        // candidate structurally "references" other. K=1 clears.
        let gate = ThresholdGate::new(1, 2);
        let other = art(
            2,
            RewriteRule {
                name: "o".into(),
                lhs: Term::Symbol(2, vec![]),
                rhs: Term::Number(crate::value::Value::Nat(1)),
            },
        );
        let candidate = art(
            1,
            RewriteRule {
                name: "c".into(),
                lhs: Term::Symbol(1, vec![]),
                rhs: apply(var(2), vec![
                    Term::Number(crate::value::Value::Nat(1)),
                    Term::Number(crate::value::Value::Nat(2)),
                ]),
            },
        );
        let hist = ArtifactHistory {
            corpus_matches: [CorpusId::from("arith"), CorpusId::from("diff")]
                .into_iter()
                .collect(),
            epochs_alive: 10,
            usage_in_window: 5,
        };
        let signal = gate.evaluate(&candidate, &[other.clone()], &hist, 11);
        assert!(signal.is_some(), "expected signal to fire");
        let s = signal.unwrap();
        assert_eq!(s.subsumed_hashes, vec![other.content_hash]);
        assert_eq!(s.cross_corpus_support.len(), 2);
    }
}
