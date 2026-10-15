//! Phase Z.7 (2026-04-19): long run with interval snapshots +
//! cross-checkpoint archive analysis.
//!
//! 40 motor-coach cycles alternating default/adaptive corpora.
//! Snapshot every 10 cycles → 5 checkpoints saved to disk under
//! target/mathscape-models/archive-*/. Cross-checkpoint
//! compare_snapshots produces a trajectory showing exactly
//! where evolution happens and where the motor plateaus.

use mathscape_compress::Motor;
use mathscape_core::bootstrap::{
    BootstrapCycleSpec,
    ExperimentScenario,
};
use mathscape_core::eval::RewriteRule;
use mathscape_core::math_problem::{
    mathematician_curriculum, run_curriculum,
};
use mathscape_core::mathscape_map::{EventHub, MapEvent};
use mathscape_core::meta_loop::{
    HeuristicProposer, MetaLoop, MetaLoopConfig,
};
use mathscape_core::model_testing::{
    certify_snapshot, compare_snapshots,
};
use mathscape_core::policy::LinearPolicy;
use mathscape_core::snapshot::{
    deep_analyze, snapshot_handle, ModelSnapshot,
};
use mathscape_core::streaming_policy::StreamingPolicyTrainer;
use mathscape_core::{
    CurriculumCoach, LiveInferenceHandle,
    RuleBasedPolicy,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Long run: 40 cycles, checkpoint every 10, full archive
/// analysis.
#[test]
fn long_run_40_cycles_with_interval_snapshots_and_cross_diff() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║ PHASE Z.7 — THE LONG RUN                                      ║");
    println!("║ 40 cycles, interval snapshots, cross-checkpoint diff          ║");
    println!("╚══════════════════════════════════════════════════════════════╝");

    let hub = Rc::new(EventHub::new());
    let library = Rc::new(RefCell::new(Vec::<RewriteRule>::new()));
    let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
    hub.subscribe(trainer.clone());
    let student = LiveInferenceHandle::new(library.clone(), trainer.clone());
    let coach = CurriculumCoach::new(
        RuleBasedPolicy,
        LiveInferenceHandle::new(library.clone(), trainer.clone()),
        hub.clone(),
    );

    let curriculum = mathematician_curriculum();

    // Archive directory for snapshots.
    let out_dir = std::env::current_dir()
        .unwrap()
        .ancestors()
        .find(|p| p.join("target").is_dir())
        .map(|p| p.join("target/mathscape-models"))
        .unwrap_or_else(|| std::path::PathBuf::from("target/mathscape-models"));
    std::fs::create_dir_all(&out_dir).expect("mkdir");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let archive_prefix = out_dir.join(format!("archive-{ts}"));
    std::fs::create_dir_all(&archive_prefix).expect("mkdir archive");

    const N_CYCLES: usize = 40;
    const SNAPSHOT_EVERY: usize = 10;

    let mut score_trajectory: Vec<f64> = Vec::new();
    let mut lib_trajectory: Vec<usize> = Vec::new();
    let mut coach_actions: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut snapshot_paths: Vec<std::path::PathBuf> = Vec::new();

    // Baseline cycle 0.
    let r0 = run_curriculum(&curriculum, &library.borrow());
    score_trajectory.push(r0.total.solved_fraction());
    lib_trajectory.push(0);
    println!(
        "  cycle  0 baseline:   score {:.3}  lib 0",
        r0.total.solved_fraction()
    );

    for cycle in 1..=N_CYCLES {
        let generator =
            if cycle % 3 == 0 { "adaptive" } else { "default" };
        let loop_ = MetaLoop::new(
            Motor::default(),
            HeuristicProposer::with_extractor("derived-laws"),
            MetaLoopConfig {
                max_phases: 2,
                sail_out_window: 0,
                policy_delta_threshold: 1e-9,
            },
        );
        let seed_spec = BootstrapCycleSpec {
            corpus_generator: generator.into(),
            law_extractor: "derived-laws".into(),
            model_updater: "default".into(),
            deduper: "canonical".into(),
            n_iterations: 5,
            seed_library: library.borrow().clone(),
            seed_policy: LinearPolicy::tensor_seeking_prior(),
            early_stop_after_stable: Some(1),
        };
        let seed_scenario = ExperimentScenario {
            name: format!("long-cycle-{cycle}"),
            phases: vec![seed_spec],
        };
        let outcome = loop_.run(seed_scenario).expect("motor");

        let final_lib = outcome.final_library().to_vec();
        {
            let mut lib_mut = library.borrow_mut();
            for rule in final_lib {
                if !lib_mut.iter().any(|r| r.name == rule.name) {
                    lib_mut.push(rule.clone());
                    let size = lib_mut.len();
                    drop(lib_mut);
                    hub.publish(&MapEvent::CoreGrew {
                        prev_core_size: size - 1,
                        new_core_size: size,
                        added_rule: rule,
                    });
                    lib_mut = library.borrow_mut();
                }
            }
        }

        let r = run_curriculum(&curriculum, &library.borrow());
        score_trajectory.push(r.total.solved_fraction());
        lib_trajectory.push(library.borrow().len());

        let action = coach.tick();
        *coach_actions
            .entry(action.kind().to_string())
            .or_insert(0) += 1;

        // Snapshot at interval.
        if cycle % SNAPSHOT_EVERY == 0 {
            let analysis = deep_analyze(&student);
            let mut snap = snapshot_handle(&student);
            snap.metadata.insert(
                "cycle".into(),
                cycle.to_string(),
            );
            snap.metadata.insert(
                "score".into(),
                format!("{:.3}", r.total.solved_fraction()),
            );
            snap.metadata.insert(
                "mastered_count".into(),
                analysis.mastered.len().to_string(),
            );
            let path = archive_prefix
                .join(format!("cycle-{cycle:03}.msnp"));
            snap.save_to_path(&path).expect("save checkpoint");
            println!(
                "  📌 cycle {cycle:3} snapshot: score {:.3}  lib {}  mastered {}/13  →  {}",
                r.total.solved_fraction(),
                library.borrow().len(),
                analysis.mastered.len(),
                path.file_name().unwrap().to_string_lossy(),
            );
            snapshot_paths.push(path);
        }
    }

    // ── Cross-checkpoint analysis ─────────────────────────────
    println!("\n  ╭──────────── CROSS-CHECKPOINT DIFF ────────────╮");
    let snaps: Vec<ModelSnapshot> = snapshot_paths
        .iter()
        .map(|p| {
            ModelSnapshot::load_from_path(p).expect("reload checkpoint")
        })
        .collect();
    for (i, snap) in snaps.iter().enumerate() {
        let cert = certify_snapshot(snap);
        println!(
            "    cycle {:3}: hash {:02x?}…  cert={}  cur={}/{}",
            snap.metadata.get("cycle").cloned().unwrap_or_default(),
            &snap.content_hash[..6],
            if cert.passed() { "PASS" } else { "FAIL" },
            cert.curriculum.total.solved_count,
            cert.curriculum.total.problem_set_size,
        );
        assert!(cert.passed(), "checkpoint {i} must certify");
    }
    if snaps.len() >= 2 {
        for window in snaps.windows(2) {
            let a = &window[0];
            let b = &window[1];
            let diff = compare_snapshots(a, b);
            let a_cycle = a.metadata.get("cycle").cloned().unwrap_or_default();
            let b_cycle = b.metadata.get("cycle").cloned().unwrap_or_default();
            println!(
                "    diff {a_cycle} → {b_cycle}:  \
                 libΔ={:+}  scoreΔ={:+}  weight_L2={:.4}  new_rules={:?}",
                diff.lib_size_b as i64 - diff.lib_size_a as i64,
                diff.curriculum_b.0 as i64 - diff.curriculum_a.0 as i64,
                diff.weight_l2_distance,
                diff.rules_only_in_b,
            );
        }
    }
    println!("  ╰────────────────────────────────────────────────╯");

    // ── Final dashboard ────────────────────────────────────────
    let first = score_trajectory.first().copied().unwrap_or(0.0);
    let last = score_trajectory.last().copied().unwrap_or(0.0);
    println!(
        "\n  FINAL: {} cycles, {} snapshots archived",
        N_CYCLES,
        snapshot_paths.len()
    );
    println!(
        "  score: {:.3} → {:.3}  (Δ {:+.3})",
        first,
        last,
        last - first
    );
    println!(
        "  library: 0 → {} rules",
        library.borrow().len()
    );
    println!("  coach actions: {:?}", coach_actions);
    println!("  archive: {}", archive_prefix.display());

    // ── Assertions ────────────────────────────────────────────
    assert!(last >= first, "monotonic total");
    for w in score_trajectory.windows(2) {
        assert!(w[1] >= w[0] - 1e-9, "non-decreasing");
    }
    assert!(
        !library.borrow().is_empty(),
        "motor discovered something"
    );
    assert!(
        snapshot_paths.len() == N_CYCLES / SNAPSHOT_EVERY,
        "expected {} snapshots, got {}",
        N_CYCLES / SNAPSHOT_EVERY,
        snapshot_paths.len()
    );
    for path in &snapshot_paths {
        assert!(path.exists(), "{} must exist", path.display());
    }

    // Every snapshot certified + library monotonic across the archive.
    let libs: Vec<usize> = snaps.iter().map(|s| s.library.len()).collect();
    for w in libs.windows(2) {
        assert!(
            w[1] >= w[0],
            "archive library monotonic: {} → {}",
            w[0],
            w[1]
        );
    }
}
