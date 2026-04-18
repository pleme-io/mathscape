//! The mathscape discovery catalog runner.
//!
//! Runs the 40+ experiment catalog (see `common/experiment.rs`) and
//! aggregates findings across experiments. This is the central
//! apparatus-level discovery loop: every experiment is a different
//! hypothesis about the discovery space, and cross-experiment
//! aggregation surfaces the "core truths" that hold across many
//! apparatus/corpus/scale cells, not just one.
//!
//! The catalog is data (`catalog()` in `common::experiment`). Adding
//! an experiment is a matter of pushing a struct to the vector; the
//! runner handles everything else. This is intentional — the
//! harness should be the knob the operator turns.
//!
//! Invocation:
//!   cargo test -p mathscape-axiom-bridge --release --test experiment_catalog \
//!     run_catalog -- --ignored --nocapture
//!
//! Release mode is essential — the catalog runs ~40 experiments each
//! of which takes ~1-3 seconds at release and 10x that at debug.

mod common;

use common::experiment::{catalog, print_catalog_summary, print_report, run_experiment};

#[test]
#[ignore = "mathscape discovery catalog — 40+ experiments aggregated to find core truths. ~3-5 min, --ignored"]
fn run_catalog() {
    let experiments = catalog();
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║ MATHSCAPE APPARATUS-LEVEL DISCOVERY CATALOG                          ║");
    println!(
        "║   {} experiments spanning apparatus × corpus × scale × config        ║",
        experiments.len()
    );
    println!("╚══════════════════════════════════════════════════════════════════════╝");

    let catalog_start = std::time::Instant::now();
    let mut reports = Vec::with_capacity(experiments.len());
    for (i, exp) in experiments.iter().enumerate() {
        let report = run_experiment(exp);
        print_report(&report);
        reports.push(report);
        if (i + 1) % 10 == 0 {
            let elapsed = catalog_start.elapsed().as_secs_f64();
            let eta = (elapsed / (i + 1) as f64) * (experiments.len() - i - 1) as f64;
            println!();
            println!(
                "  ─── progress: {}/{} done · elapsed {:.1}s · ETA {:.1}s ───",
                i + 1,
                experiments.len(),
                elapsed,
                eta
            );
        }
    }
    let total_elapsed = catalog_start.elapsed().as_secs_f64();
    println!();
    println!("catalog runtime: {total_elapsed:.1}s");

    print_catalog_summary(&reports);

    // Barren experiments are informative (e.g., prover threshold
    // too strict) — report them but don't fail. Structural
    // regression would look like MULTIPLE experiments suddenly
    // going barren where previously they were productive.
    let barren: Vec<_> = reports
        .iter()
        .filter(|r| r.total_structural_rules == 0)
        .collect();
    if !barren.is_empty() {
        println!();
        println!(
            "note: {} experiment(s) produced zero rules (apparatus config too restrictive):",
            barren.len()
        );
        for r in &barren {
            println!("  - {}: {}", r.name, r.hypothesis);
        }
    }

    // Stronger invariant: at least 80% of experiments must produce
    // rules. Total collapse of the catalog IS a regression.
    let productive = reports.len() - barren.len();
    let productive_rate = productive as f64 / reports.len() as f64;
    assert!(
        productive_rate >= 0.8,
        "only {productive}/{} experiments produced rules — catalog regression?",
        reports.len()
    );
}
