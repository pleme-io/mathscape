//! Structural pattern matchers that identify mathematical properties
//! from discovered rewrite rules, without relying on symbol names.

use mathscape_core::eval::RewriteRule;
use mathscape_core::term::Term;
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

/// Attempt to identify a rewrite rule against the known-math catalog.
pub fn identify(rule: &RewriteRule) -> Vec<Identification> {
    let mut results = Vec::new();

    // Check each property pattern
    if let Some(id) = check_commutativity(rule) {
        results.push(id);
    }
    if let Some(id) = check_associativity(rule) {
        results.push(id);
    }
    if let Some(id) = check_identity_element(rule) {
        results.push(id);
    }
    if let Some(id) = check_idempotence(rule) {
        results.push(id);
    }
    if let Some(id) = check_involution(rule) {
        results.push(id);
    }
    if let Some(id) = check_distributivity(rule) {
        results.push(id);
    }

    results
}

/// Commutativity: f(a, b) => f(b, a)
/// LHS has f(a, b), RHS has f(b, a) with same function, swapped args.
fn check_commutativity(rule: &RewriteRule) -> Option<Identification> {
    // LHS: (f ?a ?b), RHS: (f ?b ?a)
    let (lf, la) = as_binary_apply(&rule.lhs)?;
    let (rf, ra) = as_binary_apply(&rule.rhs)?;

    // Same function
    if lf != rf {
        return None;
    }

    // Args are swapped
    if la.0 == ra.1 && la.1 == ra.0 && la.0 != la.1 {
        // Check if it's specifically add or mul commutativity
        let (prop, confidence) = if is_builtin_add(lf) {
            ("add_commutativity", 1.0)
        } else if is_builtin_mul(lf) {
            ("mul_commutativity", 1.0)
        } else {
            ("commutativity", 0.9)
        };

        let catalog_entry = crate::catalog::catalog()
            .into_iter()
            .find(|p| p.id == prop)?;

        return Some(Identification {
            property_id: prop.to_string(),
            property_name: catalog_entry.name.to_string(),
            domain: catalog_entry.domain.to_string(),
            confidence,
            latex: catalog_entry.latex.to_string(),
            match_type: MatchType::Exact,
        });
    }

    None
}

/// Associativity: f(f(a, b), c) => f(a, f(b, c))
fn check_associativity(rule: &RewriteRule) -> Option<Identification> {
    // LHS: (f (f ?a ?b) ?c)
    let (lf, (ll, lr)) = as_binary_apply(&rule.lhs)?;
    let (inner_f, (a, b)) = as_binary_apply(ll)?;

    if lf != inner_f {
        return None;
    }
    let c = lr;

    // RHS: (f ?a (f ?b ?c))
    let (rf, (ra, rr)) = as_binary_apply(&rule.rhs)?;
    if rf != lf {
        return None;
    }

    let (inner_rf, (rb, rc)) = as_binary_apply(rr)?;
    if inner_rf != lf {
        return None;
    }

    // Check: a matches ra, b matches rb, c matches rc
    if a == ra && b == rb && c == rc {
        let (prop, confidence) = if is_builtin_add(lf) {
            ("add_associativity", 1.0)
        } else if is_builtin_mul(lf) {
            ("mul_associativity", 1.0)
        } else {
            ("associativity", 0.9)
        };

        let catalog_entry = crate::catalog::catalog()
            .into_iter()
            .find(|p| p.id == prop)?;

        return Some(Identification {
            property_id: prop.to_string(),
            property_name: catalog_entry.name.to_string(),
            domain: catalog_entry.domain.to_string(),
            confidence,
            latex: catalog_entry.latex.to_string(),
            match_type: MatchType::Exact,
        });
    }

    None
}

/// Identity element: f(a, e) => a or f(e, a) => a
fn check_identity_element(rule: &RewriteRule) -> Option<Identification> {
    let (f, (left, right)) = as_binary_apply(&rule.lhs)?;

    // f(a, e) => a — right identity
    if &rule.rhs == left && is_constant(right) {
        return make_identity_match(f, right);
    }

    // f(e, a) => a — left identity
    if &rule.rhs == right && is_constant(left) {
        return make_identity_match(f, left);
    }

    None
}

fn make_identity_match(f: &Term, _identity_element: &Term) -> Option<Identification> {
    let (prop, confidence) = if is_builtin_add(f) {
        ("add_identity", 1.0)
    } else if is_builtin_mul(f) {
        ("mul_identity", 1.0)
    } else {
        ("identity_element", 0.85)
    };

    let catalog_entry = crate::catalog::catalog()
        .into_iter()
        .find(|p| p.id == prop)?;

    Some(Identification {
        property_id: prop.to_string(),
        property_name: catalog_entry.name.to_string(),
        domain: catalog_entry.domain.to_string(),
        confidence,
        latex: catalog_entry.latex.to_string(),
        match_type: MatchType::Exact,
    })
}

/// Idempotence: f(a, a) => a
fn check_idempotence(rule: &RewriteRule) -> Option<Identification> {
    let (_, (left, right)) = as_binary_apply(&rule.lhs)?;

    if left == right && &rule.rhs == left {
        let entry = crate::catalog::catalog()
            .into_iter()
            .find(|p| p.id == "idempotence")?;

        return Some(Identification {
            property_id: "idempotence".to_string(),
            property_name: entry.name.to_string(),
            domain: entry.domain.to_string(),
            confidence: 0.9,
            latex: entry.latex.to_string(),
            match_type: MatchType::Exact,
        });
    }

    None
}

/// Involution: f(f(a)) => a
fn check_involution(rule: &RewriteRule) -> Option<Identification> {
    let (outer_f, outer_args) = as_apply(&rule.lhs)?;
    if outer_args.len() != 1 {
        return None;
    }

    let (inner_f, inner_args) = as_apply(&outer_args[0])?;
    if inner_args.len() != 1 {
        return None;
    }

    if outer_f != inner_f {
        return None;
    }

    if rule.rhs == inner_args[0] {
        let entry = crate::catalog::catalog()
            .into_iter()
            .find(|p| p.id == "involution")?;

        return Some(Identification {
            property_id: "involution".to_string(),
            property_name: entry.name.to_string(),
            domain: entry.domain.to_string(),
            confidence: 0.9,
            latex: entry.latex.to_string(),
            match_type: MatchType::Exact,
        });
    }

    None
}

/// Distributivity: f(a, g(b, c)) => g(f(a, b), f(a, c))
fn check_distributivity(rule: &RewriteRule) -> Option<Identification> {
    let (f, (a, g_bc)) = as_binary_apply(&rule.lhs)?;
    let (g, (b, c)) = as_binary_apply(g_bc)?;

    // RHS: g(f(a, b), f(a, c))
    let (rg, (rl, rr)) = as_binary_apply(&rule.rhs)?;
    if rg != g {
        return None;
    }

    let (rl_f, (rl_a, rl_b)) = as_binary_apply(rl)?;
    let (rr_f, (rr_a, rr_c)) = as_binary_apply(rr)?;

    if rl_f != f || rr_f != f {
        return None;
    }

    if rl_a == a && rl_b == b && rr_a == a && rr_c == c {
        let (prop, confidence) = if is_builtin_mul(f) && is_builtin_add(g) {
            ("distributivity_mul_add", 1.0)
        } else {
            ("distributivity", 0.85)
        };

        let entry = crate::catalog::catalog()
            .into_iter()
            .find(|p| p.id == prop)?;

        return Some(Identification {
            property_id: prop.to_string(),
            property_name: entry.name.to_string(),
            domain: entry.domain.to_string(),
            confidence,
            latex: entry.latex.to_string(),
            match_type: MatchType::Exact,
        });
    }

    None
}

// === Helpers ===

/// Extract a binary application: (f a b) -> (f, (a, b))
fn as_binary_apply(term: &Term) -> Option<(&Term, (&Term, &Term))> {
    if let Term::Apply(f, args) = term {
        if args.len() == 2 {
            return Some((f.as_ref(), (&args[0], &args[1])));
        }
    }
    None
}

/// Extract any application: (f args...) -> (f, args)
fn as_apply(term: &Term) -> Option<(&Term, &[Term])> {
    if let Term::Apply(f, args) = term {
        Some((f.as_ref(), args.as_slice()))
    } else {
        None
    }
}

fn is_constant(term: &Term) -> bool {
    matches!(term, Term::Number(_) | Term::Point(_))
}

fn is_builtin_add(term: &Term) -> bool {
    matches!(term, Term::Var(2))
}

fn is_builtin_mul(term: &Term) -> bool {
    matches!(term, Term::Var(3))
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
        assert!(ids.iter().any(|i| i.property_id == "add_commutativity"));
    }

    #[test]
    fn detect_identity() {
        let rule = RewriteRule {
            name: "S_007".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        let ids = identify(&rule);
        assert!(ids.iter().any(|i| i.property_id == "add_identity"));
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
        assert!(ids.iter().any(|i| i.property_id == "add_associativity"));
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
            ids.iter().any(|i| i.property_id == "mul_commutativity"),
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
            ids.iter().any(|i| i.property_id == "mul_identity"),
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
                .any(|i| i.property_id == "distributivity_mul_add"),
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
