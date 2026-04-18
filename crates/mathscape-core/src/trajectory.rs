//! R9 — Trajectory recording and state featurization.
//!
//! # The learning vision
//!
//! Mathscape's autonomous traversal is a sequence of decisions:
//! at each epoch the machine considers candidate rules, accepts
//! some, rejects others. When a rule is accepted, the library
//! gains structure; rejected candidates are evidence about what
//! shape of rule was NOT useful at that state.
//!
//! A `Trajectory` is the training data for a future scorer that
//! learns which candidates to prioritize given a library state.
//! The scorer starts as a linear function of structural features
//! and evolves into a neural network as the trajectory corpus
//! grows. The ultimate goal: a model that given any library state
//! predicts which candidate rule most advances toward tensor
//! emergence.
//!
//! # Why this matters for "evolving to the tensor"
//!
//! The R8 detector tells us when tensor structure IS present.
//! But the machine's path TO tensor is long and expensive. A
//! learned policy — trained on trajectories that reached tensor
//! faster — compresses the search. Over many runs, the scorer
//! accumulates knowledge about "what shape of library leads to
//! tensor next", letting later runs take shorter paths.
//!
//! # Scope of this module
//!
//! - `TrajectoryStep` — one epoch's decision + outcome
//! - `Trajectory` — the full sequence
//! - `LibraryFeatures` — a fixed-width feature vector extracted
//!   from a library. Includes R8's tensor density.
//! - `features_of(rules)` — the featurizer
//!
//! # Not in scope (later work)
//!
//! - The scorer itself (linear / MLP / NN)
//! - Training loop (wake-sleep)
//! - Candidate-level features (scorer inputs beyond state)
//! - Trajectory persistence to disk

use crate::eval::RewriteRule;
use crate::tensor;
use crate::term::Term;
use serde::{Deserialize, Serialize};

/// One step in a traversal — what the machine considered and did.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryStep {
    /// Absolute epoch index within the traversal.
    pub epoch: usize,
    /// Corpus index being processed at this step (if the
    /// traversal iterates corpora; else 0).
    pub corpus_index: usize,
    /// Features of the library BEFORE this step's action. The
    /// scorer's input — what we want to learn to predict from.
    pub pre_state: LibraryFeatures,
    /// What action the machine took.
    pub action: ActionKind,
    /// Whether the candidate was accepted into the library.
    pub accepted: bool,
    /// ΔDL reward observed. Positive = compression improved.
    pub delta_dl: f64,
}

/// The kind of action a step represents. Mirrors the allocator's
/// `EpochAction` enum but stays at the trajectory level — we don't
/// couple to the allocator here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    /// Propose a novel rule (compression / meta generator).
    Discover,
    /// Apply existing rules to reduce the corpus or library.
    Reinforce,
    /// Bump a mechanism parameter (ML4 self-mutation).
    Mutate,
}

/// Fixed-width structural feature vector extracted from a library.
/// Stays deliberately small and interpretable — the scorer's input
/// space should be auditable. A learned model can always augment
/// this with learned embeddings later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryFeatures {
    /// Total rule count.
    pub rule_count: usize,
    /// Mean LHS size (node count) across all rules.
    pub mean_lhs_size: f64,
    /// Mean RHS size across all rules.
    pub mean_rhs_size: f64,
    /// Mean compression ratio: size(lhs) / size(rhs). Greater than
    /// 1 means rules reduce. < 1 means rules expand (rare, usually
    /// a bug).
    pub mean_compression: f64,
    /// R8 tensor density — fraction of rules with tensor shape.
    /// Central feature: this is the signal that "tensor emerged"
    /// and what the policy ultimately tries to increase.
    pub tensor_density: f64,
    /// Count of distributive-shape rules (concrete ops).
    pub tensor_distributive_count: usize,
    /// Count of meta-distributive rules (op-variable abstraction).
    pub tensor_meta_count: usize,
    /// Distinct operator ids appearing as Apply heads. Proxy for
    /// vocabulary breadth.
    pub distinct_heads: usize,
    /// Max rule depth — complexity ceiling in the library.
    pub max_rule_depth: usize,
}

impl LibraryFeatures {
    /// Extract features from a library. Pure function — same
    /// library always yields the same features. The trajectory
    /// scorer treats this as the state encoding.
    #[must_use]
    pub fn extract(rules: &[RewriteRule]) -> Self {
        if rules.is_empty() {
            return Self {
                rule_count: 0,
                mean_lhs_size: 0.0,
                mean_rhs_size: 0.0,
                mean_compression: 0.0,
                tensor_density: 0.0,
                tensor_distributive_count: 0,
                tensor_meta_count: 0,
                distinct_heads: 0,
                max_rule_depth: 0,
            };
        }

        let n = rules.len();
        let lhs_sizes: Vec<usize> = rules.iter().map(|r| r.lhs.size()).collect();
        let rhs_sizes: Vec<usize> = rules.iter().map(|r| r.rhs.size()).collect();
        let mean_lhs =
            lhs_sizes.iter().sum::<usize>() as f64 / n as f64;
        let mean_rhs =
            rhs_sizes.iter().sum::<usize>() as f64 / n as f64;

        // Compression is lhs_size / rhs_size per rule, averaged.
        // Guard div-by-zero: an rhs of 0 shouldn't happen but if
        // it did we'd treat the ratio as 1 (neutral).
        let mean_compression = rules
            .iter()
            .zip(lhs_sizes.iter().zip(rhs_sizes.iter()))
            .map(|(_, (l, r))| {
                if *r == 0 {
                    1.0
                } else {
                    *l as f64 / *r as f64
                }
            })
            .sum::<f64>()
            / n as f64;

        let (dist_count, meta_count, _none) = tensor::shape_counts(rules);
        let tensor_density = tensor::tensor_density(rules);

        // Distinct heads: walk each rule's LHS and RHS, collect
        // operator ids (Apply heads that are Var(id)).
        let mut heads = std::collections::BTreeSet::new();
        for r in rules {
            collect_heads(&r.lhs, &mut heads);
            collect_heads(&r.rhs, &mut heads);
        }
        let distinct_heads = heads.len();

        let max_rule_depth = rules
            .iter()
            .map(|r| r.lhs.depth().max(r.rhs.depth()))
            .max()
            .unwrap_or(0);

        Self {
            rule_count: n,
            mean_lhs_size: mean_lhs,
            mean_rhs_size: mean_rhs,
            mean_compression,
            tensor_density,
            tensor_distributive_count: dist_count,
            tensor_meta_count: meta_count,
            distinct_heads,
            max_rule_depth,
        }
    }

    /// Flatten to a `Vec<f64>` — the input to a linear or neural
    /// scorer. Order matches the struct's declaration. Stable
    /// across versions of this module (add new fields at the end
    /// if you extend).
    #[must_use]
    pub fn as_vector(&self) -> Vec<f64> {
        vec![
            self.rule_count as f64,
            self.mean_lhs_size,
            self.mean_rhs_size,
            self.mean_compression,
            self.tensor_density,
            self.tensor_distributive_count as f64,
            self.tensor_meta_count as f64,
            self.distinct_heads as f64,
            self.max_rule_depth as f64,
        ]
    }

    /// Number of features. A scorer's input dimension.
    pub const WIDTH: usize = 9;
}

fn collect_heads(t: &Term, out: &mut std::collections::BTreeSet<u32>) {
    match t {
        Term::Apply(head, args) => {
            if let Term::Var(id) = head.as_ref() {
                out.insert(*id);
            }
            for a in args {
                collect_heads(a, out);
            }
        }
        Term::Fn(_, body) => collect_heads(body, out),
        Term::Symbol(_, args) => {
            for a in args {
                collect_heads(a, out);
            }
        }
        _ => {}
    }
}

/// A full traversal trajectory: the complete decision sequence.
/// Training data for a scorer. Serializable so it can be persisted
/// across runs — later work wires this to disk, a wake-sleep loop
/// trains on archived trajectories between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trajectory {
    pub steps: Vec<TrajectoryStep>,
    /// The terminal library features — state after all steps.
    /// The "outcome" side of the `(state, action, outcome)` triple.
    pub terminal_state: Option<LibraryFeatures>,
}

impl Trajectory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Did this trajectory reach tensor emergence? True iff any
    /// step's pre-state had nonzero tensor density, or the
    /// terminal state has tensor structure. This is the supervised
    /// signal: trajectories that reached tensor are positive
    /// examples.
    #[must_use]
    pub fn reached_tensor(&self) -> bool {
        if let Some(terminal) = &self.terminal_state {
            if terminal.tensor_density > 0.0 {
                return true;
            }
        }
        self.steps.iter().any(|s| s.pre_state.tensor_density > 0.0)
    }

    /// First step index at which tensor structure appeared. None
    /// if the trajectory never reached tensor.
    #[must_use]
    pub fn tensor_emergence_step(&self) -> Option<usize> {
        self.steps
            .iter()
            .position(|s| s.pre_state.tensor_density > 0.0)
    }

    /// Number of steps where a rule was accepted.
    #[must_use]
    pub fn accept_count(&self) -> usize {
        self.steps.iter().filter(|s| s.accepted).count()
    }

    /// Number of steps where a rule was rejected.
    #[must_use]
    pub fn reject_count(&self) -> usize {
        self.steps.iter().filter(|s| !s.accepted).count()
    }

    pub fn record(&mut self, step: TrajectoryStep) {
        self.steps.push(step);
    }

    pub fn finalize(&mut self, terminal: LibraryFeatures) {
        self.terminal_state = Some(terminal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{ADD, MUL};
    use crate::value::Value;

    fn var(id: u32) -> Term {
        Term::Var(id)
    }
    fn app(h: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(h), args)
    }
    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }

    fn identity_rule() -> RewriteRule {
        RewriteRule {
            name: "id".into(),
            lhs: app(var(ADD), vec![var(100), nat(0)]),
            rhs: var(100),
        }
    }

    fn distributive_rule() -> RewriteRule {
        let a = var(100);
        let b = var(101);
        let c = var(102);
        RewriteRule {
            name: "distrib".into(),
            lhs: app(
                var(MUL),
                vec![app(var(ADD), vec![a.clone(), b.clone()]), c.clone()],
            ),
            rhs: app(
                var(ADD),
                vec![
                    app(var(MUL), vec![a.clone(), c.clone()]),
                    app(var(MUL), vec![b.clone(), c.clone()]),
                ],
            ),
        }
    }

    #[test]
    fn features_empty_library_is_zero() {
        let f = LibraryFeatures::extract(&[]);
        assert_eq!(f.rule_count, 0);
        assert_eq!(f.tensor_density, 0.0);
        assert_eq!(f.tensor_distributive_count, 0);
        assert_eq!(f.distinct_heads, 0);
    }

    #[test]
    fn features_identity_only_has_zero_tensor() {
        let f = LibraryFeatures::extract(&[identity_rule()]);
        assert_eq!(f.rule_count, 1);
        assert_eq!(f.tensor_density, 0.0);
        assert!(f.mean_compression > 1.0, "identity rule reduces");
    }

    #[test]
    fn features_distributive_rule_surfaces_tensor_density() {
        let f = LibraryFeatures::extract(&[distributive_rule()]);
        assert_eq!(f.rule_count, 1);
        assert_eq!(f.tensor_distributive_count, 1);
        assert_eq!(f.tensor_meta_count, 0);
        assert!((f.tensor_density - 1.0).abs() < 1e-9);
    }

    #[test]
    fn features_mixed_library_partial_tensor_density() {
        let f = LibraryFeatures::extract(&[
            identity_rule(),
            distributive_rule(),
        ]);
        assert_eq!(f.rule_count, 2);
        assert_eq!(f.tensor_distributive_count, 1);
        assert!((f.tensor_density - 0.5).abs() < 1e-9);
    }

    #[test]
    fn features_count_distinct_heads() {
        let f = LibraryFeatures::extract(&[distributive_rule()]);
        // distributive rule uses both add (2) and mul (3) as heads.
        assert_eq!(f.distinct_heads, 2);
    }

    #[test]
    fn feature_vector_has_fixed_width() {
        let f = LibraryFeatures::extract(&[identity_rule()]);
        let v = f.as_vector();
        assert_eq!(v.len(), LibraryFeatures::WIDTH);
    }

    #[test]
    fn trajectory_empty_has_no_tensor() {
        let t = Trajectory::new();
        assert!(!t.reached_tensor());
        assert_eq!(t.tensor_emergence_step(), None);
        assert_eq!(t.accept_count(), 0);
        assert_eq!(t.reject_count(), 0);
    }

    #[test]
    fn trajectory_records_and_counts_accepts() {
        let mut t = Trajectory::new();
        let feat_empty = LibraryFeatures::extract(&[]);
        t.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: feat_empty.clone(),
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 3.2,
        });
        t.record(TrajectoryStep {
            epoch: 1,
            corpus_index: 0,
            pre_state: feat_empty.clone(),
            action: ActionKind::Discover,
            accepted: false,
            delta_dl: 0.0,
        });
        assert_eq!(t.accept_count(), 1);
        assert_eq!(t.reject_count(), 1);
        assert_eq!(t.steps.len(), 2);
    }

    #[test]
    fn trajectory_detects_tensor_emergence_step() {
        let mut t = Trajectory::new();
        let no_tensor = LibraryFeatures::extract(&[identity_rule()]);
        let with_tensor = LibraryFeatures::extract(&[
            identity_rule(),
            distributive_rule(),
        ]);

        // Step 0: pre-state has no tensor.
        t.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: no_tensor.clone(),
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 1.0,
        });
        // Step 1: pre-state already has tensor (rule was accepted
        // before this step). This is when tensor FIRST appears in
        // the pre-state — emergence_step = 1.
        t.record(TrajectoryStep {
            epoch: 1,
            corpus_index: 0,
            pre_state: with_tensor.clone(),
            action: ActionKind::Reinforce,
            accepted: false,
            delta_dl: 0.1,
        });

        assert!(t.reached_tensor());
        assert_eq!(t.tensor_emergence_step(), Some(1));
    }

    #[test]
    fn trajectory_terminal_state_flags_tensor() {
        let mut t = Trajectory::new();
        let no_tensor = LibraryFeatures::extract(&[identity_rule()]);
        let with_tensor = LibraryFeatures::extract(&[
            identity_rule(),
            distributive_rule(),
        ]);
        t.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: no_tensor,
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 1.0,
        });
        t.finalize(with_tensor);
        assert!(t.reached_tensor());
    }

    #[test]
    fn features_serde_roundtrip_via_bincode() {
        // Trajectories need to persist across runs so the scorer
        // can train on archived data. Bincode roundtrip proves
        // the serde derives are sound; JSON would work too if a
        // crate adds serde_json as a dep.
        let f = LibraryFeatures::extract(&[
            identity_rule(),
            distributive_rule(),
        ]);
        let bytes = bincode::serialize(&f).unwrap();
        let back: LibraryFeatures = bincode::deserialize(&bytes).unwrap();
        assert_eq!(f, back);
    }

    #[test]
    fn trajectory_serde_roundtrip_via_bincode() {
        let mut t = Trajectory::new();
        let feat = LibraryFeatures::extract(&[identity_rule()]);
        t.record(TrajectoryStep {
            epoch: 0,
            corpus_index: 0,
            pre_state: feat.clone(),
            action: ActionKind::Discover,
            accepted: true,
            delta_dl: 1.5,
        });
        t.finalize(feat.clone());
        let bytes = bincode::serialize(&t).unwrap();
        let back: Trajectory = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back.steps.len(), 1);
        assert!(back.terminal_state.is_some());
    }
}
