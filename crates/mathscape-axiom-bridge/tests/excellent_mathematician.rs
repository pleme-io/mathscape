//! Phase X: **the Mathematician's Curriculum run** — the real
//! motor attempting every subdomain of mathematics the kernel
//! supports, producing a per-subdomain competency report.
//!
//! This is the measurement tool that defines "excellent
//! mathematician" for this substrate. Each subdomain has 5+
//! problems. The machine's goal: score 100% on every subdomain.
//! Today's reality: some subdomains are trivial (the kernel
//! solves them at boot); others require discovered rules that
//! the motor produces over phases.
//!
//! Running this test is how we KNOW the machine is making real
//! mathematical progress — not "benchmark goes up" in the
//! abstract, but "arithmetic-nat hit 100%, then symbolic-nat,
//! then compound started climbing."

use mathscape_compress::Motor;
use mathscape_core::bootstrap::{
    BootstrapCycleSpec,
    ExperimentScenario,
};
use mathscape_core::math_problem::{
    mathematician_curriculum, run_curriculum,
};
use mathscape_core::meta_loop::{
    HeuristicProposer, MetaLoop, MetaLoopConfig,
};
use mathscape_core::policy::LinearPolicy;

/// Establish the machine's mathematical competency baseline:
///
/// 1. Run the curriculum on an EMPTY library (kernel-only)
/// 2. Run the real motor for 10 phases
/// 3. Run the curriculum on the DISCOVERED library
/// 4. Print per-subdomain progress
///
/// This is how we KNOW the machine is becoming an excellent
/// mathematician — we can see which subdomains it mastered,
/// which are close, and which are the frontier.
#[test]
fn mathematician_curriculum_measures_real_motor_competency() {
    println!("\n╔════════════════════════════════════════════════════════╗");
    println!("║ PHASE X — THE MATHEMATICIAN'S CURRICULUM                ║");
    println!("║ measuring real-motor mathematical competency           ║");
    println!("╚════════════════════════════════════════════════════════╝");

    let curriculum = mathematician_curriculum();
    println!(
        "\n  curriculum: {} problems across subdomains",
        curriculum.len()
    );

    // ── Baseline: what does the kernel alone know? ─────────────
    let baseline = run_curriculum(&curriculum, &[]);
    println!("\n  BASELINE (empty library / kernel only):");
    for line in baseline.summary().lines() {
        println!("    {line}");
    }

    // ── Run the real motor ─────────────────────────────────────
    let loop_ = MetaLoop::new(
        Motor::default(),
        HeuristicProposer::with_extractor("derived-laws"),
        MetaLoopConfig {
            max_phases: 10,
            sail_out_window: 0,
            policy_delta_threshold: 1e-9,
        },
    );
    let seed_spec = BootstrapCycleSpec {
        corpus_generator: "default".into(),
        law_extractor: "derived-laws".into(),
        model_updater: "default".into(),
        deduper: "canonical".into(),
        n_iterations: 5,
        seed_library: Vec::new(),
        seed_policy: LinearPolicy::tensor_seeking_prior(),
        early_stop_after_stable: Some(1),
    };
    let seed_scenario = ExperimentScenario {
        name: "seed-default".into(),
        phases: vec![seed_spec],
    };
    let motor_outcome = loop_.run(seed_scenario).expect("motor runs");
    let library = motor_outcome.final_library();
    println!(
        "\n  MOTOR: {} phases ran, {} rules discovered",
        motor_outcome.history.len(),
        library.len()
    );

    // ── After-motor: what did the machine master? ──────────────
    let after = run_curriculum(&curriculum, library);
    println!("\n  AFTER MOTOR (discovered library):");
    for line in after.summary().lines() {
        println!("    {line}");
    }

    // ── Progress report ────────────────────────────────────────
    println!("\n  MASTERED subdomains: {:?}", after.mastered());
    println!("  FRONTIER subdomains (0% — next learning target): {:?}", after.frontier());
    let delta = after.total.solved_fraction() - baseline.total.solved_fraction();
    println!(
        "  Total progress: {:.0}% → {:.0}% (Δ {:+.1}%)",
        baseline.total.solved_fraction() * 100.0,
        after.total.solved_fraction() * 100.0,
        delta * 100.0
    );

    // ── ASSERTIONS — what the curriculum reveals about the machine ──

    // 1. Kernel is perfect at arithmetic-nat (5/5 concrete sums).
    let baseline_nat = baseline.per_subdomain.get("arithmetic-nat").unwrap();
    assert_eq!(
        baseline_nat.solved_count, baseline_nat.problem_set_size,
        "kernel masters arithmetic-nat"
    );

    // 2. Baseline cannot solve symbolic-nat (needs discovered rules).
    let baseline_sym = baseline.per_subdomain.get("symbolic-nat").unwrap();
    assert_eq!(baseline_sym.solved_count, 0);

    // 3. Motor achieves 100% on symbolic-nat (discovered the
    //    identity rules).
    let after_sym = after.per_subdomain.get("symbolic-nat").unwrap();
    assert_eq!(
        after_sym.solved_count, after_sym.problem_set_size,
        "motor masters symbolic-nat through discovery"
    );

    // 4. Motor TOTAL score is strictly better than baseline —
    //    net progress, even if individual subdomains shift.
    assert!(
        after.total.solved_count > baseline.total.solved_count,
        "motor produces net positive progress ({} → {})",
        baseline.total.solved_count,
        after.total.solved_count
    );

    // 5. Generalization subdomain: all-or-nothing within
    //    this test's configuration (add(0, N) at multiple
    //    concrete N values; kernel evaluates each via step
    //    reduction).
    let after_gen = after.per_subdomain.get("generalization").unwrap();
    let gen_all_or_none = after_gen.solved_count == 0
        || after_gen.solved_count == after_gen.problem_set_size;
    assert!(
        gen_all_or_none,
        "generalization is all-or-nothing: got {}/{}",
        after_gen.solved_count, after_gen.problem_set_size
    );

    // ── The finding ──────────────────────────────────────────
    //
    // REAL-MOTOR observation on the default corpus, 10 phases:
    //
    //   baseline: 18/32 (56%)
    //     arithmetic-nat: 5/5 (100%) ← kernel masters this
    //     arithmetic-int: 4/5 ( 80%)
    //     symbolic-nat:   0/6 (  0%) ← needs discovered rules
    //     tensor-algebra: 2/6 ( 33%)
    //     compound:       2/5 ( 40%)
    //     generalization: 5/5 (100%)
    //
    //   after motor: 28/32 (88%)  ← NET +31.2%
    //     arithmetic-nat: 3/5 ( 60%) ← regressed (!)
    //     arithmetic-int: 3/5 ( 60%) ← regressed (!)
    //     symbolic-nat:   6/6 (100%) ✓ MASTERED
    //     tensor-algebra: 6/6 (100%) ✓ MASTERED
    //     compound:       5/5 (100%) ✓ MASTERED
    //     generalization: 5/5 (100%) ✓ MASTERED
    //
    // CRITICAL FINDING: the motor's discovered rules interfere
    // with some concrete arithmetic. When a rule like
    //   add(?x, ?y) → some-law-lhs
    // matches BEFORE the kernel's constant-folding fires, the
    // computation terminates at the wrong shape. The machine
    // gained 4 subdomain masteries at the cost of 2 concrete-
    // computation regressions.
    //
    // This is exactly the value of the curriculum: it tells us
    // HOW the machine's mathematical development is progressing,
    // not just WHETHER it's progressing. The next research move
    // is informed: figure out why discovered rules interfere
    // with kernel reduction and design the priority ordering so
    // they compose instead of conflict.
}
