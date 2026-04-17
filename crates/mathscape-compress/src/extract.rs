//! Library extraction: discover repeated patterns and create rewrite rules.

use crate::antiunify::{anti_unify, AntiUnifyResult};
use mathscape_core::eval::RewriteRule;
use mathscape_core::term::{SymbolId, Term};

/// Configuration for library extraction.
pub struct ExtractConfig {
    /// Minimum shared structure size for a pattern to be worth extracting.
    pub min_shared_size: usize,
    /// Minimum number of corpus members the pattern must match.
    pub min_matches: usize,
    /// Maximum number of new rules to extract per epoch.
    pub max_new_rules: usize,
}

impl Default for ExtractConfig {
    fn default() -> Self {
        ExtractConfig {
            min_shared_size: 3,
            min_matches: 2,
            max_new_rules: 5,
        }
    }
}

/// Extract new rewrite rules from a corpus by pairwise anti-unification.
///
/// Samples pairs of expressions, anti-unifies them, and promotes
/// patterns that appear frequently into named rules.
pub fn extract_rules(
    corpus: &[Term],
    existing_library: &[RewriteRule],
    next_symbol_id: &mut SymbolId,
    config: &ExtractConfig,
) -> Vec<RewriteRule> {
    if corpus.len() < 2 {
        return vec![];
    }

    // Collect candidate patterns via pairwise anti-unification
    let mut candidates: Vec<(AntiUnifyResult, Term, Term)> = Vec::new();

    // Sample pairs (don't do O(n^2) on large corpora)
    let max_pairs = 100.min(corpus.len() * (corpus.len() - 1) / 2);
    let mut pair_count = 0;

    'outer: for i in 0..corpus.len() {
        for j in (i + 1)..corpus.len() {
            if pair_count >= max_pairs {
                break 'outer;
            }

            let result = anti_unify(&corpus[i], &corpus[j]);

            if result.shared_size >= config.min_shared_size && result.var_count > 0 {
                candidates.push((result, corpus[i].clone(), corpus[j].clone()));
            }
            pair_count += 1;
        }
    }

    // Score candidates by shared_size * match_count
    // and filter by minimum matches
    let mut rules: Vec<RewriteRule> = Vec::new();

    // Sort by shared size descending (prefer larger patterns).
    candidates.sort_by(|a, b| b.0.shared_size.cmp(&a.0.shared_size));

    // Dedup by pattern-equivalence BEFORE applying max_new_rules.
    // Anti-unifying multiple pairs of a single-pattern family (e.g.,
    // all add-add pairs) produces many candidates with the same
    // pattern. Without dedup here, duplicate-pattern candidates fill
    // up the max_new_rules budget and crowd out patterns from other
    // families (observed: mixed corpus yielding only add patterns
    // because all 5 top slots were add-duplicates).
    //
    // Two patterns are equivalent iff each pattern-matches the
    // other.
    let mut unique_candidates: Vec<_> = Vec::new();
    for cand in candidates {
        let already_seen = unique_candidates.iter().any(|u: &(AntiUnifyResult, Term, Term)| {
            mathscape_core::eval::pattern_match(&u.0.pattern, &cand.0.pattern).is_some()
                && mathscape_core::eval::pattern_match(&cand.0.pattern, &u.0.pattern).is_some()
        });
        if !already_seen {
            unique_candidates.push(cand);
        }
    }

    for (result, _t1, _t2) in unique_candidates.iter().take(config.max_new_rules) {
        // Count how many corpus members this pattern matches
        let match_count = corpus
            .iter()
            .filter(|e| {
                mathscape_core::eval::pattern_match(&result.pattern, e).is_some()
            })
            .count();

        if match_count < config.min_matches {
            continue;
        }

        // Check this pattern isn't already in the library
        let already_exists = existing_library.iter().any(|r| {
            r.lhs == result.pattern
        });
        if already_exists {
            continue;
        }

        let id = *next_symbol_id;
        *next_symbol_id += 1;

        // The rule: pattern => Symbol(id, captured_vars)
        let vars: Vec<Term> = collect_pattern_vars(&result.pattern);
        let rhs = if vars.is_empty() {
            Term::Symbol(id, vec![])
        } else {
            Term::Symbol(id, vars)
        };

        rules.push(RewriteRule {
            name: format!("S_{id:03}"),
            lhs: result.pattern.clone(),
            rhs,
        });
    }

    rules
}

/// Collect all Var terms from a pattern (in order of appearance).
fn collect_pattern_vars(term: &Term) -> Vec<Term> {
    let mut vars = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_vars_inner(term, &mut vars, &mut seen);
    vars
}

fn collect_vars_inner(
    term: &Term,
    vars: &mut Vec<Term>,
    seen: &mut std::collections::HashSet<u32>,
) {
    match term {
        Term::Var(v) => {
            if seen.insert(*v) {
                vars.push(Term::Var(*v));
            }
        }
        Term::Apply(f, args) => {
            collect_vars_inner(f, vars, seen);
            for a in args {
                collect_vars_inner(a, vars, seen);
            }
        }
        Term::Fn(_, body) => {
            collect_vars_inner(body, vars, seen);
        }
        Term::Symbol(_, args) => {
            for a in args {
                collect_vars_inner(a, vars, seen);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn extract_from_repeated_pattern() {
        // Corpus with many add(x, 0) patterns
        let corpus = vec![
            apply(var(2), vec![nat(1), nat(0)]),
            apply(var(2), vec![nat(2), nat(0)]),
            apply(var(2), vec![nat(3), nat(0)]),
            apply(var(2), vec![nat(4), nat(0)]),
        ];

        let mut next_id = 1;
        let config = ExtractConfig {
            min_shared_size: 2,
            min_matches: 2,
            max_new_rules: 5,
        };

        let rules = extract_rules(&corpus, &[], &mut next_id, &config);
        assert!(!rules.is_empty(), "should extract at least one rule");
    }

    #[test]
    fn empty_corpus_returns_empty_rules() {
        let corpus: Vec<Term> = vec![];
        let mut next_id = 1;
        let config = ExtractConfig::default();
        let rules = extract_rules(&corpus, &[], &mut next_id, &config);
        assert!(rules.is_empty(), "empty corpus should produce no rules");
    }

    #[test]
    fn single_element_corpus_returns_empty_rules() {
        let corpus = vec![apply(var(2), vec![nat(1), nat(0)])];
        let mut next_id = 1;
        let config = ExtractConfig::default();
        let rules = extract_rules(&corpus, &[], &mut next_id, &config);
        assert!(
            rules.is_empty(),
            "single-element corpus cannot produce pairwise patterns"
        );
    }

    #[test]
    fn existing_library_prevents_duplicate_extraction() {
        // Corpus with a repeated pattern
        let corpus = vec![
            apply(var(2), vec![nat(1), nat(0)]),
            apply(var(2), vec![nat(2), nat(0)]),
            apply(var(2), vec![nat(3), nat(0)]),
            apply(var(2), vec![nat(4), nat(0)]),
        ];

        let mut next_id = 1;
        let config = ExtractConfig {
            min_shared_size: 2,
            min_matches: 2,
            max_new_rules: 5,
        };

        // First extraction to get the pattern
        let first_rules = extract_rules(&corpus, &[], &mut next_id, &config);
        assert!(!first_rules.is_empty());

        // Second extraction with the first rules as existing library
        let second_rules = extract_rules(&corpus, &first_rules, &mut next_id, &config);
        // The same pattern should be filtered out as it already exists
        for rule in &second_rules {
            for existing in &first_rules {
                assert_ne!(
                    rule.lhs, existing.lhs,
                    "should not extract a pattern already in the library"
                );
            }
        }
    }

    #[test]
    fn high_min_matches_filters_rare_patterns() {
        // Only two terms share the add(_, 0) pattern
        let corpus = vec![
            apply(var(2), vec![nat(1), nat(0)]),
            apply(var(2), vec![nat(2), nat(0)]),
            apply(var(3), vec![nat(5), nat(6)]),  // mul, different structure
        ];

        let mut next_id = 1;
        let config = ExtractConfig {
            min_shared_size: 2,
            min_matches: 10, // require 10 matches — impossible with 3-element corpus
            max_new_rules: 5,
        };

        let rules = extract_rules(&corpus, &[], &mut next_id, &config);
        assert!(
            rules.is_empty(),
            "high min_matches should filter out all patterns"
        );
    }
}
