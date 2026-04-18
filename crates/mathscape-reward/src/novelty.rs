//! Novelty scoring: generality and irreducibility of discovered symbols.

use mathscape_core::eval::{pattern_match, subsumes, RewriteRule};
use mathscape_core::term::Term;

/// Compute generality of a rewrite rule against a corpus.
/// generality(s) = |{e in C : s matches a subexpression of e}| / |C|
pub fn generality(rule: &RewriteRule, corpus: &[Term]) -> f64 {
    if corpus.is_empty() {
        return 0.0;
    }

    let matches = corpus
        .iter()
        .filter(|e| matches_anywhere(&rule.lhs, e))
        .count();

    matches as f64 / corpus.len() as f64
}

/// Check if a pattern matches anywhere in a term (root or any subtree).
fn matches_anywhere(pattern: &Term, term: &Term) -> bool {
    if pattern_match(pattern, term).is_some() {
        return true;
    }

    match term {
        Term::Apply(func, args) => {
            matches_anywhere(pattern, func) || args.iter().any(|a| matches_anywhere(pattern, a))
        }
        Term::Fn(_, body) => matches_anywhere(pattern, body),
        Term::Symbol(_, args) => args.iter().any(|a| matches_anywhere(pattern, a)),
        _ => false,
    }
}

/// Compute irreducibility: can a symbol be derived from existing library?
/// Returns 1.0 if irreducible (truly novel), 0.0 if derivable.
///
/// Simple heuristic: check if the LHS pattern of the new rule matches
/// the RHS of any existing rule after substitution. If so, the new
/// rule is derivable.
pub fn irreducibility(rule: &RewriteRule, existing_library: &[RewriteRule]) -> f64 {
    for existing in existing_library {
        // Exact subsumption: existing lhs/rhs patterns subsume rule's.
        if subsumes(&existing.lhs, &rule.lhs) && subsumes(&existing.rhs, &rule.rhs) {
            return 0.0;
        }
        // If the existing rule's RHS structurally subsumes our LHS
        // (and RHS is not a trivial variable), it might be derivable.
        if !existing.rhs.is_var() && subsumes(&existing.rhs, &rule.lhs) {
            return 0.0;
        }
    }
    1.0
}

/// Combined novelty score.
/// novelty(symbol, L) = generality(symbol) * irreducibility(symbol, L)
pub fn novelty_score(
    rule: &RewriteRule,
    corpus: &[Term],
    existing_library: &[RewriteRule],
) -> f64 {
    generality(rule, corpus) * irreducibility(rule, existing_library)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn generality_all_match() {
        let rule = RewriteRule {
            name: "add-anything".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: var(100),
        };
        let corpus = vec![
            apply(var(2), vec![nat(1), nat(2)]),
            apply(var(2), vec![nat(3), nat(4)]),
        ];
        assert_eq!(generality(&rule, &corpus), 1.0);
    }

    #[test]
    fn generality_partial_match() {
        let rule = RewriteRule {
            name: "add-zero".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        let corpus = vec![
            apply(var(2), vec![nat(1), nat(0)]), // matches
            apply(var(2), vec![nat(3), nat(4)]), // doesn't match
        ];
        assert_eq!(generality(&rule, &corpus), 0.5);
    }

    #[test]
    fn irreducibility_novel() {
        let rule = RewriteRule {
            name: "new-rule".into(),
            lhs: apply(var(3), vec![var(100), var(101)]),
            rhs: apply(var(3), vec![var(101), var(100)]),
        };
        let library = vec![RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        }];
        assert_eq!(irreducibility(&rule, &library), 1.0);
    }

    #[test]
    fn generality_empty_corpus_returns_zero() {
        let rule = RewriteRule {
            name: "r".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: var(100),
        };
        assert_eq!(generality(&rule, &[]), 0.0);
    }

    #[test]
    fn generality_no_matches_returns_zero() {
        // Rule pattern requires a ternary application (3 args),
        // but corpus only has binary applications (2 args) — no structural match.
        let rule = RewriteRule {
            name: "ternary-rule".into(),
            lhs: apply(var(100), vec![var(101), var(102), var(103)]),
            rhs: var(101),
        };
        let corpus = vec![
            apply(var(2), vec![nat(1), nat(2)]),
            apply(var(3), vec![nat(3), nat(4)]),
        ];
        assert_eq!(generality(&rule, &corpus), 0.0);
    }

    #[test]
    fn irreducibility_empty_library_returns_one() {
        let rule = RewriteRule {
            name: "anything".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        assert_eq!(irreducibility(&rule, &[]), 1.0);
    }

    #[test]
    fn irreducibility_detects_exact_subsumption() {
        // Existing rule has same LHS and RHS patterns (modulo variable matching)
        let rule = RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        let library = vec![RewriteRule {
            name: "existing-add-identity".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        }];
        assert_eq!(irreducibility(&rule, &library), 0.0);
    }

    #[test]
    fn novelty_score_zero_when_irreducibility_zero() {
        // Rule is an exact duplicate of one in the library -> irreducibility = 0
        let rule = RewriteRule {
            name: "dup".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        let corpus = vec![
            apply(var(2), vec![nat(1), nat(0)]),
            apply(var(2), vec![nat(5), nat(0)]),
        ];
        let library = vec![rule.clone()];
        // generality > 0 but irreducibility = 0, so novelty = 0
        assert_eq!(novelty_score(&rule, &corpus, &library), 0.0);
    }
}
