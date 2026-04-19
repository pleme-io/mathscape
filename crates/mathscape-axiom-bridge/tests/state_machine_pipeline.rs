//! Phase V.pipeline (2026-04-18): the end-to-end state machine.
//!
//! Candidate → Validated → ProvisionalCore → Certified → Canonical
//!
//! Demonstrates the full pipeline:
//!
//!   1. Motor runs produce VALIDATED rules (Phase J K=8 embedded)
//!   2. Map collects them; those in every seed's final become
//!      PROVISIONALCORE
//!   3. Certifier runs over provisional-core candidates;
//!      stricter K=32 × 3 seeds = 96 samples. Survivors become
//!      CERTIFIED
//!   4. Certified rules become canonical — the seed_library for
//!      the NEXT generation of motor runs
//!
//! The feedback loop: canonical rules seed future runs → those
//! runs produce more validated candidates against a stronger
//! substrate → new provisional-core candidates appear → more
//! certification → monotonically growing canonical substrate.
//!
//! This is the state machine the user described: validation
//! produces things for certification; certification measures and
//! feeds back; the overall system is an async-shaped pipeline
//! (here synchronous — same algorithmic shape) curating a tree
//! over time and transformations.

mod common;

use mathscape_compress::derive_laws_validated;
use mathscape_core::{
    bootstrap::{
        BootstrapCycle, BootstrapCycleSpec, CanonicalDeduper,
        DefaultCorpusGenerator, DefaultModelUpdater, ExperimentOutcome,
        ExperimentScenario, LawExtractor, PhaseOutcome, SpecExecutionError,
    },
    certification::{
        run_certification_step, CertificationLevel, CertifiedRule,
        DefaultCertifier,
    },
    eval::RewriteRule,
    hash::TermRef,
    mathscape_map::{BufferedConsumer, MapSnapshot, MathscapeMap},
    meta_loop::{
        HeuristicProposer, MetaLoop, MetaLoopConfig, ScenarioExecutor,
    },
    policy::LinearPolicy,
    term::Term,
    AdaptiveCorpusGenerator,
};
use std::cell::RefCell;

// ── Validated extractor (same as Phase V.certified) ─────────

struct ValidatedExtractor {
    step_limit: usize,
    min_support: usize,
    subterm_depth: usize,
    k_samples: usize,
    seed: u64,
    _stats: RefCell<Vec<mathscape_compress::LawGenStats>>,
}

impl ValidatedExtractor {
    fn new(
        step_limit: usize,
        min_support: usize,
        subterm_depth: usize,
        k_samples: usize,
        seed: u64,
    ) -> Self {
        Self {
            step_limit,
            min_support,
            subterm_depth,
            k_samples,
            seed,
            _stats: RefCell::new(Vec::new()),
        }
    }
}

impl LawExtractor for ValidatedExtractor {
    fn extract(
        &self,
        corpus: &[Term],
        library: &[RewriteRule],
    ) -> Vec<RewriteRule> {
        let mut next_id: mathscape_core::term::SymbolId =
            (library.len() + 1) as u32;
        let (laws, stats) = derive_laws_validated(
            corpus,
            library,
            self.step_limit,
            self.min_support,
            self.subterm_depth,
            self.k_samples,
            self.seed,
            &mut next_id,
        );
        self._stats.borrow_mut().push(stats);
        laws
    }
}

struct SeededExecutor {
    seed: u64,
}

impl ScenarioExecutor for SeededExecutor {
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
            let n = spec.n_iterations;
            let extractor = ValidatedExtractor::new(300, 2, 2, 8, self.seed);
            let outcome = match spec.corpus_generator.as_str() {
                "default" => {
                    let cycle = BootstrapCycle::new(
                        DefaultCorpusGenerator,
                        extractor,
                        DefaultModelUpdater::default(),
                        n,
                    );
                    if let Some(w) = spec.early_stop_after_stable {
                        cycle.run_until_stable(
                            spec.seed_library.clone(),
                            spec.seed_policy.clone(),
                            &CanonicalDeduper,
                            w,
                        )
                    } else {
                        cycle.run_with_dedup(
                            spec.seed_library.clone(),
                            spec.seed_policy.clone(),
                            &CanonicalDeduper,
                        )
                    }
                }
                "adaptive" => {
                    let corpus = AdaptiveCorpusGenerator {
                        seed: self.seed,
                        ..AdaptiveCorpusGenerator::default()
                    };
                    let cycle = BootstrapCycle::new(
                        corpus,
                        extractor,
                        DefaultModelUpdater::default(),
                        n,
                    );
                    if let Some(w) = spec.early_stop_after_stable {
                        cycle.run_until_stable(
                            spec.seed_library.clone(),
                            spec.seed_policy.clone(),
                            &CanonicalDeduper,
                            w,
                        )
                    } else {
                        cycle.run_with_dedup(
                            spec.seed_library.clone(),
                            spec.seed_policy.clone(),
                            &CanonicalDeduper,
                        )
                    }
                }
                _ => continue,
            };
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

fn run_motor(
    seed: u64,
    seed_library: Vec<RewriteRule>,
    max_phases: usize,
) -> mathscape_core::meta_loop::MetaLoopOutcome {
    let loop_ = MetaLoop::new(
        SeededExecutor { seed },
        HeuristicProposer::with_extractor("derived-laws"),
        MetaLoopConfig {
            max_phases,
            sail_out_window: 0,
            policy_delta_threshold: 1e-9,
        },
    );
    let seed_scenario = ExperimentScenario {
        name: format!("pipeline-{seed}"),
        phases: vec![BootstrapCycleSpec {
            corpus_generator: "default".into(),
            law_extractor: "derived-laws".into(),
            model_updater: "default".into(),
            deduper: "canonical".into(),
            n_iterations: 5,
            seed_library,
            seed_policy: LinearPolicy::tensor_seeking_prior(),
            early_stop_after_stable: Some(1),
        }],
    };
    loop_.run(seed_scenario).expect("motor runs")
}

fn ingest(
    map: &mut MathscapeMap,
    seed: u64,
    outcome: &mathscape_core::meta_loop::MetaLoopOutcome,
    consumer: &BufferedConsumer,
) {
    for record in &outcome.history {
        let snap = MapSnapshot::new(
            seed,
            record.phase_index,
            record.outcome.final_library().to_vec(),
            Some(record.observation.clone()),
        );
        map.push_with_events(snap, consumer, 0.6);
    }
}

#[test]
fn full_state_machine_pipeline_runs_end_to_end() {
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║ PHASE V.PIPELINE — full state-machine end-to-end      ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();
    println!("  Candidate → Validated → ProvisionalCore → Certified → Canonical");
    println!();

    // ── Generation 0: fresh motor runs produce VALIDATED rules
    //    (the extractor applies Phase J K=8 internally; every
    //    output is already at Validated level). ───────────────
    let gen0_seeds: [u64; 4] = [1, 7, 42, 99];
    let mut map = MathscapeMap::new();
    let consumer = BufferedConsumer::new();

    println!("  ── Generation 0: fresh motor runs (Validated extractor)");
    for seed in gen0_seeds {
        let outcome = run_motor(seed, Vec::new(), 6);
        let final_size = outcome.final_library().len();
        ingest(&mut map, seed, &outcome, &consumer);
        println!(
            "    seed {:>4} → final {} rules (all Validated)",
            seed, final_size
        );
    }

    // ── ProvisionalCore: rules in every gen-0 seed's final.
    let provisional_core_rules = map.core_rules();
    println!(
        "\n  ── ProvisionalCore (rules in every Gen-0 seed final): {} rules",
        provisional_core_rules.len()
    );
    for rule in &provisional_core_rules {
        println!("    {} :: {} => {}", rule.name, rule.lhs, rule.rhs);
    }

    // ── Certification stage: stricter K=32 × 3 seeds = 96
    //    samples per rule. Survivors become Certified. ───────
    let provisional_certified: Vec<CertifiedRule> = provisional_core_rules
        .iter()
        .map(|r| {
            CertifiedRule::new(r.clone(), CertificationLevel::ProvisionalCore)
        })
        .collect();
    let certifier = DefaultCertifier::default();
    let step_report =
        run_certification_step(&certifier, provisional_certified, &[]);
    println!(
        "\n  ── Certification step (K=32 × 3 seeds = 96 samples each)"
    );
    println!("    input (provisional) : {}", provisional_core_rules.len());
    println!("    elevated (certified): {}", step_report.elevated.len());
    println!("    rejected            : {}", step_report.rejected.len());
    for (rule, reason) in &step_report.rejected {
        println!("      ✗ {} :: reason = {}", rule.name, reason);
    }

    let certified_rules: Vec<RewriteRule> = step_report
        .elevated
        .iter()
        .filter(|cr| cr.level == CertificationLevel::Certified)
        .map(|cr| cr.rule.clone())
        .collect();

    println!(
        "\n  ── CERTIFIED library (post-stricter-evidence): {} rules",
        certified_rules.len()
    );
    for rule in &certified_rules {
        println!("    {} :: {} => {}", rule.name, rule.lhs, rule.rhs);
    }

    // ── Promotion: certified rules become canonical (seed_library
    //    for the next generation). ────────────────────────────
    println!("\n  ── Promotion: certified → canonical");
    println!(
        "    {} rules promoted. Will seed Generation 1.",
        certified_rules.len()
    );

    // ── Generation 1: new motor runs SEEDED with canonical
    //    substrate. Measure: do they reach further? ─────────────
    let gen1_seeds: [u64; 3] = [101, 202, 303];
    let mut gen1_finals: Vec<usize> = Vec::new();
    println!("\n  ── Generation 1: motor runs seeded with canonical library");
    for seed in gen1_seeds {
        let outcome = run_motor(seed, certified_rules.clone(), 6);
        let final_size = outcome.final_library().len();
        gen1_finals.push(final_size);
        ingest(&mut map, seed, &outcome, &consumer);
        println!(
            "    seed {:>4} (seed_lib={}) → final {} rules, Δ {:+}",
            seed,
            certified_rules.len(),
            final_size,
            final_size as i64 - certified_rules.len() as i64,
        );
    }

    // ── State machine health ───────────────────────────────────
    let all_events = consumer.drain();
    let event_kinds = {
        let mut m = std::collections::BTreeMap::new();
        for e in &all_events {
            *m.entry(e.category()).or_insert(0usize) += 1;
        }
        m
    };
    println!("\n  ── State machine aggregate");
    println!("    total events egressed : {}", all_events.len());
    for (k, v) in &event_kinds {
        println!("      {:<18}: {}", k, v);
    }
    println!(
        "    canonical library size: {}",
        certified_rules.len()
    );
    println!("    gen-0 seeds           : {}", gen0_seeds.len());
    println!("    gen-1 seeds (seeded)  : {}", gen1_seeds.len());

    // ── Invariants ─────────────────────────────────────────────

    // 1. Certified rules ⊆ ProvisionalCore (monotonicity through
    //    the pipeline).
    assert!(
        certified_rules.len() <= provisional_core_rules.len(),
        "certified ⊆ provisional-core"
    );

    // 2. Every gen-1 final ≥ canonical seed size (can't regress
    //    below what we pre-loaded).
    for &final_size in &gen1_finals {
        assert!(
            final_size >= certified_rules.len(),
            "gen-1 final ({}) must be ≥ canonical seed size ({})",
            final_size,
            certified_rules.len()
        );
    }

    // 3. Events still egress (the state machine is observable).
    assert!(
        !all_events.is_empty(),
        "state machine must emit events"
    );

    // 4. At least one rule made it ALL THE WAY from Candidate to
    //    Certified — the pipeline is not hollow.
    assert!(
        !certified_rules.is_empty(),
        "at least one rule must traverse the full pipeline"
    );

    println!("\n  ══ STATE MACHINE LIVE. ══");
    println!("  Candidate → Validated → ProvisionalCore → Certified → Canonical");
    println!("  Feedback loop closed: canonical library seeds generation 1.");
}
