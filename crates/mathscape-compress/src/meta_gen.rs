//! `MetaPatternGenerator` — dimensional discovery.
//!
//! After the machine mints a handful of concrete primitives, a
//! higher-order structure often lives in the *library itself*: the
//! LHSs of accepted rules share abstract shape that crosses concrete
//! operators. `add(?x, 0) => ?x` and `mul(?x, 1) => ?x` share the
//! pattern `?op(?x, ?id)` — the "identity-element" abstraction that
//! unifies additive and multiplicative identity into a single law.
//!
//! This generator proposes meta-patterns: anti-unify the LHSs (and
//! RHSs) of the existing library and emit new candidates whose LHS
//! pattern is more general than any single concrete rule. These are
//! the "dimensional discovery" proposals from
//! `docs/arch/machine-synthesis.md` — compressions of compressions.
//!
//! The ordinary `CompressionGenerator` runs over the corpus;
//! `MetaPatternGenerator` runs over the library. Composing the two
//! (see `CompositeGenerator`) gives the machine both regimes in one
//! epoch: concrete discovery from the corpus, meta-discovery from
//! the library. Whichever produces better ΔDL wins via the prover.

use crate::extract::{extract_rules, ExtractConfig};
use mathscape_core::{
    epoch::{Artifact, Candidate, Generator},
    eval::{pattern_equivalent, RewriteRule},
    term::{SymbolId, Term},
};

/// A [`Generator`] that proposes meta-rules by anti-unifying the
/// LHSs of the current library.
#[derive(Debug, Clone)]
pub struct MetaPatternGenerator {
    pub config: ExtractConfig,
    pub next_symbol_id: SymbolId,
    pub origin: String,
    /// Library-signature cache: if the library composition is
    /// identical to the last call, return the cached candidates
    /// instead of re-running anti-unification over every pair.
    /// Observed in practice: after the first discovery burst, the
    /// runner re-enters propose() with the same library for
    /// several epochs before reinforcement subsumes anything —
    /// every re-entry redoes O(N²) anti-unification only to have
    /// dedup throw it away. The cache cuts that to a hash check.
    last_lib_signature: Option<u64>,
    last_candidates: Vec<Candidate>,
    /// Observability: how often the cache fired. Reset on `new()`.
    pub cache_hits: u64,
    pub cache_misses: u64,
}

impl MetaPatternGenerator {
    #[must_use]
    pub fn new(config: ExtractConfig, next_symbol_id: SymbolId) -> Self {
        Self {
            config,
            next_symbol_id,
            origin: "compress/meta-antiunify".into(),
            last_lib_signature: None,
            last_candidates: Vec::new(),
            cache_hits: 0,
            cache_misses: 0,
        }
    }
}

/// Hash the library composition in a way that ignores ordering
/// of equivalent rules but captures every structural distinction
/// that could affect meta-extraction output.
fn library_signature(library: &[Artifact]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    // Collect content hashes, sort, hash the sequence. Order-
    // independent by construction.
    let mut hashes: Vec<[u8; 32]> = library.iter().map(|a| a.content_hash.0).collect();
    hashes.sort();
    let mut h = DefaultHasher::new();
    for bytes in &hashes {
        bytes.hash(&mut h);
    }
    h.finish()
}

impl Generator for MetaPatternGenerator {
    fn propose(
        &mut self,
        _epoch_id: u64,
        _corpus: &[Term],
        library: &[Artifact],
    ) -> Vec<Candidate> {
        if library.len() < 2 {
            // Meta-discovery requires at least two library entries to
            // anti-unify between.
            return vec![];
        }

        // Fast path: library unchanged since last call.
        let sig = library_signature(library);
        if self.last_lib_signature == Some(sig) {
            self.cache_hits += 1;
            return self.last_candidates.clone();
        }
        self.cache_misses += 1;

        // The "corpus" for meta-extraction is the library's LHSs. If
        // the library holds 5 rules, we anti-unify across pairs of
        // those LHSs and propose patterns that generalize them.
        let meta_corpus: Vec<Term> = library.iter().map(|a| a.rule.lhs.clone()).collect();
        let existing: Vec<RewriteRule> =
            library.iter().map(|a| a.rule.clone()).collect();

        let rules = extract_rules(
            &meta_corpus,
            &existing,
            &mut self.next_symbol_id,
            &self.config,
        );
        if std::env::var("MATHSCAPE_META_TRACE").is_ok() {
            eprintln!(
                "[meta] lib={} meta_corpus={} extract_rules returned {} raw candidates",
                library.len(),
                meta_corpus.len(),
                rules.len()
            );
            for r in &rules {
                eprintln!("[meta]   raw: {} :: {} => {}", r.name, r.lhs, r.rhs);
            }
        }

        // Dedup: drop candidates that are pattern-equivalent to
        // existing rules (same inter-batch/intra-batch logic as
        // the regular generator). Meta-patterns might generalize
        // an existing rule by exactly zero bits — no progress.
        let mut kept: Vec<RewriteRule> = Vec::new();
        for rule in rules {
            if existing.iter().any(|e| pattern_equivalent(&rule.lhs, &e.lhs)) {
                continue;
            }
            if kept.iter().any(|k| pattern_equivalent(&rule.lhs, &k.lhs)) {
                continue;
            }
            kept.push(rule);
        }

        if std::env::var("MATHSCAPE_META_TRACE").is_ok() {
            eprintln!("[meta]   kept after dedup: {}", kept.len());
            for r in &kept {
                eprintln!("[meta]   kept: {} :: {} => {}", r.name, r.lhs, r.rhs);
            }
        }
        let candidates: Vec<Candidate> = kept
            .into_iter()
            .map(|rule| Candidate {
                rule,
                origin: self.origin.clone(),
            })
            .collect();
        self.last_lib_signature = Some(sig);
        self.last_candidates = candidates.clone();
        candidates
    }
}

/// A composite generator: runs a primary generator AND a meta
/// generator every `propose` call, concatenating their candidates.
/// Both share symbol-id space (the meta generator's `next_symbol_id`
/// advances as it mints, and the next call to the primary picks up
/// where meta left off — not ideal but avoids id collisions).
///
/// Typed as a trait-object wrapper so concrete generators don't need
/// to know about meta-extraction. The composition is purely
/// additive: disable meta by setting `meta.config.max_new_rules = 0`
/// or by not constructing the composite at all.
#[derive(Debug, Clone)]
pub struct CompositeGenerator<G: Generator + Clone> {
    pub base: G,
    pub meta: MetaPatternGenerator,
}

impl<G: Generator + Clone> CompositeGenerator<G> {
    pub fn new(base: G, meta: MetaPatternGenerator) -> Self {
        Self { base, meta }
    }
}

impl<G: Generator + Clone> Generator for CompositeGenerator<G> {
    fn propose(
        &mut self,
        epoch_id: u64,
        corpus: &[Term],
        library: &[Artifact],
    ) -> Vec<Candidate> {
        if std::env::var("MATHSCAPE_META_TRACE").is_ok() {
            eprintln!(
                "[composite] propose epoch={} lib={}",
                epoch_id,
                library.len()
            );
        }
        let mut out = self.base.propose(epoch_id, corpus, library);
        let meta_candidates = self.meta.propose(epoch_id, corpus, library);
        // Dedup meta candidates against base candidates.
        for mc in meta_candidates {
            if out.iter().any(|b| pattern_equivalent(&mc.rule.lhs, &b.rule.lhs)) {
                continue;
            }
            out.push(mc);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::{
        epoch::{AcceptanceCertificate, Artifact, InMemoryRegistry, Registry},
        test_helpers::{apply, nat, var},
    };

    fn lib_artifact(name: &str, lhs: Term, rhs: Term) -> Artifact {
        Artifact::seal(
            RewriteRule {
                name: name.into(),
                lhs,
                rhs,
            },
            0,
            AcceptanceCertificate::trivial_conjecture(1.0),
            vec![],
        )
    }

    #[test]
    fn meta_emits_nothing_for_single_rule_library() {
        let library = vec![lib_artifact(
            "add-identity",
            apply(var(2), vec![var(100), nat(0)]),
            var(100),
        )];
        let mut g = MetaPatternGenerator::new(
            ExtractConfig {
                min_shared_size: 1,
                min_matches: 2,
                max_new_rules: 3,
            },
            100,
        );
        let candidates = g.propose(0, &[], &library);
        assert!(candidates.is_empty());
    }

    #[test]
    fn meta_discovers_identity_element_from_add_and_mul() {
        // The canonical dimensional-discovery scenario: the library
        // holds add-identity and mul-identity; the meta-generator
        // should propose something like `?op(?x, ?id) => ?x` — the
        // identity-element abstraction.
        let library = vec![
            lib_artifact(
                "add-identity",
                apply(var(2), vec![var(100), nat(0)]),
                var(100),
            ),
            lib_artifact(
                "mul-identity",
                apply(var(3), vec![var(100), nat(1)]),
                var(100),
            ),
        ];
        let mut g = MetaPatternGenerator::new(
            ExtractConfig {
                min_shared_size: 1,
                min_matches: 2,
                max_new_rules: 3,
            },
            200,
        );
        let candidates = g.propose(0, &[], &library);
        assert!(
            !candidates.is_empty(),
            "meta-generator should find the identity-element abstraction"
        );
        // The meta pattern's LHS should have the operator as a
        // variable (not a concrete var(2) or var(3)), and the
        // identity-value position should also be a variable.
        let meta = &candidates[0].rule;
        if let Term::Apply(f, args) = &meta.lhs {
            // Function position is a variable (not a specific atom).
            assert!(
                matches!(**f, Term::Var(_)),
                "meta-lhs function position should be a variable (operator-variable)"
            );
            assert_eq!(args.len(), 2, "binary op structure preserved");
            // At least one argument position should also be a
            // variable (the identity value differs between 0 and 1).
            assert!(args.iter().any(|a| matches!(a, Term::Var(_))));
        } else {
            panic!("meta-lhs must be Apply; got {:?}", meta.lhs);
        }
    }

    #[test]
    fn composite_includes_both_base_and_meta_candidates() {
        use crate::CompressionGenerator;

        // Build a registry where base generator sees corpus and meta
        // generator sees library.
        let corpus = vec![
            apply(var(2), vec![nat(5), nat(0)]),
            apply(var(2), vec![nat(3), nat(0)]),
        ];
        let library = vec![
            lib_artifact(
                "add-identity",
                apply(var(2), vec![var(100), nat(0)]),
                var(100),
            ),
            lib_artifact(
                "mul-identity",
                apply(var(3), vec![var(100), nat(1)]),
                var(100),
            ),
        ];
        let base = CompressionGenerator::new(
            ExtractConfig {
                min_shared_size: 1,
                min_matches: 2,
                max_new_rules: 3,
            },
            300,
        );
        let meta = MetaPatternGenerator::new(
            ExtractConfig {
                min_shared_size: 1,
                min_matches: 2,
                max_new_rules: 3,
            },
            400,
        );
        let mut composite = CompositeGenerator::new(base, meta);
        let candidates = composite.propose(0, &corpus, &library);
        // At minimum the meta generator should produce a candidate
        // (base gen may also produce; if corpus is already covered
        // by the library via library-reduction it will produce none).
        let meta_origin_count = candidates
            .iter()
            .filter(|c| c.origin == "compress/meta-antiunify")
            .count();
        assert!(
            meta_origin_count > 0,
            "composite must emit at least one meta-origin candidate; got candidates = {}",
            candidates.len(),
        );
    }

    #[test]
    fn meta_caches_on_unchanged_library() {
        let library = vec![
            lib_artifact(
                "add-identity",
                apply(var(2), vec![var(100), nat(0)]),
                var(100),
            ),
            lib_artifact(
                "mul-identity",
                apply(var(3), vec![var(100), nat(1)]),
                var(100),
            ),
        ];
        let mut g = MetaPatternGenerator::new(
            ExtractConfig {
                min_shared_size: 1,
                min_matches: 2,
                max_new_rules: 5,
            },
            600,
        );
        let first = g.propose(0, &[], &library);
        assert_eq!(g.cache_hits, 0);
        assert_eq!(g.cache_misses, 1);

        // Re-call with the same library: must hit cache, must
        // return identical candidates.
        let second = g.propose(1, &[], &library);
        assert_eq!(g.cache_hits, 1);
        assert_eq!(g.cache_misses, 1);
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.rule.name, b.rule.name);
            assert_eq!(a.rule.lhs, b.rule.lhs);
        }

        // Change the library: cache must invalidate.
        let mut library2 = library.clone();
        library2.push(lib_artifact(
            "square",
            apply(var(4), vec![var(100), var(100)]),
            var(100),
        ));
        let _third = g.propose(2, &[], &library2);
        assert_eq!(g.cache_hits, 1);
        assert_eq!(g.cache_misses, 2);

        // And a re-call with library2 hits cache again.
        let _fourth = g.propose(3, &[], &library2);
        assert_eq!(g.cache_hits, 2);
        assert_eq!(g.cache_misses, 2);
    }

    #[test]
    fn meta_cache_is_order_independent() {
        // Signature hashes content_hash set, not position order —
        // library with rules in different order should still hit
        // cache.
        let a = lib_artifact(
            "add-identity",
            apply(var(2), vec![var(100), nat(0)]),
            var(100),
        );
        let b = lib_artifact(
            "mul-identity",
            apply(var(3), vec![var(100), nat(1)]),
            var(100),
        );
        let mut g = MetaPatternGenerator::new(
            ExtractConfig {
                min_shared_size: 1,
                min_matches: 2,
                max_new_rules: 5,
            },
            700,
        );
        let _ = g.propose(0, &[], &[a.clone(), b.clone()]);
        assert_eq!(g.cache_misses, 1);
        let _ = g.propose(1, &[], &[b, a]);
        // Should have hit the cache since the set of content
        // hashes is identical.
        assert_eq!(
            g.cache_hits, 1,
            "cache must be order-independent (sorted content hashes)"
        );
    }

    #[test]
    fn meta_respects_existing_rule_dedup() {
        // If the library already contains the meta pattern, the
        // generator must not re-propose it.
        let library = vec![
            lib_artifact(
                "add-identity",
                apply(var(2), vec![var(100), nat(0)]),
                var(100),
            ),
            lib_artifact(
                "mul-identity",
                apply(var(3), vec![var(100), nat(1)]),
                var(100),
            ),
            // The meta pattern is ALREADY there as a seeded rule.
            lib_artifact(
                "pre-seeded-meta",
                apply(var(200), vec![var(100), var(201)]),
                var(100),
            ),
        ];
        let mut g = MetaPatternGenerator::new(
            ExtractConfig {
                min_shared_size: 1,
                min_matches: 2,
                max_new_rules: 3,
            },
            500,
        );
        let candidates = g.propose(0, &[], &library);
        // Every candidate's lhs must be non-equivalent to the seeded
        // meta rule (which is pre-seeded above at var(200), var(201)).
        // If any candidate matches the seeded lhs structure exactly,
        // dedup failed.
        let seeded = apply(var(200), vec![var(100), var(201)]);
        for c in &candidates {
            assert!(
                !pattern_equivalent(&c.rule.lhs, &seeded),
                "meta-generator re-proposed pattern already in library: {:?}",
                c.rule.lhs
            );
        }
        // Registry churn: keep the test deterministic.
        let _reg = InMemoryRegistry::new();
    }
}
