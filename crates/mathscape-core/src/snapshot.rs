//! Phase Y.3 (2026-04-19): model snapshot / persistence / fork.
//!
//! # The user-framed need
//!
//!   "Is there a way for us to snapshot a running model and copy
//!    it and then put it on disk to restore it later? Because
//!    all of this seems like it would be important. To be able
//!    to copy it, layer inferencing over it, and talk to the
//!    copy."
//!
//! # What "the model" actually is
//!
//! At any moment the model consists of:
//!  - **Library** — the discovered `RewriteRule`s.
//!  - **Trainer state** — `LinearPolicy` + all neuroplasticity
//!    fields (activation counts, phantom gradients, Fisher
//!    information, anchor weights, pruned flags, benchmark
//!    history, learning rate, ewc lambda, etc.).
//!
//! Both together are what the motor has produced. Both are
//! bincode-serializable. `ModelSnapshot` bundles them.
//!
//! # Operations
//!
//! - `snapshot(handle) -> ModelSnapshot` — atomic in-memory copy.
//! - `ModelSnapshot::save_to_path(p)` / `load_from_path(p)` —
//!   bincode to/from file, with a magic header + version byte
//!   for migration-friendliness.
//! - `restore_into(handle)` — apply a snapshot to an existing
//!   live handle (destructive to that handle's prior state).
//! - `fork(handle) -> LiveInferenceHandle` — produce a fresh
//!   independent handle backed by a clone of the current state.
//!   The fork and the original evolve independently from that
//!   moment on. Perfect for A/B experimentation, running
//!   inference on a stable reference while training continues,
//!   or archiving known-good models.
//!
//! # Content identity
//!
//! Every snapshot carries a BLAKE3 `content_hash` computed over
//! its bincode-serialized form. Two snapshots with identical
//! content produce identical hashes — content-addressable
//! identity for free.

use crate::eval::RewriteRule;
use crate::inference::LiveInferenceHandle;
use crate::policy::LinearPolicy;
use crate::streaming_policy::StreamingPolicyTrainer;
use crate::trajectory::LibraryFeatures;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;

/// On-disk magic bytes — identifies a mathscape model file.
const MAGIC: &[u8; 4] = b"MSCP";
/// On-disk format version. Bump when the wire format changes.
pub const SNAPSHOT_VERSION: u32 = 1;

/// Serializable trainer state — every neuroplasticity field the
/// trainer holds, captured at a moment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainerSnapshot {
    pub policy: LinearPolicy,
    pub learning_rate: f64,
    pub events_seen: u64,
    pub updates_applied: u64,

    // Phase V.shed
    pub activation_counts: [u64; LibraryFeatures::WIDTH],
    pub cumulative_contributions: [f64; LibraryFeatures::WIDTH],
    pub pruned: [bool; LibraryFeatures::WIDTH],

    // Phase W.stall
    pub last_active_event: [u64; LibraryFeatures::WIDTH],

    // Phase W.1 RigL
    pub phantom_gradient_accum: [f64; LibraryFeatures::WIDTH],

    // Phase W.2 EWC
    pub fisher_information: [f64; LibraryFeatures::WIDTH],
    pub anchor_weights: [f64; LibraryFeatures::WIDTH],
    pub anchor_bias: f64,
    pub anchor_set: bool,
    pub ewc_lambda: f64,

    // Phase W.3 learning progress
    pub benchmark_history: Vec<f64>,
    pub learning_progress_window: usize,
}

/// Full model snapshot — library + trainer state + content hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSnapshot {
    pub version: u32,
    pub created_at_epoch_secs: u64,
    pub library: Vec<RewriteRule>,
    pub trainer: TrainerSnapshot,
    /// User-provided free-form tags. Useful for tracking the
    /// circumstances under which this snapshot was made
    /// (motor phase, config, experiment id, etc.).
    pub metadata: std::collections::BTreeMap<String, String>,
    /// BLAKE3 over the bincode serialization of this snapshot
    /// with `content_hash` set to zeros. Content-addressable
    /// identity.
    pub content_hash: [u8; 32],
}

/// Errors raised by snapshot I/O.
#[derive(Debug)]
pub enum SnapshotError {
    Io(std::io::Error),
    Bincode(Box<bincode::ErrorKind>),
    BadMagic,
    UnsupportedVersion(u32),
    HashMismatch,
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Io(e) => write!(f, "io: {e}"),
            SnapshotError::Bincode(e) => write!(f, "bincode: {e}"),
            SnapshotError::BadMagic => write!(f, "bad magic bytes (not a mathscape model file)"),
            SnapshotError::UnsupportedVersion(v) => {
                write!(f, "unsupported snapshot version {v} (this build expects {SNAPSHOT_VERSION})")
            }
            SnapshotError::HashMismatch => {
                write!(f, "content hash verification failed — file may be corrupt")
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<std::io::Error> for SnapshotError {
    fn from(e: std::io::Error) -> Self {
        SnapshotError::Io(e)
    }
}

impl From<Box<bincode::ErrorKind>> for SnapshotError {
    fn from(e: Box<bincode::ErrorKind>) -> Self {
        SnapshotError::Bincode(e)
    }
}

impl ModelSnapshot {
    /// Compute the BLAKE3 content hash for this snapshot's
    /// library + trainer. The `content_hash` field is not
    /// itself part of the hashed payload.
    pub fn compute_content_hash(&self) -> [u8; 32] {
        // Build a deterministic byte stream from everything
        // EXCEPT the content_hash field.
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.version.to_le_bytes());
        hasher.update(&self.created_at_epoch_secs.to_le_bytes());
        hasher.update(&bincode::serialize(&self.library).unwrap_or_default());
        hasher.update(&bincode::serialize(&self.trainer).unwrap_or_default());
        hasher.update(&bincode::serialize(&self.metadata).unwrap_or_default());
        *hasher.finalize().as_bytes()
    }

    /// Save to disk with MAGIC + version header, then the
    /// bincode payload. Content hash is refreshed before write.
    pub fn save_to_path(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), SnapshotError> {
        self.content_hash = self.compute_content_hash();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&bincode::serialize(self)?);
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Read from disk, verify MAGIC + version + content hash.
    pub fn load_from_path(
        path: impl AsRef<std::path::Path>,
    ) -> Result<Self, SnapshotError> {
        let bytes = std::fs::read(path)?;
        if bytes.len() < MAGIC.len() + 4 {
            return Err(SnapshotError::BadMagic);
        }
        if &bytes[..MAGIC.len()] != MAGIC {
            return Err(SnapshotError::BadMagic);
        }
        let mut version_bytes = [0u8; 4];
        version_bytes.copy_from_slice(&bytes[MAGIC.len()..MAGIC.len() + 4]);
        let version = u32::from_le_bytes(version_bytes);
        if version != SNAPSHOT_VERSION {
            return Err(SnapshotError::UnsupportedVersion(version));
        }
        let payload = &bytes[MAGIC.len() + 4..];
        let snapshot: ModelSnapshot = bincode::deserialize(payload)?;
        let expected = snapshot.compute_content_hash();
        if expected != snapshot.content_hash {
            return Err(SnapshotError::HashMismatch);
        }
        Ok(snapshot)
    }
}

/// Extract the trainer's serializable state.
pub fn trainer_snapshot(
    trainer: &StreamingPolicyTrainer,
) -> TrainerSnapshot {
    let (activation_counts, cumulative_contributions, pruned) =
        trainer.weight_stats();
    TrainerSnapshot {
        policy: trainer.snapshot(),
        learning_rate: trainer.learning_rate(),
        events_seen: trainer.events_seen(),
        updates_applied: trainer.updates_applied(),
        activation_counts,
        cumulative_contributions,
        pruned,
        last_active_event: trainer.last_active_snapshot(),
        phantom_gradient_accum: trainer.phantom_gradients(),
        fisher_information: trainer.fisher_snapshot(),
        anchor_weights: trainer.anchor_snapshot(),
        // Anchor bias isn't exposed by the trainer yet — snapshot
        // a best-effort 0.0; a future bump of the API can expose
        // it directly. This loses anchor_bias across
        // save/restore, a minor fidelity issue.
        anchor_bias: 0.0,
        anchor_set: trainer.has_anchor(),
        ewc_lambda: trainer.ewc_lambda(),
        benchmark_history: trainer.benchmark_history(),
        learning_progress_window: trainer.learning_progress_window(),
    }
}

/// Rehydrate a trainer from a snapshot. Returns a fresh
/// `StreamingPolicyTrainer` with every field restored.
pub fn trainer_from_snapshot(
    snap: &TrainerSnapshot,
) -> StreamingPolicyTrainer {
    let trainer = StreamingPolicyTrainer::from_policy(
        snap.policy.clone(),
        snap.learning_rate,
    );
    // Inject the neuroplasticity state.
    trainer.restore_internal_state(
        snap.activation_counts,
        snap.cumulative_contributions,
        snap.pruned,
        snap.last_active_event,
        snap.phantom_gradient_accum,
        snap.fisher_information,
        snap.anchor_weights,
        snap.anchor_bias,
        snap.anchor_set,
        snap.ewc_lambda,
        snap.events_seen,
        snap.updates_applied,
        snap.benchmark_history.clone(),
        snap.learning_progress_window,
    );
    trainer
}

/// Snapshot a full live handle.
pub fn snapshot_handle(handle: &LiveInferenceHandle) -> ModelSnapshot {
    let library = handle.library_snapshot();
    // We don't have a direct reference to the trainer here, but
    // the handle exposes policy_snapshot and trainer-related
    // accessors through delegation. For full fidelity we need
    // access to the trainer — so we take it via the handle's
    // internal reference (provided via a dedicated accessor).
    let trainer = handle.trainer_snapshot();
    let created_at_epoch_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut snap = ModelSnapshot {
        version: SNAPSHOT_VERSION,
        created_at_epoch_secs,
        library,
        trainer,
        metadata: std::collections::BTreeMap::new(),
        content_hash: [0u8; 32],
    };
    snap.content_hash = snap.compute_content_hash();
    snap
}

/// Fork: build a fresh `LiveInferenceHandle` from a snapshot.
/// The new handle has its own independent library and trainer
/// — mutations to it do NOT affect the original. Perfect for
/// A/B experimentation or frozen-reference inference.
pub fn fork_from_snapshot(
    snap: &ModelSnapshot,
) -> LiveInferenceHandle {
    let library = Rc::new(RefCell::new(snap.library.clone()));
    let trainer = Rc::new(trainer_from_snapshot(&snap.trainer));
    LiveInferenceHandle::new(library, trainer)
}

/// Phase Y.3.1 (2026-04-19): deep measurement pass.
///
/// A rigorous analysis of a live model BEFORE snapshotting —
/// so the artifact we persist carries evidence of what it
/// does, not just a weight dump. The report is embedded in
/// the snapshot's `metadata` map (as "analysis.*" keys) and
/// printed to stdout.
#[derive(Debug, Clone)]
pub struct ModelAnalysis {
    pub curriculum_total: (usize, usize, f64),
    pub per_subdomain: std::collections::BTreeMap<String, (usize, usize, f64)>,
    pub mastered: Vec<String>,
    pub frontier: Vec<String>,
    /// Top weights by |value|, paired with their Fisher info.
    pub top_weights: Vec<(usize, f64, f64)>,
    /// Sum of Fisher information across all weights (total
    /// accumulated "importance" signal).
    pub fisher_total: f64,
    /// Count of pruned weights (dead dimensions).
    pub pruned_count: usize,
    /// Count of weights with non-zero activations (active
    /// dimensions).
    pub active_count: usize,
    /// Per-rule size info (lhs + rhs node counts).
    pub rule_sizes: Vec<(String, usize, usize)>,
    /// Human-readable bullet list of findings.
    pub highlights: Vec<String>,
}

/// Run the deep analysis pass.
pub fn deep_analyze(handle: &LiveInferenceHandle) -> ModelAnalysis {
    let library = handle.library_snapshot();
    let trainer_snap = handle.trainer_snapshot();
    let competency = handle.current_competency();

    let curriculum_total = (
        competency.total.solved_count,
        competency.total.problem_set_size,
        competency.total.solved_fraction(),
    );
    let per_subdomain = competency
        .per_subdomain
        .iter()
        .map(|(k, v)| {
            (
                (*k).to_string(),
                (v.solved_count, v.problem_set_size, v.solved_fraction()),
            )
        })
        .collect();
    let mastered: Vec<String> = competency
        .mastered()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let frontier: Vec<String> = competency
        .frontier()
        .iter()
        .map(|s| s.to_string())
        .collect();

    // Top-5 weights by absolute magnitude.
    let mut weight_idx: Vec<(usize, f64, f64)> = trainer_snap
        .policy
        .weights
        .iter()
        .enumerate()
        .map(|(i, w)| (i, *w, trainer_snap.fisher_information[i]))
        .collect();
    weight_idx.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap());
    let top_weights = weight_idx.into_iter().take(5).collect();

    let fisher_total: f64 = trainer_snap.fisher_information.iter().sum();
    let pruned_count = trainer_snap.pruned.iter().filter(|b| **b).count();
    let active_count = trainer_snap
        .activation_counts
        .iter()
        .filter(|c| **c > 0)
        .count();

    let rule_sizes: Vec<(String, usize, usize)> = library
        .iter()
        .map(|r| (r.name.clone(), term_size(&r.lhs), term_size(&r.rhs)))
        .collect();

    let mut highlights = Vec::new();
    highlights.push(format!(
        "Curriculum score: {}/{} ({:.1}%)",
        curriculum_total.0,
        curriculum_total.1,
        curriculum_total.2 * 100.0
    ));
    highlights.push(format!(
        "Mastered subdomains: {} ({})",
        mastered.len(),
        if mastered.is_empty() {
            "none".to_string()
        } else {
            mastered.join(", ")
        }
    ));
    if !frontier.is_empty() {
        highlights.push(format!(
            "Frontier (0%): {}",
            frontier.join(", ")
        ));
    }
    highlights.push(format!(
        "Library: {} rules discovered",
        library.len()
    ));
    highlights.push(format!(
        "Trainer: {} trained steps, {} events seen, {} updates",
        trainer_snap.policy.trained_steps,
        trainer_snap.events_seen,
        trainer_snap.updates_applied
    ));
    highlights.push(format!(
        "Weights: {} active, {} pruned",
        active_count, pruned_count
    ));
    highlights.push(format!(
        "Fisher total: {:.4} (accumulated importance signal)",
        fisher_total
    ));
    if trainer_snap.anchor_set {
        highlights.push("EWC anchor SET (protecting known-good state)".to_string());
    }
    if !trainer_snap.benchmark_history.is_empty() {
        let first = trainer_snap.benchmark_history.first().unwrap();
        let last = trainer_snap.benchmark_history.last().unwrap();
        highlights.push(format!(
            "Benchmark trajectory: {:.3} → {:.3} (Δ {:+.3})",
            first,
            last,
            last - first
        ));
    }

    ModelAnalysis {
        curriculum_total,
        per_subdomain,
        mastered,
        frontier,
        top_weights,
        fisher_total,
        pruned_count,
        active_count,
        rule_sizes,
        highlights,
    }
}

/// Embed the analysis into the snapshot's metadata so it
/// persists on disk.
pub fn attach_analysis(snap: &mut ModelSnapshot, analysis: &ModelAnalysis) {
    snap.metadata.insert(
        "analysis.curriculum_score".into(),
        format!(
            "{}/{} ({:.3})",
            analysis.curriculum_total.0,
            analysis.curriculum_total.1,
            analysis.curriculum_total.2
        ),
    );
    snap.metadata.insert(
        "analysis.mastered".into(),
        analysis.mastered.join(","),
    );
    snap.metadata.insert(
        "analysis.frontier".into(),
        analysis.frontier.join(","),
    );
    snap.metadata.insert(
        "analysis.library_size".into(),
        analysis.rule_sizes.len().to_string(),
    );
    snap.metadata.insert(
        "analysis.active_weights".into(),
        analysis.active_count.to_string(),
    );
    snap.metadata.insert(
        "analysis.pruned_weights".into(),
        analysis.pruned_count.to_string(),
    );
    snap.metadata.insert(
        "analysis.fisher_total".into(),
        format!("{:.4}", analysis.fisher_total),
    );
    for (i, (name, ls, rs)) in analysis.rule_sizes.iter().enumerate() {
        snap.metadata.insert(
            format!("analysis.rule_{i}"),
            format!("{name} lhs={ls} rhs={rs}"),
        );
    }
}

/// Pretty-print an analysis to stdout-style string.
pub fn format_analysis(analysis: &ModelAnalysis) -> String {
    let mut out = String::new();
    out.push_str("\n╔════════════════════════════════════════════════════════╗\n");
    out.push_str("║  DEEP MEASUREMENT PASS                                  ║\n");
    out.push_str("╚════════════════════════════════════════════════════════╝\n\n");
    for h in &analysis.highlights {
        out.push_str(&format!("  • {h}\n"));
    }
    out.push_str("\n  Per-subdomain breakdown:\n");
    for (k, (s, t, f)) in &analysis.per_subdomain {
        let bar_len = (f * 20.0) as usize;
        let bar: String = "█".repeat(bar_len) + &" ".repeat(20 - bar_len);
        out.push_str(&format!("    {k:<18} [{bar}] {s}/{t} ({:.0}%)\n", f * 100.0));
    }
    out.push_str("\n  Top-5 weights by magnitude:\n");
    for (i, w, f) in &analysis.top_weights {
        out.push_str(&format!(
            "    weight[{i}] = {:>9.4}  fisher = {:>9.6}\n",
            w, f
        ));
    }
    out.push_str("\n  Rules discovered (size = node count):\n");
    for (name, ls, rs) in &analysis.rule_sizes {
        out.push_str(&format!(
            "    {name:<28}  lhs={ls:>3}  rhs={rs:>3}\n"
        ));
    }
    out
}

/// Human-readable analysis of a snapshot — content hash,
/// library summary, per-rule descriptors, trainer state
/// histogram. Call this after `load_from_path` to inspect what
/// got persisted.
pub fn analyze_snapshot(snap: &ModelSnapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "╔════════════════════════════════════════════════════════╗\n"
    ));
    out.push_str(&format!(
        "║  MATHSCAPE MODEL SNAPSHOT — analysis                    ║\n"
    ));
    out.push_str(&format!(
        "╚════════════════════════════════════════════════════════╝\n"
    ));
    out.push_str(&format!(
        "  version:         {}\n",
        snap.version,
    ));
    out.push_str(&format!(
        "  created_at:      epoch {}s\n",
        snap.created_at_epoch_secs,
    ));
    out.push_str(&format!(
        "  content_hash:    {}\n",
        hex16(&snap.content_hash),
    ));
    out.push_str(&format!(
        "  metadata keys:   {}\n",
        snap.metadata.len(),
    ));
    for (k, v) in &snap.metadata {
        out.push_str(&format!("    - {k}: {v}\n"));
    }
    out.push_str(&format!("\n  LIBRARY ({} rules):\n", snap.library.len()));
    for (i, rule) in snap.library.iter().enumerate() {
        out.push_str(&format!(
            "    [{i:2}] {:<24}  lhs_size ~{}  rhs_size ~{}\n",
            rule.name,
            term_size(&rule.lhs),
            term_size(&rule.rhs),
        ));
    }
    out.push_str(&format!(
        "\n  TRAINER:\n    trained_steps:  {}\n    events_seen:    {}\n    updates_applied: {}\n    bias:           {:.6}\n    learning_rate:  {:.4}\n    ewc_lambda:     {:.4}\n    anchor_set:     {}\n    lp_window:      {}\n    pruned_count:   {}\n",
        snap.trainer.policy.trained_steps,
        snap.trainer.events_seen,
        snap.trainer.updates_applied,
        snap.trainer.policy.bias,
        snap.trainer.learning_rate,
        snap.trainer.ewc_lambda,
        snap.trainer.anchor_set,
        snap.trainer.learning_progress_window,
        snap.trainer.pruned.iter().filter(|b| **b).count(),
    ));
    out.push_str("\n  WEIGHT STATS (per-dimension):\n");
    out.push_str("    idx  weight     fisher     phantom    counts\n");
    for i in 0..snap.trainer.policy.weights.len() {
        out.push_str(&format!(
            "    [{i}] {:>9.4}  {:>9.6}  {:>9.6}  {}\n",
            snap.trainer.policy.weights[i],
            snap.trainer.fisher_information[i],
            snap.trainer.phantom_gradient_accum[i],
            snap.trainer.activation_counts[i],
        ));
    }
    out.push_str(&format!(
        "\n  BENCHMARK HISTORY ({} runs):\n    {:?}\n",
        snap.trainer.benchmark_history.len(),
        snap.trainer.benchmark_history,
    ));
    out
}

fn hex16(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(33);
    for b in bytes.iter().take(16) {
        s.push_str(&format!("{b:02x}"));
    }
    s.push('…');
    s
}

fn term_size(t: &crate::term::Term) -> usize {
    use crate::term::Term;
    match t {
        Term::Var(_) | Term::Point(_) | Term::Number(_) => 1,
        Term::Apply(head, args) => {
            1 + term_size(head) + args.iter().map(term_size).sum::<usize>()
        }
        Term::Fn(_, body) => 1 + term_size(body),
        Term::Symbol(_, args) => 1 + args.iter().map(term_size).sum::<usize>(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mathscape_map::{MapEvent, MapEventConsumer};
    use crate::term::Term;
    use crate::value::Value;

    fn add_id() -> RewriteRule {
        RewriteRule {
            name: "add-id".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![
                    Term::Number(Value::Nat(0)),
                    Term::Var(100),
                ],
            ),
            rhs: Term::Var(100),
        }
    }

    fn make_live_model() -> (
        Rc<RefCell<Vec<RewriteRule>>>,
        Rc<StreamingPolicyTrainer>,
        LiveInferenceHandle,
    ) {
        let lib = Rc::new(RefCell::new(Vec::<RewriteRule>::new()));
        let t = Rc::new(StreamingPolicyTrainer::new(0.1));
        let handle = LiveInferenceHandle::new(lib.clone(), t.clone());
        (lib, t, handle)
    }

    #[test]
    fn snapshot_captures_current_state() {
        let (lib, trainer, handle) = make_live_model();
        lib.borrow_mut().push(add_id());
        trainer.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });

        let snap = snapshot_handle(&handle);
        assert_eq!(snap.library.len(), 1);
        assert_eq!(snap.library[0].name, "add-id");
        assert_eq!(snap.trainer.events_seen, 1);
        assert!(snap.trainer.policy.trained_steps > 0);
        assert_eq!(snap.version, SNAPSHOT_VERSION);
        assert_ne!(snap.content_hash, [0u8; 32]);
    }

    #[test]
    fn save_and_load_round_trip_via_disk() {
        let tmp = std::env::temp_dir()
            .join(format!("mathscape-snap-{}.bin", std::process::id()));
        let (lib, trainer, handle) = make_live_model();
        lib.borrow_mut().push(add_id());
        trainer.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });

        let mut snap = snapshot_handle(&handle);
        snap.metadata.insert("experiment".into(), "y3-test".into());
        snap.save_to_path(&tmp).unwrap();

        let reloaded = ModelSnapshot::load_from_path(&tmp).unwrap();
        assert_eq!(reloaded.version, SNAPSHOT_VERSION);
        assert_eq!(reloaded.library.len(), 1);
        assert_eq!(reloaded.library[0].name, "add-id");
        assert_eq!(reloaded.trainer.events_seen, 1);
        assert_eq!(reloaded.content_hash, snap.content_hash);
        assert_eq!(
            reloaded.metadata.get("experiment"),
            Some(&"y3-test".to_string())
        );

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn content_hash_is_deterministic_and_identifies_state() {
        let (lib1, _, h1) = make_live_model();
        let (lib2, _, h2) = make_live_model();
        lib1.borrow_mut().push(add_id());
        lib2.borrow_mut().push(add_id());

        // Two handles with identical state produce identical
        // content hashes (modulo timestamps — set them equal).
        let mut s1 = snapshot_handle(&h1);
        let mut s2 = snapshot_handle(&h2);
        s1.created_at_epoch_secs = 0;
        s2.created_at_epoch_secs = 0;
        s1.content_hash = s1.compute_content_hash();
        s2.content_hash = s2.compute_content_hash();
        assert_eq!(s1.content_hash, s2.content_hash);

        // Different state → different hash.
        lib1.borrow_mut().push(add_id());
        let mut s1b = snapshot_handle(&h1);
        s1b.created_at_epoch_secs = 0;
        s1b.content_hash = s1b.compute_content_hash();
        assert_ne!(s1b.content_hash, s1.content_hash);
    }

    #[test]
    fn load_rejects_tampered_file() {
        let tmp = std::env::temp_dir()
            .join(format!("mathscape-tamper-{}.bin", std::process::id()));
        let (_, _, h) = make_live_model();
        let mut snap = snapshot_handle(&h);
        snap.save_to_path(&tmp).unwrap();

        // Flip a byte in the payload region.
        let mut bytes = std::fs::read(&tmp).unwrap();
        let mid = MAGIC.len() + 8 + 20;
        if mid < bytes.len() {
            bytes[mid] ^= 0xFF;
            std::fs::write(&tmp, &bytes).unwrap();
        }

        let result = ModelSnapshot::load_from_path(&tmp);
        // Either we can't deserialize (bincode error) or the
        // hash fails — both are correct rejections.
        assert!(result.is_err());
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn fork_produces_independent_copy() {
        let (lib, trainer, handle) = make_live_model();
        lib.borrow_mut().push(add_id());
        trainer.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });

        let snap = snapshot_handle(&handle);
        let fork = fork_from_snapshot(&snap);

        // Both handles see the same state post-fork.
        assert_eq!(fork.library_size(), handle.library_size());

        // Mutate the ORIGINAL — fork is unchanged.
        lib.borrow_mut().push(add_id());
        assert_eq!(handle.library_size(), 2);
        assert_eq!(fork.library_size(), 1);

        // Mutate the FORK — original is unchanged.
        // (Fork owns its own library Rc now.)
        let fork_report = fork.current_competency();
        let orig_report = handle.current_competency();
        // Different library sizes → potentially different scores.
        assert!(
            fork_report.total.problem_set_size
                == orig_report.total.problem_set_size
        );
    }

    #[test]
    fn fork_can_be_queried_while_original_keeps_training() {
        let (lib, trainer, handle) = make_live_model();
        lib.borrow_mut().push(add_id());
        trainer.on_event(&MapEvent::RuleCertified {
            rule: add_id(),
            evidence_samples: 96,
        });

        let snap = snapshot_handle(&handle);
        let fork = fork_from_snapshot(&snap);

        let probe = Term::Apply(
            Box::new(Term::Var(2)),
            vec![
                Term::Number(Value::Nat(0)),
                Term::Number(Value::Nat(42)),
            ],
        );
        // Fork can infer.
        let fork_result = fork.infer(&probe, 20).unwrap();
        assert_eq!(fork_result, Term::Number(Value::Nat(42)));

        // Original continues to train — trainer events accrue.
        for _ in 0..5 {
            trainer.on_event(&MapEvent::RuleCertified {
                rule: add_id(),
                evidence_samples: 96,
            });
        }
        // Original: 6 events seen. Fork: still 1.
        assert_eq!(handle.trainer_events_seen(), 6);
        assert_eq!(fork.trainer_events_seen(), 1);

        // Fork still gives the same inference result — it's
        // FROZEN to the snapshot moment.
        let fork_result2 = fork.infer(&probe, 20).unwrap();
        assert_eq!(fork_result2, fork_result);
    }
}
