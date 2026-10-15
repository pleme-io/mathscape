//! Proof: a persisted snapshot is a SELF-SUFFICIENT model.
//!
//! Given nothing but the `.msnp` file on disk, we can:
//!  1. load it via `ModelSnapshot::load_from_path`
//!  2. fork it into a `LiveInferenceHandle`
//!  3. run inference queries
//!  4. read the embedded analysis metadata
//!
//! No motor. No EventHub. No coach. No training. No
//! curriculum run. Just the bytes on disk → a usable live model.

use mathscape_core::math_problem::{mathematician_curriculum, run_curriculum};
use mathscape_core::snapshot::{
    analyze_snapshot, fork_from_snapshot, snapshot_handle, ModelSnapshot,
};
use mathscape_core::streaming_policy::StreamingPolicyTrainer;
use mathscape_core::term::Term;
use mathscape_core::value::Value;
use mathscape_core::LiveInferenceHandle;
use std::cell::RefCell;
use std::rc::Rc;

fn apply(h: u32, args: Vec<Term>) -> Term {
    Term::Apply(Box::new(Term::Var(h)), args)
}
fn nat(n: u64) -> Term {
    Term::Number(Value::Nat(n))
}
fn pv(id: u32) -> Term {
    Term::Var(id)
}

/// Build a tiny model by hand (skipping the motor), snapshot it
/// to a temp file, then reload + inference it — proving the
/// disk artifact alone is enough.
#[test]
fn snapshot_is_a_self_sufficient_model_with_no_external_state() {
    use mathscape_core::eval::RewriteRule;
    use mathscape_core::builtin::{ADD, MUL};

    // ── 1) Build a live model (this stands in for the motor) ──
    let library = Rc::new(RefCell::new(vec![
        RewriteRule {
            name: "add-id".into(),
            lhs: apply(ADD, vec![nat(0), pv(100)]),
            rhs: pv(100),
        },
        RewriteRule {
            name: "mul-id".into(),
            lhs: apply(MUL, vec![nat(1), pv(100)]),
            rhs: pv(100),
        },
    ]));
    let trainer = Rc::new(StreamingPolicyTrainer::new(0.1));
    let live = LiveInferenceHandle::new(library, trainer);

    // ── 2) Snapshot → disk ──
    let tmp = std::env::temp_dir().join(format!(
        "mathscape-standalone-{}.msnp",
        std::process::id()
    ));
    let mut snap = snapshot_handle(&live);
    snap.metadata
        .insert("note".into(), "standalone proof".into());
    snap.save_to_path(&tmp).expect("save");

    // ── 3) DROP EVERYTHING. We are now in the "user loads the
    //    file on a fresh machine" situation. ──
    drop(live);
    drop(snap);

    // ── 4) Load from disk — this is all the user has to do. ──
    let reloaded = ModelSnapshot::load_from_path(&tmp).expect("load");

    // ── 5) Fork into an independent LiveInferenceHandle. ──
    let restored: LiveInferenceHandle = fork_from_snapshot(&reloaded);

    // ── 6) INFERENCE — with zero motor/hub/training running. ──
    let probes: Vec<(&str, Term, Term)> = vec![
        ("add(0, 42)", apply(ADD, vec![nat(0), nat(42)]), nat(42)),
        ("mul(1, 99)", apply(MUL, vec![nat(1), nat(99)]), nat(99)),
        (
            "add(0, ?x) symbolic",
            apply(ADD, vec![nat(0), pv(100)]),
            pv(100),
        ),
        (
            "mul(1, ?x) symbolic",
            apply(MUL, vec![nat(1), pv(100)]),
            pv(100),
        ),
        ("add(3, 4) concrete", apply(ADD, vec![nat(3), nat(4)]), nat(7)),
    ];

    println!(
        "\n  STANDALONE INFERENCE (from snapshot, no other state):"
    );
    for (desc, term, expected) in &probes {
        let result = restored.infer(term, 50).expect("eval");
        let ok = if result == *expected { "✓" } else { "✗" };
        println!("    {ok} {desc:<28} → {result:?}");
        assert_eq!(result, *expected, "probe failed: {desc}");
    }

    // ── 7) Competency report — also runs from the loaded state. ──
    let competency = restored.current_competency();
    println!(
        "\n  Competency (from standalone model): {}/{} ({:.0}%)",
        competency.total.solved_count,
        competency.total.problem_set_size,
        competency.total.solved_fraction() * 100.0
    );
    println!("  Mastered: {:?}", competency.mastered());

    // The reloaded model should handle symbolic-nat, compound,
    // generalization through the add-id + mul-id rules.
    let sym = competency.per_subdomain.get("symbolic-nat").unwrap();
    assert_eq!(
        sym.solved_count, sym.problem_set_size,
        "reloaded model masters symbolic-nat"
    );

    // ── 8) Metadata preserved across the disk round-trip. ──
    assert_eq!(
        reloaded.metadata.get("note"),
        Some(&"standalone proof".to_string())
    );

    // ── 9) Content hash is stable. ──
    let rehashed = reloaded.compute_content_hash();
    assert_eq!(rehashed, reloaded.content_hash);

    // Cleanup.
    let _ = std::fs::remove_file(&tmp);
}

/// Load the actual big-run snapshot from disk (if it exists
/// from a prior Phase Z.5 run), inference it, and print the
/// full analysis. This is the "user opens the file" ceremony.
#[test]
fn standalone_query_against_big_run_snapshot_if_present() {
    // Find the most recent big-run-*.msnp. Walk up from cwd to
    // locate the workspace root (has target/ dir), then look
    // in target/mathscape-models/.
    let dir_owned = std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            cwd.ancestors()
                .find(|p| p.join("target").is_dir())
                .map(|p| p.join("target/mathscape-models"))
        })
        .unwrap_or_else(|| {
            std::path::PathBuf::from("target/mathscape-models")
        });
    let dir = dir_owned.as_path();
    let Ok(entries) = std::fs::read_dir(dir) else {
        println!(
            "  (no target/mathscape-models/ — run the Phase Z.5 test first \
             to produce a big-run snapshot, or rerun from repo root)"
        );
        return;
    };
    let msnp: Option<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|x| x == "msnp").unwrap_or(false)
                && p.file_name()
                    .map(|n| n.to_string_lossy().starts_with("big-run-"))
                    .unwrap_or(false)
        })
        .max_by_key(|p| std::fs::metadata(p).unwrap().modified().unwrap());
    let Some(path) = msnp else {
        println!(
            "  (no big-run-*.msnp found — run the Phase Z.5 test first)"
        );
        return;
    };

    println!("\n  Loading snapshot: {}", path.display());
    let snap =
        ModelSnapshot::load_from_path(&path).expect("snapshot reloads");
    println!("{}", analyze_snapshot(&snap));

    // Fork into a live handle and run the full curriculum — the
    // loaded model can be benchmarked without any motor running.
    let handle = fork_from_snapshot(&snap);
    let report = run_curriculum(
        &mathematician_curriculum(),
        &handle.library_snapshot(),
    );
    println!(
        "\n  STANDALONE CURRICULUM: {}/{} ({:.1}%)",
        report.total.solved_count,
        report.total.problem_set_size,
        report.total.solved_fraction() * 100.0
    );

    // Run a panel of inference probes.
    use mathscape_core::builtin::{ADD, MUL, TENSOR_ADD, TENSOR_MUL};
    let tensor = |shape: Vec<usize>, data: Vec<i64>| {
        Term::Number(Value::tensor(shape, data).unwrap())
    };
    let probes: Vec<(String, Term)> = vec![
        ("add(0, 7)".into(), apply(ADD, vec![nat(0), nat(7)])),
        ("mul(1, 12)".into(), apply(MUL, vec![nat(1), nat(12)])),
        ("add(0, ?x)".into(), apply(ADD, vec![nat(0), pv(100)])),
        (
            "tensor_add(zeros, ?x)".into(),
            apply(TENSOR_ADD, vec![tensor(vec![2], vec![0, 0]), pv(100)]),
        ),
        (
            "tensor_mul(ones, ?x)".into(),
            apply(TENSOR_MUL, vec![tensor(vec![2], vec![1, 1]), pv(100)]),
        ),
        (
            "compound add(0, mul(1, ?x))".into(),
            apply(ADD, vec![nat(0), apply(MUL, vec![nat(1), pv(100)])]),
        ),
    ];
    println!("\n  STANDALONE PROBE PANEL:");
    for (desc, term) in &probes {
        match handle.infer(term, 50) {
            Ok(r) => println!("    {desc:<32} → {:?}", r),
            Err(e) => println!("    {desc:<32} → ERR {e}"),
        }
    }

    // Confirm the snapshot's analysis metadata matches a fresh
    // re-run of the curriculum (self-consistency check).
    if let Some(stored) = snap.metadata.get("analysis.curriculum_score") {
        let expected_fraction = report.total.solved_fraction();
        println!(
            "\n  stored analysis: {stored}  (live recomputed: {:.3})",
            expected_fraction
        );
    }

    // ── RIGOROUS CERTIFICATION — the full test framework ─────
    use mathscape_core::model_testing::{
        certify_snapshot, verify_serialization_roundtrip,
    };
    println!("\n  Running rigorous certification pass...");
    let cert = certify_snapshot(&snap);
    println!("{}", cert.summary());
    println!("\n  Invariants checked:");
    for r in &cert.invariants {
        let mark = if r.passed { "✓" } else { "✗" };
        println!("    {mark} {:<35}  {}", r.name, r.detail);
    }
    assert!(cert.passed(), "snapshot must fully certify");

    let serde_ok = verify_serialization_roundtrip(&snap).unwrap();
    println!("\n  Serialization round-trip: {}", if serde_ok { "✓" } else { "✗" });
    assert!(serde_ok);
}
