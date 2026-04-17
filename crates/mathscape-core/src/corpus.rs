//! Corpus — the input evidence that drives discovery and reinforcement.
//!
//! Phase K of the realization plan. `CorpusSnapshot` names a set of
//! expressions with a stable id so gate 5 (cross-corpus N) can count
//! *distinct* corpora rather than treating every epoch's input as
//! anonymous. `CorpusLog` accumulates per-artifact match history
//! across epochs, providing the `ArtifactHistory` value the
//! PromotionGate consumes.
//!
//! CorpusLog is caller-side metadata — it does not ride inside the
//! Registry or Epoch. Orchestration layers (the CLI, the service)
//! maintain it alongside the registry.
//!
//! See `docs/arch/corpus-dynamics.md` and
//! `docs/arch/machine-synthesis.md`.

use crate::eval::pattern_match;
use crate::hash::TermRef;
use crate::promotion::CorpusId;
use crate::promotion_gate::ArtifactHistory;
use crate::term::Term;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

/// A named, content-addressed snapshot of a corpus.
///
/// Two corpora with the same `id` but different `terms` produce
/// different `content_hash`es — snapshot equality is structural.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusSnapshot {
    pub id: CorpusId,
    pub terms: Vec<Term>,
    pub content_hash: TermRef,
    pub epoch_id_first_seen: u64,
}

impl CorpusSnapshot {
    /// Named corpus with content-addressed hash.
    #[must_use]
    pub fn new(id: impl Into<CorpusId>, terms: Vec<Term>, epoch_id: u64) -> Self {
        let id = id.into();
        let bytes = bincode::serialize(&(&id, &terms))
            .expect("CorpusSnapshot::new: bincode serialization infallible");
        Self {
            id,
            terms,
            content_hash: TermRef::from_bytes(&bytes),
            epoch_id_first_seen: epoch_id,
        }
    }

    /// Anonymous corpus — use when no meaningful id is available.
    /// Maps to `id = "default"`. Gate 5 will never promote anything
    /// unless real named corpora are used, which is the intended
    /// behavior: anonymous corpus runs stay in the library forever.
    #[must_use]
    pub fn anonymous(terms: Vec<Term>) -> Self {
        Self::new("default", terms, 0)
    }

    #[must_use]
    pub fn terms(&self) -> &[Term] {
        &self.terms
    }
}

/// Accumulated per-artifact match history. Caller-side metadata.
///
/// Record matches as they happen (after acceptance, during
/// reinforcement scans). Use `history_for(hash)` to extract the
/// `ArtifactHistory` the `PromotionGate` consumes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CorpusLog {
    pub corpus_matches: HashMap<TermRef, BTreeSet<CorpusId>>,
    pub first_seen_epoch: HashMap<TermRef, u64>,
    pub usage_by_epoch: HashMap<TermRef, Vec<u64>>,
}

impl CorpusLog {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that an artifact's pattern matched at least one term in
    /// the given corpus during the given epoch.
    pub fn record_match(
        &mut self,
        artifact_hash: TermRef,
        corpus_id: &CorpusId,
        epoch_id: u64,
    ) {
        self.corpus_matches
            .entry(artifact_hash)
            .or_default()
            .insert(corpus_id.clone());
        self.first_seen_epoch
            .entry(artifact_hash)
            .or_insert(epoch_id);
        self.usage_by_epoch
            .entry(artifact_hash)
            .or_default()
            .push(epoch_id);
    }

    /// Scan a corpus and record a match for every artifact whose lhs
    /// pattern matches any term in the corpus.
    pub fn scan_corpus(
        &mut self,
        corpus: &CorpusSnapshot,
        artifacts: impl IntoIterator<Item = (TermRef, Term)>,
        epoch_id: u64,
    ) {
        for (artifact_hash, lhs) in artifacts {
            if corpus
                .terms
                .iter()
                .any(|t| pattern_match(&lhs, t).is_some())
            {
                self.record_match(artifact_hash, &corpus.id, epoch_id);
            }
        }
    }

    /// Build the `ArtifactHistory` the PromotionGate consumes.
    #[must_use]
    pub fn history_for(
        &self,
        artifact_hash: TermRef,
        current_epoch: u64,
        usage_window: u64,
    ) -> ArtifactHistory {
        let corpus_matches = self
            .corpus_matches
            .get(&artifact_hash)
            .cloned()
            .unwrap_or_default();
        let epochs_alive = self
            .first_seen_epoch
            .get(&artifact_hash)
            .map(|first| current_epoch.saturating_sub(*first))
            .unwrap_or(0);
        let usage_in_window = self
            .usage_by_epoch
            .get(&artifact_hash)
            .map(|epochs| {
                let floor = current_epoch.saturating_sub(usage_window);
                epochs.iter().filter(|e| **e >= floor).count() as u64
            })
            .unwrap_or(0);
        ArtifactHistory {
            corpus_matches,
            epochs_alive,
            usage_in_window,
        }
    }

    /// Number of distinct corpora an artifact has matched across.
    #[must_use]
    pub fn cross_corpus_count(&self, artifact_hash: TermRef) -> usize {
        self.corpus_matches
            .get(&artifact_hash)
            .map_or(0, |s| s.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{apply, nat, var};

    fn h(b: u8) -> TermRef {
        TermRef([b; 32])
    }

    #[test]
    fn snapshot_content_hash_is_deterministic() {
        let a = CorpusSnapshot::new("arith", vec![nat(1), nat(2)], 0);
        let b = CorpusSnapshot::new("arith", vec![nat(1), nat(2)], 0);
        assert_eq!(a.content_hash, b.content_hash);
    }

    #[test]
    fn snapshots_differ_on_id_or_terms() {
        let a = CorpusSnapshot::new("arith", vec![nat(1)], 0);
        let b = CorpusSnapshot::new("calculus", vec![nat(1)], 0);
        assert_ne!(a.content_hash, b.content_hash);
        let c = CorpusSnapshot::new("arith", vec![nat(2)], 0);
        assert_ne!(a.content_hash, c.content_hash);
    }

    #[test]
    fn record_match_accumulates_distinct_corpora() {
        let mut log = CorpusLog::new();
        log.record_match(h(1), &"arith".into(), 0);
        log.record_match(h(1), &"diff".into(), 1);
        log.record_match(h(1), &"arith".into(), 2);
        assert_eq!(log.cross_corpus_count(h(1)), 2);
    }

    #[test]
    fn history_for_reports_epochs_alive() {
        let mut log = CorpusLog::new();
        log.record_match(h(1), &"arith".into(), 5);
        let history = log.history_for(h(1), 15, 100);
        assert_eq!(history.epochs_alive, 10);
        assert_eq!(history.corpus_matches.len(), 1);
    }

    #[test]
    fn history_usage_window_filters_old_hits() {
        let mut log = CorpusLog::new();
        log.record_match(h(1), &"c".into(), 0);
        log.record_match(h(1), &"c".into(), 5);
        log.record_match(h(1), &"c".into(), 50);
        log.record_match(h(1), &"c".into(), 95);
        // window=30 at epoch 100 → only count matches at epoch ≥ 70
        let history = log.history_for(h(1), 100, 30);
        assert_eq!(history.usage_in_window, 1);
    }

    #[test]
    fn scan_corpus_records_matches_for_patterned_artifacts() {
        let mut log = CorpusLog::new();
        let corpus = CorpusSnapshot::new(
            "arith",
            vec![apply(var(2), vec![nat(1), nat(0)]), apply(var(2), vec![nat(5), nat(0)])],
            0,
        );
        // Pattern: add(?x, 0)
        let pattern = apply(var(2), vec![var(100), nat(0)]);
        log.scan_corpus(&corpus, vec![(h(1), pattern)], 0);
        assert_eq!(log.cross_corpus_count(h(1)), 1);
    }

    #[test]
    fn scan_corpus_skips_non_matching_artifacts() {
        let mut log = CorpusLog::new();
        let corpus = CorpusSnapshot::new("arith", vec![nat(42)], 0);
        // Pattern that does not match anything in the corpus.
        let pattern = apply(var(3), vec![var(100)]);
        log.scan_corpus(&corpus, vec![(h(1), pattern)], 0);
        assert_eq!(log.cross_corpus_count(h(1)), 0);
    }

    #[test]
    fn history_for_returns_defaults_for_unknown_artifact() {
        let log = CorpusLog::new();
        let history = log.history_for(h(9), 100, 30);
        assert!(history.corpus_matches.is_empty());
        assert_eq!(history.epochs_alive, 0);
        assert_eq!(history.usage_in_window, 0);
    }
}
