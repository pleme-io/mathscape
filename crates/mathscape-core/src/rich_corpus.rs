//! Phase Z.8 (2026-04-19): RichCorpusGenerator — much wider
//! shape diversity for the law extractor.
//!
//! # Why
//!
//! The `DefaultCorpusGenerator` feeds the motor only
//! LEFT-oriented identity positions: `tensor_add(zeros, op)`,
//! `tensor_mul(ones, op)`. Anti-unification extracts patterns
//! from what it sees; left-only corpus → left-only rules.
//!
//! Phase Z.6/Z.7 surfaced the resulting gap empirically: the
//! `right-identity` subdomain scored 1/5 after 40 cycles. The
//! motor literally never observed `add(?x, 0)` shapes.
//!
//! # What
//!
//! `RichCorpusGenerator` emits examples covering:
//!
//! 1. **Left-oriented identities** (compat with the default):
//!    `add(0, x)`, `mul(1, x)`, `tensor_add(zeros, x)`,
//!    `tensor_mul(ones, x)`.
//!
//! 2. **RIGHT-oriented identities** (new): `add(x, 0)`,
//!    `mul(x, 1)`, `tensor_add(x, zeros)`, `tensor_mul(x, ones)`.
//!    Same mathematical law, different syntactic pattern — the
//!    thing the default corpus never shows.
//!
//! 3. **Commutativity-inducing pairs**: the SAME computation
//!    written two ways. Both orderings present in the corpus
//!    lets the extractor notice the structural equivalence.
//!
//! 4. **Int + Nat variants** in addition to tensor forms —
//!    broader operator coverage.
//!
//! 5. **Mixed-orientation compositions** at later iterations —
//!    `add(0, add(x, 0))`, `mul(x, mul(1, y))`, etc.
//!
//! # How it plugs in
//!
//! Implements the existing `CorpusGenerator` trait — slot
//! into any `BootstrapCycle` or meta-loop scenario via
//! `corpus_generator: "rich"`. A registry-side mapping can
//! expose this name (see `execute_spec_core` if using
//! spec-driven scenarios).

use crate::bootstrap::CorpusGenerator;
use crate::builtin::{ADD, MUL, TENSOR_ADD, TENSOR_MUL};
use crate::eval::RewriteRule;
use crate::term::Term;
use crate::value::Value;

/// A much-wider corpus generator. See module docs for shape
/// strategy.
#[derive(Debug, Clone, Default)]
pub struct RichCorpusGenerator;

impl CorpusGenerator for RichCorpusGenerator {
    fn generate(&self, iteration: usize, _library: &[RewriteRule]) -> Vec<Term> {
        let apply = |h: u32, args: Vec<Term>| -> Term {
            Term::Apply(Box::new(Term::Var(h)), args)
        };
        let nat = |n: u64| Term::Number(Value::Nat(n));
        let tensor = |shape: Vec<usize>, data: Vec<i64>| {
            Term::Number(Value::tensor(shape, data).unwrap())
        };
        let zeros = tensor(vec![2], vec![0, 0]);
        let ones = tensor(vec![2], vec![1, 1]);

        // Operand diversity: nat operands + tensor operands.
        let nat_ops: Vec<Term> = (2u64..=9).map(nat).collect();
        let tensor_ops: Vec<Term> = (2..=9)
            .map(|k| tensor(vec![2], vec![k as i64, (k + 1) as i64]))
            .collect();

        let mut corpus: Vec<Term> = Vec::new();

        match iteration {
            0 => {
                // ── Tier 0: all four identity orientations at
                //    both concrete orderings, across add + mul
                //    + tensor_add + tensor_mul.

                // Nat add: left + right orientations.
                for op in &nat_ops {
                    corpus.push(apply(ADD, vec![nat(0), op.clone()]));
                    corpus.push(apply(ADD, vec![op.clone(), nat(0)])); // RIGHT
                }
                // Nat mul: left + right.
                for op in &nat_ops {
                    corpus.push(apply(MUL, vec![nat(1), op.clone()]));
                    corpus.push(apply(MUL, vec![op.clone(), nat(1)])); // RIGHT
                }
                // Tensor add: left + right.
                for op in &tensor_ops {
                    corpus.push(apply(TENSOR_ADD, vec![zeros.clone(), op.clone()]));
                    corpus.push(apply(TENSOR_ADD, vec![op.clone(), zeros.clone()])); // RIGHT
                }
                // Tensor mul: left + right.
                for op in &tensor_ops {
                    corpus.push(apply(TENSOR_MUL, vec![ones.clone(), op.clone()]));
                    corpus.push(apply(TENSOR_MUL, vec![op.clone(), ones.clone()])); // RIGHT
                }
            }
            1 => {
                // ── Tier 1: commutativity-inducing pairs.
                //    Both orderings of concrete add/mul so the
                //    extractor sees `add(a, b)` AND `add(b, a)`
                //    produce the same structural value.
                for a in 1u64..=5 {
                    for b in 1u64..=5 {
                        if a == b {
                            continue;
                        }
                        corpus.push(apply(ADD, vec![nat(a), nat(b)]));
                        corpus.push(apply(ADD, vec![nat(b), nat(a)]));
                        corpus.push(apply(MUL, vec![nat(a), nat(b)]));
                        corpus.push(apply(MUL, vec![nat(b), nat(a)]));
                    }
                }
            }
            2 => {
                // ── Tier 2: mixed-orientation nested compositions.
                //    Exercise the extractor on BOTH orientations
                //    composed at depth.
                for op in &nat_ops {
                    let l_id = apply(ADD, vec![nat(0), op.clone()]);
                    let r_id = apply(ADD, vec![op.clone(), nat(0)]);
                    corpus.push(apply(ADD, vec![nat(0), l_id.clone()]));
                    corpus.push(apply(ADD, vec![r_id.clone(), nat(0)]));
                    corpus.push(apply(ADD, vec![l_id.clone(), nat(0)])); // cross
                    corpus.push(apply(ADD, vec![nat(0), r_id.clone()])); // cross

                    let l_mul = apply(MUL, vec![nat(1), op.clone()]);
                    let r_mul = apply(MUL, vec![op.clone(), nat(1)]);
                    corpus.push(apply(MUL, vec![nat(1), l_mul.clone()]));
                    corpus.push(apply(MUL, vec![r_mul.clone(), nat(1)]));
                }
                for op in &tensor_ops {
                    let l_tid = apply(TENSOR_ADD, vec![zeros.clone(), op.clone()]);
                    let r_tid = apply(TENSOR_ADD, vec![op.clone(), zeros.clone()]);
                    corpus.push(apply(TENSOR_ADD, vec![zeros.clone(), l_tid]));
                    corpus.push(apply(TENSOR_ADD, vec![r_tid, zeros.clone()]));
                }
            }
            _ => {
                // ── Tier 3+: cross-operator compositions mixing
                //    add and mul with both orientations.
                for op in &nat_ops {
                    // mul(1, add(0, op)) and add(0, mul(1, op))
                    corpus.push(apply(
                        MUL,
                        vec![nat(1), apply(ADD, vec![nat(0), op.clone()])],
                    ));
                    corpus.push(apply(
                        ADD,
                        vec![nat(0), apply(MUL, vec![nat(1), op.clone()])],
                    ));
                    // Right-oriented variants of same.
                    corpus.push(apply(
                        MUL,
                        vec![apply(ADD, vec![op.clone(), nat(0)]), nat(1)],
                    ));
                    corpus.push(apply(
                        ADD,
                        vec![apply(MUL, vec![op.clone(), nat(1)]), nat(0)],
                    ));
                }
            }
        }
        corpus
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iter_0_includes_both_orientations() {
        let g = RichCorpusGenerator;
        let corpus = g.generate(0, &[]);

        // Confirm the corpus contains ADD applied with nat(0) at
        // position 0 AND with nat(0) at position 1.
        let has_left_zero = corpus.iter().any(|t| {
            if let Term::Apply(h, args) = t {
                matches!(&**h, Term::Var(ADD))
                    && args.len() == 2
                    && matches!(&args[0], Term::Number(Value::Nat(0)))
            } else {
                false
            }
        });
        let has_right_zero = corpus.iter().any(|t| {
            if let Term::Apply(h, args) = t {
                matches!(&**h, Term::Var(ADD))
                    && args.len() == 2
                    && matches!(&args[1], Term::Number(Value::Nat(0)))
                    && !matches!(&args[0], Term::Number(Value::Nat(0)))
            } else {
                false
            }
        });

        assert!(has_left_zero, "left-oriented add(0, _) present");
        assert!(has_right_zero, "right-oriented add(_, 0) present");
    }

    #[test]
    fn iter_0_covers_all_four_identity_operators() {
        let g = RichCorpusGenerator;
        let corpus = g.generate(0, &[]);
        let head_ids: std::collections::BTreeSet<u32> = corpus
            .iter()
            .filter_map(|t| {
                if let Term::Apply(h, _) = t {
                    if let Term::Var(id) = &**h {
                        Some(*id)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        assert!(head_ids.contains(&ADD), "ADD present");
        assert!(head_ids.contains(&MUL), "MUL present");
        assert!(head_ids.contains(&TENSOR_ADD), "TENSOR_ADD present");
        assert!(head_ids.contains(&TENSOR_MUL), "TENSOR_MUL present");
    }

    #[test]
    fn iter_1_emits_commutativity_pairs() {
        let g = RichCorpusGenerator;
        let corpus = g.generate(1, &[]);
        // add(2, 3) AND add(3, 2) should both appear.
        let has_ab = corpus.iter().any(|t| {
            if let Term::Apply(h, args) = t {
                matches!(&**h, Term::Var(ADD))
                    && matches!(&args[0], Term::Number(Value::Nat(2)))
                    && matches!(&args[1], Term::Number(Value::Nat(3)))
            } else {
                false
            }
        });
        let has_ba = corpus.iter().any(|t| {
            if let Term::Apply(h, args) = t {
                matches!(&**h, Term::Var(ADD))
                    && matches!(&args[0], Term::Number(Value::Nat(3)))
                    && matches!(&args[1], Term::Number(Value::Nat(2)))
            } else {
                false
            }
        });
        assert!(has_ab && has_ba, "commutativity pair present");
    }

    #[test]
    fn corpus_size_grows_reasonably() {
        let g = RichCorpusGenerator;
        let i0 = g.generate(0, &[]).len();
        let i1 = g.generate(1, &[]).len();
        let i2 = g.generate(2, &[]).len();
        // Should be substantially more than the default's ~16.
        assert!(i0 > 30, "tier 0 has broad coverage (got {i0})");
        assert!(i1 > 30, "tier 1 has commutativity pairs (got {i1})");
        assert!(i2 > 30, "tier 2 has compositions (got {i2})");
    }
}
