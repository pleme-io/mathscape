//! R21 — Tensor discovery proof.
//!
//! Demonstrates that the machine's existing anti-unification
//! pipeline can discover tensor-structure rules when given
//! corpora that exercise tensor operators. The operators were
//! added in R13+R19; this test proves the discovery path is OPEN
//! for them — no special treatment needed, the registry entries
//! suffice.
//!
//! The test does NOT claim the machine autonomously discovers
//! specific named laws (identity, commutativity) without further
//! scaffolding; it claims the infrastructure SEES tensor
//! expressions and produces rule candidates that reference
//! tensor heads. That's the gate — once the machine can produce
//! tensor-shaped candidates, the existing prover + lifecycle can
//! promote them via the normal path.

mod common;

use common::tensor_corpus;
use mathscape_compress::extract::{extract_rules, ExtractConfig};
use mathscape_core::term::Term;

fn rule_touches_tensor_op(rule: &mathscape_core::eval::RewriteRule) -> bool {
    // Tensor ops live in ids 20..=28 (R13+R16). FloatTensor 40..=47 (R19).
    fn walk(t: &Term) -> bool {
        match t {
            Term::Apply(head, args) => {
                let hit = match head.as_ref() {
                    Term::Var(id) => (20..=28).contains(id) || (40..=47).contains(id),
                    _ => false,
                };
                hit || walk(head) || args.iter().any(walk)
            }
            Term::Fn(_, body) => walk(body),
            Term::Symbol(_, args) => args.iter().any(walk),
            _ => false,
        }
    }
    walk(&rule.lhs) || walk(&rule.rhs)
}

#[test]
fn tensor_corpus_produces_tensor_headed_candidates() {
    // Generate a tensor-heavy corpus via the new generator and
    // confirm anti-unification produces at least one candidate
    // rule that references a tensor operator. If zero tensor
    // heads appear, the discovery pipeline is NOT seeing the
    // corpus as tensor-shaped — a regression in the R13+R19 work.

    let corpus = tensor_corpus(42, 3, 40);

    // Sanity: corpus should contain tensor_add or tensor_mul at
    // the top of at least some expressions (since generator
    // produces Applys at non-leaf depths).
    let contains_tensor_op = corpus.iter().any(|t| {
        if let Term::Apply(head, _) = t {
            matches!(head.as_ref(), Term::Var(20) | Term::Var(21))
        } else {
            false
        }
    });
    assert!(
        contains_tensor_op,
        "tensor_corpus must produce Applys with tensor_add/tensor_mul heads"
    );

    // Extract candidate rules.
    let mut next_id: mathscape_core::term::SymbolId = 0;
    let config = ExtractConfig {
        min_shared_size: 2,
        min_matches: 2,
        max_new_rules: 20,
    };
    let rules = extract_rules(&corpus, &[], &mut next_id, &config);

    // Not all extractions need be tensor-touching (some may
    // abstract away operators entirely into meta-rules), but at
    // least ONE should reference a tensor head — otherwise the
    // corpus isn't feeding tensor structure into the pipeline.
    let tensor_rules: Vec<_> = rules
        .iter()
        .filter(|r| rule_touches_tensor_op(r))
        .collect();
    assert!(
        !tensor_rules.is_empty(),
        "expected at least one extracted rule to touch a tensor op; \
         got {} rules total, none tensor-headed. Sample: {:?}",
        rules.len(),
        rules.first().map(|r| (r.lhs.clone(), r.rhs.clone())),
    );
}

#[test]
fn tensor_corpus_is_deterministic_across_seeds() {
    // Determinism invariant: same seed, same depth, same count
    // ⇒ identical corpus. Proves the generator is suitable for
    // the machine's deterministic_replay discipline (should it
    // ever be promoted to the canonical zoo).
    let a = tensor_corpus(7, 3, 20);
    let b = tensor_corpus(7, 3, 20);
    assert_eq!(a, b);

    // Different seed ⇒ different corpus.
    let c = tensor_corpus(8, 3, 20);
    assert_ne!(a, c);
}

#[test]
fn tensor_corpus_size_matches_request() {
    assert_eq!(tensor_corpus(1, 3, 10).len(), 10);
    assert_eq!(tensor_corpus(1, 3, 50).len(), 50);
}
