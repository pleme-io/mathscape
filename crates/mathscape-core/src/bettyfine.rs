//! The bettyfine — the modal attractor basin of mathscape's
//! discovery moduli space.
//!
//! See `docs/arch/bettyfine.md` for the mathematical grounding.
//! This module exposes the bettyfine operationally:
//!
//!   - `bettyfine_library(vocab, start_id)` synthesizes the
//!     canonical rule set for a given operator vocabulary
//!     without running discovery. ~89% of discovery traversals
//!     at current machinery scale produce (alpha-equivalent to)
//!     this library; serving it directly is an O(|vocab|)
//!     short-circuit of an O(seeds × corpus × library) sweep.
//!
//! The library has one "Symbol-naming" rule per operator:
//!
//!   (op ?a ?b ... ) => Sym_k(op, ?a, ?b, ...)
//!
//! This is the machine's canonical layer-0 compression: bind
//! each operator-application to a new named symbol that
//! preserves both operator and args. Further compression laws
//! (identity, associativity, commutativity) sit BELOW this
//! layer — they take specific rules, not the universal
//! Symbol-naming shape.
//!
//! ### Current-machinery bettyfine content (empirical)
//!
//! At budget=15, depth=4, pure-procedural input, 89% of 1024
//! seeds produce a library alpha-equivalent to:
//!
//!   succ-universal :  (Var(4) ?x)         => S_k(Var(4), ?x)
//!   add-family OR    (Var(2) ?x ?y)       => S_k(Var(2), ?x, ?y)
//!   mul-family       (Var(3) ?x ?y)       => S_k(Var(3), ?x, ?y)
//!
//! Under operator-abstraction, these three collapse into one
//! "shape" — the bettyfine's canonical form.

use crate::eval::RewriteRule;
use crate::term::{SymbolId, Term};

/// Describes one operator in mathscape's vocabulary.
#[derive(Debug, Clone, Copy)]
pub struct OperatorSpec {
    /// The Var id of the operator (Var(2) = add, Var(3) = mul,
    /// Var(4) = succ, etc.). Concrete ops occupy id < 100.
    pub var_id: u32,
    /// Arity — number of args the operator takes. Unary (succ) = 1,
    /// binary (add, mul) = 2.
    pub arity: usize,
    /// Human-readable name for debugging.
    pub name: &'static str,
}

impl OperatorSpec {
    pub const SUCC: Self = Self { var_id: 4, arity: 1, name: "succ" };
    pub const ADD: Self = Self { var_id: 2, arity: 2, name: "add" };
    pub const MUL: Self = Self { var_id: 3, arity: 2, name: "mul" };

    /// The standard zoo vocabulary used in the autonomous-traversal
    /// milestone. Other vocabularies produce bettyfines with
    /// different operator sets but the same canonical shape.
    pub fn standard_vocabulary() -> Vec<Self> {
        vec![Self::SUCC, Self::ADD, Self::MUL]
    }
}

/// Synthesize the bettyfine library for a given vocabulary.
///
/// Returns |vocab| rules, one per operator, each of the form
///   (op ?v100 ?v101 ... ) => Sym_k(op, ?v100, ?v101, ...)
/// where `k` runs from `start_symbol_id` upward.
///
/// This is the canonical layer-0 compression: each operator
/// application is bound to a fresh named symbol that preserves
/// both the operator and its arguments. The rule is
/// structure-preserving (bijective) — no information loss.
///
/// For the standard vocabulary, the result is a 3-rule library
/// alpha-equivalent to what ~89% of seed-driven traversals
/// produce at current machinery scale.
#[must_use]
pub fn bettyfine_library(vocab: &[OperatorSpec], start_symbol_id: SymbolId) -> Vec<RewriteRule> {
    let mut rules = Vec::with_capacity(vocab.len());
    // Pattern variable ids start at 100 (fresh-var convention)
    // and advance across args within each rule.
    for (i, op) in vocab.iter().enumerate() {
        let sym_id = start_symbol_id + i as SymbolId;
        let arg_vars: Vec<Term> = (0..op.arity)
            .map(|k| Term::Var(100 + k as u32))
            .collect();

        // LHS: (op ?v100 ?v101 ...)
        let lhs = Term::Apply(
            Box::new(Term::Var(op.var_id)),
            arg_vars.clone(),
        );

        // RHS: Sym_k(Var(op.var_id), ?v100, ?v101, ...)
        let mut rhs_args = Vec::with_capacity(op.arity + 1);
        rhs_args.push(Term::Var(op.var_id));
        for v in &arg_vars {
            rhs_args.push(v.clone());
        }
        let rhs = Term::Symbol(sym_id, rhs_args);

        rules.push(RewriteRule {
            name: format!("bettyfine_{}", op.name),
            lhs,
            rhs,
        });
    }
    rules
}

/// Number of rules in the bettyfine for the standard vocabulary.
/// At current machinery: 3 (succ + add + mul).
#[must_use]
pub fn standard_bettyfine_cardinality() -> usize {
    OperatorSpec::standard_vocabulary().len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{alpha_equivalent, pattern_match};

    #[test]
    fn bettyfine_is_vocabulary_sized() {
        let lib = bettyfine_library(&OperatorSpec::standard_vocabulary(), 0);
        assert_eq!(lib.len(), 3);
        assert_eq!(standard_bettyfine_cardinality(), 3);
    }

    #[test]
    fn every_bettyfine_rule_matches_its_operator_application() {
        // The bettyfine is USEFUL: each rule actually matches
        // concrete applications of its operator. Verifies the
        // synthesis is well-formed.
        let lib = bettyfine_library(&OperatorSpec::standard_vocabulary(), 100);
        use crate::test_helpers::{apply, nat, var};

        // succ rule matches (succ 5)
        let term = apply(var(4), vec![nat(5)]);
        assert!(
            pattern_match(&lib[0].lhs, &term).is_some(),
            "succ bettyfine rule should match (succ n)"
        );

        // add rule matches (add 3 0)
        let term = apply(var(2), vec![nat(3), nat(0)]);
        assert!(
            pattern_match(&lib[1].lhs, &term).is_some(),
            "add bettyfine rule should match (add x y)"
        );

        // mul rule matches (mul 7 1)
        let term = apply(var(3), vec![nat(7), nat(1)]);
        assert!(
            pattern_match(&lib[2].lhs, &term).is_some(),
            "mul bettyfine rule should match (mul x y)"
        );
    }

    #[test]
    fn bettyfine_rules_are_alpha_unique() {
        // No two rules in the bettyfine are alpha-equivalent.
        // If they were, eager collapse would reduce the library
        // at reinforcement time.
        let lib = bettyfine_library(&OperatorSpec::standard_vocabulary(), 0);
        for i in 0..lib.len() {
            for j in (i + 1)..lib.len() {
                assert!(
                    !alpha_equivalent(&lib[i], &lib[j]),
                    "bettyfine rules {} and {} are alpha-equivalent — eager collapse \
                     would reduce the library to fewer rules",
                    i, j
                );
            }
        }
    }

    #[test]
    fn bettyfine_is_deterministic() {
        // Same input → same output. Bettyfine synthesis must be
        // a pure function of (vocab, start_id).
        let vocab = OperatorSpec::standard_vocabulary();
        let a = bettyfine_library(&vocab, 1000);
        let b = bettyfine_library(&vocab, 1000);
        assert_eq!(a.len(), b.len());
        for (ra, rb) in a.iter().zip(b.iter()) {
            assert_eq!(ra.name, rb.name);
            assert_eq!(ra.lhs, rb.lhs);
            assert_eq!(ra.rhs, rb.rhs);
        }
    }

    #[test]
    fn bettyfine_start_id_offsets_symbols() {
        // Different start_ids produce structurally-equal but
        // nominally-different libraries. Under anonymization
        // they'd merge (alpha-equivalent).
        let vocab = OperatorSpec::standard_vocabulary();
        let a = bettyfine_library(&vocab, 0);
        let b = bettyfine_library(&vocab, 500);
        // Nominal inequality...
        assert_ne!(a[0].rhs, b[0].rhs);
        // ...but alpha-equivalence preserved.
        for (ra, rb) in a.iter().zip(b.iter()) {
            assert!(
                alpha_equivalent(ra, rb),
                "bettyfine rules should be alpha-equivalent across start_id \
                 choices — the symbol id is a free parameter"
            );
        }
    }
}
