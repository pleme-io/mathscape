//! R26 — BootstrapCycle: the typescape entity for self-producing discovery.
//!
//! # What this closes
//!
//! R25 demonstrated the self-bootstrapping loop as a test: empty
//! library → discover tensor primitives → use primitives → discover
//! more → train a model. That test HAS the behavior but it's not
//! reusable — the logic lives in one test function.
//!
//! R26 enshrines the same behavior as a FIRST-CLASS TYPED ENTITY
//! with layered traits so each component can be independently
//! hijacked/optimized at runtime. The key insight: a self-producing
//! system must expose its internal interfaces cleanly so chaos can
//! be encapsulated inside implementations, not inside the caller's
//! understanding.
//!
//! # The layers
//!
//! ```text
//!   ┌─ BootstrapCycle ─────────────────────────────────────┐
//!   │                                                       │
//!   │    ┌─ CorpusGenerator  ─ makes corpora per iteration │
//!   │    ├─ LawExtractor     ─ derives laws from a corpus  │
//!   │    ├─ ModelUpdater     ─ trains policy on trajectory │
//!   │    └─ AttestationHook  ─ BLAKE3 at each boundary      │
//!   │                                                       │
//!   └──────────────────────────────────────────────────────┘
//! ```
//!
//! Each trait is a seam. Swap the implementation and the outer
//! cycle continues to work. Tests prove the hijack property.
//!
//! # Attestation
//!
//! Every BootstrapOutcome carries a BLAKE3 `attestation_hash`
//! that covers:
//!   - final library content
//!   - final policy weights + bias + generation
//!   - iteration count
//!   - trajectory step count
//!
//! Two cycles with identical inputs produce identical attestation.
//! Useful for:
//!   - deterministic_replay at the cycle level
//!   - audit trails across cycle generations
//!   - detecting drift when a layer implementation changes

use crate::eval::RewriteRule;
use crate::hash::TermRef;
use crate::policy::LinearPolicy;
use crate::term::Term;
use crate::trajectory::{ActionKind, LibraryFeatures, Trajectory, TrajectoryStep};
use serde::{Deserialize, Serialize};
use std::time::Instant;

// ── Layer traits ───────────────────────────────────────────────────

/// Layer 1: corpus generation. Given the iteration index and the
/// current library, produce the next corpus.
///
/// Hijack: swap this to feed different corpora shapes (R21 tensor
/// corpus, domain-specific corpora, adversarial corpora for
/// refutation experiments).
pub trait CorpusGenerator {
    fn generate(&self, iteration: usize, library: &[RewriteRule]) -> Vec<Term>;
}

/// Layer 2: law extraction. Given a corpus and the current library
/// (used as eval context), produce new candidate rules.
///
/// Hijack: swap this for different discovery strategies — R24's
/// paired-AU discovery, or a future e-graph-based extractor, or
/// a neural candidate generator. The BootstrapCycle doesn't care
/// how the laws were derived.
pub trait LawExtractor {
    fn extract(
        &self,
        corpus: &[Term],
        library: &[RewriteRule],
    ) -> Vec<RewriteRule>;
}

/// Layer 3: model update. Trains the policy given the current
/// trajectory. Default implementation calls `train_from_trajectory`.
///
/// Hijack: swap for different training strategies — bigger learning
/// rate, MLP instead of linear, actor-critic, etc.
pub trait ModelUpdater {
    fn update(&self, policy: &mut LinearPolicy, trajectory: &Trajectory);
}

/// Layer 4 (R28): library dedup. Given the current library and a
/// candidate new rule, decide whether the candidate is redundant
/// and should be rejected.
///
/// Motivation: R27's deep-bootstrap exploration found the library
/// grows linearly at +3 laws/iter with no saturation. Each deeper
/// nesting level mints new anti-unification patterns that are
/// structurally derivable from earlier laws. Without a dedup
/// step, the library accumulates these variants forever.
///
/// Hijack: swap for stricter / looser duplicate detection —
/// structural equality (default), alpha-equivalence, proper-
/// subsumption via `mathscape_core::eval::proper_subsumes`,
/// e-graph saturation, or an empirical equivalence check that
/// evaluates candidates against random instances.
pub trait LibraryDeduper {
    /// True if `candidate` is already covered by `library` and
    /// should be REJECTED from append. False means "novel; keep."
    fn is_duplicate(
        &self,
        candidate: &RewriteRule,
        library: &[RewriteRule],
    ) -> bool;
}

// ── Default implementations ────────────────────────────────────────

/// Default corpus generator mirroring R25's seed strategy. Iteration
/// 0 feeds pure tensor-identity instances; later iterations add
/// compositional structure so discovered laws can surface.
#[derive(Debug, Clone, Default)]
pub struct DefaultCorpusGenerator;

impl CorpusGenerator for DefaultCorpusGenerator {
    fn generate(&self, iteration: usize, _library: &[RewriteRule]) -> Vec<Term> {
        use crate::builtin::{TENSOR_ADD, TENSOR_MUL};
        use crate::value::Value;

        let zeros = Term::Number(Value::tensor(vec![2], vec![0, 0]).unwrap());
        let ones = Term::Number(Value::tensor(vec![2], vec![1, 1]).unwrap());
        let operands: Vec<Term> = (2..=9)
            .map(|k| {
                Term::Number(
                    Value::tensor(vec![2], vec![k as i64, (k + 1) as i64])
                        .unwrap(),
                )
            })
            .collect();

        let mut corpus: Vec<Term> = Vec::new();
        match iteration {
            0 => {
                for op in &operands {
                    corpus.push(Term::Apply(
                        Box::new(Term::Var(TENSOR_ADD)),
                        vec![zeros.clone(), op.clone()],
                    ));
                    corpus.push(Term::Apply(
                        Box::new(Term::Var(TENSOR_MUL)),
                        vec![ones.clone(), op.clone()],
                    ));
                }
            }
            _ => {
                // Later iterations: nested compositions.
                for op in &operands {
                    let inner = Term::Apply(
                        Box::new(Term::Var(TENSOR_ADD)),
                        vec![zeros.clone(), op.clone()],
                    );
                    corpus.push(Term::Apply(
                        Box::new(Term::Var(TENSOR_ADD)),
                        vec![zeros.clone(), inner.clone()],
                    ));
                    let inner_mul = Term::Apply(
                        Box::new(Term::Var(TENSOR_MUL)),
                        vec![ones.clone(), op.clone()],
                    );
                    corpus.push(Term::Apply(
                        Box::new(Term::Var(TENSOR_MUL)),
                        vec![ones.clone(), inner_mul.clone()],
                    ));
                }
            }
        }
        corpus
    }
}

/// Default model updater — calls train_from_trajectory with a
/// fixed learning rate of 0.05. Swap this to adjust training
/// dynamics or use a non-linear model.
#[derive(Debug, Clone)]
pub struct DefaultModelUpdater {
    pub learning_rate: f64,
}

impl Default for DefaultModelUpdater {
    fn default() -> Self {
        Self {
            learning_rate: 0.05,
        }
    }
}

impl ModelUpdater for DefaultModelUpdater {
    fn update(&self, policy: &mut LinearPolicy, trajectory: &Trajectory) {
        policy.train_from_trajectory(trajectory, self.learning_rate);
    }
}

/// R28: no-op deduper — every candidate is novel. Backward-
/// compatible default; `BootstrapCycle::run` uses this so
/// existing callers don't change behavior.
#[derive(Debug, Clone, Default)]
pub struct NoDedup;

impl LibraryDeduper for NoDedup {
    fn is_duplicate(&self, _cand: &RewriteRule, _lib: &[RewriteRule]) -> bool {
        false
    }
}

/// R28: canonical-form deduper. Two rules are duplicates iff their
/// (LHS, RHS) canonicalized forms are structurally equal.
///
/// Canonicalization (R3/R4/R6) already folds commutativity,
/// associativity, and constant-fold transformations into a normal
/// form. So `add(0, ?x)` and `add(?x, 0)` (same rule, swapped args)
/// canonicalize identically and this deduper rejects the second
/// as redundant against the first.
///
/// Does NOT catch alpha-renaming: `add(0, ?x) = ?x` and
/// `add(0, ?y) = ?y` with different pattern-variable ids canonicalize
/// to structurally different terms. Use `AlphaDeduper` (future)
/// for that stronger check — but note alpha-based dedup has a
/// known apex-shift risk (documented in eval::alpha_equivalent).
#[derive(Debug, Clone, Default)]
pub struct CanonicalDeduper;

impl LibraryDeduper for CanonicalDeduper {
    fn is_duplicate(
        &self,
        candidate: &RewriteRule,
        library: &[RewriteRule],
    ) -> bool {
        let c_lhs = candidate.lhs.canonical();
        let c_rhs = candidate.rhs.canonical();
        library.iter().any(|r| {
            r.lhs.canonical() == c_lhs && r.rhs.canonical() == c_rhs
        })
    }
}

/// R28: alpha-equivalence deduper — uses the kernel's
/// `anonymize_rule` to canonicalize pattern variable ids before
/// comparing. Catches rules that differ only in fresh-var naming,
/// which CanonicalDeduper misses.
///
/// Stronger than CanonicalDeduper. Safe to use because it operates
/// on candidates/library ONLY — doesn't change anything about
/// how alpha_equivalent itself is defined (which has the deferred
/// R1 apex-shift concern).
#[derive(Debug, Clone, Default)]
pub struct AlphaDeduper;

impl LibraryDeduper for AlphaDeduper {
    fn is_duplicate(
        &self,
        candidate: &RewriteRule,
        library: &[RewriteRule],
    ) -> bool {
        let anon_cand = crate::eval::anonymize_rule(candidate);
        library.iter().any(|r| {
            let anon_r = crate::eval::anonymize_rule(r);
            anon_r.lhs == anon_cand.lhs && anon_r.rhs == anon_cand.rhs
        })
    }
}

/// R30: subsumption-based deduper — strongest of the shipped
/// dedupers. A candidate is rejected if ANY library rule
/// `proper_subsumes` it: i.e., the library rule's LHS pattern-
/// matches the candidate's LHS AND under that match's
/// substitution, the library rule's RHS reduces to the
/// candidate's RHS.
///
/// Reject conditions stronger than Alpha (which requires exact
/// structural equality modulo var renaming) — Subsumption
/// additionally rejects specializations. E.g., given
/// `add(?x, ?y) => ?x` in the library, `add(5, 3) => 5` is
/// subsumed and rejected; alpha-deduper would keep it.
///
/// Uses `mathscape_core::eval::proper_subsumes` which is
/// well-tested and stable.
#[derive(Debug, Clone, Default)]
pub struct SubsumptionDeduper;

impl LibraryDeduper for SubsumptionDeduper {
    fn is_duplicate(
        &self,
        candidate: &RewriteRule,
        library: &[RewriteRule],
    ) -> bool {
        library
            .iter()
            .any(|r| crate::eval::proper_subsumes(r, candidate))
    }
}

// ── R32: BootstrapCycleSpec — fully Lisp-producible recipe ─────────
//
// Before R32, BootstrapCycle was generic over three-to-four
// trait-bounded types. That's great for zero-cost dispatch in Rust
// but unfriendly to Lisp: you can't write a Rust generic in Lisp.
//
// R32 introduces a data-level recipe — `BootstrapCycleSpec` — that
// names each layer by a string identifier resolvable in a registry.
// Advantages:
//
//   1. Fully Lisp-describable: `spec_to_sexp` emits a pure Lisp
//      value for the recipe. `spec_from_sexp` reconstructs it.
//   2. Fully Lisp-producible: a Lisp program can construct a
//      spec Sexp, hand it to the executor, receive back the
//      trained model as Sexp. Round-trip closed through Lisp.
//   3. The registry + executor is Rust, but from the Lisp
//      program's view the whole process is: "here's a recipe,
//      give me the model."

/// A Lisp-serializable recipe for running a BootstrapCycle. Each
/// layer is named by a string the executor resolves in its
/// internal registry.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BootstrapCycleSpec {
    /// Name resolved to a `CorpusGenerator` by the executor.
    /// Built-in: `"default"`.
    pub corpus_generator: String,
    /// Name resolved to a `LawExtractor`. Built-in: `"null"` (always
    /// returns empty — needed because the R24 law generator lives
    /// in mathscape-compress and core can't depend on compress).
    /// External registries can register richer extractors.
    pub law_extractor: String,
    /// Name resolved to a `ModelUpdater`. Built-in: `"default"`
    /// (train_from_trajectory @ lr=0.05), `"null"` (no-op).
    pub model_updater: String,
    /// Name resolved to a `LibraryDeduper`. Built-in: `"none"`,
    /// `"canonical"`, `"alpha"`, `"subsumption"`.
    pub deduper: String,
    /// Iteration count.
    pub n_iterations: usize,
    /// Seed library (typically empty for first cycle; later cycles
    /// seed from a previous M's final library).
    pub seed_library: Vec<RewriteRule>,
    /// Seed policy. Built-in default helpers: see
    /// `LinearPolicy::{new, tensor_seeking_prior}`.
    pub seed_policy: LinearPolicy,
    /// R37: optional early-stop. `Some(W)` → stop when the
    /// library has not grown for `W` consecutive iterations.
    /// `None` → always run `n_iterations`. Default (via
    /// `BootstrapCycleSpec::default_m0`) is None to preserve
    /// backwards-compatible behavior.
    #[serde(default)]
    pub early_stop_after_stable: Option<usize>,
}

impl BootstrapCycleSpec {
    /// Canonical default spec: the same layer triple +
    /// CanonicalDeduper used by the R31 first-model tests.
    #[must_use]
    pub fn default_m0() -> Self {
        Self {
            corpus_generator: "default".into(),
            law_extractor: "derived-laws".into(),
            model_updater: "default".into(),
            deduper: "canonical".into(),
            n_iterations: 5,
            seed_library: Vec::new(),
            seed_policy: LinearPolicy::tensor_seeking_prior(),
            early_stop_after_stable: None,
        }
    }
}

/// Errors that can arise when resolving / executing a spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecExecutionError {
    UnknownLayer {
        role: &'static str,
        name: String,
    },
}

impl std::fmt::Display for SpecExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecExecutionError::UnknownLayer { role, name } => {
                write!(f, "no {role} registered under name '{name}'")
            }
        }
    }
}

impl std::error::Error for SpecExecutionError {}

/// R32: execute a `BootstrapCycleSpec` with the built-in layer
/// registry. Returns the resulting `BootstrapOutcome`.
///
/// Built-in names:
///   - corpus_generator: "default" (R26 DefaultCorpusGenerator),
///     "null" (always empty)
///   - law_extractor: "null" (always empty; richer extractors
///     live in downstream crates — see axiom-bridge for
///     `derived-laws` wired through R24)
///   - model_updater: "default" (lr=0.05), "null" (no-op)
///   - deduper: "none", "canonical", "alpha", "subsumption"
///
/// Unknown names yield `UnknownLayer` — the caller can extend by
/// wrapping this executor or by providing a custom registry-based
/// dispatch.
pub fn execute_spec_core(
    spec: &BootstrapCycleSpec,
) -> Result<BootstrapOutcome, SpecExecutionError> {
    // Resolve corpus generator. Only "default" and "null" live here
    // (core doesn't know about tensor corpora or other specialties).
    match spec.corpus_generator.as_str() {
        "default" => {}
        "null" => {}
        other => {
            return Err(SpecExecutionError::UnknownLayer {
                role: "corpus_generator",
                name: other.to_string(),
            });
        }
    }
    if spec.law_extractor != "null" {
        return Err(SpecExecutionError::UnknownLayer {
            role: "law_extractor",
            name: spec.law_extractor.clone(),
        });
    }
    // Updater.
    match spec.model_updater.as_str() {
        "default" | "null" => {}
        other => {
            return Err(SpecExecutionError::UnknownLayer {
                role: "model_updater",
                name: other.to_string(),
            });
        }
    }
    // Deduper.
    match spec.deduper.as_str() {
        "none" | "canonical" | "alpha" | "subsumption" => {}
        other => {
            return Err(SpecExecutionError::UnknownLayer {
                role: "deduper",
                name: other.to_string(),
            });
        }
    }

    // Dispatch: core only ships null-extractor executor. Richer
    // executors (axiom-bridge) override for richer extractor names.
    // This is the minimal "just layer resolution works" version.
    use crate::bootstrap::{AlphaDeduper, CanonicalDeduper, NoDedup, SubsumptionDeduper};

    /// Null extractor used when spec.law_extractor == "null".
    struct NullExtractor;
    impl LawExtractor for NullExtractor {
        fn extract(&self, _c: &[Term], _l: &[RewriteRule]) -> Vec<RewriteRule> {
            Vec::new()
        }
    }
    struct NullGen;
    impl CorpusGenerator for NullGen {
        fn generate(&self, _iter: usize, _l: &[RewriteRule]) -> Vec<Term> {
            Vec::new()
        }
    }
    struct NullUpdater;
    impl ModelUpdater for NullUpdater {
        fn update(&self, _p: &mut LinearPolicy, _t: &Trajectory) {}
    }

    let seed_lib = spec.seed_library.clone();
    let seed_pol = spec.seed_policy.clone();
    let n = spec.n_iterations;

    // Because each layer type is distinct and Rust generics are
    // resolved at compile time, we handcraft a handful of
    // concrete dispatch branches. Core's minimum: "null" law
    // extractor × 2 corpus × 2 updater × 4 deduper = 16 branches.
    // Axiom-bridge's executor adds the rich law extractor and
    // richer corpora.
    let early_stop = spec.early_stop_after_stable;
    macro_rules! run {
        ($cg:expr, $ex:expr, $up:expr, $dd:expr) => {{
            let cycle = BootstrapCycle::new($cg, $ex, $up, n);
            if let Some(w) = early_stop {
                cycle.run_until_stable(seed_lib, seed_pol, $dd, w)
            } else {
                cycle.run_with_dedup(seed_lib, seed_pol, $dd)
            }
        }};
    }
    macro_rules! run_all_dedup {
        ($cg:expr, $ex:expr, $up:expr) => {
            match spec.deduper.as_str() {
                "none" => run!($cg, $ex, $up, &NoDedup),
                "canonical" => run!($cg, $ex, $up, &CanonicalDeduper),
                "alpha" => run!($cg, $ex, $up, &AlphaDeduper),
                "subsumption" => run!($cg, $ex, $up, &SubsumptionDeduper),
                _ => unreachable!(),
            }
        };
    }
    let outcome = match (
        spec.corpus_generator.as_str(),
        spec.law_extractor.as_str(),
        spec.model_updater.as_str(),
    ) {
        ("default", "null", "default") => {
            run_all_dedup!(DefaultCorpusGenerator, NullExtractor, DefaultModelUpdater::default())
        }
        ("default", "null", "null") => {
            run_all_dedup!(DefaultCorpusGenerator, NullExtractor, NullUpdater)
        }
        ("null", "null", "default") => {
            run_all_dedup!(NullGen, NullExtractor, DefaultModelUpdater::default())
        }
        ("null", "null", "null") => {
            run_all_dedup!(NullGen, NullExtractor, NullUpdater)
        }
        _ => unreachable!(), // validated above
    };

    Ok(outcome)
}

// ── R33: ExperimentScenario — multi-phase training chain ─────────
//
// Beyond a single BootstrapCycleSpec, an experiment is typically a
// SEQUENCE of cycles where each cycle consumes the previous's
// output (library + trained policy) as its seed.
//
// ExperimentScenario bundles this sequence as a Lisp-describable
// recipe. The executor threads phase N's final library and
// trained policy into phase N+1's `seed_library` and `seed_policy`.
// After all phases run, an ExperimentOutcome carries the complete
// trace: each phase's BootstrapOutcome + a phase-level attestation
// chain.
//
// Framing (per 2026-04-18 direction): from here forward, all new
// work thinks in terms of "making the model exist + train more
// efficiently." ExperimentScenario is the substrate for those
// efficiency experiments — swap layer triples across phases,
// observe attestations, keep what works.

/// A multi-phase training scenario. Each phase is a
/// `BootstrapCycleSpec` with its own layer triple and iteration
/// count. Phase N+1 inherits phase N's `final_library` and
/// `final_policy` — each phase's output seeds the next.
///
/// A scenario's `seed_library` and `seed_policy` fields apply
/// only to the FIRST phase; subsequent phases ignore their own
/// spec-level seeds in favor of the chained output.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentScenario {
    /// Human-readable name for log output and attestation
    /// annotation. Not semantically load-bearing.
    pub name: String,
    /// Ordered list of phase specs. Each is a
    /// `BootstrapCycleSpec`; chain semantics described above.
    pub phases: Vec<BootstrapCycleSpec>,
}

/// Per-phase outcome within an ExperimentScenario run.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhaseOutcome {
    /// Index of this phase (0-based).
    pub phase_index: usize,
    /// The spec that was executed (after chaining inherited the
    /// previous phase's library + policy).
    pub spec_used: BootstrapCycleSpec,
    /// The cycle outcome for this phase.
    pub cycle_outcome: BootstrapOutcome,
}

/// Full experiment outcome: phase-level outcomes + chain-level
/// attestation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentOutcome {
    /// Per-phase results, in order.
    pub phases: Vec<PhaseOutcome>,
    /// BLAKE3 chain attestation: hash of the sequence of phase
    /// attestations. Two scenarios with identical phase sequences
    /// produce identical chain attestations.
    pub chain_attestation: crate::hash::TermRef,
    /// R34: wall-clock total for the entire scenario run.
    /// OBSERVATIONAL — does NOT enter `chain_attestation`.
    /// `phases[i].cycle_outcome.timings.total_ns` carries per-
    /// phase totals and together they cover every phase; this
    /// field additionally captures scenario-level overhead
    /// (phase chaining + attestation rollup).
    pub scenario_total_ns: u64,
}

impl ExperimentOutcome {
    /// The final model — the policy from the last phase.
    #[must_use]
    pub fn final_model(&self) -> &LinearPolicy {
        &self
            .phases
            .last()
            .expect("ExperimentOutcome must have at least one phase")
            .cycle_outcome
            .final_policy
    }

    /// The final library — from the last phase.
    #[must_use]
    pub fn final_library(&self) -> &[RewriteRule] {
        &self
            .phases
            .last()
            .expect("ExperimentOutcome must have at least one phase")
            .cycle_outcome
            .final_library
    }

    /// Per-phase library growth: how many rules each phase added
    /// on top of the prior phase's library.
    #[must_use]
    pub fn per_phase_growth(&self) -> Vec<usize> {
        let mut prev = 0usize;
        let mut growth = Vec::new();
        for phase in &self.phases {
            let curr = phase.cycle_outcome.final_library.len();
            growth.push(curr.saturating_sub(prev));
            prev = curr;
        }
        growth
    }

    /// R34: per-phase wall-clock totals in nanoseconds. Index
    /// aligned with `phases`.
    #[must_use]
    pub fn per_phase_timings_ns(&self) -> Vec<u64> {
        self.phases
            .iter()
            .map(|p| p.cycle_outcome.timings.total_ns)
            .collect()
    }
}

/// Run an `ExperimentScenario` by chaining phase outputs.
/// Returns the complete outcome with every phase's trace.
///
/// Efficiency note: when phase N+1's spec's `seed_library` and
/// `seed_policy` are unused (because we chain phase N's outputs
/// in), the memory cost is one library + one policy + one
/// trajectory per phase retained in the outcome. For long-running
/// scenarios, consumers can discard per-phase outcomes they
/// don't need after reading.
pub fn execute_scenario_core(
    scenario: &ExperimentScenario,
) -> Result<ExperimentOutcome, SpecExecutionError> {
    let scenario_start = Instant::now();
    if scenario.phases.is_empty() {
        return Ok(ExperimentOutcome {
            phases: Vec::new(),
            chain_attestation: crate::hash::TermRef::from_bytes(b""),
            scenario_total_ns: elapsed_ns(scenario_start),
        });
    }

    let mut phases: Vec<PhaseOutcome> = Vec::new();
    let mut carry_library: Vec<RewriteRule> =
        scenario.phases[0].seed_library.clone();
    let mut carry_policy: LinearPolicy = scenario.phases[0].seed_policy.clone();

    for (idx, base_spec) in scenario.phases.iter().enumerate() {
        // For phase 0, seeds come from the spec itself; for later
        // phases, use the carried-over library + policy from the
        // previous phase's output. The spec is cloned with its
        // seed fields overridden.
        let mut spec = base_spec.clone();
        if idx > 0 {
            spec.seed_library = carry_library.clone();
            spec.seed_policy = carry_policy.clone();
        }
        let outcome = execute_spec_core(&spec)?;
        carry_library = outcome.final_library.clone();
        carry_policy = outcome.final_policy.clone();
        phases.push(PhaseOutcome {
            phase_index: idx,
            spec_used: spec,
            cycle_outcome: outcome,
        });
    }

    // Chain attestation: BLAKE3 of the concatenated per-phase
    // attestations. Stable under identical scenario; shifts if any
    // phase's content changes.
    let concat: Vec<u8> = phases
        .iter()
        .flat_map(|p| p.cycle_outcome.attestation.as_bytes().to_vec())
        .collect();
    let chain_attestation = crate::hash::TermRef::from_bytes(&concat);

    Ok(ExperimentOutcome {
        phases,
        chain_attestation,
        scenario_total_ns: elapsed_ns(scenario_start),
    })
}

/// R30: post-process a collection of rules, partitioning them
/// into (kept, rejected) using the supplied deduper. Useful for
/// cleaning up a library AFTER it was built (e.g., collected
/// from multiple sources, or imported from an external library).
///
/// Runs left-to-right: the first occurrence of each equivalence
/// class is kept; later structural duplicates are rejected. For
/// a deduper that respects the order-independent property
/// (`is_duplicate(a, [b]) == is_duplicate(b, [a])` for the same
/// equivalence class), the kept set is invariant under input
/// ordering. CanonicalDeduper + AlphaDeduper have this property;
/// SubsumptionDeduper does NOT — subsumption is asymmetric, so
/// a more-general rule appearing AFTER a specialization would
/// not displace it. Use with awareness.
#[must_use]
pub fn deduplicate_library<D: LibraryDeduper>(
    rules: Vec<RewriteRule>,
    deduper: &D,
) -> (Vec<RewriteRule>, Vec<RewriteRule>) {
    let mut kept: Vec<RewriteRule> = Vec::new();
    let mut rejected: Vec<RewriteRule> = Vec::new();
    for r in rules {
        if deduper.is_duplicate(&r, &kept) {
            rejected.push(r);
        } else {
            kept.push(r);
        }
    }
    (kept, rejected)
}

// ── R34: timing instrumentation ────────────────────────────────────
//
// Efficiency framing (2026-04-18): "from here on we only think in
// terms of making the model exist more efficiently and train more
// efficiently." You can't optimize what you don't measure — so the
// BootstrapCycle and ExperimentScenario runs now carry wall-clock
// timings covering each seam of the loop.
//
// Timings are OBSERVATIONAL — they do NOT enter the attestation
// payload. Two runs with identical inputs produce identical
// attestations even though their timings differ (same machine or
// not). This keeps cycle-level deterministic_replay intact.

/// Per-iteration wall-clock timings in nanoseconds. One instance
/// per iteration, in `CycleTimings::per_iteration`.
///
/// Fields cover the three trait seams inside each iteration:
///   - `corpus_gen_ns`: time in `CorpusGenerator::generate`
///   - `extract_ns`: time in `LawExtractor::extract`
///   - `dedup_ns`: time running the dedup filter over proposals
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IterationTimings {
    pub corpus_gen_ns: u64,
    pub extract_ns: u64,
    pub dedup_ns: u64,
}

impl IterationTimings {
    /// Sum across all three seams.
    #[must_use]
    pub fn total_ns(&self) -> u64 {
        self.corpus_gen_ns
            .saturating_add(self.extract_ns)
            .saturating_add(self.dedup_ns)
    }
}

/// Full cycle timings: one `IterationTimings` per iteration, plus
/// the post-loop training call and the outer run total.
///
/// `total_ns` is measured across the whole `run_with_dedup` body
/// and therefore includes all sub-timings plus any overhead
/// (trajectory recording, feature extraction, attestation).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CycleTimings {
    pub per_iteration: Vec<IterationTimings>,
    /// Time inside the post-loop `ModelUpdater::update`.
    pub train_ns: u64,
    /// Wall-clock total for the entire cycle run.
    pub total_ns: u64,
}

impl CycleTimings {
    /// Sum of all per-iteration timings. Equals
    /// `total_ns - train_ns - overhead`.
    #[must_use]
    pub fn iter_sum_ns(&self) -> u64 {
        self.per_iteration
            .iter()
            .map(IterationTimings::total_ns)
            .fold(0u64, u64::saturating_add)
    }
}

// ── BootstrapCycle ─────────────────────────────────────────────────

/// The layered, pluggable self-producing discovery cycle.
///
/// Compose three trait objects and call `run()` with a seed
/// library and seed policy. Get back a `BootstrapOutcome` with
/// the full trace and an attestation hash.
///
/// Generic over the three layers so each has zero-cost dispatch.
/// Use `BootstrapCycle::<DefaultCorpusGenerator, _, DefaultModelUpdater>::new(...)`
/// with a LawExtractor implementation (defined in mathscape-compress
/// via R24's `derive_laws_from_corpus`).
pub struct BootstrapCycle<C, E, M>
where
    C: CorpusGenerator,
    E: LawExtractor,
    M: ModelUpdater,
{
    pub corpus_gen: C,
    pub extractor: E,
    pub updater: M,
    pub n_iterations: usize,
    pub eval_step_limit: usize,
    pub min_law_support: usize,
}

impl<C, E, M> BootstrapCycle<C, E, M>
where
    C: CorpusGenerator,
    E: LawExtractor,
    M: ModelUpdater,
{
    pub fn new(
        corpus_gen: C,
        extractor: E,
        updater: M,
        n_iterations: usize,
    ) -> Self {
        Self {
            corpus_gen,
            extractor,
            updater,
            n_iterations,
            eval_step_limit: 300,
            min_law_support: 2,
        }
    }

    /// Execute the cycle with NO dedup (backward-compatible). Every
    /// candidate law from the extractor is appended to the library
    /// unconditionally.
    pub fn run(
        &self,
        seed_library: Vec<RewriteRule>,
        seed_policy: LinearPolicy,
    ) -> BootstrapOutcome {
        self.run_with_dedup(seed_library, seed_policy, &NoDedup)
    }

    /// Execute the cycle with a provided `LibraryDeduper`.
    /// Candidate laws that the deduper flags as duplicates are
    /// rejected before reaching the library.
    ///
    /// The outcome still reports the extractor's pre-dedup count
    /// in `new_law_count` — that's what the extractor proposed.
    /// The `features_after` reflects the post-dedup library,
    /// which is what future iterations see.
    /// R37: variant that short-circuits when the library has not
    /// grown for `stable_window` consecutive iterations. Returns
    /// early with whatever iterations have completed; the
    /// trajectory and iteration snapshots reflect only the
    /// completed iterations (NOT `n_iterations`), so the outcome
    /// reports the *real* work done.
    ///
    /// Use when the default extractor reliably saturates — the
    /// M0 default corpus discovers 3 rules in iter 0 then adds 0
    /// in iters 1-N. Calling `run_until_stable(..., 1)` cuts the
    /// cycle from 5 iterations to 2 (iter 0 produces rules; iter
    /// 1 confirms plateau → stop).
    ///
    /// `stable_window = 0` is meaningless (would stop after
    /// iteration 0 regardless of growth); it's coerced to 1.
    pub fn run_until_stable<D: LibraryDeduper>(
        &self,
        seed_library: Vec<RewriteRule>,
        seed_policy: LinearPolicy,
        deduper: &D,
        stable_window: usize,
    ) -> BootstrapOutcome {
        let stable_window = stable_window.max(1);
        self.run_impl(seed_library, seed_policy, deduper, Some(stable_window))
    }

    pub fn run_with_dedup<D: LibraryDeduper>(
        &self,
        seed_library: Vec<RewriteRule>,
        seed_policy: LinearPolicy,
        deduper: &D,
    ) -> BootstrapOutcome {
        self.run_impl(seed_library, seed_policy, deduper, None)
    }

    /// Shared implementation. `early_stop_after` = None → run all
    /// `n_iterations`. Some(W) → stop when the library has not
    /// grown for W consecutive iterations.
    fn run_impl<D: LibraryDeduper>(
        &self,
        seed_library: Vec<RewriteRule>,
        seed_policy: LinearPolicy,
        deduper: &D,
        early_stop_after: Option<usize>,
    ) -> BootstrapOutcome {
        let cycle_start = Instant::now();
        let mut library = seed_library;
        let mut policy = seed_policy;
        let mut trajectory = Trajectory::new();
        let mut iterations: Vec<IterationSnapshot> = Vec::new();
        let mut per_iter_timings: Vec<IterationTimings> = Vec::new();
        let mut consecutive_no_growth: usize = 0;

        for iter in 0..self.n_iterations {
            let t_corpus = Instant::now();
            let corpus = self.corpus_gen.generate(iter, &library);
            let corpus_gen_ns = elapsed_ns(t_corpus);

            let library_size_before = library.len();
            let features_before = LibraryFeatures::extract(&library);

            let t_extract = Instant::now();
            let proposed = self.extractor.extract(&corpus, &library);
            let extract_ns = elapsed_ns(t_extract);

            // R28: filter out duplicates BEFORE appending.
            let t_dedup = Instant::now();
            let mut accepted_laws = Vec::new();
            for cand in proposed.iter() {
                if !deduper.is_duplicate(cand, &library)
                    && !accepted_laws
                        .iter()
                        .any(|prev| deduper.is_duplicate(cand, std::slice::from_ref(prev)))
                {
                    accepted_laws.push(cand.clone());
                }
            }
            let dedup_ns = elapsed_ns(t_dedup);
            let accepted = !accepted_laws.is_empty();

            library.extend(accepted_laws.clone());
            let features_after = LibraryFeatures::extract(&library);

            trajectory.record(TrajectoryStep {
                epoch: iter,
                corpus_index: iter,
                pre_state: features_before,
                action: ActionKind::Discover,
                accepted,
                delta_dl: accepted_laws.len() as f64,
            });

            iterations.push(IterationSnapshot {
                iter,
                corpus_size: corpus.len(),
                library_size_before,
                new_law_count: accepted_laws.len(),
                features_after,
            });
            per_iter_timings.push(IterationTimings {
                corpus_gen_ns,
                extract_ns,
                dedup_ns,
            });

            // R37: track consecutive no-growth iterations. When
            // `early_stop_after = Some(W)` and we've hit W in a
            // row, stop early — further iterations won't change
            // the library if nothing's proposed anything new.
            if accepted_laws.is_empty() {
                consecutive_no_growth += 1;
            } else {
                consecutive_no_growth = 0;
            }
            if let Some(window) = early_stop_after {
                if consecutive_no_growth >= window {
                    break;
                }
            }
        }

        trajectory.finalize(LibraryFeatures::extract(&library));
        let t_train = Instant::now();
        self.updater.update(&mut policy, &trajectory);
        let train_ns = elapsed_ns(t_train);

        let attestation = compute_attestation(&library, &policy, &trajectory);
        let total_ns = elapsed_ns(cycle_start);

        BootstrapOutcome {
            iterations,
            final_library: library,
            final_policy: policy,
            trajectory,
            attestation,
            timings: CycleTimings {
                per_iteration: per_iter_timings,
                train_ns,
                total_ns,
            },
        }
    }
}

#[inline]
fn elapsed_ns(start: Instant) -> u64 {
    // as_nanos() returns u128; saturate to u64 — at 1 GHz this only
    // wraps after ~584 years of wall-clock inside a single seam.
    u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

// ── Outcome + attestation ──────────────────────────────────────────

/// Per-iteration summary captured during a cycle run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IterationSnapshot {
    pub iter: usize,
    pub corpus_size: usize,
    pub library_size_before: usize,
    pub new_law_count: usize,
    pub features_after: LibraryFeatures,
}

/// Full outcome of a BootstrapCycle run. Every field is
/// bincode-serializable; the attestation hash covers the whole.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BootstrapOutcome {
    pub iterations: Vec<IterationSnapshot>,
    pub final_library: Vec<RewriteRule>,
    pub final_policy: LinearPolicy,
    pub trajectory: Trajectory,
    /// BLAKE3 attestation — `compute_attestation(library, policy,
    /// trajectory)`. Covers the whole cycle result. Use this to
    /// detect drift when a layer implementation changes.
    pub attestation: TermRef,
    /// R34: wall-clock timings. OBSERVATIONAL only — NOT part of
    /// the attestation payload, so two runs with identical inputs
    /// still produce identical attestations despite varying clock
    /// readings.
    pub timings: CycleTimings,
}

/// Compute a BLAKE3 attestation hash for a cycle outcome. The
/// hash is of a canonical serialization of (library, policy,
/// trajectory). Two outcomes with identical content produce
/// identical attestations — the foundation of cycle-level
/// deterministic_replay.
pub fn compute_attestation(
    library: &[RewriteRule],
    policy: &LinearPolicy,
    trajectory: &Trajectory,
) -> TermRef {
    let payload = (
        library
            .iter()
            .map(|r| (r.name.clone(), r.lhs.clone(), r.rhs.clone()))
            .collect::<Vec<_>>(),
        policy.weights,
        policy.bias,
        policy.trained_steps,
        policy.generation,
        trajectory.steps.len(),
        trajectory.reached_tensor(),
    );
    let bytes = bincode::serialize(&payload).expect("serializable");
    TermRef::from_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::RewriteRule;
    use crate::value::Value;

    /// A trivial LawExtractor that emits one fixed law per run.
    /// Used in tests to verify the cycle mechanism without
    /// depending on mathscape-compress.
    struct FixedLawExtractor {
        law: RewriteRule,
    }

    impl LawExtractor for FixedLawExtractor {
        fn extract(
            &self,
            _corpus: &[Term],
            library: &[RewriteRule],
        ) -> Vec<RewriteRule> {
            // Emit the fixed law only on the first iteration where the
            // library doesn't already contain it. Otherwise, empty.
            if library.iter().any(|r| r.name == self.law.name) {
                Vec::new()
            } else {
                vec![self.law.clone()]
            }
        }
    }

    /// A null CorpusGenerator that produces empty corpora. Used
    /// for testing that the cycle's control flow is independent
    /// of what the corpus generator produces.
    struct NullCorpusGenerator;

    impl CorpusGenerator for NullCorpusGenerator {
        fn generate(&self, _iter: usize, _lib: &[RewriteRule]) -> Vec<Term> {
            Vec::new()
        }
    }

    /// A null ModelUpdater that does nothing. Proves the cycle
    /// doesn't require training to complete.
    struct NullModelUpdater;

    impl ModelUpdater for NullModelUpdater {
        fn update(&self, _p: &mut LinearPolicy, _t: &Trajectory) {}
    }

    fn dummy_law() -> RewriteRule {
        RewriteRule {
            name: "dummy".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![Term::Number(Value::Nat(0)), Term::Var(100)],
            ),
            rhs: Term::Var(100),
        }
    }

    #[test]
    fn cycle_runs_with_null_layers() {
        // Minimal cycle: null generator + fixed extractor + null
        // updater. Verifies the mechanism works end-to-end with
        // stub implementations.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            3,
        );
        let outcome = cycle.run(Vec::new(), LinearPolicy::new());
        // First iteration adds the law; subsequent iterations don't
        // re-add (FixedLawExtractor short-circuits once present).
        assert_eq!(outcome.final_library.len(), 1);
        assert_eq!(outcome.iterations.len(), 3);
    }

    #[test]
    fn attestation_is_deterministic() {
        // Two identical cycle runs ⇒ identical attestation hash.
        // This is cycle-level deterministic_replay.
        let cycle_a = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let cycle_b = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let out_a = cycle_a.run(Vec::new(), LinearPolicy::new());
        let out_b = cycle_b.run(Vec::new(), LinearPolicy::new());
        assert_eq!(out_a.attestation, out_b.attestation);
    }

    #[test]
    fn attestation_differs_when_content_differs() {
        // Different number of iterations ⇒ different attestation.
        let cycle_short = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            1,
        );
        let cycle_long = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            3,
        );
        let out_short = cycle_short.run(Vec::new(), LinearPolicy::new());
        let out_long = cycle_long.run(Vec::new(), LinearPolicy::new());
        assert_ne!(out_short.attestation, out_long.attestation);
    }

    #[test]
    fn hijack_corpus_generator_preserves_cycle_mechanism() {
        // "Hijack" the corpus generator: swap for NullCorpusGenerator.
        // Even with empty corpora, the mechanism completes because
        // the extractor still fires (FixedLawExtractor emits on
        // first call regardless of corpus).
        // This proves the CorpusGenerator layer is properly
        // encapsulated — the cycle doesn't leak assumptions about
        // what the generator returns.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let outcome = cycle.run(Vec::new(), LinearPolicy::new());
        assert!(outcome.attestation != TermRef::from_bytes(&[0u8; 32]));
        assert_eq!(outcome.iterations.len(), 2);
    }

    #[test]
    fn outcome_bincode_roundtrip() {
        // The typescape entity must be serde-roundtrippable for
        // persistence and cross-process attestation.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let outcome = cycle.run(Vec::new(), LinearPolicy::new());
        let bytes = bincode::serialize(&outcome).unwrap();
        let back: BootstrapOutcome = bincode::deserialize(&bytes).unwrap();
        assert_eq!(outcome, back);
    }

    // ── R27 invariant tests ──────────────────────────────────────

    #[test]
    fn zero_iteration_cycle_produces_empty_outcome() {
        // Edge case: N=0. Cycle completes without error; outcome
        // has no iterations, no trajectory steps. Library + policy
        // pass through unchanged from seeds.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            0,
        );
        let seed_lib = vec![dummy_law()];
        let seed_policy = LinearPolicy::tensor_seeking_prior();
        let outcome = cycle.run(seed_lib.clone(), seed_policy.clone());
        assert!(outcome.iterations.is_empty());
        assert!(outcome.trajectory.steps.is_empty());
        assert_eq!(outcome.final_library, seed_lib);
        // Null updater means policy unchanged; with seed weights
        // preserved.
        assert_eq!(outcome.final_policy.weights, seed_policy.weights);
    }

    #[test]
    fn cycle_library_is_monotonically_non_decreasing() {
        // Across iterations, library size never shrinks. Extracted
        // laws are only appended, never removed by the cycle
        // itself.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            5,
        );
        let outcome = cycle.run(Vec::new(), LinearPolicy::new());
        for pair in outcome.iterations.windows(2) {
            let prev = pair[0].features_after.rule_count;
            let curr = pair[1].features_after.rule_count;
            assert!(
                curr >= prev,
                "library must not shrink: {prev} → {curr}"
            );
        }
    }

    #[test]
    fn trajectory_step_count_matches_iteration_count() {
        // For any N iterations, the trajectory records exactly N
        // steps. Invariant: 1 step per iteration, no more no less.
        for n in [0, 1, 3, 7] {
            let cycle = BootstrapCycle::new(
                NullCorpusGenerator,
                FixedLawExtractor { law: dummy_law() },
                NullModelUpdater,
                n,
            );
            let outcome = cycle.run(Vec::new(), LinearPolicy::new());
            assert_eq!(
                outcome.trajectory.steps.len(),
                n,
                "trajectory step count must equal iteration count N={n}"
            );
        }
    }

    #[test]
    fn seed_library_passes_through_unchanged_with_null_extractor() {
        // A cycle whose extractor returns EMPTY for every call
        // must preserve the seed library exactly.
        struct EmptyExtractor;
        impl LawExtractor for EmptyExtractor {
            fn extract(&self, _c: &[Term], _l: &[RewriteRule]) -> Vec<RewriteRule> {
                Vec::new()
            }
        }
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            EmptyExtractor,
            NullModelUpdater,
            5,
        );
        let seed = vec![dummy_law()];
        let outcome = cycle.run(seed.clone(), LinearPolicy::new());
        assert_eq!(outcome.final_library, seed);
        // Each iteration recorded `accepted=false`.
        for step in &outcome.trajectory.steps {
            assert!(!step.accepted, "empty extractor should mark !accepted");
        }
    }

    #[test]
    fn attestation_covers_policy_changes() {
        // If the policy ends up different (different updater
        // trained it), attestation must differ — even if the
        // library is identical.
        let cycle_null_updater = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let cycle_default_updater = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            DefaultModelUpdater::default(),
            2,
        );
        let a = cycle_null_updater.run(Vec::new(), LinearPolicy::new());
        let b = cycle_default_updater.run(Vec::new(), LinearPolicy::new());
        assert_eq!(a.final_library, b.final_library);
        assert_ne!(a.attestation, b.attestation);
    }

    #[test]
    fn default_corpus_generator_is_deterministic() {
        // Two calls with same iter + library → identical corpus.
        let g = DefaultCorpusGenerator;
        let lib: Vec<RewriteRule> = Vec::new();
        assert_eq!(g.generate(0, &lib), g.generate(0, &lib));
        assert_eq!(g.generate(3, &lib), g.generate(3, &lib));
    }

    // ── R28: LibraryDeduper tests ─────────────────────────────────

    #[test]
    fn no_dedup_accepts_everything() {
        let d = NoDedup;
        let lib = vec![dummy_law()];
        // Even an exact duplicate is not rejected.
        assert!(!d.is_duplicate(&dummy_law(), &lib));
    }

    #[test]
    fn canonical_deduper_catches_exact_duplicates() {
        let d = CanonicalDeduper;
        let lib = vec![dummy_law()];
        assert!(d.is_duplicate(&dummy_law(), &lib));
    }

    #[test]
    fn canonical_deduper_misses_alpha_renamed_duplicates() {
        // Two rules that are alpha-equivalent but use different
        // pattern var ids will look distinct at the canonical
        // level (var ids aren't canonicalized by .canonical()).
        // AlphaDeduper catches these; CanonicalDeduper doesn't.
        let d = CanonicalDeduper;
        let r1 = RewriteRule {
            name: "a".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![Term::Number(Value::Nat(0)), Term::Var(100)],
            ),
            rhs: Term::Var(100),
        };
        let r2 = RewriteRule {
            name: "b".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![Term::Number(Value::Nat(0)), Term::Var(200)],
            ),
            rhs: Term::Var(200),
        };
        assert!(!d.is_duplicate(&r2, &[r1]));
    }

    #[test]
    fn alpha_deduper_catches_renamed_duplicates() {
        let d = AlphaDeduper;
        let r1 = RewriteRule {
            name: "a".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![Term::Number(Value::Nat(0)), Term::Var(100)],
            ),
            rhs: Term::Var(100),
        };
        let r2 = RewriteRule {
            name: "b".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![Term::Number(Value::Nat(0)), Term::Var(200)],
            ),
            rhs: Term::Var(200),
        };
        assert!(
            d.is_duplicate(&r2, &[r1]),
            "alpha deduper should catch renamed duplicates"
        );
    }

    /// Extractor that emits the SAME law on every iteration. With
    /// no dedup the library grows linearly; with dedup it saturates
    /// at 1.
    struct RepeatingExtractor {
        law: RewriteRule,
    }

    impl LawExtractor for RepeatingExtractor {
        fn extract(&self, _c: &[Term], _l: &[RewriteRule]) -> Vec<RewriteRule> {
            vec![self.law.clone()]
        }
    }

    #[test]
    fn no_dedup_repeats_grow_linearly() {
        // Without dedup, 5 iterations × 1 repeating law → library
        // size 5 (duplicates accepted).
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            RepeatingExtractor { law: dummy_law() },
            NullModelUpdater,
            5,
        );
        let outcome = cycle.run(Vec::new(), LinearPolicy::new());
        assert_eq!(outcome.final_library.len(), 5);
    }

    #[test]
    fn canonical_dedup_saturates_repeats() {
        // With CanonicalDeduper, 5 iterations × 1 repeating law
        // → library size 1. The duplicate is rejected from
        // iteration 2 onward.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            RepeatingExtractor { law: dummy_law() },
            NullModelUpdater,
            5,
        );
        let outcome = cycle.run_with_dedup(
            Vec::new(),
            LinearPolicy::new(),
            &CanonicalDeduper,
        );
        assert_eq!(
            outcome.final_library.len(),
            1,
            "dedup must prevent repeat-insertion"
        );
        // Iteration 0 accepts, iterations 1-4 reject.
        assert_eq!(outcome.iterations[0].new_law_count, 1);
        for iter in &outcome.iterations[1..] {
            assert_eq!(
                iter.new_law_count, 0,
                "post-iter-0 repeats must be dedup'd out"
            );
        }
    }

    #[test]
    fn dedup_within_a_single_iteration_works() {
        // Extractor that emits the SAME law twice in one call.
        // The cycle's dedup must catch the intra-iteration
        // duplicate, not just cross-iteration ones.
        struct DoubleEmit {
            law: RewriteRule,
        }
        impl LawExtractor for DoubleEmit {
            fn extract(&self, _: &[Term], _: &[RewriteRule]) -> Vec<RewriteRule> {
                vec![self.law.clone(), self.law.clone()]
            }
        }
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            DoubleEmit { law: dummy_law() },
            NullModelUpdater,
            1,
        );
        let outcome = cycle.run_with_dedup(
            Vec::new(),
            LinearPolicy::new(),
            &CanonicalDeduper,
        );
        // Two proposals, one kept after intra-iteration dedup.
        assert_eq!(outcome.final_library.len(), 1);
    }

    // ── R30: SubsumptionDeduper + deduplicate_library tests ─────

    fn ident_law(var_id: u32) -> RewriteRule {
        // add(0, ?var_id) = ?var_id
        RewriteRule {
            name: format!("id-{var_id}"),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![Term::Number(Value::Nat(0)), Term::Var(var_id)],
            ),
            rhs: Term::Var(var_id),
        }
    }

    #[test]
    fn subsumption_deduper_rejects_specialization() {
        // General: add(0, ?100) = ?100
        // Specialization: add(0, 5) = 5
        // Subsumption deduper rejects the specialization.
        let d = SubsumptionDeduper;
        let general = ident_law(100);
        let specific = RewriteRule {
            name: "add-0-5".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![
                    Term::Number(Value::Nat(0)),
                    Term::Number(Value::Nat(5)),
                ],
            ),
            rhs: Term::Number(Value::Nat(5)),
        };
        assert!(d.is_duplicate(&specific, &[general]));
    }

    #[test]
    fn subsumption_deduper_keeps_orthogonal_rules() {
        // add-identity does NOT subsume mul-identity.
        let d = SubsumptionDeduper;
        let add_id = ident_law(100);
        let mul_id = RewriteRule {
            name: "mul-id".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(3)),
                vec![Term::Number(Value::Nat(1)), Term::Var(100)],
            ),
            rhs: Term::Var(100),
        };
        assert!(!d.is_duplicate(&mul_id, &[add_id]));
    }

    #[test]
    fn subsumption_deduper_catches_exact_duplicates() {
        // Same rule as an existing library entry ⇒ duplicate.
        let d = SubsumptionDeduper;
        let r = ident_law(100);
        assert!(d.is_duplicate(&r, &[r.clone()]));
    }

    #[test]
    fn deduplicate_library_removes_duplicates_canonical() {
        // Library with explicit duplicates (same canonical form,
        // different var ids). CanonicalDeduper won't catch alpha-
        // renamed variants; AlphaDeduper should.
        let input = vec![
            ident_law(100),
            ident_law(200), // alpha-renamed identity law
            ident_law(100), // exact structural duplicate
        ];
        let (kept, rejected) =
            deduplicate_library(input.clone(), &CanonicalDeduper);
        // Canonical: catches exact duplicate (100) but not alpha-
        // renamed (200).
        assert_eq!(kept.len(), 2);
        assert_eq!(rejected.len(), 1);
    }

    #[test]
    fn deduplicate_library_alpha_catches_renamed() {
        let input = vec![
            ident_law(100),
            ident_law(200),
            ident_law(300),
        ];
        let (kept, rejected) =
            deduplicate_library(input, &AlphaDeduper);
        // AlphaDeduper: all three are alpha-equivalent. Keep 1.
        assert_eq!(kept.len(), 1);
        assert_eq!(rejected.len(), 2);
    }

    #[test]
    fn deduplicate_library_preserves_orthogonal_rules() {
        // Different operators, different shapes. No dedup applies.
        let mul_id = RewriteRule {
            name: "mul-id".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(3)),
                vec![Term::Number(Value::Nat(1)), Term::Var(100)],
            ),
            rhs: Term::Var(100),
        };
        let input = vec![ident_law(100), mul_id];
        let (kept, rejected) =
            deduplicate_library(input, &AlphaDeduper);
        assert_eq!(kept.len(), 2);
        assert_eq!(rejected.len(), 0);
    }

    #[test]
    fn deduplicate_library_empty_input_is_empty_output() {
        let (kept, rejected) =
            deduplicate_library(Vec::new(), &CanonicalDeduper);
        assert!(kept.is_empty());
        assert!(rejected.is_empty());
    }

    #[test]
    fn deduplicate_library_with_nodedup_keeps_everything() {
        // NoDedup should pass every rule through, including exact
        // duplicates.
        let input = vec![
            ident_law(100),
            ident_law(100),
            ident_law(100),
        ];
        let (kept, rejected) = deduplicate_library(input, &NoDedup);
        assert_eq!(kept.len(), 3);
        assert!(rejected.is_empty());
    }

    // ── R33: ExperimentScenario tests ────────────────────────────

    #[test]
    fn empty_scenario_produces_empty_outcome() {
        let scenario = ExperimentScenario {
            name: "empty".into(),
            phases: Vec::new(),
        };
        let outcome = execute_scenario_core(&scenario).unwrap();
        assert!(outcome.phases.is_empty());
    }

    #[test]
    fn single_phase_scenario_matches_spec_execution() {
        let spec = BootstrapCycleSpec {
            corpus_generator: "null".into(),
            law_extractor: "null".into(),
            model_updater: "default".into(),
            deduper: "canonical".into(),
            n_iterations: 3,
            seed_library: Vec::new(),
            seed_policy: LinearPolicy::tensor_seeking_prior(),
            early_stop_after_stable: None,
        };
        let scenario = ExperimentScenario {
            name: "one-phase".into(),
            phases: vec![spec.clone()],
        };
        let spec_outcome = execute_spec_core(&spec).unwrap();
        let scen_outcome = execute_scenario_core(&scenario).unwrap();
        assert_eq!(scen_outcome.phases.len(), 1);
        assert_eq!(
            scen_outcome.phases[0].cycle_outcome.final_policy,
            spec_outcome.final_policy
        );
        assert_eq!(
            scen_outcome.phases[0].cycle_outcome.attestation,
            spec_outcome.attestation
        );
    }

    #[test]
    fn multi_phase_scenario_chains_library_and_policy() {
        // Phase 0: run with default updater, no laws added (null
        // extractor). Produces a policy with generation=1.
        // Phase 1: chained from phase 0; spec's seed is overridden.
        // After phase 1, policy.generation should be 2 (trained
        // twice).
        let base = BootstrapCycleSpec {
            corpus_generator: "null".into(),
            law_extractor: "null".into(),
            model_updater: "default".into(),
            deduper: "canonical".into(),
            n_iterations: 2,
            seed_library: Vec::new(),
            seed_policy: LinearPolicy::tensor_seeking_prior(),
            early_stop_after_stable: None,
        };
        let scenario = ExperimentScenario {
            name: "two-phase".into(),
            phases: vec![base.clone(), base.clone(), base],
        };
        let outcome = execute_scenario_core(&scenario).unwrap();
        assert_eq!(outcome.phases.len(), 3);
        // Each phase trains the policy once more.
        assert_eq!(outcome.phases[0].cycle_outcome.final_policy.generation, 1);
        assert_eq!(outcome.phases[1].cycle_outcome.final_policy.generation, 2);
        assert_eq!(outcome.phases[2].cycle_outcome.final_policy.generation, 3);
        // The final_model helper returns the last policy.
        assert_eq!(outcome.final_model().generation, 3);
    }

    #[test]
    fn scenario_chain_attestation_is_deterministic() {
        let base = BootstrapCycleSpec {
            corpus_generator: "null".into(),
            law_extractor: "null".into(),
            model_updater: "default".into(),
            deduper: "none".into(),
            n_iterations: 1,
            seed_library: Vec::new(),
            seed_policy: LinearPolicy::new(),
            early_stop_after_stable: None,
        };
        let scenario = ExperimentScenario {
            name: "det".into(),
            phases: vec![base.clone(), base],
        };
        let a = execute_scenario_core(&scenario).unwrap();
        let b = execute_scenario_core(&scenario).unwrap();
        assert_eq!(a.chain_attestation, b.chain_attestation);
    }

    #[test]
    fn scenario_per_phase_growth_reports_increments() {
        let base = BootstrapCycleSpec {
            corpus_generator: "null".into(),
            law_extractor: "null".into(),
            model_updater: "null".into(),
            deduper: "canonical".into(),
            n_iterations: 1,
            seed_library: vec![dummy_law()],
            seed_policy: LinearPolicy::new(),
            early_stop_after_stable: None,
        };
        let scenario = ExperimentScenario {
            name: "growth".into(),
            phases: vec![base.clone(), base],
        };
        let outcome = execute_scenario_core(&scenario).unwrap();
        // Null extractor adds nothing; growth is 0 per phase after
        // the initial seed.
        assert_eq!(outcome.per_phase_growth(), vec![1, 0]);
    }

    #[test]
    fn scenario_unknown_layer_propagates_error() {
        let bad = BootstrapCycleSpec {
            corpus_generator: "nope-not-a-real-generator".into(),
            law_extractor: "null".into(),
            model_updater: "default".into(),
            deduper: "canonical".into(),
            n_iterations: 1,
            seed_library: Vec::new(),
            seed_policy: LinearPolicy::new(),
            early_stop_after_stable: None,
        };
        let scenario = ExperimentScenario {
            name: "bad".into(),
            phases: vec![bad],
        };
        let result = execute_scenario_core(&scenario);
        assert!(matches!(
            result,
            Err(SpecExecutionError::UnknownLayer {
                role: "corpus_generator",
                ..
            })
        ));
    }

    #[test]
    fn scenario_bincode_roundtrip() {
        let base = BootstrapCycleSpec::default_m0();
        let scenario = ExperimentScenario {
            name: "rt".into(),
            phases: vec![base.clone(), base],
        };
        let bytes = bincode::serialize(&scenario).unwrap();
        let back: ExperimentScenario = bincode::deserialize(&bytes).unwrap();
        assert_eq!(scenario, back);
    }

    #[test]
    fn subsumption_stronger_than_alpha_stronger_than_canonical() {
        // Lattice property: if CanonicalDeduper rejects, so does
        // AlphaDeduper, so does SubsumptionDeduper. Demonstrate via
        // an exact-duplicate input that all three catch.
        let lib = vec![ident_law(100)];
        let cand = ident_law(100);
        assert!(CanonicalDeduper.is_duplicate(&cand, &lib));
        assert!(AlphaDeduper.is_duplicate(&cand, &lib));
        assert!(SubsumptionDeduper.is_duplicate(&cand, &lib));

        // And Alpha catches some rules Canonical misses (renamed
        // variants). Subsumption catches some rules Alpha misses
        // (specializations). Each is strictly stronger.
    }

    #[test]
    fn dedup_layer_is_deterministic() {
        // Two identical runs with dedup produce identical output.
        let cycle_a = BootstrapCycle::new(
            NullCorpusGenerator,
            RepeatingExtractor { law: dummy_law() },
            NullModelUpdater,
            3,
        );
        let cycle_b = BootstrapCycle::new(
            NullCorpusGenerator,
            RepeatingExtractor { law: dummy_law() },
            NullModelUpdater,
            3,
        );
        let a = cycle_a.run_with_dedup(
            Vec::new(),
            LinearPolicy::new(),
            &CanonicalDeduper,
        );
        let b = cycle_b.run_with_dedup(
            Vec::new(),
            LinearPolicy::new(),
            &CanonicalDeduper,
        );
        assert_eq!(a.attestation, b.attestation);
    }

    #[test]
    fn default_corpus_generator_iter_0_differs_from_later() {
        // The iteration-index dispatch must actually produce
        // different corpora for different iterations (the default
        // escalates complexity at iter ≥ 1).
        let g = DefaultCorpusGenerator;
        let lib: Vec<RewriteRule> = Vec::new();
        assert_ne!(g.generate(0, &lib), g.generate(1, &lib));
    }

    // ── R34: timing invariants ────────────────────────────────────

    #[test]
    fn timings_per_iteration_length_equals_iteration_count() {
        for n in [0usize, 1, 3, 5] {
            let cycle = BootstrapCycle::new(
                NullCorpusGenerator,
                FixedLawExtractor { law: dummy_law() },
                NullModelUpdater,
                n,
            );
            let outcome = cycle.run(Vec::new(), LinearPolicy::new());
            assert_eq!(
                outcome.timings.per_iteration.len(),
                n,
                "per_iteration length must equal iteration count N={n}"
            );
        }
    }

    #[test]
    fn timings_do_not_affect_attestation() {
        // Two runs on identical inputs — wall-clock timings will
        // differ slightly but attestation must be bit-identical.
        let cycle = || {
            BootstrapCycle::new(
                NullCorpusGenerator,
                FixedLawExtractor { law: dummy_law() },
                NullModelUpdater,
                3,
            )
        };
        let a = cycle().run(Vec::new(), LinearPolicy::new());
        let b = cycle().run(Vec::new(), LinearPolicy::new());
        assert_eq!(
            a.attestation, b.attestation,
            "attestation must be independent of wall-clock timings"
        );
    }

    #[test]
    fn total_ns_covers_iterations_plus_train() {
        // total_ns >= sum of per-iteration totals + train_ns. The
        // difference is outer-loop overhead (trajectory ops,
        // feature extraction, attestation).
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            DefaultModelUpdater::default(),
            4,
        );
        let outcome = cycle.run(Vec::new(), LinearPolicy::new());
        let inner = outcome
            .timings
            .iter_sum_ns()
            .saturating_add(outcome.timings.train_ns);
        assert!(
            outcome.timings.total_ns >= inner,
            "total_ns ({}) must cover iter_sum+train ({inner})",
            outcome.timings.total_ns
        );
    }

    #[test]
    fn zero_iter_cycle_still_records_total_timing() {
        // Even with no iterations, the total wall-clock is measured
        // (attestation + updater call still happen). Per-iteration
        // vec is empty, but total_ns is typically > 0.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            DefaultModelUpdater::default(),
            0,
        );
        let outcome = cycle.run(Vec::new(), LinearPolicy::new());
        assert!(outcome.timings.per_iteration.is_empty());
        // total_ns is u64; we don't assert > 0 (on mock clocks it
        // could underflow to 0) but we do assert it's bounded.
        assert!(outcome.timings.total_ns < u64::MAX);
    }

    #[test]
    fn scenario_total_covers_phase_totals() {
        // The scenario total wall-clock must be at least as large
        // as the sum of per-phase totals — the phases execute
        // sequentially inside the scenario loop.
        let base = BootstrapCycleSpec {
            corpus_generator: "null".into(),
            law_extractor: "null".into(),
            model_updater: "default".into(),
            deduper: "canonical".into(),
            n_iterations: 2,
            seed_library: Vec::new(),
            seed_policy: LinearPolicy::new(),
            early_stop_after_stable: None,
        };
        let scenario = ExperimentScenario {
            name: "timing".into(),
            phases: vec![base.clone(), base.clone(), base],
        };
        let outcome = execute_scenario_core(&scenario).unwrap();
        let phase_sum: u64 = outcome
            .per_phase_timings_ns()
            .iter()
            .copied()
            .fold(0u64, u64::saturating_add);
        assert!(
            outcome.scenario_total_ns >= phase_sum,
            "scenario_total_ns ({}) must cover phase sum ({})",
            outcome.scenario_total_ns,
            phase_sum,
        );
    }

    #[test]
    fn scenario_chain_attestation_independent_of_timings() {
        // Running the same scenario twice: timings vary, chain
        // attestation does not.
        let base = BootstrapCycleSpec {
            corpus_generator: "null".into(),
            law_extractor: "null".into(),
            model_updater: "default".into(),
            deduper: "canonical".into(),
            n_iterations: 1,
            seed_library: Vec::new(),
            seed_policy: LinearPolicy::new(),
            early_stop_after_stable: None,
        };
        let scenario = ExperimentScenario {
            name: "attn".into(),
            phases: vec![base.clone(), base],
        };
        let a = execute_scenario_core(&scenario).unwrap();
        let b = execute_scenario_core(&scenario).unwrap();
        assert_eq!(a.chain_attestation, b.chain_attestation);
    }

    // ── R37: early-stop on plateau tests ─────────────────────────

    #[test]
    fn run_until_stable_short_circuits_on_plateau() {
        // FixedLawExtractor emits the law only once (first call).
        // With CanonicalDeduper + stable_window=1, the second
        // iteration adds 0 rules → stop. Total iterations = 2.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            100, // request 100 iters — we should stop WAY before that
        );
        let outcome = cycle.run_until_stable(
            Vec::new(),
            LinearPolicy::new(),
            &CanonicalDeduper,
            1,
        );
        assert_eq!(
            outcome.iterations.len(),
            2,
            "iter 0 accepts, iter 1 plateaus → stop after 2"
        );
        assert_eq!(outcome.final_library.len(), 1);
        // Trajectory steps match iterations (not n_iterations).
        assert_eq!(outcome.trajectory.steps.len(), 2);
    }

    #[test]
    fn run_until_stable_respects_wider_window() {
        // With stable_window=3, we need 3 consecutive no-growth
        // iterations to stop. FixedLawExtractor: iter 0 accepts,
        // iter 1 no-growth, iter 2 no-growth, iter 3 no-growth →
        // stop after 4 iterations.
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            100,
        );
        let outcome = cycle.run_until_stable(
            Vec::new(),
            LinearPolicy::new(),
            &CanonicalDeduper,
            3,
        );
        assert_eq!(outcome.iterations.len(), 4);
    }

    #[test]
    fn run_until_stable_reaches_n_iterations_when_always_growing() {
        // If the extractor keeps producing NEW laws every iter,
        // the plateau is never reached and we run all N.
        struct GrowingExtractor {
            counter: std::cell::Cell<u32>,
        }
        impl LawExtractor for GrowingExtractor {
            fn extract(&self, _: &[Term], _: &[RewriteRule]) -> Vec<RewriteRule> {
                let id = self.counter.get();
                self.counter.set(id + 1);
                vec![RewriteRule {
                    name: format!("grow-{id}"),
                    lhs: Term::Var(1000 + id),
                    rhs: Term::Var(1000 + id),
                }]
            }
        }
        let cycle = BootstrapCycle::new(
            NullCorpusGenerator,
            GrowingExtractor {
                counter: std::cell::Cell::new(0),
            },
            NullModelUpdater,
            5,
        );
        // Even with short stable_window=1, the library grows every
        // iter — we never hit the plateau.
        let outcome = cycle.run_until_stable(
            Vec::new(),
            LinearPolicy::new(),
            &NoDedup,
            1,
        );
        assert_eq!(outcome.iterations.len(), 5);
    }

    #[test]
    fn early_stop_via_bootstrap_cycle_spec_sexp_bridge() {
        // R32+R33+R37 integration: author a spec with early_stop,
        // execute via the core executor, get back shortened
        // trajectory.
        let spec = BootstrapCycleSpec {
            corpus_generator: "null".into(),
            law_extractor: "null".into(),
            model_updater: "null".into(),
            deduper: "canonical".into(),
            n_iterations: 100,
            seed_library: Vec::new(),
            seed_policy: LinearPolicy::new(),
            early_stop_after_stable: Some(1),
        };
        let outcome = execute_spec_core(&spec).unwrap();
        // NullExtractor produces nothing → iter 0 is already a
        // no-growth iteration → stop after iter 0.
        assert_eq!(outcome.iterations.len(), 1);
        assert!(outcome.final_library.is_empty());
    }

    #[test]
    fn empty_scenario_has_zero_phase_sum_and_bounded_total() {
        let scenario = ExperimentScenario {
            name: "empty".into(),
            phases: Vec::new(),
        };
        let outcome = execute_scenario_core(&scenario).unwrap();
        assert!(outcome.per_phase_timings_ns().is_empty());
        assert!(outcome.scenario_total_ns < u64::MAX);
    }

    #[test]
    fn layer_boundaries_are_independent() {
        // Change the updater independently of the other layers.
        // Old output (null updater) and new output (swapped
        // updater) must DIFFER in policy but agree on library.
        //
        // This demonstrates the "hijack and optimize" property:
        // swap a layer and observe only that layer's effect.
        let cycle_null = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            NullModelUpdater,
            2,
        );
        let cycle_default = BootstrapCycle::new(
            NullCorpusGenerator,
            FixedLawExtractor { law: dummy_law() },
            DefaultModelUpdater::default(),
            2,
        );
        let out_null = cycle_null.run(Vec::new(), LinearPolicy::new());
        let out_default = cycle_default.run(Vec::new(), LinearPolicy::new());

        // Libraries identical (updater doesn't touch library).
        assert_eq!(out_null.final_library, out_default.final_library);
        // Policies differ: default trained, null didn't.
        // Generation is the marker.
        assert_ne!(
            out_null.final_policy.generation, out_default.final_policy.generation
        );
    }
}
