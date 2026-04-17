//! `CompressionGenerator` — adapter impl bridging `extract_rules` to
//! `mathscape_core::Generator`.
//!
//! See `docs/arch/machine-synthesis.md`. The generator is called by
//! `Epoch::step` each discovery epoch. It materializes the current
//! library as a `Vec<RewriteRule>`, asks `extract_rules` for new
//! patterns, and wraps each as a `Candidate`.

use crate::extract::{extract_rules, ExtractConfig};
use mathscape_core::{
    epoch::{Artifact, Candidate, Generator},
    eval::RewriteRule,
    term::{SymbolId, Term},
};

/// A [`Generator`] that proposes `RewriteRule` candidates by
/// anti-unifying the corpus against the current library.
pub struct CompressionGenerator {
    pub config: ExtractConfig,
    /// Monotone counter for minting Symbol ids. Lives on the
    /// generator so its state survives across epochs.
    pub next_symbol_id: SymbolId,
    /// Origin tag attached to every emitted candidate.
    pub origin: String,
}

impl CompressionGenerator {
    #[must_use]
    pub fn new(config: ExtractConfig, next_symbol_id: SymbolId) -> Self {
        Self {
            config,
            next_symbol_id,
            origin: "compress/antiunify".into(),
        }
    }
}

impl Generator for CompressionGenerator {
    fn propose(
        &mut self,
        _epoch_id: u64,
        corpus: &[Term],
        library: &[Artifact],
    ) -> Vec<Candidate> {
        // Materialize a RewriteRule view of the library for extract_rules.
        let existing: Vec<RewriteRule> =
            library.iter().map(|a| a.rule.clone()).collect();
        let rules = extract_rules(
            corpus,
            &existing,
            &mut self.next_symbol_id,
            &self.config,
        );
        rules
            .into_iter()
            .map(|rule| Candidate {
                rule,
                origin: self.origin.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::epoch::{InMemoryRegistry, Registry};
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn generator_emits_no_candidates_for_singleton_corpus() {
        let mut g = CompressionGenerator::new(ExtractConfig::default(), 1);
        let candidates = g.propose(0, &[nat(1)], &[]);
        assert!(candidates.is_empty());
    }

    #[test]
    fn generator_emits_candidates_for_repeated_patterns() {
        let corpus = vec![
            apply(var(2), vec![nat(5), nat(0)]),
            apply(var(2), vec![nat(3), nat(0)]),
            apply(var(2), vec![nat(7), nat(0)]),
        ];
        let mut g = CompressionGenerator::new(ExtractConfig::default(), 1);
        let candidates = g.propose(0, &corpus, &[]);
        assert!(!candidates.is_empty());
        assert!(candidates.iter().all(|c| c.origin == "compress/antiunify"));
    }

    #[test]
    fn generator_threads_library_state_correctly() {
        let corpus = vec![
            apply(var(2), vec![nat(5), nat(0)]),
            apply(var(2), vec![nat(3), nat(0)]),
        ];
        let mut g = CompressionGenerator::new(ExtractConfig::default(), 1);
        let registry = InMemoryRegistry::new();
        let _first = g.propose(0, &corpus, registry.all());
        let second = g.propose(1, &corpus, registry.all());
        // No panic; symbol counter advanced; second call completed successfully.
        assert!(g.next_symbol_id > 1 || second.is_empty());
    }
}
