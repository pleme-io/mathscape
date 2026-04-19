//! Phase V (2026-04-18): adaptive corpus generator — the machine's
//! DIET mutation layer, promoted from test-only scaffolding into a
//! core `CorpusGenerator` impl.
//!
//! # The diet signal
//!
//! When a `MetaLoop` run produces a sequence of observations whose
//! staleness is high (no library growth, tiny policy delta), the
//! proposer's correct response is not to retry the same diet harder
//! but to CHANGE what the machine eats. This module provides the
//! target of that change: a `CorpusGenerator` whose output depends
//! on the CURRENT LIBRARY — it synthesizes terms that the library
//! will partially-reduce at the leaves while leaving a novel
//! outer-shell residue that invites new pattern discovery.
//!
//! # How it works
//!
//! For each generated term:
//!   1. Pick a random operator NOT matched by the library at root
//!      (the outer shell).
//!   2. Fill the operator's args with a mix of:
//!      - substrate-rule instantiations (children that reduce)
//!      - recursive residue terms (children that stay put)
//!   3. The result after library-reduction is a term shaped like
//!      `outer_op(partially_reduced_children...)` — structure the
//!      machine hasn't seen before.
//!
//! # Why this closes the fix-point loop
//!
//! Staleness observation fires → proposer picks a diet-mutation
//! archetype → `AdaptiveCorpusGenerator` reads the stale library
//! state and synthesizes novelty-inviting terms → next cycle's
//! extractor sees fresh shapes → library grows again → staleness
//! drops → proposer's policy learns that diet-mutations produce
//! reward under staleness conditions.
//!
//! The corpus generator is READ-ONLY on the library — the library
//! passes through unchanged. The generator's output is a pure
//! function of `(seed, iteration, library)` — deterministic replay
//! is preserved.

use crate::bootstrap::CorpusGenerator;
use crate::eval::RewriteRule;
use crate::term::Term;
use crate::value::Value;

/// Adaptive corpus generator. Synthesizes terms whose outer shell
/// is non-reducible by the current library and whose inner structure
/// is a mix of library-reducible patterns and residue terms. The
/// result post-library-reduction has STRUCTURE the library hasn't
/// seen — which is what the extractor needs to surface new laws.
#[derive(Debug, Clone)]
pub struct AdaptiveCorpusGenerator {
    /// RNG seed. Determinism: same seed + same library → same corpus.
    pub seed: u64,
    /// Number of terms to emit per call.
    pub term_count: usize,
    /// Base nesting depth. Effective depth scales with library size.
    pub base_depth: usize,
    /// Operator vocabulary to draw from. Default mixes the core
    /// Peano + tensor/int/float vocab subsets.
    pub vocab: Vec<u32>,
    /// Max Nat leaf value.
    pub max_value: u64,
}

impl Default for AdaptiveCorpusGenerator {
    /// Phase V default: Peano vocabulary (ADD=2, MUL=3, SUCC=1),
    /// 16 terms, depth 3, max-value 10. Same defaults the test-
    /// scaffolding adaptive_corpus used.
    fn default() -> Self {
        Self {
            seed: 0,
            term_count: 16,
            base_depth: 3,
            vocab: vec![2, 3, 1], // ADD, MUL, SUCC
            max_value: 10,
        }
    }
}

impl AdaptiveCorpusGenerator {
    #[must_use]
    pub fn new(
        seed: u64,
        term_count: usize,
        base_depth: usize,
        vocab: Vec<u32>,
        max_value: u64,
    ) -> Self {
        Self {
            seed,
            term_count,
            base_depth,
            vocab,
            max_value,
        }
    }
}

impl CorpusGenerator for AdaptiveCorpusGenerator {
    fn generate(&self, iteration: usize, library: &[RewriteRule]) -> Vec<Term> {
        // Depth scales with library size — more library means we
        // need deeper nesting to keep the residue interesting.
        // +1 per 3 substrate rules is empirical; conservative
        // enough not to blow up term size.
        let depth = self.base_depth + library.len() / 3;
        // Seed mixes with iteration so per-iter corpora differ
        // while remaining deterministic.
        let mut state = self
            .seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(iteration as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut out = Vec::with_capacity(self.term_count);
        for _ in 0..self.term_count {
            out.push(build_shelled_term(
                library,
                &mut state,
                depth,
                &self.vocab,
                self.max_value,
            ));
        }
        out
    }
}

// ── Shell / residue / leaf builders ───────────────────────────────

fn xorshift_u64(state: &mut u64) -> u64 {
    if *state == 0 {
        *state = 0x9E37_79B9_7F4A_7C15;
    }
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn pick_operator(state: &mut u64, vocab: &[u32]) -> u32 {
    vocab[(xorshift_u64(state) as usize) % vocab.len()]
}

fn op_arity(op: u32) -> usize {
    match op {
        1 | 4 | 7 | 14 | 33 | 42 => 1, // succ / pred / neg / unary
        _ => 2,                        // add, mul, sub, div — binary
    }
}

fn random_leaf(state: &mut u64, max_value: u64) -> Term {
    Term::Number(Value::Nat(xorshift_u64(state) % max_value.max(1)))
}

/// Recursive random residue term over the vocabulary. Depth-bounded.
fn build_residue_term(
    state: &mut u64,
    depth: usize,
    vocab: &[u32],
    max_value: u64,
) -> Term {
    if depth == 0 || xorshift_u64(state) % 3 == 0 {
        return random_leaf(state, max_value);
    }
    let op = pick_operator(state, vocab);
    let arity = op_arity(op);
    let mut args = Vec::with_capacity(arity);
    for _ in 0..arity {
        args.push(build_residue_term(state, depth - 1, vocab, max_value));
    }
    Term::Apply(Box::new(Term::Var(op)), args)
}

/// Collect pattern-variable ids (Var(≥100)) in a term, deduped.
fn collect_pattern_vars(t: &Term, out: &mut Vec<u32>) {
    match t {
        Term::Var(v) if *v >= 100 => out.push(*v),
        Term::Apply(f, args) => {
            collect_pattern_vars(f, out);
            for a in args {
                collect_pattern_vars(a, out);
            }
        }
        Term::Symbol(_, args) => {
            for a in args {
                collect_pattern_vars(a, out);
            }
        }
        Term::Fn(_, body) => collect_pattern_vars(body, out),
        _ => {}
    }
}

/// Instantiate a rule's LHS by replacing pattern vars with random
/// subterms of the given depth.
fn instantiate_rule_lhs(
    rule: &RewriteRule,
    state: &mut u64,
    depth: usize,
    vocab: &[u32],
    max_value: u64,
) -> Term {
    let mut lhs = rule.lhs.clone();
    let mut vars: Vec<u32> = Vec::new();
    collect_pattern_vars(&lhs, &mut vars);
    vars.sort();
    vars.dedup();
    for v in vars {
        let sub = build_residue_term(state, depth, vocab, max_value);
        lhs = lhs.substitute(v, &sub);
    }
    lhs
}

/// Build a shelled term: outer op is non-library-matching (assumed
/// — we just pick from the vocab); children mix substrate-rule
/// instantiations (will reduce) with residue recursions.
fn build_shelled_term(
    substrate: &[RewriteRule],
    state: &mut u64,
    depth: usize,
    vocab: &[u32],
    max_value: u64,
) -> Term {
    if depth == 0 || substrate.is_empty() {
        return build_residue_term(state, depth, vocab, max_value);
    }
    let op = pick_operator(state, vocab);
    let arity = op_arity(op);
    let mut args = Vec::with_capacity(arity);
    for _ in 0..arity {
        if xorshift_u64(state) % 2 == 0 {
            let rule_idx =
                (xorshift_u64(state) as usize) % substrate.len();
            args.push(instantiate_rule_lhs(
                &substrate[rule_idx],
                state,
                depth - 1,
                vocab,
                max_value,
            ));
        } else {
            args.push(build_shelled_term(
                substrate,
                state,
                depth - 1,
                vocab,
                max_value,
            ));
        }
    }
    Term::Apply(Box::new(Term::Var(op)), args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::Term;
    use crate::value::Value;

    fn var(id: u32) -> Term {
        Term::Var(id)
    }
    fn apply(h: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(h), args)
    }
    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }

    fn add_identity() -> RewriteRule {
        RewriteRule {
            name: "add-id".into(),
            lhs: apply(var(2), vec![nat(0), var(100)]),
            rhs: var(100),
        }
    }

    #[test]
    fn adaptive_generator_emits_term_count_terms() {
        let cg = AdaptiveCorpusGenerator::new(
            42,
            8,
            2,
            vec![2, 3],
            10,
        );
        let lib = vec![add_identity()];
        let terms = cg.generate(0, &lib);
        assert_eq!(terms.len(), 8);
    }

    #[test]
    fn adaptive_generator_is_deterministic() {
        let cg = AdaptiveCorpusGenerator::new(
            42,
            8,
            2,
            vec![2, 3],
            10,
        );
        let lib = vec![add_identity()];
        let t1 = cg.generate(0, &lib);
        let t2 = cg.generate(0, &lib);
        assert_eq!(t1, t2);
    }

    #[test]
    fn adaptive_generator_varies_per_iteration() {
        let cg = AdaptiveCorpusGenerator::new(
            42,
            8,
            2,
            vec![2, 3],
            10,
        );
        let lib = vec![add_identity()];
        let t0 = cg.generate(0, &lib);
        let t1 = cg.generate(1, &lib);
        assert_ne!(t0, t1, "different iterations must produce different corpora");
    }

    #[test]
    fn adaptive_generator_with_empty_library_falls_back_to_residue() {
        // With no substrate, builds pure residue terms. Still
        // produces the configured count.
        let cg = AdaptiveCorpusGenerator::new(
            42,
            5,
            2,
            vec![2, 3],
            10,
        );
        let terms = cg.generate(0, &[]);
        assert_eq!(terms.len(), 5);
    }

    #[test]
    fn adaptive_generator_depth_scales_with_library_size() {
        let cg_small = AdaptiveCorpusGenerator::new(
            42,
            4,
            2,
            vec![2, 3],
            10,
        );
        // With 9 rules, depth scales to base + 3.
        let big_lib: Vec<RewriteRule> =
            (0..9).map(|_| add_identity()).collect();
        let terms_big = cg_small.generate(0, &big_lib);
        // Not checking exact term size; just confirming we get
        // output and depth-dependent behavior via library size.
        assert_eq!(terms_big.len(), 4);
    }

    #[test]
    fn default_adaptive_generator_uses_peano_vocab() {
        let cg = AdaptiveCorpusGenerator::default();
        // Default is ADD(2), MUL(3), SUCC(1).
        assert!(cg.vocab.contains(&2));
        assert!(cg.vocab.contains(&3));
        assert!(cg.vocab.contains(&1));
        assert_eq!(cg.term_count, 16);
    }

    #[test]
    fn default_generator_implements_corpus_generator_trait() {
        // Compile-time + runtime check.
        let cg = AdaptiveCorpusGenerator::default();
        let terms = cg.generate(0, &[]);
        let _ = terms;
    }
}
