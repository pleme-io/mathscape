//! The typed border of the garimpo acquisition space.
//!
//! This module realizes `theory/GARIMPO.md §3.5` as real Rust: the
//! acquisition space is **fully known because it is fully typed** — you
//! know exactly what a valid acquisition is, and every invalid one has
//! no constructor. Per `UNREPRESENTABILITY.md §II`, each primitive
//! carries its honest seal-tier, and where the tier is
//! `only-mitigated` today the type makes the floor **visible** (both a
//! weak `Sample`/`TrustedPrimitive` constructor and a destination
//! `Proof`/`EquivalenceChecked` constructor exist side by side).
//!
//! Reuse, not re-implementation: `Layer` wraps a
//! `mathscape_core::eval::RewriteRule`; the acquisition DAG reuses the
//! BLAKE3 `mathscape_core::hash::TermRef` content-addressing shape that
//! the shipped `Artifact` `parent_hashes` DAG already uses
//! (`epoch.rs`). This crate adds the *typed acquisition invariants* on
//! top of that substrate; it does not fork it.

use mathscape_core::eval::RewriteRule;
use mathscape_core::hash::TermRef;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ── Typed errors (no silent stubs; typed gaps for the destination) ────

/// The one typed error the acquisition interpreter returns. Modeled on
/// the sui-spec `SpecError::Interp { phase }` shape: an unimplemented
/// destination surface returns a **typed gap** naming its phase, never
/// a fake `Ok`, never a `panic!`/`todo!`/`unimplemented!` in a
/// production path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecError {
    /// A destination capability that is deliberately not yet built —
    /// the proof-grade seal (§3.5.1) and the per-fold-back equivalence
    /// gate (§3.5.2). Carries the phase name so consumers see the gap
    /// mechanically. This is the honest floor made legible: the
    /// constructor for `SealWitness::Proof` / the
    /// `SemanticsPreservingWitness::EquivalenceChecked` interpreter
    /// path returns this, not a rounded-up pass.
    Interp { phase: &'static str },
    /// A structurally-invalid acquisition the border refuses to build:
    /// a DL-increasing absorb, an unsealed layer counted as ground, a
    /// lost/unreachable DAG node.
    Invariant { what: &'static str },
    /// A malformed `(defacquisition …)` form at the parse boundary.
    Parse { detail: String },
}

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpecError::Interp { phase } => {
                write!(f, "acquisition interpreter gap at phase `{phase}` (destination, not yet built)")
            }
            SpecError::Invariant { what } => {
                write!(f, "acquisition invariant violated: {what}")
            }
            SpecError::Parse { detail } => {
                write!(f, "acquisition (defacquisition …) parse error: {detail}")
            }
        }
    }
}

impl std::error::Error for SpecError {}

// ── Description length (the MDL currency) ─────────────────────────────

/// A description length in bits. The MDL currency of the ratchet
/// (`epoch.rs::AcceptanceCertificate` computes the ratios; this is the
/// scalar the absorb decision gates on).
///
/// Wraps a non-negative `u64` bit-count. A description length cannot be
/// negative — the constructor rejects nothing here because `u64` is the
/// invariant, but keeping it a newtype means `old_dl`/`new_dl` can
/// never be transposed by accident at a call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Dl(pub u64);

impl Dl {
    #[must_use]
    pub fn bits(n: u64) -> Self {
        Dl(n)
    }
}

impl fmt::Display for Dl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} bits", self.0)
    }
}

// ── SealWitness — INV-1 (§3.5.1) ──────────────────────────────────────

/// The two proof-grade destinations. `Saturated` is a saturated
/// e-graph equivalence decision (parse-time-rejected tier);
/// `LeanKernel` is a kernel-checked Lean 4 proof
/// (truly-unrepresentable tier — the `Primitive` status IS a proof
/// object, Curry-Howard). Neither is buildable today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofKind {
    Saturated,
    LeanKernel,
}

/// Proof that a candidate layer is sound enough to count as
/// substrate-ground. **Two constructors — the weak floor and the
/// destination — so the honest tier is legible in the type itself.**
///
/// - `Sample { .. }` — SHIPPED. `validate_semantically` agreed on `k`
///   random samples through the primitive evaluator (default `k=16`).
///   Seal-tier **only-mitigated**: `k` agreeing samples can admit a
///   *false generalization* (holds on the sampled range, fails
///   beyond). This is the floor, named as such.
/// - `Proof { .. }` — DESTINATION. Saturated-equivalence
///   (parse-time-rejected) or kernel-checked Lean
///   (truly-unrepresentable). Its **constructor is gated** behind the
///   interpreter (`interp::seal_proof`), which returns
///   `SpecError::Interp { phase: "proof-seal" }` today — a typed gap,
///   never a fake pass (`lean_export.rs` is a `sorry`-stub).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SealWitness {
    Sample { k: usize },
    Proof { kind: ProofKind },
}

impl SealWitness {
    /// The shipped floor. Sixteen-ish agreeing samples — honestly
    /// `only-mitigated`.
    #[must_use]
    pub fn sample(k: usize) -> Self {
        SealWitness::Sample { k }
    }

    /// Is this the proof-grade destination seal (vs the sampled floor)?
    /// Consumers use this to flag a `Sample`-sealed layer as
    /// not-yet-proof-sealed.
    #[must_use]
    pub fn is_proof_grade(&self) -> bool {
        matches!(self, SealWitness::Proof { .. })
    }

    /// The honest tier label for this witness, per
    /// `UNREPRESENTABILITY.md §II`.
    #[must_use]
    pub fn tier(&self) -> Tier {
        match self {
            SealWitness::Sample { .. } => Tier::OnlyMitigated,
            SealWitness::Proof { kind: ProofKind::Saturated } => Tier::ParseTimeRejected,
            SealWitness::Proof { kind: ProofKind::LeanKernel } => Tier::TrulyUnrepresentable,
        }
    }
}

/// The honest three-tier ladder (`UNREPRESENTABILITY.md §II`). Ordered
/// weakest → strongest so a consumer can `>=`-compare a floor against a
/// required minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tier {
    OnlyMitigated,
    ParseTimeRejected,
    TrulyUnrepresentable,
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Tier::OnlyMitigated => "only-mitigated",
            Tier::ParseTimeRejected => "parse-time-rejected",
            Tier::TrulyUnrepresentable => "truly-unrepresentable",
        };
        write!(f, "{s}")
    }
}

// ── Layer — ground iff sealed (INV-1, §3.5.1) ─────────────────────────

/// A discovered layer: a rewrite rule plus the witness that seals it
/// sound. A `Layer` is substrate-ground **only if sealed** — there is
/// no way to build a `Layer` without a `SealWitness`, so the illegal
/// state *"a confirmed layer that still contains an unproven rule"* has
/// no constructor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layer {
    pub rule: RewriteRule,
    pub seal: SealWitness,
}

impl Layer {
    /// Build a layer sealed by the shipped `Sample` floor. Honest tier:
    /// `only-mitigated`.
    #[must_use]
    pub fn sampled(rule: RewriteRule, k: usize) -> Self {
        Layer {
            rule,
            seal: SealWitness::sample(k),
        }
    }

    /// True iff this layer rests on the sampled floor (not a
    /// proof-grade seal) — the flag consumers use to surface the
    /// not-yet-proof-sealed set.
    #[must_use]
    pub fn is_sample_sealed(&self) -> bool {
        !self.seal.is_proof_grade()
    }
}

// ── SemanticsPreservingWitness — INV-2 (§3.5.2) ───────────────────────

/// Proof that re-expressing earlier layers through a new primitive
/// preserves what the substrate proves. Carried by every `FoldBack`.
///
/// - `TrustedPrimitive { .. }` — SHIPPED. `migrate_library` trusts the
///   gate-confirmed primitive's `(lhs ⇒ rhs)` and substitutes directly;
///   structural pattern-match correctness plus INV-1 is the mitigation.
///   Seal-tier **only-mitigated** — no per-rewrite semantic re-check.
/// - `EquivalenceChecked { .. }` — DESTINATION. Per-migration
///   `check_equivalence(old_rhs, new_rhs, probes, SATURATED)` returned
///   `Some(true)`. Seal-tier **parse-time-rejected**. Its interpreter
///   path returns `SpecError::Interp { phase: "equivalence-gate" }`
///   today — a typed gap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticsPreservingWitness {
    TrustedPrimitive { via_seal: SealWitness },
    EquivalenceChecked { saturated: bool },
}

impl SemanticsPreservingWitness {
    #[must_use]
    pub fn tier(&self) -> Tier {
        match self {
            SemanticsPreservingWitness::TrustedPrimitive { .. } => Tier::OnlyMitigated,
            SemanticsPreservingWitness::EquivalenceChecked { .. } => Tier::ParseTimeRejected,
        }
    }
}

/// A fold-back: re-express earlier layers through a newly-minted
/// primitive. A `FoldBack` **cannot exist** without a
/// `SemanticsPreservingWitness` — a fold-back that changes an existing
/// layer's meaning has no constructor (INV-2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoldBack {
    /// The primitive layer whose `lhs`-pattern is substituted into the
    /// earlier layers' `rhs` terms.
    pub via_primitive: Layer,
    /// How many earlier-layer subterms were rewritten (the migration's
    /// footprint — mirrors `MigrationReport`).
    pub rewritten: usize,
    pub witness: SemanticsPreservingWitness,
}

// ── AbsorbDecision — significance-as-a-type (§3.5.3, §1.5) ─────────────

/// Constructs **only** when folding the layer back strictly lowers
/// total description length. A DL-increasing layer cannot be absorbed —
/// significance is a *constructor precondition*, not a runtime `if`.
/// This is the Chaitin/MDL "absorb iff `DL(S ⊕ ℓ, folded) < DL(S)`"
/// criterion (§1.5) as a total constructor. Buildable **today**;
/// seal-tier **parse-time-rejected** (the DL-increasing absorb is
/// refused at the border).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbsorbDecision {
    old_dl: Dl,
    new_dl: Dl, // invariant, upheld by `try_new`: new_dl < old_dl
}

impl AbsorbDecision {
    /// The MDL re-optimization trigger. Returns `Some` iff
    /// `new_dl < old_dl`; a layer that costs bits (or is DL-neutral)
    /// yields `None`. There is no other constructor, so an
    /// `AbsorbDecision` in hand is a *proof* that the absorb pays for
    /// itself.
    #[must_use]
    pub fn try_new(old_dl: Dl, new_dl: Dl) -> Option<AbsorbDecision> {
        if new_dl < old_dl {
            Some(AbsorbDecision { old_dl, new_dl })
        } else {
            None
        }
    }

    #[must_use]
    pub fn old_dl(&self) -> Dl {
        self.old_dl
    }

    #[must_use]
    pub fn new_dl(&self) -> Dl {
        self.new_dl
    }

    /// Bits saved by the absorb — always positive by the invariant.
    #[must_use]
    pub fn bits_saved(&self) -> u64 {
        self.old_dl.0 - self.new_dl.0
    }
}

// ── Frontier — incompleteness as a type (§3.5.4, §1.5) ────────────────

/// A complexity level (Chaitin's `K`, in bits of algorithmic-
/// information content of the substrate). Ordered so the frontier can
/// advance upward monotonically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Complexity(pub u64);

/// The acquisition frontier. By Chaitin's incompleteness (§1.5) the
/// seam is **never satisfiable** — so there is **no `Closed` variant**.
/// The state "everything provable has been proven" is
/// *unrepresentable*: omitting the arm is the theorem, encoded.
/// Seal-tier **truly-unrepresentable**, buildable today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frontier {
    /// Dark space remains above `ceiling` — abstractions strictly more
    /// complex than the substrate can certify from below.
    Open { ceiling: Complexity },
    /// `K` is being pushed up: minting a new primitive that raises the
    /// substrate's own complexity, opening a fresh frontier above.
    Advancing { from: Complexity, to: Complexity },
    // NO `Closed`. The absence is load-bearing.
}

impl Frontier {
    /// A fresh frontier at complexity `ceiling`.
    #[must_use]
    pub fn open(ceiling: Complexity) -> Self {
        Frontier::Open { ceiling }
    }

    /// Advance the ceiling upward (mint a primitive). Chaitin: this
    /// opens a new frontier, it never closes one — the result is always
    /// another non-terminal `Frontier`, never a satisfied state.
    #[must_use]
    pub fn advance(self, to: Complexity) -> Frontier {
        let from = match self {
            Frontier::Open { ceiling } => ceiling,
            Frontier::Advancing { to: prev, .. } => prev,
        };
        Frontier::Advancing { from, to }
    }
}

// ── AcquisitionState / AcquisitionDAG — reversible (§3.5.5) ────────────

/// One node of the acquisition history. Content-addressed (BLAKE3, via
/// `TermRef`) with a `parent` link — reuses the shipped `Artifact`
/// `parent_hashes` DAG shape (`epoch.rs`), **not** the `ModelSnapshot`
/// struct (which has no parent field — §2.5). A `move` in the DAG
/// (a layer, a fold-back) is a new node whose `parent` points at the
/// state it derived from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcquisitionState {
    /// BLAKE3 content hash of `(parent, frontier, layers)` — the node's
    /// identity, recomputed by `seal` so it cannot be forged by field
    /// mutation.
    pub content_hash: TermRef,
    /// The state this one was derived from. `None` only for the genesis
    /// node.
    pub parent: Option<TermRef>,
    /// The frontier at this node.
    pub frontier: Frontier,
    /// The layers held at this node (content of the substrate).
    pub layers: Vec<Layer>,
}

impl AcquisitionState {
    /// Canonical constructor: computes `content_hash` from
    /// `(parent, frontier, layers)`. The hash invariant cannot be
    /// bypassed short of mutating fields post-construction — same
    /// discipline as `Artifact::seal`.
    #[must_use]
    pub fn seal(parent: Option<TermRef>, frontier: Frontier, layers: Vec<Layer>) -> Self {
        let content_hash = Self::canonical_hash(&parent, &frontier, &layers);
        AcquisitionState {
            content_hash,
            parent,
            frontier,
            layers,
        }
    }

    fn canonical_hash(
        parent: &Option<TermRef>,
        frontier: &Frontier,
        layers: &[Layer],
    ) -> TermRef {
        let bytes = bincode::serialize(&(parent, frontier, layers))
            .expect("AcquisitionState canonical_hash: bincode serialization infallible for owned types");
        TermRef::from_bytes(&bytes)
    }

    /// The genesis node — an empty substrate at a starting frontier,
    /// with no parent.
    #[must_use]
    pub fn genesis(ceiling: Complexity) -> Self {
        Self::seal(None, Frontier::open(ceiling), Vec::new())
    }
}

/// The whole acquisition space as a content-addressed, reversible DAG:
/// every layer and every fold-back is a node; `restore`/`fork` are
/// total. A **lost or unreachable state is unrepresentable** — every
/// node is content-addressed and parent-linked, so "move back and forth
/// and do what we want" is typed, not hoped for.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AcquisitionDAG {
    nodes: BTreeMap<TermRef, AcquisitionState>,
    head: Option<TermRef>,
}

impl AcquisitionDAG {
    #[must_use]
    pub fn new() -> Self {
        AcquisitionDAG {
            nodes: BTreeMap::new(),
            head: None,
        }
    }

    /// Insert a node and advance `head` to it. Idempotent by
    /// content-hash: re-inserting the same state is a no-op that
    /// re-points head — content-addressing makes duplicate nodes
    /// unrepresentable.
    pub fn commit(&mut self, state: AcquisitionState) -> TermRef {
        let hash = state.content_hash;
        self.nodes.insert(hash, state);
        self.head = Some(hash);
        hash
    }

    /// The current head node.
    #[must_use]
    pub fn head(&self) -> Option<&AcquisitionState> {
        self.head.and_then(|h| self.nodes.get(&h))
    }

    /// Look a node up by content-hash. Every committed state is
    /// reachable — no state is ever lost.
    #[must_use]
    pub fn get(&self, hash: &TermRef) -> Option<&AcquisitionState> {
        self.nodes.get(hash)
    }

    /// **Restore** — move head back (or forward) to any committed node.
    /// The `git revert` for the acquisition space. Returns the restored
    /// state, or `SpecError::Invariant` if the target was never
    /// committed (a lost/unreachable state — which content-addressing
    /// prevents in practice).
    pub fn restore(&mut self, hash: &TermRef) -> Result<&AcquisitionState, SpecError> {
        if self.nodes.contains_key(hash) {
            self.head = Some(*hash);
            Ok(self.nodes.get(hash).expect("just checked containment"))
        } else {
            Err(SpecError::Invariant {
                what: "restore target is not a committed node (lost/unreachable state)",
            })
        }
    }

    /// **Fork** — an independent DAG rooted at a committed node's
    /// lineage. The fork and the original evolve separately from that
    /// point (A/B a speculative direction; keep the winner by
    /// content-hash). Walks `parent` links from `hash` to genesis so
    /// the fork carries the full history it needs to reason about, then
    /// sets its head at the fork point.
    pub fn fork(&self, hash: &TermRef) -> Result<AcquisitionDAG, SpecError> {
        let mut forked = AcquisitionDAG::new();
        let mut cursor = Some(*hash);
        // Collect the lineage newest→genesis, then commit oldest→newest
        // so parents exist before children and head lands at the fork
        // point.
        let mut lineage: Vec<AcquisitionState> = Vec::new();
        while let Some(h) = cursor {
            let node = self.nodes.get(&h).ok_or(SpecError::Invariant {
                what: "fork encountered a lost/unreachable ancestor",
            })?;
            lineage.push(node.clone());
            cursor = node.parent;
        }
        for node in lineage.into_iter().rev() {
            forked.commit(node);
        }
        Ok(forked)
    }

    /// Number of committed nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
