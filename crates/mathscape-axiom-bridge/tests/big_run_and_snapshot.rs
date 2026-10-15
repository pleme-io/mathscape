//! Phase Z.5 (2026-04-19): THE BIG RUN.
//!
//! 20 motor-coach cycles alternating between default and
//! adaptive corpora. Deep analysis pass at the end. Final
//! snapshot saved to disk under `target/mathscape-models/`
//! with the analysis embedded in metadata — reproducibly
//! locatable for later inspection.
//!
//! The file is intentionally persisted outside the test's
//! tempdir so the user can open it, analyze it, and reload
//! it into a live handle via `ModelSnapshot::load_from_path`.

use mathscape_compress::Motor;
use mathscape_core::bootstrap::{
    BootstrapCycleSpec,
    ExperimentScenario,
};
use mathscape_core::eval::RewriteRule;
use mathscape_core::math_problem::{mathematician_curriculum, run_curriculum};
use mathscape_core::mathscape_map::{EventHub, MapEvent};
use mathscape_core::meta_loop::{
    HeuristicProposer, MetaLoop, MetaLoopConfig,
};
use mathscape_core::policy::LinearPolicy;
use mathscape_core::snapshot::{
    attach_analysis, deep_analyze, format_analysis, snapshot_handle,
};
use mathscape_core::streaming_policy::StreamingPolicyTrainer;
use mathscape_core::{
    BufferedConsumer, CurriculumCoach,
    LiveInferenceHandle, RuleBasedPolicy,
};
use std::cell::RefCell;
use std::rc::Rc;

/// THE BIG RUN. Deep measurement. Snapshot to disk.
#[test]
fn big_run_with_analysis_and_snapshot() {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║ PHASE Z.5 — THE BIG RUN                                     ║");
    println!("║ 20 cycles, analysis, disk snapshot                          ║");
    println!("╚════════════════════════════════════════════════════════════╝");

    // ── Live wiring ─────────────────────────────────────────────
    let hub = Rc::new(EventHub::new());
    let library = Rc::new(RefCell::new(Vec::<RewriteRule>::new()));
    let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
    let buffer = Rc::new(BufferedConsumer::new());

    hub.subscribe(trainer.clone());
    hub.subscribe(buffer.clone());

    let student = LiveInferenceHandle::new(library.clone(), trainer.clone());
    let coach = CurriculumCoach::new(
        RuleBasedPolicy,
        LiveInferenceHandle::new(library.clone(), trainer.clone()),
        hub.clone(),
    );

    let curriculum = mathematician_curriculum();
    let mut score_trajectory: Vec<f64> = Vec::new();
    let mut lib_trajectory: Vec<usize> = Vec::new();
    let mut action_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();

    // Baseline
    let r0 = run_curriculum(&curriculum, &library.borrow());
    score_trajectory.push(r0.total.solved_fraction());
    lib_trajectory.push(0);

    const N_CYCLES: usize = 20;
    for cycle in 1..=N_CYCLES {
        // Alternate corpora — adaptive every 3rd cycle forces
        // the motor into diet mutation.
        let generator = if cycle % 3 == 0 { "adaptive" } else { "default" };

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
            name: format!("big-cycle-{cycle}-{generator}"),
            phases: vec![seed_spec],
        };
        let outcome = loop_.run(seed_scenario).expect("motor runs");

        // Merge motor's discoveries into the shared library.
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
        *action_counts.entry(action.kind().to_string()).or_insert(0) += 1;

        // Terse progress log every cycle.
        println!(
            "  cycle {:>2}  corpus={:<8}  score={:.3}  lib={:>2}  coach={}",
            cycle,
            generator,
            r.total.solved_fraction(),
            library.borrow().len(),
            action.kind(),
        );
    }

    // ── DEEP ANALYSIS PASS ──────────────────────────────────────
    println!("\n  Running deep analysis pass before snapshot...");
    let analysis = deep_analyze(&student);
    println!("{}", format_analysis(&analysis));

    // ── Build + save snapshot with analysis embedded ────────────
    let mut snap = snapshot_handle(&student);
    snap.metadata.insert(
        "session.cycles_run".into(),
        N_CYCLES.to_string(),
    );
    snap.metadata.insert(
        "session.coach_policy".into(),
        "rule-based".into(),
    );
    snap.metadata.insert(
        "session.phase".into(),
        "Z.5 big-run".into(),
    );
    let actions_summary: String = action_counts
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    snap.metadata
        .insert("session.coach_actions".into(), actions_summary);
    snap.metadata.insert(
        "session.score_trajectory".into(),
        format!("{:?}", score_trajectory),
    );
    attach_analysis(&mut snap, &analysis);

    // Write to target/mathscape-models/ — persists after test.
    let out_dir = std::env::current_dir()
        .unwrap()
        .ancestors()
        .find(|p| p.join("target").is_dir())
        .map(|p| p.join("target/mathscape-models"))
        .unwrap_or_else(|| std::path::PathBuf::from("target/mathscape-models"));
    std::fs::create_dir_all(&out_dir).expect("mkdir target/mathscape-models");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file_path = out_dir.join(format!("big-run-{ts}.msnp"));
    snap.save_to_path(&file_path).expect("save snapshot");

    // ── Final dashboard ────────────────────────────────────────
    println!("\n  ╔══════════════════ FINAL DASHBOARD ═══════════════════╗");
    println!("    cycles run:            {N_CYCLES}");
    println!(
        "    score trajectory:      [{}]",
        score_trajectory
            .iter()
            .map(|s| format!("{s:.2}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("    library trajectory:    {:?}", lib_trajectory);
    println!(
        "    coach action counts:   {:?}",
        action_counts
    );
    println!(
        "    final curriculum:      {}/{} ({:.1}%)",
        analysis.curriculum_total.0,
        analysis.curriculum_total.1,
        analysis.curriculum_total.2 * 100.0
    );
    println!(
        "    mastered subdomains:   {}",
        analysis.mastered.join(", ")
    );
    println!("    content hash:          {:02x?}…", &snap.content_hash[..8]);
    println!("    snapshot path:         {}", file_path.display());
    println!(
        "    snapshot file size:    {} bytes",
        std::fs::metadata(&file_path).unwrap().len()
    );
    println!("    events through hub:    {}", hub.published_count());
    println!("    trainer events seen:   {}", trainer.events_seen());
    println!("  ╚══════════════════════════════════════════════════════╝\n");

    // ── Assertions ──────────────────────────────────────────────
    let first = score_trajectory.first().copied().unwrap_or(0.0);
    let last = score_trajectory.last().copied().unwrap_or(0.0);
    assert!(last >= first, "final ≥ baseline: {first} → {last}");
    for w in score_trajectory.windows(2) {
        assert!(w[1] >= w[0] - 1e-9, "monotonic: {} → {}", w[0], w[1]);
    }
    assert!(
        *lib_trajectory.last().unwrap() > 0,
        "library has rules after {N_CYCLES} cycles"
    );
    assert!(
        file_path.exists(),
        "snapshot written to {}",
        file_path.display()
    );
    let file_size = std::fs::metadata(&file_path).unwrap().len();
    assert!(file_size > 1000, "snapshot is non-trivial ({file_size} bytes)");

    // Reload + verify the file parses and the analysis metadata is there.
    let reloaded = mathscape_core::snapshot::ModelSnapshot::load_from_path(
        &file_path,
    )
    .expect("snapshot reloads cleanly");
    assert_eq!(reloaded.content_hash, snap.content_hash);
    assert!(reloaded
        .metadata
        .contains_key("analysis.curriculum_score"));
    assert!(reloaded.metadata.contains_key("session.cycles_run"));
    assert_eq!(
        reloaded.metadata.get("session.cycles_run"),
        Some(&N_CYCLES.to_string())
    );
    assert_eq!(reloaded.library.len(), snap.library.len());

    println!(
        "  ✓ snapshot reloaded: {} rules, analysis metadata preserved",
        reloaded.library.len()
    );
    println!();
}
