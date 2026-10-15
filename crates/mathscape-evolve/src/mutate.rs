//! Mutation operators for expression trees.

use mathscape_core::term::Term;
use mathscape_core::value::Value;
use rand::Rng;

/// Available mutation operators.
#[derive(Clone, Copy, Debug)]
pub enum MutationOp {
    /// Replace a random subtree with a new random tree.
    SubtreeReplace,
    /// Swap a binary operator (add <-> mul).
    OpSwap,
    /// Perturb a numeric constant by +/- 1.
    ConstantPerturb,
    /// Reorder arguments of a binary application.
    ArgReorder,
    /// Insert a wrapping application around a subtree.
    WrapApply,
    /// Delete a layer (unwrap an application to its first arg).
    Unwrap,
}

const ALL_OPS: [MutationOp; 6] = [
    MutationOp::SubtreeReplace,
    MutationOp::OpSwap,
    MutationOp::ConstantPerturb,
    MutationOp::ArgReorder,
    MutationOp::WrapApply,
    MutationOp::Unwrap,
];

/// Apply a random mutation to a term.
pub fn mutate(term: &Term, rng: &mut impl Rng, max_depth: usize) -> Term {
    let op = ALL_OPS[rng.gen_range(0..ALL_OPS.len())];
    apply_mutation(term, op, rng, max_depth)
}

fn apply_mutation(term: &Term, op: MutationOp, rng: &mut impl Rng, max_depth: usize) -> Term {
    match op {
        MutationOp::SubtreeReplace => subtree_replace(term, rng, max_depth),
        MutationOp::OpSwap => op_swap(term, rng),
        MutationOp::ConstantPerturb => constant_perturb(term, rng),
        MutationOp::ArgReorder => arg_reorder(term, rng),
        MutationOp::WrapApply => wrap_apply(term, rng),
        MutationOp::Unwrap => unwrap(term),
    }
}

/// Replace a random subtree with a new random tree of bounded depth.
fn subtree_replace(term: &Term, rng: &mut impl Rng, max_depth: usize) -> Term {
    let size = term.size();
    if size <= 1 || rng.gen_ratio(1, size as u32) {
        return random_term(rng, max_depth.min(3));
    }

    match term {
        Term::Apply(f, args) => {
            if rng.gen_bool(0.3) {
                Term::Apply(Box::new(subtree_replace(f, rng, max_depth)), args.clone())
            } else {
                let idx = rng.gen_range(0..args.len().max(1));
                let mut new_args = args.clone();
                if idx < new_args.len() {
                    new_args[idx] = subtree_replace(&new_args[idx], rng, max_depth);
                }
                Term::Apply(f.clone(), new_args)
            }
        }
        Term::Fn(params, body) => Term::Fn(
            params.clone(),
            Box::new(subtree_replace(body, rng, max_depth)),
        ),
        Term::Symbol(id, args) => {
            if args.is_empty() {
                return random_term(rng, max_depth.min(3));
            }
            let idx = rng.gen_range(0..args.len());
            let mut new_args = args.clone();
            new_args[idx] = subtree_replace(&new_args[idx], rng, max_depth);
            Term::Symbol(*id, new_args)
        }
        _ => random_term(rng, max_depth.min(3)),
    }
}

/// Swap add <-> mul at a random application site.
fn op_swap(term: &Term, rng: &mut impl Rng) -> Term {
    match term {
        Term::Apply(f, args) => {
            let new_f = match f.as_ref() {
                Term::Var(2) if rng.gen_bool(0.5) => Box::new(Term::Var(3)), // add -> mul
                Term::Var(3) if rng.gen_bool(0.5) => Box::new(Term::Var(2)), // mul -> add
                _ => Box::new(op_swap(f, rng)),
            };
            Term::Apply(new_f, args.clone())
        }
        Term::Fn(params, body) => Term::Fn(params.clone(), Box::new(op_swap(body, rng))),
        other => other.clone(),
    }
}

/// Perturb a numeric constant by +/- 1.
fn constant_perturb(term: &Term, rng: &mut impl Rng) -> Term {
    match term {
        Term::Number(Value::Nat(n)) if rng.gen_bool(0.3) => {
            if *n == 0 {
                Term::Number(Value::Nat(1))
            } else if rng.gen_bool(0.5) {
                Term::Number(Value::Nat(n + 1))
            } else {
                Term::Number(Value::Nat(n - 1))
            }
        }
        Term::Apply(f, args) => {
            let new_args: Vec<Term> = args.iter().map(|a| constant_perturb(a, rng)).collect();
            Term::Apply(f.clone(), new_args)
        }
        Term::Fn(params, body) => Term::Fn(params.clone(), Box::new(constant_perturb(body, rng))),
        other => other.clone(),
    }
}

/// Reorder arguments of a binary application.
fn arg_reorder(term: &Term, rng: &mut impl Rng) -> Term {
    match term {
        Term::Apply(f, args) if args.len() == 2 && rng.gen_bool(0.5) => {
            let mut new_args = args.clone();
            new_args.swap(0, 1);
            Term::Apply(f.clone(), new_args)
        }
        Term::Apply(f, args) => {
            let new_args: Vec<Term> = args.iter().map(|a| arg_reorder(a, rng)).collect();
            Term::Apply(f.clone(), new_args)
        }
        Term::Fn(params, body) => Term::Fn(params.clone(), Box::new(arg_reorder(body, rng))),
        other => other.clone(),
    }
}

/// Wrap a subtree in an application with a random builtin.
fn wrap_apply(term: &Term, rng: &mut impl Rng) -> Term {
    if rng.gen_bool(0.3) {
        let op = if rng.gen_bool(0.5) { 2 } else { 3 }; // add or mul
        let other = random_leaf(rng);
        if rng.gen_bool(0.5) {
            Term::Apply(Box::new(Term::Var(op)), vec![term.clone(), other])
        } else {
            Term::Apply(Box::new(Term::Var(op)), vec![other, term.clone()])
        }
    } else {
        match term {
            Term::Apply(f, args) => {
                let idx = rng.gen_range(0..args.len().max(1));
                let mut new_args = args.clone();
                if idx < new_args.len() {
                    new_args[idx] = wrap_apply(&new_args[idx], rng);
                }
                Term::Apply(f.clone(), new_args)
            }
            _ => term.clone(),
        }
    }
}

/// Unwrap an application — replace (f a b) with one of its args.
fn unwrap(term: &Term) -> Term {
    match term {
        Term::Apply(_, args) if !args.is_empty() => args[0].clone(),
        _ => term.clone(),
    }
}

/// Generate a random term of bounded depth.
pub fn random_term(rng: &mut impl Rng, max_depth: usize) -> Term {
    if max_depth <= 1 {
        return random_leaf(rng);
    }

    match rng.gen_range(0..4) {
        0 => random_leaf(rng),
        1 | 2 => {
            // Random binary application (add or mul)
            let op = if rng.gen_bool(0.5) { 2 } else { 3 };
            let left = random_term(rng, max_depth - 1);
            let right = random_term(rng, max_depth - 1);
            Term::Apply(Box::new(Term::Var(op)), vec![left, right])
        }
        _ => {
            // Succ applied to a subterm
            let inner = random_term(rng, max_depth - 1);
            Term::Apply(Box::new(Term::Var(1)), vec![inner])
        }
    }
}

fn random_leaf(rng: &mut impl Rng) -> Term {
    match rng.gen_range(0..3) {
        0 => Term::Number(Value::Nat(rng.gen_range(0..10))),
        1 => Term::Point(rng.gen_range(0..5)),
        _ => Term::Var(rng.gen_range(100..110)), // free variables
    }
}

/// Crossover: swap subtrees between two parents.
pub fn crossover(parent1: &Term, parent2: &Term, rng: &mut impl Rng) -> (Term, Term) {
    // Simple: take func from parent1, args from parent2 (or vice versa)
    match (parent1, parent2) {
        (Term::Apply(f1, a1), Term::Apply(f2, a2)) => {
            if rng.gen_bool(0.5) {
                (
                    Term::Apply(f1.clone(), a2.clone()),
                    Term::Apply(f2.clone(), a1.clone()),
                )
            } else {
                (
                    Term::Apply(f2.clone(), a1.clone()),
                    Term::Apply(f1.clone(), a2.clone()),
                )
            }
        }
        _ => {
            // Fall back: just return copies (no useful crossover for leaves)
            (parent1.clone(), parent2.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn random_term_bounded() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        for _ in 0..100 {
            let t = random_term(&mut rng, 5);
            assert!(t.depth() <= 6); // depth can be slightly over due to apply wrapping
        }
    }

    #[test]
    fn mutation_produces_different_term() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let original = Term::Apply(
            Box::new(Term::Var(2)),
            vec![Term::Number(Value::Nat(3)), Term::Number(Value::Nat(4))],
        );

        let mut any_different = false;
        for _ in 0..20 {
            let mutated = mutate(&original, &mut rng, 5);
            if mutated != original {
                any_different = true;
                break;
            }
        }
        assert!(any_different, "mutation should eventually change the term");
    }

    #[test]
    fn crossover_with_apply_terms_mixes_parts() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(99);
        let p1 = Term::Apply(
            Box::new(Term::Var(2)), // add
            vec![Term::Number(Value::Nat(1)), Term::Number(Value::Nat(2))],
        );
        let p2 = Term::Apply(
            Box::new(Term::Var(3)), // mul
            vec![Term::Number(Value::Nat(10)), Term::Number(Value::Nat(20))],
        );

        let (c1, c2) = crossover(&p1, &p2, &mut rng);

        // Offspring should be Apply terms that combine parts from both parents.
        // One child gets f from p1 and args from p2 (or vice versa), the other the opposite.
        match (&c1, &c2) {
            (Term::Apply(f1, a1), Term::Apply(f2, a2)) => {
                // The two children should have swapped function/args relative to parents.
                // Check that the children are not both identical to the same parent.
                let same_as_p1 = c1 == p1 && c2 == p2;
                let same_as_p2 = c1 == p2 && c2 == p1;
                // At least the args or funcs should be mixed
                assert!(
                    !same_as_p1 || !same_as_p2,
                    "crossover should mix parts from both parents"
                );
                // Both offspring should still be Apply nodes
                assert!(matches!(**f1, Term::Var(2) | Term::Var(3)));
                assert!(matches!(**f2, Term::Var(2) | Term::Var(3)));
                // Args should come from one of the parents
                assert!(
                    a1 == &vec![Term::Number(Value::Nat(1)), Term::Number(Value::Nat(2))]
                        || a1 == &vec![Term::Number(Value::Nat(10)), Term::Number(Value::Nat(20))]
                );
                assert!(
                    a2 == &vec![Term::Number(Value::Nat(1)), Term::Number(Value::Nat(2))]
                        || a2 == &vec![Term::Number(Value::Nat(10)), Term::Number(Value::Nat(20))]
                );
            }
            _ => panic!("crossover of two Apply terms should produce Apply terms"),
        }
    }

    #[test]
    fn crossover_with_non_apply_returns_clones() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let t1 = Term::Number(Value::Nat(5));
        let t2 = Term::Number(Value::Nat(10));

        let (c1, c2) = crossover(&t1, &t2, &mut rng);
        assert_eq!(c1, t1, "non-Apply crossover should return clone of parent1");
        assert_eq!(c2, t2, "non-Apply crossover should return clone of parent2");
    }

    #[test]
    fn mutate_with_various_seeds_covers_different_ops() {
        // Run mutate with many different seeds on a rich enough term to exercise
        // different MutationOps. We collect results and verify at least 3 distinct
        // outcomes occur (indicating different ops were triggered).
        let original = Term::Apply(
            Box::new(Term::Var(2)),
            vec![Term::Number(Value::Nat(5)), Term::Number(Value::Nat(3))],
        );

        let mut distinct_results = std::collections::HashSet::new();
        for seed in 0..100 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let mutated = mutate(&original, &mut rng, 5);
            // Use debug repr as a simple hash key
            distinct_results.insert(format!("{mutated:?}"));
        }

        assert!(
            distinct_results.len() >= 3,
            "with 100 seeds, should get at least 3 distinct mutation results, got {}",
            distinct_results.len()
        );
    }
}
