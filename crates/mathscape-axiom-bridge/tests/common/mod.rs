//! Shared test helpers for autonomous-traversal and flex-multi-layer
//! integration tests.
//!
//! `experiment` submodule (2026-04-18): the mathscape experiment
//! harness — parameterizes apparatus-level discovery experiments
//! as data, enabling a 100+ experiment catalog to probe the
//! discovery space systematically.
//!
//! Extracted 2026-04-18 to de-duplicate ~500 lines of term builders,
//! zoo corpora, and the procedural generator across two test files.
//! Kept intentionally thin — these are pure construction helpers,
//! not pipeline wrappers. Pipeline logic (run_traversal,
//! run_ensemble_traversal, etc.) stays in the test file that tests
//! it so the test is self-describing.
//!
//! This module is `pub(crate)`-equivalent via cargo's integration-test
//! module convention: each test file can `mod common;` to pull it in.

#![allow(dead_code)]

pub mod experiment;

use mathscape_core::{term::Term, value::Value};

// ── Term builders ───────────────────────────────────────────────

pub fn apply(f: Term, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(f), args)
}

pub fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}

pub fn var(id: u32) -> Term {
    Term::Var(id)
}

// ── Canonical zoo corpora ───────────────────────────────────────
//
// Seven hand-crafted corpus shapes that together form the
// "canonical zoo" used to anchor the autonomous-traversal
// milestone. Each corpus probes a structurally-distinct dimension:
//
//   arith_right_id      add(n, 0) — right-identity on add
//   mult_right_id       mul(n, 1) — right-identity on mul
//   compositional       nested + flat add/mul identities
//   left_identity       add(0, n) and mul(1, n) — mirror forms
//   doubling            add(n, n) — same-var-twice pattern
//   successor_chain     succ(succ(...succ(n))) — unary nesting
//   cross_op            add(mul(n, 2), 0) and mul(add(n, 0), 3) —
//                       reduction-cascade shapes

pub fn arith_right_id() -> Vec<Term> {
    (1..=10).map(|n| apply(var(2), vec![nat(n), nat(0)])).collect()
}

pub fn mult_right_id() -> Vec<Term> {
    (1..=10).map(|n| apply(var(3), vec![nat(n), nat(1)])).collect()
}

pub fn compositional() -> Vec<Term> {
    let mut v = Vec::new();
    for n in 1..=6 {
        v.push(apply(var(2), vec![nat(n), nat(0)]));
        v.push(apply(var(2), vec![apply(var(2), vec![nat(n), nat(0)]), nat(0)]));
        v.push(apply(var(3), vec![nat(n), nat(1)]));
        v.push(apply(var(3), vec![apply(var(3), vec![nat(n), nat(1)]), nat(1)]));
    }
    v
}

pub fn left_identity() -> Vec<Term> {
    let mut v = Vec::new();
    for n in 1..=8 {
        v.push(apply(var(2), vec![nat(0), nat(n)]));
        v.push(apply(var(3), vec![nat(1), nat(n)]));
    }
    v
}

pub fn doubling() -> Vec<Term> {
    (1..=10).map(|n| apply(var(2), vec![nat(n), nat(n)])).collect()
}

pub fn successor_chain() -> Vec<Term> {
    let mut v = Vec::new();
    for base in 0..=3u64 {
        for depth in 1..=4usize {
            let mut t = nat(base);
            for _ in 0..depth {
                t = apply(var(4), vec![t]);
            }
            v.push(t);
        }
    }
    v
}

pub fn cross_op() -> Vec<Term> {
    let mut v = Vec::new();
    for n in 1..=6u64 {
        v.push(apply(
            var(2),
            vec![apply(var(3), vec![nat(n), nat(2)]), nat(0)],
        ));
        v.push(apply(
            var(3),
            vec![apply(var(2), vec![nat(n), nat(0)]), nat(3)],
        ));
    }
    v
}

/// The canonical zoo: all 7 hand-crafted shapes in their canonical
/// order (the order determines which corpus "anchors" saturation).
pub fn canonical_zoo() -> Vec<(String, Vec<Term>)> {
    vec![
        ("arith-right-id".into(), arith_right_id()),
        ("mult-right-id".into(), mult_right_id()),
        ("compositional".into(), compositional()),
        ("left-identity".into(), left_identity()),
        ("doubling".into(), doubling()),
        ("successor-chain".into(), successor_chain()),
        ("cross-op".into(), cross_op()),
    ]
}

// ── Procedural corpus generator ─────────────────────────────────
//
// Seeded xorshift over the operator vocabulary {add=Var(2),
// mul=Var(3), succ=Var(4)} with constants in [0, 10] and
// configurable max depth. Deterministic given `seed` — same seed
// always produces the same corpus.

pub fn procedural(seed: u64, max_depth: usize, term_count: usize) -> Vec<Term> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).max(1);
    let mut next_u64 = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let ops: [u32; 3] = [2, 3, 4];

    fn build(
        depth: usize,
        max_depth: usize,
        ops: &[u32],
        next: &mut dyn FnMut() -> u64,
    ) -> Term {
        if depth >= max_depth || next() % 3 == 0 {
            let v = (next() % 11) as u64;
            return nat(v);
        }
        let op_idx = (next() % ops.len() as u64) as usize;
        let op = ops[op_idx];
        let arity = if op == 4 { 1 } else { 2 };
        let mut args = Vec::with_capacity(arity);
        for _ in 0..arity {
            args.push(build(depth + 1, max_depth, ops, next));
        }
        apply(var(op), args)
    }

    let mut out = Vec::with_capacity(term_count);
    for _ in 0..term_count {
        out.push(build(0, max_depth, &ops, &mut next_u64));
    }
    out
}
