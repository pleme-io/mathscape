//! Structural pattern matchers that identify mathematical properties
//! from discovered rewrite rules, without relying on symbol names.

use mathscape_core::eval::RewriteRule;
use mathscape_core::translation::Translation;
use serde::{Deserialize, Serialize};

/// A match result: a discovered symbol maps to a known mathematical property.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identification {
    /// The known property this matches.
    pub property_id: String,
    /// Human-readable property name.
    pub property_name: String,
    /// Mathematical domain.
    pub domain: String,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f64,
    /// LaTeX representation.
    pub latex: String,
    /// How the match was determined.
    pub match_type: MatchType,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MatchType {
    /// Exact structural match with the known pattern.
    Exact,
    /// Match modulo variable renaming.
    AlphaEquivalent,
    /// Partial match (some structural similarity).
    Partial,
}

/// Identify a rewrite rule against the known-math catalog. The most
/// specific catalogued law comes first; the general laws it also
/// instantiates follow at lower confidence.
pub fn identify(rule: &RewriteRule) -> Vec<Identification> {
    let translation = Translation::of(rule);
    let Some(named) = translation.named() else {
        return Vec::new();
    };
    let vocabulary = mathscape_core::translation::vocabulary();
    std::iter::once((named.invariant_id.as_str(), 1.0))
        .chain(named.also_instance_of.iter().map(|id| (id.as_str(), 0.9)))
        .filter_map(|(id, confidence)| {
            let def = vocabulary.invariant(id)?;
            Some(Identification {
                property_id: def.id.clone(),
                property_name: def.canonical_name.clone(),
                domain: def.math_domain.clone(),
                confidence,
                latex: def.latex.clone(),
                match_type: MatchType::Exact,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn detect_commutativity() {
        let rule = RewriteRule {
            name: "S_042".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: apply(var(2), vec![var(101), var(100)]),
        };
        let ids = identify(&rule);
        assert!(ids.iter().any(|i| i.property_id == "add-commutativity"));
    }

    #[test]
    fn detect_identity() {
        let rule = RewriteRule {
            name: "S_007".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        let ids = identify(&rule);
        assert!(ids.iter().any(|i| i.property_id == "add-identity"));
    }

    #[test]
    fn detect_associativity() {
        // add(add(a, b), c) => add(a, add(b, c))
        let rule = RewriteRule {
            name: "S_099".into(),
            lhs: apply(var(2), vec![apply(var(2), vec![var(100), var(101)]), var(102)]),
            rhs: apply(var(2), vec![var(100), apply(var(2), vec![var(101), var(102)])]),
        };
        let ids = identify(&rule);
        assert!(ids.iter().any(|i| i.property_id == "add-associativity"));
    }

    #[test]
    fn detect_mul_commutativity() {
        // mul(a, b) => mul(b, a)
        let rule = RewriteRule {
            name: "S_050".into(),
            lhs: apply(var(3), vec![var(100), var(101)]),
            rhs: apply(var(3), vec![var(101), var(100)]),
        };
        let ids = identify(&rule);
        assert!(
            ids.iter().any(|i| i.property_id == "mul-commutativity"),
            "should detect mul commutativity, got: {ids:?}"
        );
    }

    #[test]
    fn detect_mul_identity() {
        // mul(a, 1) => a
        let rule = RewriteRule {
            name: "S_051".into(),
            lhs: apply(var(3), vec![var(100), nat(1)]),
            rhs: var(100),
        };
        let ids = identify(&rule);
        assert!(
            ids.iter().any(|i| i.property_id == "mul-identity"),
            "should detect mul identity, got: {ids:?}"
        );
    }

    #[test]
    fn detect_idempotence() {
        // f(a, a) => a (using a generic function var(5))
        let rule = RewriteRule {
            name: "S_060".into(),
            lhs: apply(var(5), vec![var(100), var(100)]),
            rhs: var(100),
        };
        let ids = identify(&rule);
        assert!(
            ids.iter().any(|i| i.property_id == "idempotence"),
            "should detect idempotence, got: {ids:?}"
        );
    }

    #[test]
    fn detect_involution() {
        // f(f(a)) => a (using a generic function var(5))
        let rule = RewriteRule {
            name: "S_070".into(),
            lhs: apply(var(5), vec![apply(var(5), vec![var(100)])]),
            rhs: var(100),
        };
        let ids = identify(&rule);
        assert!(
            ids.iter().any(|i| i.property_id == "involution"),
            "should detect involution, got: {ids:?}"
        );
    }

    #[test]
    fn detect_distributivity() {
        // mul(a, add(b, c)) => add(mul(a, b), mul(a, c))
        let rule = RewriteRule {
            name: "S_080".into(),
            lhs: apply(var(3), vec![var(100), apply(var(2), vec![var(101), var(102)])]),
            rhs: apply(
                var(2),
                vec![
                    apply(var(3), vec![var(100), var(101)]),
                    apply(var(3), vec![var(100), var(102)]),
                ],
            ),
        };
        let ids = identify(&rule);
        assert!(
            ids.iter()
                .any(|i| i.property_id == "distributivity-mul-add"),
            "should detect distributivity of mul over add, got: {ids:?}"
        );
    }

    #[test]
    fn non_matching_rule_returns_empty() {
        // A rule that doesn't match any known property pattern:
        // add(a, b) => mul(a, b) — not commutativity, identity, etc.
        let rule = RewriteRule {
            name: "S_999".into(),
            lhs: apply(var(2), vec![var(100), var(101)]),
            rhs: apply(var(3), vec![var(100), var(101)]),
        };
        let ids = identify(&rule);
        assert!(
            ids.is_empty(),
            "should return empty for non-matching rule, got: {ids:?}"
        );
    }
}
