//! Phase Y.3.3 (2026-04-19): resumption proof.
//!
//! A trained model saved to disk isn't just a read-only artifact —
//! it's a resumable training state. Load the `.msnp`, feed its
//! library + policy back as the motor's seed, run more cycles,
//! certify the result, compare to the original. The disk
//! artifact IS the training checkpoint.
//!
//! What this proves concretely:
//!  1. A snapshot carries enough state to resume training
//!     without losing any prior progress.
//!  2. Continued training from a snapshot produces a NEW
//!     snapshot that certifies + diffs cleanly against the
//!     original.
//!  3. The evolution loop works end-to-end on disk: train →
//!     save → load → train more → save again → compare.

use mathscape_compress::derive_laws_from_corpus_instrumented;
use mathscape_core::bootstrap::{
    BootstrapCycle, BootstrapCycleSpec, CanonicalDeduper,
    DefaultCorpusGenerator, DefaultModelUpdater, ExperimentOutcome,
    ExperimentScenario, LawExtractor, PhaseOutcome, SpecExecutionError,
};
use mathscape_core::eval::RewriteRule;
use mathscape_core::hash::TermRef;
use mathscape_core::mathscape_map::EventHub;
use mathscape_core::meta_loop::{
    HeuristicProposer, MetaLoop, MetaLoopConfig, ScenarioExecutor,
};
use mathscape_core::model_testing::{
    certify_snapshot, compare_snapshots,
};
use mathscape_core::policy::LinearPolicy;
use mathscape_core::snapshot::{
    fork_from_snapshot, snapshot_handle, ModelSnapshot,
};
use mathscape_core::streaming_policy::StreamingPolicyTrainer;
use mathscape_core::term::Term;
use mathscape_core::{AdaptiveCorpusGenerator, LiveInferenceHandle};
use std::cell::RefCell;
use std::rc::Rc;

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
        let (laws, _) = derive_laws_from_corpus_instrumented(
            corpus,
            library,
            self.step_limit,
            self.min_support,
            &mut next_id,
        );
        laws
    }
}

struct MotorExecutor;
impl ScenarioExecutor for MotorExecutor {
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
            let outcome = run_spec(&spec)?;
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

fn run_spec(
    spec: &BootstrapCycleSpec,
) -> Result<mathscape_core::bootstrap::BootstrapOutcome, SpecExecutionError> {
    let n = spec.n_iterations;
    let extractor = DerivedLawsExtractor {
        step_limit: 300,
        min_support: 2,
    };
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
            let cycle = BootstrapCycle::new(
                AdaptiveCorpusGenerator::default(),
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
        other => {
            return Err(SpecExecutionError::UnknownLayer {
                role: "corpus_generator",
                name: other.to_string(),
            });
        }
    };
    Ok(outcome)
}

/// Resume a saved model from disk + continue training + snapshot.
#[test]
fn resume_training_from_disk_snapshot_and_evolve() {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║ PHASE Y.3.3 — RESUME FROM SNAPSHOT + EVOLVE             ║");
    println!("╚════════════════════════════════════════════════════════╝");

    // ── 1) Build a baseline snapshot via a quick motor run. ──
    let hub = Rc::new(EventHub::new());
    let library = Rc::new(RefCell::new(Vec::<RewriteRule>::new()));
    let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
    hub.subscribe(trainer.clone());
    let live = LiveInferenceHandle::new(library.clone(), trainer.clone());

    let loop_ = MetaLoop::new(
        MotorExecutor,
        HeuristicProposer::with_extractor("derived-laws"),
        MetaLoopConfig {
            max_phases: 3,
            sail_out_window: 0,
            policy_delta_threshold: 1e-9,
        },
    );
    let seed = BootstrapCycleSpec {
        corpus_generator: "default".into(),
        law_extractor: "derived-laws".into(),
        model_updater: "default".into(),
        deduper: "canonical".into(),
        n_iterations: 5,
        seed_library: Vec::new(),
        seed_policy: LinearPolicy::tensor_seeking_prior(),
        early_stop_after_stable: Some(1),
    };
    let scenario = ExperimentScenario {
        name: "checkpoint-initial".into(),
        phases: vec![seed.clone()],
    };
    let outcome = loop_.run(scenario).expect("motor");
    for rule in outcome.final_library().iter() {
        if !library.borrow().iter().any(|r| r.name == rule.name) {
            library.borrow_mut().push(rule.clone());
        }
    }
    let checkpoint_a = snapshot_handle(&live);
    let baseline_library_len = checkpoint_a.library.len();
    println!(
        "\n  Checkpoint A: {} rules discovered, content hash {:02x?}…",
        baseline_library_len,
        &checkpoint_a.content_hash[..8]
    );

    // ── 2) Certify checkpoint A. ──
    let cert_a = certify_snapshot(&checkpoint_a);
    println!("  Cert A:      {}", if cert_a.passed() { "PASS" } else { "FAIL" });
    println!(
        "  Cert A curriculum: {}/{}",
        cert_a.curriculum.total.solved_count,
        cert_a.curriculum.total.problem_set_size
    );
    assert!(cert_a.passed(), "checkpoint A must certify");

    // ── 3) Save to disk. ──
    let tmp_a = std::env::temp_dir().join(format!(
        "mathscape-resume-a-{}.msnp",
        std::process::id()
    ));
    let mut a_to_save = checkpoint_a.clone();
    a_to_save.save_to_path(&tmp_a).expect("save A");

    // ── 4) SIMULATE A FRESH PROCESS: drop the live state, load
    //    the snapshot from disk, fork into a new handle. ──
    drop(live);
    drop(trainer);
    drop(library);
    drop(hub);

    let reloaded_a =
        ModelSnapshot::load_from_path(&tmp_a).expect("load A");
    assert_eq!(
        reloaded_a.content_hash, checkpoint_a.content_hash,
        "reload preserves content hash"
    );
    let resumed_handle = fork_from_snapshot(&reloaded_a);
    println!(
        "\n  Resumed from disk: {} rules, cold-reload OK",
        resumed_handle.library_size()
    );

    // ── 5) Continue training FROM the resumed state. Motor's
    //    seed_library = the reloaded library, seed_policy = the
    //    reloaded policy. Anything the motor discovers now is
    //    BUILDING ON the checkpoint, not starting fresh. ──
    let resumed_library_vec: Vec<RewriteRule> =
        resumed_handle.library_snapshot();
    let resumed_policy: LinearPolicy = resumed_handle.policy_snapshot();
    let continue_loop = MetaLoop::new(
        MotorExecutor,
        HeuristicProposer::with_extractor("derived-laws"),
        MetaLoopConfig {
            max_phases: 3,
            sail_out_window: 0,
            policy_delta_threshold: 1e-9,
        },
    );
    let continue_spec = BootstrapCycleSpec {
        corpus_generator: "adaptive".into(), // try a different corpus
        law_extractor: "derived-laws".into(),
        model_updater: "default".into(),
        deduper: "canonical".into(),
        n_iterations: 5,
        seed_library: resumed_library_vec.clone(),
        seed_policy: resumed_policy.clone(),
        early_stop_after_stable: Some(1),
    };
    let continue_scenario = ExperimentScenario {
        name: "checkpoint-continued".into(),
        phases: vec![continue_spec],
    };
    let continued = continue_loop.run(continue_scenario).expect("continue");
    let continued_library = continued.final_library().to_vec();

    // Merge any new rules back into the resumed handle's library.
    let live_lib = resumed_handle.library_rc();
    for rule in continued_library {
        if !live_lib.borrow().iter().any(|r| r.name == rule.name) {
            live_lib.borrow_mut().push(rule);
        }
    }

    // ── 6) Snapshot the CONTINUED model. ──
    let checkpoint_b = snapshot_handle(&resumed_handle);
    let tmp_b = std::env::temp_dir().join(format!(
        "mathscape-resume-b-{}.msnp",
        std::process::id()
    ));
    let mut b_to_save = checkpoint_b.clone();
    b_to_save.save_to_path(&tmp_b).expect("save B");

    println!(
        "\n  Checkpoint B: {} rules, content hash {:02x?}…",
        checkpoint_b.library.len(),
        &checkpoint_b.content_hash[..8]
    );

    let cert_b = certify_snapshot(&checkpoint_b);
    println!("  Cert B:      {}", if cert_b.passed() { "PASS" } else { "FAIL" });
    println!(
        "  Cert B curriculum: {}/{}",
        cert_b.curriculum.total.solved_count,
        cert_b.curriculum.total.problem_set_size
    );
    assert!(cert_b.passed(), "checkpoint B must certify");

    // ── 7) Compare A to B. The evolution story made explicit. ──
    let diff = compare_snapshots(&checkpoint_a, &checkpoint_b);
    println!("\n{}", diff.summary());

    // ── Assertions — what resumption MUST guarantee. ──

    // A1: B has at least as many rules as A (monotonic library
    // growth during continued training).
    assert!(
        diff.lib_size_b >= diff.lib_size_a,
        "B ({} rules) ≥ A ({} rules) — resumed training \
         never loses rules",
        diff.lib_size_b,
        diff.lib_size_a
    );

    // A2: Every rule in A is still present in B.
    assert!(
        diff.rules_only_in_a.is_empty(),
        "No rule dropped during resumption: {:?}",
        diff.rules_only_in_a
    );

    // A3: B's curriculum ≥ A's curriculum — competency is
    // preserved (or grows) across the resumption.
    assert!(
        diff.curriculum_b.0 >= diff.curriculum_a.0,
        "B curriculum ({}/{}) ≥ A curriculum ({}/{})",
        diff.curriculum_b.0,
        diff.curriculum_b.1,
        diff.curriculum_a.0,
        diff.curriculum_a.1,
    );

    // A4: The two snapshots have different content hashes IFF
    // actual changes occurred.
    let changed = diff.lib_size_b != diff.lib_size_a
        || diff.weight_l2_distance > 1e-9
        || diff.bias_delta.abs() > 1e-9;
    assert_eq!(
        !diff.content_hash_equal, changed,
        "content_hash_equal iff no changes"
    );

    // ── Log the operational shape. ──
    println!("\n  ╔══════════════ RESUMPTION LEDGER ═══════════════╗");
    println!("    baseline path:      {}", tmp_a.display());
    println!("    continued path:     {}", tmp_b.display());
    println!("    library: A={} → B={}", diff.lib_size_a, diff.lib_size_b);
    println!(
        "    curriculum: A={}/{} → B={}/{}",
        diff.curriculum_a.0,
        diff.curriculum_a.1,
        diff.curriculum_b.0,
        diff.curriculum_b.1,
    );
    println!("    bias delta: {:+.6}", diff.bias_delta);
    println!("    weight L2:  {:.6}", diff.weight_l2_distance);
    println!(
        "    hashes identical: {}",
        diff.content_hash_equal
    );
    println!("  ╚═════════════════════════════════════════════════╝\n");

    let _ = std::fs::remove_file(&tmp_a);
    let _ = std::fs::remove_file(&tmp_b);
}
