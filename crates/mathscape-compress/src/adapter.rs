//! `CompressionGenerator` — adapter impl bridging `extract_rules` to
//! `mathscape_core::Generator`.
//!
//! See `docs/arch/machine-synthesis.md`. The generator is called by
//! `Epoch::step` each discovery epoch. It materializes the current
//! library as a `Vec<RewriteRule>`, asks `extract_rules` for new
//! patterns, and wraps each as a `Candidate`.

use crate::extract::ExtractConfig;
use mathscape_core::{
    epoch::{Artifact, Candidate, Generator},
    eval::{pattern_equivalent, pattern_match, RewriteRule},
    term::{SymbolId, Term},
};

/// Apply library rewrite rules bottom-up to a term until fixed-point.
/// Reinforcement and status advancement are the runtime's problem;
/// this function just produces the library-reduced view the
/// generator needs to see "what's left" when proposing new patterns.
///
/// Bottom-up: rewrite children first so that when a parent's children
/// are rewritten, a new root-level rule match may fire.
/// Step-bounded to avoid pathological non-termination from a
/// mis-designed library (shouldn't happen with the lattice's
/// current semantics, but cheap insurance).
fn rewrite_fixed_point(term: &Term, library: &[RewriteRule], max_steps: usize) -> Term {
    let mut current = rewrite_children(term, library, max_steps);
    for _ in 0..max_steps {
        let next = rewrite_root_once(&current, library);
        if next == current {
            return current;
        }
        // A root rewrite may have exposed new children to rewrite.
        current = rewrite_children(&next, library, max_steps);
    }
    current
}

fn rewrite_root_once(term: &Term, library: &[RewriteRule]) -> Term {
    for rule in library {
        if let Some(bindings) = pattern_match(&rule.lhs, term) {
            let mut rhs = rule.rhs.clone();
            for (var, val) in &bindings {
                rhs = rhs.substitute(*var, val);
            }
            return rhs;
        }
    }
    term.clone()
}

fn rewrite_children(term: &Term, library: &[RewriteRule], max_steps: usize) -> Term {
    match term {
        Term::Point(_) | Term::Number(_) | Term::Var(_) => term.clone(),
        Term::Fn(params, body) => Term::Fn(
            params.clone(),
            Box::new(rewrite_fixed_point(body, library, max_steps)),
        ),
        Term::Apply(f, args) => {
            let rewritten_args: Vec<Term> = args
                .iter()
                .map(|a| rewrite_fixed_point(a, library, max_steps))
                .collect();
            let rewritten_f = rewrite_fixed_point(f, library, max_steps);
            Term::Apply(Box::new(rewritten_f), rewritten_args)
        }
        Term::Symbol(id, args) => Term::Symbol(
            *id,
            args.iter()
                .map(|a| rewrite_fixed_point(a, library, max_steps))
                .collect(),
        ),
    }
}

/// A [`Generator`] that proposes `RewriteRule` candidates by
/// anti-unifying the corpus against the current library.
#[derive(Debug, Clone)]
pub struct CompressionGenerator {
    pub config: ExtractConfig,
    /// Monotone counter for minting Symbol ids. Lives on the
    /// generator so its state survives across epochs.
    pub next_symbol_id: SymbolId,
    /// Origin tag attached to every emitted candidate.
    pub origin: String,
    /// Phase I: enable subterm-aware anti-unification. When true,
    /// `propose` uses `extract_rules_with_options(..., true)`, so
    /// candidates span subterm positions, not just roots. Unlocks
    /// patterns invisible to root-only AU. Off by default to
    /// preserve the established bettyfine.
    pub subterm_au: bool,
}

impl CompressionGenerator {
    #[must_use]
    pub fn new(config: ExtractConfig, next_symbol_id: SymbolId) -> Self {
        Self {
            config,
            next_symbol_id,
            origin: "compress/antiunify".into(),
            subterm_au: false,
        }
    }

    /// Phase I: builder that enables subterm-aware AU. Candidates
    /// include patterns from subterm positions, not just roots.
    #[must_use]
    pub fn with_subterm_au(mut self) -> Self {
        self.subterm_au = true;
        self.origin = "compress/subterm-antiunify".into();
        self
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

        // Rewrite the corpus through the current library BEFORE anti-
        // unifying. Anti-unification over the raw corpus gets stuck
        // re-deriving patterns the library already covers; working
        // on the library-reduced view is what exposes higher-order
        // structure. This is the "dimensional discovery" move from
        // docs/arch/machine-synthesis.md — each primitive peels a
        // layer of the corpus, revealing what's left to compress.
        //
        // Empty library falls through to the raw corpus (no-op).
        let reduced_corpus: Vec<Term> = if existing.is_empty() {
            corpus.to_vec()
        } else {
            corpus
                .iter()
                .map(|t| rewrite_fixed_point(t, &existing, 64))
                .collect()
        };

        let rules = crate::extract::extract_rules_with_options(
            &reduced_corpus,
            &existing,
            &mut self.next_symbol_id,
            &self.config,
            self.subterm_au,
        );
        // Generator-side dedup (inter-batch AND intra-batch): drop
        // any candidate whose lhs is pattern-equivalent to either an
        // existing library entry's lhs or another candidate already
        // accepted in this same batch.
        //
        // Inter-batch catches "I already extracted this pattern last
        // epoch." Intra-batch catches "anti-unification produced two
        // different candidate terms with equivalent lhs patterns" —
        // observed as S_001 and S_002 with identical lhs but
        // different Symbol ids in rhs. Both are identical
        // rediscovery; the reinforcement pass would clean them up
        // via mutual subsumption, but catching them here saves a
        // round-trip through the collapse machinery.
        //
        // Two lhs terms are "pattern-equivalent" iff each
        // pattern-matches the other (equivalence class under
        // rewriting). Centralized in mathscape_core::eval.
        let mut kept: Vec<RewriteRule> = Vec::new();
        for rule in rules {
            // Inter-batch: already in library?
            if existing.iter().any(|e| pattern_equivalent(&rule.lhs, &e.lhs)) {
                continue;
            }
            // Intra-batch: equivalent to a candidate already kept?
            if kept.iter().any(|k| pattern_equivalent(&rule.lhs, &k.lhs)) {
                continue;
            }
            kept.push(rule);
        }
        kept.into_iter()
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
