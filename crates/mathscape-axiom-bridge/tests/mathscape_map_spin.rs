//! Phase V.spin (2026-04-18): the extensive motor spin as a
//! self-refactoring Merkle-tree exercise.
//!
//! The map of mathscape is a DAG of rules identified by their
//! content hashes; each motor phase is a mutation on that tree.
//! Running the motor across many seeds produces many trajectories;
//! the UNION of discovered rules is the total mapped territory,
//! and the CORE (rules in every run's final library) is the
//! invariant mathematics — what every seed finds.
//!
//! This test executes the loop the user framed:
//!
//!   1. Run the motor on multiple fresh seeds (different starting
//!      trajectories through mathscape).
//!   2. Accumulate snapshots into a `MathscapeMap`.
//!   3. Extract `core_rules` — the invariant mathematics.
//!   4. Run ONE MORE motor session with core_rules as its
//!      seed_library — the map's "candidate for efficiency" is
//!      fed back to the primary algorithm.
//!   5. Compare: does the core-seeded run reach further than the
//!      fresh-seeded runs did?
//!
//! If (5) is yes, the map has genuinely functioned as an
//! efficiency channel — the accumulated invariant mathematics
//! extends what the next run can reach. This is the
//! "map refactors down to its core and presents the core back
//! to the primary algorithm" mechanism landed.

mod common;

use mathscape_compress::derive_laws_from_corpus_instrumented;
use mathscape_core::{
    bootstrap::{
        BootstrapCycle, BootstrapCycleSpec, CanonicalDeduper,
        DefaultCorpusGenerator, DefaultModelUpdater, ExperimentOutcome,
        ExperimentScenario, LawExtractor, PhaseOutcome, SpecExecutionError,
    },
    eval::RewriteRule,
    hash::TermRef,
    mathscape_map::{MapSnapshot, MathscapeMap},
    meta_loop::{
        HeuristicProposer, MetaLoop, MetaLoopConfig, ScenarioExecutor,
    },
    policy::LinearPolicy,
    term::Term,
    AdaptiveCorpusGenerator,
};
use std::cell::RefCell;

// ── Motor executor with seed parameter ─────────────────────────

struct SeededMotorExecutor {
    seed: u64,
}

impl SeededMotorExecutor {
    fn new(seed: u64) -> Self {
        Self { seed }
    }
}

impl ScenarioExecutor for SeededMotorExecutor {
    fn execute(
        &self,
        scenario: &ExperimentScenario,
    ) -> Result<ExperimentOutcome, SpecExecutionError> {
        let mut phases = Vec::new();
        let mut carry_lib = scenario
            .phases
            .first()
            .map(|p| p.seed_library.clone())
            .unwrap_or_default();
        let mut carry_pol = scenario
            .phases
            .first()
            .map(|p| p.seed_policy.clone())
            .unwrap_or_else(LinearPolicy::tensor_seeking_prior);
        for (idx, base_spec) in scenario.phases.iter().enumerate() {
            let mut spec = base_spec.clone();
            if idx > 0 {
                spec.seed_library = carry_lib.clone();
                spec.seed_policy = carry_pol.clone();
            }
            let outcome = run_spec_seeded(&spec, self.seed)?;
            carry_lib = outcome.final_library.clone();
            carry_pol = outcome.final_policy.clone();
            phases.push(PhaseOutcome {
                phase_index: idx,
                spec_used: spec,
                cycle_outcome: outcome,
            });
        }
        let concat: Vec<u8> = phases
            .iter()
            .flat_map(|p| p.cycle_outcome.attestation.as_bytes().to_vec())
            .collect();
        Ok(ExperimentOutcome {
            phases,
            chain_attestation: TermRef::from_bytes(&concat),
            scenario_total_ns: 0,
        })
    }
}

struct DerivedLawsExtractor {
    step_limit: usize,
    min_support: usize,
    _stats: RefCell<Vec<mathscape_compress::LawGenStats>>,
}

impl DerivedLawsExtractor {
    fn new(step_limit: usize, min_support: usize) -> Self {
        Self {
            step_limit,
            min_support,
            _stats: RefCell::new(Vec::new()),
        }
    }
}

impl LawExtractor for DerivedLawsExtractor {
    fn extract(
        &self,
        corpus: &[Term],
        library: &[RewriteRule],
    ) -> Vec<RewriteRule> {
        let mut next_id: mathscape_core::term::SymbolId =
            (library.len() + 1) as u32;
        let (laws, stats) = derive_laws_from_corpus_instrumented(
            corpus,
            library,
            self.step_limit,
            self.min_support,
            &mut next_id,
        );
        self._stats.borrow_mut().push(stats);
        laws
    }
}

fn run_spec_seeded(
    spec: &BootstrapCycleSpec,
    seed: u64,
) -> Result<mathscape_core::bootstrap::BootstrapOutcome, SpecExecutionError> {
    let n = spec.n_iterations;
    let extractor = DerivedLawsExtractor::new(300, 2);
    let seed_lib = spec.seed_library.clone();
    let seed_pol = spec.seed_policy.clone();

    let outcome = match spec.corpus_generator.as_str() {
        "default" => {
            let cycle = BootstrapCycle::new(
                DefaultCorpusGenerator,
                extractor,
                DefaultModelUpdater::default(),
                n,
            );
            if let Some(w) = spec.early_stop_after_stable {
                cycle.run_until_stable(seed_lib, seed_pol, &CanonicalDeduper, w)
            } else {
                cycle.run_with_dedup(seed_lib, seed_pol, &CanonicalDeduper)
            }
        }
        "adaptive" => {
            let corpus = AdaptiveCorpusGenerator {
                seed, // Phase V.spin: seeded per-run
                ..AdaptiveCorpusGenerator::default()
            };
            let cycle = BootstrapCycle::new(
                corpus,
                extractor,
                DefaultModelUpdater::default(),
                n,
            );
            if let Some(w) = spec.early_stop_after_stable {
                cycle.run_until_stable(seed_lib, seed_pol, &CanonicalDeduper, w)
            } else {
                cycle.run_with_dedup(seed_lib, seed_pol, &CanonicalDeduper)
            }
        }
        _ => {
            let mini = ExperimentScenario {
                name: "fallback".into(),
                phases: vec![spec.clone()],
            };
            let inner = mathscape_core::bootstrap::execute_scenario_core(&mini)?;
            return Ok(inner.phases.into_iter().next().unwrap().cycle_outcome);
        }
    };
    Ok(outcome)
}

fn build_seed_scenario(
    seed_library: Vec<RewriteRule>,
) -> ExperimentScenario {
    let seed_spec = BootstrapCycleSpec {
        corpus_generator: "default".into(),
        law_extractor: "derived-laws".into(),
        model_updater: "default".into(),
        deduper: "canonical".into(),
        n_iterations: 5,
        seed_library,
        seed_policy: LinearPolicy::tensor_seeking_prior(),
        early_stop_after_stable: Some(1),
    };
    ExperimentScenario {
        name: "spin-seed".into(),
        phases: vec![seed_spec],
    }
}

fn run_motor_for_seed(
    seed: u64,
    seed_library: Vec<RewriteRule>,
    max_phases: usize,
) -> mathscape_core::meta_loop::MetaLoopOutcome {
    let loop_ = MetaLoop::new(
        SeededMotorExecutor::new(seed),
        HeuristicProposer::with_extractor("derived-laws"),
        MetaLoopConfig {
            max_phases,
            sail_out_window: 0,
            policy_delta_threshold: 1e-9,
        },
    );
    let seed_scenario = build_seed_scenario(seed_library);
    loop_.run(seed_scenario).expect("motor runs")
}

fn outcome_to_snapshots(
    seed: u64,
    outcome: &mathscape_core::meta_loop::MetaLoopOutcome,
) -> Vec<MapSnapshot> {
    outcome
        .history
        .iter()
        .map(|record| {
            MapSnapshot::new(
                seed,
                record.phase_index,
                record.outcome.final_library().to_vec(),
                Some(record.observation.clone()),
            )
        })
        .collect()
}

#[test]
fn big_spin_builds_mathscape_map_across_seeds() {
    // ════════════════════════════════════════════════════════════════
    // Phase 1: run the motor with fresh seeds (no prior core). Each
    // run traces its own trajectory through mathscape.
    // ════════════════════════════════════════════════════════════════
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ PHASE V.SPIN — THE MATHSCAPE MAP AS MERKLE TREE       ║");
    println!("╚══════════════════════════════════════════════════════╝");

    let fresh_seeds: [u64; 4] = [1, 7, 42, 99];
    let max_phases = 8;
    let mut map = MathscapeMap::new();
    println!("\n── Phase A: fresh runs on {} seeds, {} phases each",
        fresh_seeds.len(), max_phases);
    for seed in fresh_seeds {
        let outcome = run_motor_for_seed(seed, Vec::new(), max_phases);
        let snaps = outcome_to_snapshots(seed, &outcome);
        let final_size = snaps.last().map(|s| s.size()).unwrap_or(0);
        let diet_phases = outcome
            .history
            .iter()
            .filter(|r| r.scenario.name.contains("adaptive-diet"))
            .count();
        println!(
            "  seed {:>3}: {:>2} phases → final library {:>2} rules, {:>2} diet mutations",
            seed,
            outcome.history.len(),
            final_size,
            diet_phases,
        );
        map.snapshots.extend(snaps);
    }

    // ════════════════════════════════════════════════════════════════
    // Phase 2: refactor the Merkle tree. Extract the CORE — rules
    // present in every seed's final. This is the invariant
    // mathematics of the current motor configuration.
    // ════════════════════════════════════════════════════════════════
    let summary_before = map.summary();
    let core = map.core_rules();
    let union = map.union_rules();
    println!("\n── Phase B: refactor the map");
    println!("  snapshots across all seeds : {}", summary_before.total_snapshots);
    println!("  distinct library roots     : {}", summary_before.unique_roots);
    println!("  mutation edges (root-swaps): {}", summary_before.mutation_edges);
    println!("  UNION rules (all seeds)    : {}", union.len());
    println!("  CORE  rules (every seed)   : {}", core.len());
    println!();
    println!("  core rules (invariant mathematics):");
    for rule in &core {
        println!("    {} :: {} => {}", rule.name, rule.lhs, rule.rhs);
    }

    // ════════════════════════════════════════════════════════════════
    // Phase 3: present the core to the primary algorithm as a
    // candidate for efficiency. Run one more seed's motor WITH
    // core_rules pre-loaded as seed_library.
    // ════════════════════════════════════════════════════════════════
    let core_seed: u64 = 777;
    println!(
        "\n── Phase C: core-seeded run on seed {} (seed_library = core of {} rules)",
        core_seed, core.len()
    );
    let core_outcome =
        run_motor_for_seed(core_seed, core.clone(), max_phases);
    let core_snaps = outcome_to_snapshots(core_seed, &core_outcome);
    let core_final_size =
        core_snaps.last().map(|s| s.size()).unwrap_or(0);
    let core_diet_phases = core_outcome
        .history
        .iter()
        .filter(|r| r.scenario.name.contains("adaptive-diet"))
        .count();
    println!(
        "  core-seeded: {:>2} phases → final library {:>2} rules, {:>2} diet mutations",
        core_outcome.history.len(),
        core_final_size,
        core_diet_phases,
    );

    // Fresh comparison: run the same seed 777 WITHOUT the core to
    // make the comparison fair.
    println!(
        "\n── Phase D: same seed {} but FRESH (no core seed) — comparator",
        core_seed
    );
    let fresh_outcome = run_motor_for_seed(core_seed, Vec::new(), max_phases);
    let fresh_snaps = outcome_to_snapshots(core_seed, &fresh_outcome);
    let fresh_final_size =
        fresh_snaps.last().map(|s| s.size()).unwrap_or(0);
    let fresh_diet_phases = fresh_outcome
        .history
        .iter()
        .filter(|r| r.scenario.name.contains("adaptive-diet"))
        .count();
    println!(
        "  fresh (no core): {:>2} phases → final library {:>2} rules, {:>2} diet mutations",
        fresh_outcome.history.len(),
        fresh_final_size,
        fresh_diet_phases,
    );

    // ════════════════════════════════════════════════════════════════
    // Phase 4: measurement — did the core-seeded run reach further?
    // ════════════════════════════════════════════════════════════════
    let efficiency_delta =
        core_final_size as i64 - fresh_final_size as i64;
    println!("\n── Phase E: efficiency measurement");
    println!("  core-seeded final size: {}", core_final_size);
    println!("  fresh    final size  : {}", fresh_final_size);
    println!("  Δ (core − fresh)     : {efficiency_delta:+}");
    if efficiency_delta > 0 {
        println!("  ✓ core-seeded run reached FURTHER — map-as-efficiency works");
    } else if efficiency_delta == 0 {
        println!("  ≈ equivalent — core seeding neither helped nor hurt on this corpus");
    } else {
        println!("  ✗ core-seeded run UNDER-reached — surprising; investigate");
    }

    // ════════════════════════════════════════════════════════════════
    // Phase 5: the unified map, including the core-seeded run.
    // ════════════════════════════════════════════════════════════════
    map.snapshots.extend(core_snaps);
    let final_summary = map.summary();
    println!("\n── Final unified map (all {} runs)", fresh_seeds.len() + 1);
    println!("  snapshots     : {}", final_summary.total_snapshots);
    println!("  unique roots  : {}", final_summary.unique_roots);
    println!("  seeds         : {}", final_summary.seeds);
    println!("  union rules   : {}", final_summary.union_rule_count);
    println!("  core rules    : {}", final_summary.core_rule_count);
    println!("  mutation edges: {}", final_summary.mutation_edges);

    // ════════════════════════════════════════════════════════════════
    // Invariants
    // ════════════════════════════════════════════════════════════════
    assert!(
        final_summary.total_snapshots >= fresh_seeds.len(),
        "at least one snapshot per seed"
    );
    assert!(
        final_summary.seeds >= fresh_seeds.len(),
        "≥{} distinct seeds recorded",
        fresh_seeds.len()
    );
    assert!(
        final_summary.union_rule_count >= core.len(),
        "union ≥ core (by definition)"
    );
    assert!(
        final_summary.core_rule_count <= final_summary.union_rule_count,
        "core ⊆ union"
    );

    println!("\n  ══ Map built. Merkle tree tracked. Core refactored. ══");
}

#[test]
fn mathscape_map_is_deterministic_across_replays() {
    // Same seed sequence + same (no) initial library → identical map.
    let seeds: [u64; 3] = [1, 2, 3];
    let build_map = || {
        let mut m = MathscapeMap::new();
        for seed in seeds {
            let outcome = run_motor_for_seed(seed, Vec::new(), 4);
            m.snapshots.extend(outcome_to_snapshots(seed, &outcome));
        }
        m
    };
    let a = build_map();
    let b = build_map();
    let roots_a: std::collections::BTreeSet<_> = a.unique_roots();
    let roots_b: std::collections::BTreeSet<_> = b.unique_roots();
    assert_eq!(
        roots_a, roots_b,
        "two replays of the same seed set must visit the same library roots"
    );
}
