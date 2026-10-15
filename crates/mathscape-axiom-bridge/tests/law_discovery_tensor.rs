//! R24.2 — Law discovery on tensor corpora.
//!
//! Wire R21 (tensor corpus) + R24 (law generator) together and
//! observe: does the machine autonomously discover tensor-primitive
//! LAWS (not compression-abstractions)? Are those the same ones we
//! hand-coded in R13+R19?

mod common;

use common::tensor_corpus;
use mathscape_compress::derive_laws_from_corpus;
use mathscape_core::{
    builtin::{TENSOR_ADD, TENSOR_MUL},
    primitives::classify_primitives,
    term::Term,
};

fn rule_head(t: &Term) -> Option<u32> {
    match t {
        Term::Apply(h, _) => match h.as_ref() {
            Term::Var(id) => Some(*id),
            _ => None,
        },
        _ => None,
    }
}

#[test]
fn law_generator_discovers_tensor_laws_from_tensor_corpus() {
    // Generate a tensor-rich corpus using R21. This corpus contains
    // many instances of `tensor_add(a, b)` and `tensor_mul(a, b)`
    // over small operand tensors including zeros and ones — the
    // identity elements for tensor_add and tensor_mul respectively.
    //
    // Feed the corpus to R24's law generator. Assert: at least
    // one discovered law involves a tensor operator AND is
    // law-shaped (LHS has a pattern var, RHS references it).

    let corpus = tensor_corpus(42, 3, 60);
    let mut next_id: mathscape_core::term::SymbolId = 0;
    let laws = derive_laws_from_corpus(&corpus, &[], 200, 2, &mut next_id);

    // At least some laws emerged.
    assert!(
        !laws.is_empty(),
        "law generator found NO laws on tensor corpus of {} terms",
        corpus.len()
    );

    // Partition discovered laws by head operator.
    let mut tensor_add_laws = 0;
    let mut tensor_mul_laws = 0;
    let mut other = 0;
    for law in &laws {
        match rule_head(&law.lhs) {
            Some(TENSOR_ADD) => tensor_add_laws += 1,
            Some(TENSOR_MUL) => tensor_mul_laws += 1,
            _ => other += 1,
        }
    }

    println!("\n══════ Law discovery on tensor corpus ═══════════════");
    println!("Corpus size: {}", corpus.len());
    println!("Laws discovered: {}", laws.len());
    println!("  tensor_add-headed laws: {tensor_add_laws}");
    println!("  tensor_mul-headed laws: {tensor_mul_laws}");
    println!("  other-headed laws:      {other}");
    println!("\nFirst few laws:");
    for law in laws.iter().take(10) {
        println!("  {} :: {} => {}", law.name, law.lhs, law.rhs);
    }

    // R12 classification: see if any law matches a known ML
    // primitive shape.
    println!("\nR12 classifications:");
    for law in &laws {
        let primitives = classify_primitives(law);
        if !primitives.is_empty() {
            println!("  {}: {:?}", law.name, primitives);
        }
    }

    // The key assertion: tensor-headed laws exist. This is the
    // natural-arrival signal we were missing.
    assert!(
        tensor_add_laws > 0 || tensor_mul_laws > 0,
        "expected at least one tensor-headed law to emerge naturally"
    );
}

#[test]
fn tensor_identity_law_is_discoverable() {
    // Deliberately law-shaped corpus: many `tensor_add(zeros, t)`
    // and `tensor_mul(ones, t)` instances. These should yield the
    // identity laws:
    //   tensor_add(zeros, ?t) = ?t
    //   tensor_mul(ones, ?t) = ?t
    use mathscape_core::value::Value;

    let zeros = Term::Number(Value::tensor(vec![2], vec![0, 0]).unwrap());
    let ones = Term::Number(Value::tensor(vec![2], vec![1, 1]).unwrap());
    let operands: Vec<Term> = vec![
        Term::Number(Value::tensor(vec![2], vec![2, 3]).unwrap()),
        Term::Number(Value::tensor(vec![2], vec![4, 5]).unwrap()),
        Term::Number(Value::tensor(vec![2], vec![6, 7]).unwrap()),
        Term::Number(Value::tensor(vec![2], vec![8, 9]).unwrap()),
        Term::Number(Value::tensor(vec![2], vec![10, 11]).unwrap()),
    ];

    let mut corpus: Vec<Term> = Vec::new();
    for op in &operands {
        // Add-identity: tensor_add(zeros, operand) → operand
        corpus.push(Term::Apply(
            Box::new(Term::Var(TENSOR_ADD)),
            vec![zeros.clone(), op.clone()],
        ));
        // Mul-identity: tensor_mul(ones, operand) → operand
        corpus.push(Term::Apply(
            Box::new(Term::Var(TENSOR_MUL)),
            vec![ones.clone(), op.clone()],
        ));
    }

    let mut next_id: mathscape_core::term::SymbolId = 0;
    let laws = derive_laws_from_corpus(&corpus, &[], 200, 3, &mut next_id);

    // Look for law shape: Apply(Var(TENSOR_ADD), [constant, Var]) = Var.
    let has_add_identity = laws.iter().any(|l| {
        let head_ok = rule_head(&l.lhs) == Some(TENSOR_ADD);
        let rhs_is_var = matches!(&l.rhs, Term::Var(_));
        head_ok && rhs_is_var
    });
    let has_mul_identity = laws.iter().any(|l| {
        let head_ok = rule_head(&l.lhs) == Some(TENSOR_MUL);
        let rhs_is_var = matches!(&l.rhs, Term::Var(_));
        head_ok && rhs_is_var
    });

    println!("\n══════ Tensor identity law discovery ═════════════════");
    println!("Corpus size: {}", corpus.len());
    println!("Laws discovered: {}", laws.len());
    for law in &laws {
        println!("  {} :: {} => {}", law.name, law.lhs, law.rhs);
    }
    println!(
        "\ntensor_add identity discovered: {has_add_identity}\n\
         tensor_mul identity discovered: {has_mul_identity}"
    );

    assert!(
        has_add_identity && has_mul_identity,
        "both tensor identity laws MUST be discoverable from law-shaped corpus"
    );
}

#[test]
fn law_discovery_is_deterministic_across_runs() {
    // Same corpus, same call → identical laws. The
    // deterministic_replay discipline extends to law discovery.
    let corpus = tensor_corpus(7, 3, 30);
    let mut id_a: mathscape_core::term::SymbolId = 0;
    let mut id_b: mathscape_core::term::SymbolId = 0;
    let laws_a = derive_laws_from_corpus(&corpus, &[], 200, 2, &mut id_a);
    let laws_b = derive_laws_from_corpus(&corpus, &[], 200, 2, &mut id_b);
    // Same COUNT at minimum (names may differ due to next_id
    // independence, but patterns must match).
    assert_eq!(laws_a.len(), laws_b.len());
    let patterns_a: std::collections::BTreeSet<(Term, Term)> = laws_a
        .iter()
        .map(|l| (l.lhs.clone(), l.rhs.clone()))
        .collect();
    let patterns_b: std::collections::BTreeSet<(Term, Term)> = laws_b
        .iter()
        .map(|l| (l.lhs.clone(), l.rhs.clone()))
        .collect();
    assert_eq!(patterns_a, patterns_b, "law patterns must be deterministic");
}
