//! Diagnostic: what does extract_rules produce on a mixed corpus?
//!
//! The flex layer output showed that a mixed corpus (add-identity +
//! mul-identity) yielded only ONE discovered pattern, not two. This
//! test instruments extract_rules directly to see exactly what the
//! generator considered vs kept — the finding names whether the
//! single-pattern result is a generator limitation or a corpus
//! property.

use mathscape_compress::extract::{extract_rules, ExtractConfig};
use mathscape_core::{
    eval::RewriteRule,
    term::{SymbolId, Term},
    value::Value,
};

fn apply(f: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(f), args)
}
fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}
fn var(id: u32) -> Term {
    Term::Var(id)
}

fn mixed_corpus() -> Vec<Term> {
    let mut v = Vec::new();
    for n in 1..=8 {
        v.push(apply(var(2), vec![nat(n), nat(0)])); // add(n, 0)
        v.push(apply(var(3), vec![nat(n), nat(1)])); // mul(n, 1)
    }
    v
}

fn add_only_corpus() -> Vec<Term> {
    (1..=8).map(|n| apply(var(2), vec![nat(n), nat(0)])).collect()
}

fn mul_only_corpus() -> Vec<Term> {
    (1..=8).map(|n| apply(var(3), vec![nat(n), nat(1)])).collect()
}

#[test]
fn observe_mixed_corpus_extraction() {
    let mut next_id: SymbolId = 1;
    let cfg = ExtractConfig::default();
    let rules = extract_rules(&mixed_corpus(), &[], &mut next_id, &cfg);

    println!(
        "\n── mixed corpus: {} terms → {} rules extracted ──",
        mixed_corpus().len(),
        rules.len()
    );
    for (i, r) in rules.iter().enumerate() {
        println!("  [{i}] {}: {} => {}", r.name, r.lhs, r.rhs);
    }

    // Classify by the head of the lhs.
    let mut add_patterns = 0;
    let mut mul_patterns = 0;
    let mut other_patterns = 0;
    for r in &rules {
        match &r.lhs {
            Term::Apply(f, _) => match f.as_ref() {
                Term::Var(2) => add_patterns += 1,
                Term::Var(3) => mul_patterns += 1,
                _ => other_patterns += 1,
            },
            _ => other_patterns += 1,
        }
    }
    println!("  add-head patterns: {add_patterns}");
    println!("  mul-head patterns: {mul_patterns}");
    println!("  other patterns:    {other_patterns}");
}

#[test]
fn observe_single_family_corpora() {
    let mut next_id: SymbolId = 1;
    let cfg = ExtractConfig::default();

    let add_rules = extract_rules(&add_only_corpus(), &[], &mut next_id.clone(), &cfg);
    println!("\n── add-only corpus: {} rules ──", add_rules.len());
    for (i, r) in add_rules.iter().enumerate() {
        println!("  [{i}] {}: {} => {}", r.name, r.lhs, r.rhs);
    }

    let mut next_id = 1;
    let mul_rules = extract_rules(&mul_only_corpus(), &[], &mut next_id, &cfg);
    println!("\n── mul-only corpus: {} rules ──", mul_rules.len());
    for (i, r) in mul_rules.iter().enumerate() {
        println!("  [{i}] {}: {} => {}", r.name, r.lhs, r.rhs);
    }
}

#[test]
fn observe_mixed_with_flex_config() {
    // The flex tests use a custom ExtractConfig — verify it also
    // produces both patterns after the dedup fix.
    let cfg = ExtractConfig {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 3,
    };
    let mut next_id: SymbolId = 1;
    let rules = extract_rules(&mixed_corpus(), &[], &mut next_id, &cfg);
    println!(
        "\n── mixed corpus (flex config): {} rules ──",
        rules.len()
    );
    for (i, r) in rules.iter().enumerate() {
        println!("  [{i}] {}: {} => {}", r.name, r.lhs, r.rhs);
    }
    let heads: std::collections::BTreeSet<u32> = rules
        .iter()
        .filter_map(|r| match &r.lhs {
            Term::Apply(f, _) => match f.as_ref() {
                Term::Var(id) => Some(*id),
                _ => None,
            },
            _ => None,
        })
        .collect();
    println!("Distinct lhs heads: {heads:?}");
}

#[test]
fn mixed_corpus_should_find_both_patterns() {
    // The theoretical expectation: a mixed corpus with 8 add and
    // 8 mul terms should yield patterns for both heads. This test
    // asserts that expectation. If it FAILS, we've identified a
    // real generator limitation; the failure message is the next
    // blocker the machine names.
    let mut next_id: SymbolId = 1;
    let cfg = ExtractConfig::default();
    let rules = extract_rules(&mixed_corpus(), &[], &mut next_id, &cfg);

    let heads: std::collections::BTreeSet<u32> = rules
        .iter()
        .filter_map(|r| match &r.lhs {
            Term::Apply(f, _) => match f.as_ref() {
                Term::Var(id) => Some(*id),
                _ => None,
            },
            _ => None,
        })
        .collect();

    println!("\nDistinct lhs heads in extracted rules: {heads:?}");

    // EXPECTED: both 2 (add) and 3 (mul) should appear.
    // This may FAIL — if so, the failure tells us a real gap.
    assert!(
        heads.contains(&2),
        "expected add-head (var 2) in rules, got heads: {heads:?}"
    );
    assert!(
        heads.contains(&3),
        "expected mul-head (var 3) in rules, got heads: {heads:?}"
    );
}
