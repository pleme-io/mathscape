//! Diagnostic: extract_rules produces 3 rules for the mixed corpus,
//! but the flex run shows library size 1. Where does the collapse
//! happen?

use mathscape_compress::{extract::ExtractConfig, CompressionGenerator};
use mathscape_core::{
    control::{Allocator, EpochAction, RealizationPolicy, RewardEstimator},
    epoch::{Epoch, Generator, InMemoryRegistry, Registry, RuleEmitter},
    term::Term,
    value::Value,
};
use mathscape_reward::{reward::RewardConfig, StatisticalProver};

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
        v.push(apply(var(2), vec![nat(n), nat(0)]));
        v.push(apply(var(3), vec![nat(n), nat(1)]));
    }
    v
}

#[test]
fn trace_mixed_corpus_through_generator_to_registry() {
    let mut g = CompressionGenerator::new(
        ExtractConfig {
            min_shared_size: 2,
            min_matches: 2,
            max_new_rules: 3,
        },
        1,
    );
    let corpus = mixed_corpus();

    // Step 1: What does the generator emit as candidates?
    let candidates = g.propose(0, &corpus, &[]);
    println!(
        "\n▶ Generator.propose() returned {} candidates",
        candidates.len()
    );
    for (i, c) in candidates.iter().enumerate() {
        println!("  [{i}] {}: {} => {}", c.rule.name, c.rule.lhs, c.rule.rhs);
    }

    // Step 2: Run one Discover epoch with real prover + registry.
    let g2 = CompressionGenerator::new(
        ExtractConfig {
            min_shared_size: 2,
            min_matches: 2,
            max_new_rules: 3,
        },
        1,
    );
    let mut epoch = Epoch::new(
        g2,
        StatisticalProver::new(RewardConfig::default(), 0.0),
        RuleEmitter,
        InMemoryRegistry::new(),
    );
    let trace = epoch.step_with_action(&corpus, EpochAction::Discover);
    println!(
        "\n▶ After one Discover epoch: registry.len() = {}, trace.accepted = {}, trace.rejected = {}",
        epoch.registry.len(),
        trace.accepted,
        trace.rejected,
    );
    for (i, a) in epoch.registry.all().iter().enumerate() {
        println!("  [{i}] {}: {} => {}", a.rule.name, a.rule.lhs, a.rule.rhs);
    }

    // Step 3: Run a second Discover epoch to see if the library grows further.
    let trace2 = epoch.step_with_action(&corpus, EpochAction::Discover);
    println!(
        "\n▶ After second Discover: registry.len() = {}, accepted = {}, rejected = {}",
        epoch.registry.len(),
        trace2.accepted,
        trace2.rejected,
    );

    // Step 4: What's step_auto's choice look like?
    let mut alloc = Allocator::new(
        RealizationPolicy::default(),
        RewardEstimator::new(0.3),
    );
    for i in 0..5 {
        let trace = epoch.step_auto(&corpus, &mut alloc);
        println!(
            "  step_auto #{i}: action={:?}, events={}, registry={}",
            trace.action,
            trace.events.len(),
            epoch.registry.len(),
        );
    }
}
