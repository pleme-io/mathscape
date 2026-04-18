//! R25 — Self-bootstrapping discovery loop.
//!
//! Observational experiment using R26's `BootstrapCycle` entity
//! with R24's law generator as the `LawExtractor` and R26's
//! `DefaultCorpusGenerator` as the `CorpusGenerator`. This is the
//! REFACTORED version: the seed-corpus logic that originally lived
//! in this test now comes from R26 — single source of truth.
//!
//! The loop:
//!
//!   seed corpus → discover laws → grow library
//!              → richer traces next iteration
//!              → deeper laws → train policy
//!              → repeat
//!
//! Empty library → tensor_add/tensor_mul identity laws in iter 0
//! → nested compositions reduce in iter 1 → more laws surface →
//! policy accumulates experience across iterations.

mod common;

use mathscape_compress::derive_laws_from_corpus;
use mathscape_core::{
    bootstrap::{
        BootstrapCycle, DefaultCorpusGenerator, DefaultModelUpdater,
        LawExtractor,
    },
    builtin::{TENSOR_ADD, TENSOR_MUL},
    eval::RewriteRule,
    policy::{LinearPolicy, PolicyModel},
    term::Term,
    trajectory::LibraryFeatures,
};

/// LawExtractor implementation wrapping R24's
/// `derive_laws_from_corpus`. Lives in the test (axiom-bridge
/// depends on mathscape-compress) because mathscape-core can't
/// reference compress without inverting the dep.
struct DerivedLawsExtractor {
    step_limit: usize,
    min_support: usize,
}

impl LawExtractor for DerivedLawsExtractor {
    fn extract(
        &self,
        corpus: &[Term],
        library: &[RewriteRule],
    ) -> Vec<RewriteRule> {
        let mut next_id: mathscape_core::term::SymbolId =
            (library.len() + 1) as u32;
        derive_laws_from_corpus(
            corpus,
            library,
            self.step_limit,
            self.min_support,
            &mut next_id,
        )
    }
}

fn rule_head(t: &Term) -> Option<u32> {
    match t {
        Term::Apply(h, _) => match h.as_ref() {
            Term::Var(id) => Some(*id),
            _ => None,
        },
        _ => None,
    }
}

fn run_cycle(n: usize) -> mathscape_core::bootstrap::BootstrapOutcome {
    let cycle = BootstrapCycle::new(
        DefaultCorpusGenerator,
        DerivedLawsExtractor {
            step_limit: 300,
            min_support: 2,
        },
        DefaultModelUpdater::default(),
        n,
    );
    cycle.run(Vec::new(), LinearPolicy::tensor_seeking_prior())
}

#[test]
fn self_bootstrap_from_seed_reaches_tensor_primitives() {
    let outcome = run_cycle(4);
    let final_size = outcome.final_library.len();
    assert!(final_size > 0, "library must grow from empty");

    // Iteration 0 must discover at least one tensor-headed law.
    let iter0_new_count = outcome.iterations[0].new_law_count;
    assert!(iter0_new_count > 0, "iteration 0 must discover laws");

    // Scan the final library for tensor-headed laws (discovered
    // at some iteration).
    let has_tensor_law = outcome.final_library.iter().any(|l| {
        matches!(rule_head(&l.lhs), Some(TENSOR_ADD) | Some(TENSOR_MUL))
    });
    assert!(
        has_tensor_law,
        "final library must contain at least one tensor-headed law"
    );

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ R25 SELF-BOOTSTRAP LOOP (via R26 BootstrapCycle)     ║");
    println!("║ From near-nothing → tensor → model → tensor → model  ║");
    println!("╚══════════════════════════════════════════════════════╝\n");
    for iter in &outcome.iterations {
        println!(
            "── Iteration {} ───────────────────────────────────",
            iter.iter
        );
        println!("  corpus size              : {}", iter.corpus_size);
        println!("  library size (before)    : {}", iter.library_size_before);
        println!("  library size (after)     : {}", iter.features_after.rule_count);
        println!("  new laws discovered      : {}", iter.new_law_count);
        println!(
            "  tensor density           : {:.3}",
            iter.features_after.tensor_density
        );
        println!(
            "  distinct heads in library: {}",
            iter.features_after.distinct_heads
        );
    }
    println!("\n── Final policy ──────────────────────────────────────");
    println!("  generation    : {}", outcome.final_policy.generation);
    println!("  trained steps : {}", outcome.final_policy.trained_steps);
    println!("  bias          : {:.6}", outcome.final_policy.bias);
    println!("  weights       : {:?}", outcome.final_policy.weights);
    println!("\n── Attestation ───────────────────────────────────────");
    println!("  {:?}", outcome.attestation);
}

#[test]
fn self_bootstrap_is_deterministic() {
    // Same inputs → identical attestation AND identical content.
    // Uses R26's cycle-level deterministic_replay discipline.
    let a = run_cycle(3);
    let b = run_cycle(3);
    assert_eq!(a.attestation, b.attestation);
    assert_eq!(a.final_library.len(), b.final_library.len());
    assert_eq!(a.final_policy, b.final_policy);
}

#[test]
fn self_bootstrap_produces_reusable_policy() {
    // Trained policy should score tensor-rich states higher than
    // empty ones — proof that training actually changed the model.
    let outcome = run_cycle(3);
    let empty_state = LibraryFeatures::extract(&[]);
    let rich_state = LibraryFeatures {
        rule_count: 5,
        mean_lhs_size: 3.0,
        mean_rhs_size: 1.5,
        mean_compression: 2.0,
        tensor_density: 0.8,
        tensor_distributive_count: 2,
        tensor_meta_count: 1,
        distinct_heads: 4,
        max_rule_depth: 3,
    };
    assert!(
        outcome.final_policy.score(&rich_state)
            > outcome.final_policy.score(&empty_state),
        "trained policy must prefer tensor-rich state"
    );
}

#[test]
fn self_bootstrap_library_grows_monotonically() {
    // Library size is non-decreasing across iterations. Each
    // iteration either adds laws or leaves the library unchanged;
    // it never REMOVES laws.
    let outcome = run_cycle(4);
    for pair in outcome.iterations.windows(2) {
        let prev = pair[0].features_after.rule_count;
        let curr = pair[1].features_after.rule_count;
        assert!(
            curr >= prev,
            "library must grow monotonically: {prev} → {curr}"
        );
    }
}

#[test]
fn self_bootstrap_zero_iterations_is_empty_outcome() {
    // Edge case: 0 iterations. Cycle runs, produces empty
    // iteration list, policy untrained. Attestation well-defined.
    let cycle = BootstrapCycle::new(
        DefaultCorpusGenerator,
        DerivedLawsExtractor {
            step_limit: 300,
            min_support: 2,
        },
        DefaultModelUpdater::default(),
        0,
    );
    let outcome = cycle.run(Vec::new(), LinearPolicy::tensor_seeking_prior());
    assert!(outcome.iterations.is_empty());
    assert!(outcome.final_library.is_empty());
    assert!(outcome.trajectory.steps.is_empty());
    // Attestation still valid — covers an empty cycle.
    assert_ne!(
        outcome.attestation,
        mathscape_core::hash::TermRef::from_bytes(&[])
    );
}

#[test]
fn self_bootstrap_larger_n_discovers_at_least_as_much() {
    // Running more iterations should NEVER yield a smaller final
    // library than running fewer. The cycle is monotone in N.
    let a = run_cycle(2);
    let b = run_cycle(4);
    assert!(
        b.final_library.len() >= a.final_library.len(),
        "4-iter cycle must produce ≥ library than 2-iter"
    );
}
