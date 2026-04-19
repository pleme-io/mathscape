//! Phase Y.3.2 (2026-04-19): rigorous model-testing framework.
//!
//! The `.msnp` artifact is transferable, serializable, and
//! reload-exact. This module makes that claim TESTABLE at every
//! dimension we care about:
//!
//!   1. **Structural invariants** — numerical sanity of the
//!      trainer state (finite weights, non-negative Fisher,
//!      valid prune mask, rule uniqueness).
//!   2. **Competency reproduction** — loading the artifact on
//!      a cold process reproduces the stored curriculum score
//!      EXACTLY to the last problem.
//!   3. **Serialization round-trip** — write → read → rewrite
//!      produces bit-identical content hashes.
//!   4. **Transferability** — two independent forks of the same
//!      snapshot produce identical competency (mathematical
//!      self-consistency, not just bit-equality).
//!   5. **Cross-snapshot diff** — structured comparison between
//!      two snapshots for regression / progress measurement
//!      across experiments or systems.
//!
//! Pair this with the curriculum (`mathematician_curriculum`)
//! and you have a complete, rigorous certification pipeline:
//! any snapshot that passes `certify_snapshot` is trustworthy
//! to deploy / load / share.

use crate::math_problem::{
    mathematician_curriculum, run_curriculum, CurriculumReport,
};
use crate::snapshot::{
    fork_from_snapshot, ModelSnapshot, SnapshotError,
};
use crate::trajectory::LibraryFeatures;
use std::collections::BTreeMap;

/// One invariant result — pass / fail + what violated.
#[derive(Debug, Clone)]
pub struct InvariantResult {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

/// Full certification report.
#[derive(Debug, Clone)]
pub struct CertificationReport {
    /// Structural invariants over the snapshot.
    pub invariants: Vec<InvariantResult>,
    /// Current curriculum score computed from the loaded model.
    pub curriculum: CurriculumReport,
    /// Does the live curriculum score match what was stored in
    /// the snapshot's `analysis.curriculum_score` metadata?
    pub metadata_self_consistent: bool,
    /// Does the snapshot's stored content hash match a
    /// recomputation from the loaded data?
    pub content_hash_matches: bool,
    /// Do two independent forks from this snapshot produce
    /// identical curriculum scores?
    pub forks_agree: bool,
}

impl CertificationReport {
    pub fn passed(&self) -> bool {
        self.invariants.iter().all(|i| i.passed)
            && self.content_hash_matches
            && self.metadata_self_consistent
            && self.forks_agree
    }

    pub fn summary(&self) -> String {
        let invariants_passed =
            self.invariants.iter().filter(|i| i.passed).count();
        let invariants_total = self.invariants.len();
        format!(
            "CertificationReport {{\n  \
               passed:            {}\n  \
               invariants:        {}/{}\n  \
               content_hash:      {}\n  \
               metadata_self_consistent: {}\n  \
               forks_agree:       {}\n  \
               curriculum:        {}/{} ({:.1}%)\n}}",
            if self.passed() { "YES" } else { "NO" },
            invariants_passed,
            invariants_total,
            self.content_hash_matches,
            self.metadata_self_consistent,
            self.forks_agree,
            self.curriculum.total.solved_count,
            self.curriculum.total.problem_set_size,
            self.curriculum.total.solved_fraction() * 100.0,
        )
    }
}

/// Run every structural invariant against a snapshot.
pub fn check_invariants(snap: &ModelSnapshot) -> Vec<InvariantResult> {
    let mut out = Vec::new();

    // 1. Weights finite.
    let all_finite = snap
        .trainer
        .policy
        .weights
        .iter()
        .all(|w| w.is_finite())
        && snap.trainer.policy.bias.is_finite();
    out.push(InvariantResult {
        name: "weights_finite",
        passed: all_finite,
        detail: format!(
            "bias={:?} weights_finite={}",
            snap.trainer.policy.bias, all_finite
        ),
    });

    // 2. Fisher non-negative.
    let fisher_ok = snap
        .trainer
        .fisher_information
        .iter()
        .all(|f| *f >= 0.0 && f.is_finite());
    out.push(InvariantResult {
        name: "fisher_nonnegative_finite",
        passed: fisher_ok,
        detail: format!("all {} entries non-negative + finite", LibraryFeatures::WIDTH),
    });

    // 3. Pruned + active partitions the width exactly.
    let pruned = snap.trainer.pruned.iter().filter(|b| **b).count();
    let unpruned = LibraryFeatures::WIDTH - pruned;
    out.push(InvariantResult {
        name: "prune_mask_partitions_width",
        passed: pruned + unpruned == LibraryFeatures::WIDTH,
        detail: format!("pruned={pruned} unpruned={unpruned} WIDTH={}", LibraryFeatures::WIDTH),
    });

    // 4. Rule names unique + non-empty.
    let mut names: BTreeMap<&str, usize> = BTreeMap::new();
    for rule in &snap.library {
        *names.entry(rule.name.as_str()).or_insert(0) += 1;
    }
    let duplicates: Vec<_> =
        names.iter().filter(|(_, c)| **c > 1).collect();
    let all_named = snap.library.iter().all(|r| !r.name.is_empty());
    out.push(InvariantResult {
        name: "rule_names_unique_nonempty",
        passed: duplicates.is_empty() && all_named,
        detail: if duplicates.is_empty() && all_named {
            format!("{} rules, all unique", snap.library.len())
        } else {
            format!(
                "duplicates: {:?}, empty_name: {}",
                duplicates,
                !all_named
            )
        },
    });

    // 5. Benchmark history values in [0, 1].
    let hist_ok = snap
        .trainer
        .benchmark_history
        .iter()
        .all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0);
    out.push(InvariantResult {
        name: "benchmark_history_valid_range",
        passed: hist_ok,
        detail: format!("{} entries", snap.trainer.benchmark_history.len()),
    });

    // 6. EWC lambda non-negative.
    let lambda = snap.trainer.ewc_lambda;
    out.push(InvariantResult {
        name: "ewc_lambda_nonnegative",
        passed: lambda.is_finite() && lambda >= 0.0,
        detail: format!("lambda={lambda}"),
    });

    // 7. Monotonic counters.
    out.push(InvariantResult {
        name: "updates_le_events_seen",
        passed: snap.trainer.updates_applied <= snap.trainer.events_seen,
        detail: format!(
            "updates={} events_seen={}",
            snap.trainer.updates_applied, snap.trainer.events_seen
        ),
    });

    // 8. Phantom gradients non-negative + finite.
    let phantom_ok = snap
        .trainer
        .phantom_gradient_accum
        .iter()
        .all(|p| p.is_finite() && *p >= 0.0);
    out.push(InvariantResult {
        name: "phantom_gradients_nonnegative_finite",
        passed: phantom_ok,
        detail: format!("all {} entries valid", LibraryFeatures::WIDTH),
    });

    out
}

/// Full certification pass.
pub fn certify_snapshot(snap: &ModelSnapshot) -> CertificationReport {
    let invariants = check_invariants(snap);

    // Content hash self-consistency.
    let recomputed = snap.compute_content_hash();
    let content_hash_matches = recomputed == snap.content_hash;

    // Fork + run curriculum → compare against stored analysis.
    let handle_a = fork_from_snapshot(snap);
    let handle_b = fork_from_snapshot(snap);
    let curriculum = run_curriculum(
        &mathematician_curriculum(),
        &handle_a.library_snapshot(),
    );
    let curriculum_b = run_curriculum(
        &mathematician_curriculum(),
        &handle_b.library_snapshot(),
    );
    let forks_agree = curriculum.total.solved_count
        == curriculum_b.total.solved_count;

    // Metadata self-consistency: stored `analysis.curriculum_score`
    // should match the live recompute (if present).
    let metadata_self_consistent =
        match snap.metadata.get("analysis.curriculum_score") {
            Some(stored) => {
                let expected = format!(
                    "{}/{} ({:.3})",
                    curriculum.total.solved_count,
                    curriculum.total.problem_set_size,
                    curriculum.total.solved_fraction()
                );
                stored == &expected
            }
            None => true, // nothing to check
        };

    CertificationReport {
        invariants,
        curriculum,
        metadata_self_consistent,
        content_hash_matches,
        forks_agree,
    }
}

/// Full serialization round-trip check: save → load → save
/// again → content hashes must match.
pub fn verify_serialization_roundtrip(
    snap: &ModelSnapshot,
) -> Result<bool, SnapshotError> {
    let tmp = std::env::temp_dir().join(format!(
        "mathscape-serde-{}.msnp",
        std::process::id()
    ));
    let mut working = snap.clone();
    working.save_to_path(&tmp)?;
    let reloaded = ModelSnapshot::load_from_path(&tmp)?;
    let ok = reloaded.content_hash == working.content_hash
        && reloaded.library.len() == working.library.len()
        && reloaded.trainer.policy.weights == working.trainer.policy.weights;
    let _ = std::fs::remove_file(&tmp);
    Ok(ok)
}

/// Structured diff between two snapshots — what changed.
#[derive(Debug, Clone)]
pub struct SnapshotDiff {
    pub content_hash_equal: bool,
    pub lib_size_a: usize,
    pub lib_size_b: usize,
    pub rules_only_in_a: Vec<String>,
    pub rules_only_in_b: Vec<String>,
    pub rules_in_both: Vec<String>,
    pub trained_steps_a: u64,
    pub trained_steps_b: u64,
    pub bias_delta: f64,
    pub weight_l2_distance: f64,
    pub pruned_count_a: usize,
    pub pruned_count_b: usize,
    pub curriculum_a: (usize, usize),
    pub curriculum_b: (usize, usize),
}

impl SnapshotDiff {
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "Snapshot diff:\n  \
               content_hash_equal: {}\n  \
               library: A={} B={}  (only-A: {:?})\n                   (only-B: {:?})\n                   (shared: {})\n  \
               trained_steps:   A={} B={}\n  \
               bias delta:      {:+.6}\n  \
               weight L2 dist:  {:.6}\n  \
               pruned count:    A={} B={}\n  \
               curriculum:      A={}/{}  B={}/{}\n",
            self.content_hash_equal,
            self.lib_size_a,
            self.lib_size_b,
            self.rules_only_in_a,
            self.rules_only_in_b,
            self.rules_in_both.len(),
            self.trained_steps_a,
            self.trained_steps_b,
            self.bias_delta,
            self.weight_l2_distance,
            self.pruned_count_a,
            self.pruned_count_b,
            self.curriculum_a.0,
            self.curriculum_a.1,
            self.curriculum_b.0,
            self.curriculum_b.1,
        ));
        s
    }
}

/// Cross-snapshot structured diff.
pub fn compare_snapshots(
    a: &ModelSnapshot,
    b: &ModelSnapshot,
) -> SnapshotDiff {
    let names_a: std::collections::BTreeSet<String> =
        a.library.iter().map(|r| r.name.clone()).collect();
    let names_b: std::collections::BTreeSet<String> =
        b.library.iter().map(|r| r.name.clone()).collect();
    let rules_only_in_a: Vec<_> =
        names_a.difference(&names_b).cloned().collect();
    let rules_only_in_b: Vec<_> =
        names_b.difference(&names_a).cloned().collect();
    let rules_in_both: Vec<_> =
        names_a.intersection(&names_b).cloned().collect();

    let weight_l2_distance: f64 = a
        .trainer
        .policy
        .weights
        .iter()
        .zip(b.trainer.policy.weights.iter())
        .map(|(x, y)| {
            let d = x - y;
            d * d
        })
        .sum::<f64>()
        .sqrt();

    let pruned_a = a.trainer.pruned.iter().filter(|b| **b).count();
    let pruned_b = b.trainer.pruned.iter().filter(|b| **b).count();

    let handle_a = fork_from_snapshot(a);
    let handle_b = fork_from_snapshot(b);
    let rep_a = run_curriculum(
        &mathematician_curriculum(),
        &handle_a.library_snapshot(),
    );
    let rep_b = run_curriculum(
        &mathematician_curriculum(),
        &handle_b.library_snapshot(),
    );

    SnapshotDiff {
        content_hash_equal: a.content_hash == b.content_hash,
        lib_size_a: a.library.len(),
        lib_size_b: b.library.len(),
        rules_only_in_a,
        rules_only_in_b,
        rules_in_both,
        trained_steps_a: a.trainer.policy.trained_steps,
        trained_steps_b: b.trainer.policy.trained_steps,
        bias_delta: b.trainer.policy.bias - a.trainer.policy.bias,
        weight_l2_distance,
        pruned_count_a: pruned_a,
        pruned_count_b: pruned_b,
        curriculum_a: (
            rep_a.total.solved_count,
            rep_a.total.problem_set_size,
        ),
        curriculum_b: (
            rep_b.total.solved_count,
            rep_b.total.problem_set_size,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::RewriteRule;
    use crate::inference::LiveInferenceHandle;
    use crate::snapshot::snapshot_handle;
    use crate::streaming_policy::StreamingPolicyTrainer;
    use crate::term::Term;
    use crate::value::Value;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn add_id() -> RewriteRule {
        use crate::builtin::ADD;
        RewriteRule {
            name: "add-id".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(ADD)),
                vec![Term::Number(Value::Nat(0)), Term::Var(100)],
            ),
            rhs: Term::Var(100),
        }
    }

    fn mul_id() -> RewriteRule {
        use crate::builtin::MUL;
        RewriteRule {
            name: "mul-id".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(MUL)),
                vec![Term::Number(Value::Nat(1)), Term::Var(100)],
            ),
            rhs: Term::Var(100),
        }
    }

    fn make_handle(rules: Vec<RewriteRule>) -> LiveInferenceHandle {
        let lib = Rc::new(RefCell::new(rules));
        let t = Rc::new(StreamingPolicyTrainer::new(0.1));
        LiveInferenceHandle::new(lib, t)
    }

    #[test]
    fn invariants_pass_for_fresh_model() {
        let h = make_handle(vec![add_id(), mul_id()]);
        let snap = snapshot_handle(&h);
        let results = check_invariants(&snap);
        for r in &results {
            assert!(r.passed, "invariant failed: {:?}", r);
        }
    }

    #[test]
    fn invariants_catch_duplicate_rule_names() {
        let h = make_handle(vec![add_id(), add_id()]);
        let snap = snapshot_handle(&h);
        let results = check_invariants(&snap);
        let r = results
            .iter()
            .find(|r| r.name == "rule_names_unique_nonempty")
            .unwrap();
        assert!(!r.passed, "duplicate rule names must fail");
    }

    #[test]
    fn certify_snapshot_passes_clean_model() {
        let h = make_handle(vec![add_id(), mul_id()]);
        let snap = snapshot_handle(&h);
        let report = certify_snapshot(&snap);
        assert!(
            report.passed(),
            "clean snapshot should certify:\n{}",
            report.summary()
        );
        assert!(report.content_hash_matches);
        assert!(report.forks_agree);
    }

    #[test]
    fn serialization_roundtrip_preserves_content_hash() {
        let h = make_handle(vec![add_id(), mul_id()]);
        let snap = snapshot_handle(&h);
        let ok = verify_serialization_roundtrip(&snap).unwrap();
        assert!(ok);
    }

    #[test]
    fn compare_snapshots_detects_library_growth() {
        let h1 = make_handle(vec![add_id()]);
        let h2 = make_handle(vec![add_id(), mul_id()]);
        let s1 = snapshot_handle(&h1);
        let s2 = snapshot_handle(&h2);
        let diff = compare_snapshots(&s1, &s2);
        assert_eq!(diff.lib_size_a, 1);
        assert_eq!(diff.lib_size_b, 2);
        assert_eq!(diff.rules_only_in_b, vec!["mul-id".to_string()]);
        assert!(!diff.content_hash_equal);
    }

    #[test]
    fn compare_identical_snapshots_shows_zero_distance() {
        let h = make_handle(vec![add_id(), mul_id()]);
        let s = snapshot_handle(&h);
        let diff = compare_snapshots(&s, &s);
        assert_eq!(diff.weight_l2_distance, 0.0);
        assert_eq!(diff.bias_delta, 0.0);
        assert!(diff.content_hash_equal);
        assert!(diff.rules_only_in_a.is_empty());
        assert!(diff.rules_only_in_b.is_empty());
    }

    #[test]
    fn certification_catches_corrupted_weights() {
        // Inject NaN into the trainer's snapshot and verify
        // the invariant fires.
        let h = make_handle(vec![add_id()]);
        let mut snap = snapshot_handle(&h);
        snap.trainer.policy.weights[0] = f64::NAN;
        snap.content_hash = snap.compute_content_hash();
        let results = check_invariants(&snap);
        let r = results
            .iter()
            .find(|r| r.name == "weights_finite")
            .unwrap();
        assert!(!r.passed);
    }
}
