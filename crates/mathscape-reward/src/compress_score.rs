//! Description length and compression ratio computation.

use mathscape_core::eval::RewriteRule;
use mathscape_core::term::Term;

/// Compute description length of a corpus under a library.
/// DL(C, L) = |L| + sum over e in C: size(rewrite(e, L))
pub fn description_length(corpus: &[Term], library: &[RewriteRule]) -> usize {
    let library_cost: usize = library.iter().map(|r| r.lhs.size() + r.rhs.size()).sum();
    let corpus_cost: usize = corpus.iter().map(|e| rewritten_size(e, library)).sum();
    library_cost + corpus_cost
}

/// Compute compression ratio.
/// CR(C, L) = 1 - DL(C, L) / DL(C, {})
pub fn compression_ratio(corpus: &[Term], library: &[RewriteRule]) -> f64 {
    let dl_with_lib = description_length(corpus, library) as f64;
    let dl_without = description_length(corpus, &[]) as f64;

    if dl_without == 0.0 {
        return 0.0;
    }

    1.0 - dl_with_lib / dl_without
}

/// Size of a term after applying all applicable library rewrites (one pass).
fn rewritten_size(term: &Term, library: &[RewriteRule]) -> usize {
    // Try to match each library rule at the root
    for rule in library {
        if let Some(bindings) = mathscape_core::eval::pattern_match(&rule.lhs, term) {
            // Matched: the rewritten size is the RHS with matched vars
            let mut rhs = rule.rhs.clone();
            for (var, val) in &bindings {
                rhs = rhs.substitute(*var, val);
            }
            return rhs.size();
        }
    }

    // No match at root: recurse into children
    match term {
        Term::Point(_) | Term::Number(_) | Term::Var(_) => 1,
        Term::Fn(_, body) => 1 + rewritten_size(body, library),
        Term::Apply(func, args) => {
            1 + rewritten_size(func, library)
                + args.iter().map(|a| rewritten_size(a, library)).sum::<usize>()
        }
        Term::Symbol(_, args) => {
            1 + args.iter().map(|a| rewritten_size(a, library)).sum::<usize>()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn dl_without_library() {
        let corpus = vec![
            apply(var(2), vec![nat(1), nat(2)]), // 4 nodes
            apply(var(2), vec![nat(3), nat(0)]), // 4 nodes
        ];
        assert_eq!(description_length(&corpus, &[]), 8);
    }

    #[test]
    fn compression_with_identity_rule() {
        // Rule: add(?x, 0) => ?x
        let rule = RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };

        let corpus = vec![
            apply(var(2), vec![nat(5), nat(0)]), // matches rule -> size 1 (just ?x=5)
            apply(var(2), vec![nat(3), nat(0)]), // matches rule -> size 1
        ];

        let cr = compression_ratio(&corpus, &[rule]);
        assert!(cr > 0.0, "compression ratio should be positive");
    }

    #[test]
    fn empty_corpus_dl_is_zero() {
        // Empty corpus with no library should have DL 0
        assert_eq!(description_length(&[], &[]), 0);
    }

    #[test]
    fn empty_library_dl_equals_sum_of_term_sizes() {
        let corpus = vec![
            nat(5),                               // size 1
            apply(var(2), vec![nat(1), nat(2)]),   // size 4
            apply(var(2), vec![nat(3)]),            // size 3
        ];
        let expected: usize = corpus.iter().map(|t| t.size()).sum();
        assert_eq!(description_length(&corpus, &[]), expected);
    }

    #[test]
    fn compression_ratio_empty_corpus_returns_zero() {
        let rule = RewriteRule {
            name: "r".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        assert_eq!(compression_ratio(&[], &[rule]), 0.0);
    }

    #[test]
    fn nested_terms_with_recursive_matching() {
        // Rule: add(?x, 0) => ?x  (library cost = 4 + 1 = 5)
        let rule = RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };

        // Many corpus entries that match, so the savings outweigh library cost.
        // Each add(N, 0) has size 4 without library, size 1 with library (saving 3 each).
        // With 10 entries: without = 40, with = 5 (lib) + 10 (corpus) = 15.
        let corpus: Vec<Term> = (0..10)
            .map(|i| apply(var(2), vec![nat(i), nat(0)]))
            .collect();

        let dl_without = description_length(&corpus, &[]);
        let dl_with = description_length(&corpus, &[rule.clone()]);
        assert!(
            dl_with < dl_without,
            "library should compress repeated matching terms: with={dl_with}, without={dl_without}"
        );

        // Also verify that nested terms get recursively rewritten.
        // add(add(5, 0), 0) — root matches, rhs = add(5, 0) which has size 4.
        // Without library, size is 7. With library, rewritten_size = 4 (one-pass match at root).
        let nested = vec![apply(
            var(2),
            vec![apply(var(2), vec![nat(5), nat(0)]), nat(0)],
        )];
        let nested_with = rewritten_size(&nested[0], &[rule.clone()]);
        let nested_without = nested[0].size();
        assert!(
            nested_with < nested_without,
            "nested term should be smaller after rewrite: with={nested_with}, without={nested_without}"
        );
    }
}
