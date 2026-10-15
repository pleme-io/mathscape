# Mathscape Machine — The Synthesis

This document is the **canonical architectural picture**. It supersedes
apparent contradictions among the other `docs/arch/` documents: where
they differ, this document is authoritative. Every other doc has been
updated to cite it.

## The single primitive: Events

Every motion of the mathscape machine is a typed `Event`. Epochs,
traces, ΔDL, regimes, registries, allocators — all derive from the
event stream. If you understand events, you understand the machine.

```rust
#[derive(Debug, Clone)]
pub enum Event {
    // Discovery pass events
    Proposal     { candidate: Candidate,           delta_dl: f64 },
    Reject       { candidate_hash: TermRef,        reasons: Vec<Rejection> },
    Accept       { artifact: Artifact,             delta_dl: f64 },

    // Reinforcement pass events
    StatusAdvance(StatusAdvance),                               // delta_dl inside
    Merge        { kept: TermRef, merged: TermRef, delta_dl: f64 },
    Subsumption  { absorbed: TermRef, subsumer: TermRef, delta_dl: f64 },
    Canonicalize { artifact: TermRef, rewritten: RewriteRule, delta_dl: f64 },

    // Promotion pass events
    Promote      { signal: PromotionSignal,        delta_dl: f64 },
    Migrate      { report: MigrationReport,        delta_dl: f64 },
    Demote       { artifact: TermRef, reason: DemotionReason, delta_dl: f64 },

    // Regime transitions (meta-events)
    RegimeTransition { from: Regime, to: Regime, epoch_id: u64 },
}

impl Event {
    pub fn delta_dl(&self) -> f64 { /* match on variant */ }
    pub fn category(&self) -> EventCategory { /* Discovery | Reinforce | Promote | Meta */ }
}
```

`EpochTrace` is, to a first approximation, `Vec<Event>` plus an
`EpochAction` tag and a `Regime`. The aggregate score `V(epoch)` is
the sum of `event.delta_dl()` over the trace. The regime detector
reads the trace. The reward estimator updates from the trace. The
allocator chooses the next action from the estimator. Nothing outside
the event stream is load-bearing.

## The five architectural objects

Only five. Everything else is policy that parameterizes them.

```
┌─ Registry ─────────── Merkle DAG of Artifacts + MigrationReports + PromotionSignals
│     ▲
│     │ insert / query
│     │
├─ Event pipeline ───── reinforce() + discover() + promote() + migrate()
│     ▲   │
│     │   │ emits events
│     │   ▼
├─ RewardEstimator ─── EWMA over event ΔDLs per category
│     ▲
│     │
├─ Allocator ────────── given estimator + plateau detection → EpochAction
│     ▲
│     │
└─ Epoch ──────────────  composes the above; `step(corpus) -> EpochTrace`
```

| Object           | Responsibility                                                         | File                                    |
|------------------|------------------------------------------------------------------------|-----------------------------------------|
| `Registry`       | append-only content-addressed store; derivation DAG                     | `mathscape-core::epoch::Registry`       |
| `EventPipeline`  | the three passes (reinforce / discover / promote)                       | `mathscape-core::pipeline`              |
| `RewardEstimator`| maintains `reinforce_mean`, `discover_mean`, plateau indicator          | `mathscape-core::control::Estimator`    |
| `Allocator`      | emits `EpochAction` from estimator state                                | `mathscape-core::control::Allocator`    |
| `Epoch`          | `step(corpus) -> EpochTrace` = allocate → pipeline → trace → update    | `mathscape-core::epoch::Epoch`          |

Policy parameterizes the Pipeline: `Verifier` (status advances),
`Generator` (discovery proposals), `PromotionGate` (gates 4–5),
`BridgeToAxiomForge` (gates 6–7).

## The unified set of forces (five)

| Force                         | Measured by                                          | Role                                   |
|-------------------------------|------------------------------------------------------|----------------------------------------|
| **Compression pressure**      | corpus ΔDL                                           | drives discovery                        |
| **Coverage preservation**     | `coverage_delta`                                     | disqualifies fake compressions          |
| **Irreducibility**            | e-graph saturation vs library                        | discovery gate 3                        |
| **Cross-corpus applicability**| `cross_corpus_support` tally                         | promotion gate 5                        |
| **Axiomatization pressure**   | status-advance rate per epoch                        | drives reinforcement                    |

The first three act on *proposals*. The fourth acts on *library
residents* over time. The fifth acts on *every rule every epoch*.
Each corresponds to one or more gates.

## The unified gate lattice

Ten gates, organized into three classes that map cleanly to the pass
they belong in.

### Discovery gates (local, cheap, run per proposal)

| # | Gate                 | Pass      | Enforces                                            |
|---|----------------------|-----------|-----------------------------------------------------|
| 1 | compression floor ε  | Discover  | ΔDL ≥ ε                                             |
| 2 | coverage delta ≥ 0   | Discover  | no lost matches                                      |
| 3 | irreducibility        | Discover  | e-graph separates lhs from any existing rhs          |

### Reinforcement advance gates (structural, run per resident rule)

| # | Gate                 | Pass       | Status transition                                    |
|---|----------------------|------------|------------------------------------------------------|
| V | equivalence verified | Reinforce  | Conjectured → Verified                               |
| X | proof exported       | Reinforce  | Verified → Exported                                  |
| A | canonicality stable  | Reinforce  | Exported → Axiomatized                               |

### Promotion gates (cross-corpus, cross-crate)

| # | Gate                 | Pass      | Enforces                                             |
|---|----------------------|-----------|------------------------------------------------------|
| 4 | condensation K        | Promote   | subsumes ≥ K library entries                         |
| 5 | cross-corpus N        | Promote   | appears in ≥ N distinct corpora                      |
| 6 | axiom-forge obligations | Promote | 7 structural proofs in axiom-forge                   |
| 7 | rustc typecheck       | Promote   | generated Rust compiles                              |

Every rule's lifecycle is a monotone walk up this lattice. Demotion
is the only way down.

## The unified regime names (canonical)

The three regimes have exactly these names everywhere in the codebase
and docs. Previous mentions of "Exploration / Consolidation / Promotion"
have been replaced with the canonical names from `reward-calculus.md`
because they map directly to *which event category dominates ΔDL*.

| Regime        | Dominant event category   | What the epoch spends compute on                    |
|---------------|---------------------------|------------------------------------------------------|
| **Reductive** | Reinforce                 | status advances, merges, subsumptions, canonicalize  |
| **Explosive** | Discovery                 | new proposals, acceptance gates                      |
| **Promotive** | Promote                   | promotion signals, axiom-forge roundtrips, migration |

Regime = `argmax` over categories of ΔDL sum in the last W epochs.

## The canonical status lattice (type-state)

```
        Proposed                                    (just entered the system)
           │  gates 1–3
           ▼
      Conjectured ──────────── fails advance for W epochs ──► Demoted(stale)
           │  gate V
           ▼
       Verified ───────────── subsumed by another rule ─────► Subsumed(hash)
           │  gate X
           ▼
       Exported ─────────── usage drops below M for W ──────► Demoted(unused)
           │  gate A
           ▼
      Axiomatized
           │  gates 4–5
           ▼
       Promoted
           │  gates 6–7
           ▼
     Primitive ─────────── usage across corpora falls ──────► Demoted(retired)
```

Only Proposed → Conjectured → Verified → Exported → Axiomatized →
Promoted → Primitive is forward motion. Subsumed and Demoted are
terminal (but the underlying rule can be re-proposed later, starting
again at Proposed).

## Type-level proofs (the typescape)

The invariants are enforced by types, not by asserts or runtime checks.

### Invariant 1: Every Artifact's `content_hash` matches its fields

Enforced by private constructor + sealed trait:

```rust
pub struct Artifact { /* all fields pub(crate) */ }

impl Artifact {
    /// The only constructor. Computes the hash from inputs.
    pub fn seal(
        rule: RewriteRule,
        epoch_id: u64,
        cert: AcceptanceCertificate,
        parent_hashes: Vec<TermRef>,
    ) -> Self {
        let content_hash = Self::canonical_hash(&rule, epoch_id, &cert);
        Self { rule, epoch_id, certificate: cert, parent_hashes, content_hash }
    }
}
```

No way to construct an Artifact with a mismatched hash. Serde
deserializers round-trip through `seal`.

### Invariant 2: Status transitions are monotone

Enforced by type-state where affordable, enum + exhaustive match
otherwise:

```rust
// Type-state at the statically-typed API surface:
pub struct Conjectured(Artifact);
pub struct Verified   (Artifact, EgraphEvidence);
pub struct Exported   (Artifact, EgraphEvidence, LeanProof);
pub struct Axiomatized(Artifact, EgraphEvidence, LeanProof, CanonicalityWindow);

impl Conjectured {
    pub fn verify(self, ev: EgraphEvidence) -> Result<Verified, Self> { ... }
}
impl Verified {
    pub fn export(self, lp: LeanProof) -> Result<Exported, Self> { ... }
}
// ... each step consumes the previous state
```

Storage uses a serde-friendly enum `ProofStatus`; the typed wrappers
are the API that performs transitions. Cross-check in `Registry::insert`
that `ProofStatus` only advances.

### Invariant 3: MigrationReport requires a successful Promotion

Enforced by constructor signature:

```rust
pub struct MigrationReport { /* pub(crate) */ }

impl MigrationReport {
    /// Consumes a `PromotedArtifact` — which is itself only
    /// constructible from a successful axiom-forge roundtrip.
    pub fn from_promotion(
        promoted: PromotedArtifact,
        rewritten: Vec<TermRef>,
        deduplicated: Vec<TermRef>,
    ) -> Self { ... }
}

pub struct PromotedArtifact(Artifact, AxiomIdentity, RustcAccepted);
pub struct RustcAccepted { _private: () }  // constructed only by the bridge
```

You cannot type-check a MigrationReport without passing through the
full promotion path.

### Invariant 4: Every Event carries a ΔDL

Enforced by the type itself — the `delta_dl` field is on every variant
and `Event::delta_dl()` is total.

### Invariant 5: EpochTrace events match the declared EpochAction

Enforced by a run-time assertion at trace finalization:

```rust
impl EpochTrace {
    pub(crate) fn finalize(self) -> Self {
        for ev in &self.events {
            assert!(ev.category().matches(self.action),
                "event category {:?} does not match epoch action {:?}",
                ev.category(), self.action);
        }
        self
    }
}
```

Stronger (type-level) version via phantom-tagged events is possible but
costs ergonomics. This assertion is cheap and sufficient for v0.

### Invariant 6: Registry is append-only

Enforced by trait:

```rust
pub trait Registry {
    fn insert(&mut self, artifact: Artifact);
    fn supersede(&mut self, old: TermRef, new: Artifact); // only on Subsumption event
    fn all(&self) -> &[Artifact];
    // NO `remove`. Subsumption marks the old as Subsumed(new), doesn't delete.
}
```

Demotion changes status; never removes an entry.

## The machine in its best form

### Layer 1 — core kernel (`mathscape-core`)

Depends only on `blake3`, `serde`, `bincode`.

- `term.rs` — existing `Term` + `TermRef`
- `eval.rs` — existing `RewriteRule` + pattern matching
- `epoch.rs` — `Artifact`, `Candidate`, `Event`, `EpochTrace`,
  `EpochAction`, `Registry` trait + `InMemoryRegistry`
- `control.rs` — `Allocator`, `RewardEstimator`, `Regime`,
  `RealizationPolicy`
- `lifecycle.rs` — `ProofStatus` enum, type-state wrappers
  `Conjectured`/`Verified`/etc.
- `pipeline.rs` — `Pipeline::discover`, `Pipeline::reinforce`,
  `Pipeline::promote` (generic over trait impls)

### Layer 2 — role impls (one adapter per existing crate)

- `mathscape-evolve::CorpusSource` — Population → corpus vector
- `mathscape-compress::CompressionGenerator` → `Generator`
- `mathscape-reward::StatisticalProver` → per-proposal verdict
- `mathscape-proof::EgraphVerifier` → status advance Conjectured→Verified
- `mathscape-proof::LeanExporter` → status advance Verified→Exported
- `mathscape-store::StoreRegistry` → persistent `Registry`

### Layer 3 — bridges

- `mathscape-axiom-bridge` (new) — Promotion gates 6–7, invokes axiom-forge

### Layer 4 — runners

- `mathscape-cli` — REPL + `run N` thin wrappers over `Epoch::step`
- `mathscape-service` — long-running loop calling `Epoch::step` every
  tick, persisting to `StoreRegistry`
- `mathscape-mcp` — read-only query surface over the trace stream

### Dependency rule

Later layers depend on earlier layers. No upward dependencies. The
kernel (`mathscape-core`) has no knowledge of axiom-forge or Population.

## Why this is the best form

1. **Events are the single primitive.** Adding a new event kind
   (e.g., a new reinforcement rule) requires adding one enum variant
   and one ΔDL formula — nothing else. Every downstream piece
   (estimator, regime detector, trace serialization) cascades
   automatically.

2. **Policy is fully separated from mechanism.** `RealizationPolicy`
   holds every tunable number; `Pipeline` is generic over
   `Verifier`/`Generator`/`PromotionGate`. Two policies produce two
   different trajectories with no code changes. The system is a *pure
   function* from policy + corpus to registry.

3. **The type system enforces the gate lattice.** You cannot skip a
   gate or advance in the wrong order — the type-state wrappers
   forbid it. The seven-gate process is literally unrepresentable in
   the wrong order at the API level.

4. **ΔDL is the single metric.** No ad-hoc reward shapes per pass.
   Every event contributes a number in the same currency. Allocator
   choice is a greedy selection over a scalar.

5. **Reinforcement is the default loop.** Discovery fires only when
   plateau. Promotion fires only when gates 4–5 clear. The cheap work
   dominates; the expensive work is episodic and justified.

6. **Registry is the source of truth.** The machine is
   content-addressed end-to-end. Replay is `for each event in
   event_log, apply(registry)`. Determinism is provable by hash
   equality.

## Proof obligations we owe ourselves

After implementing per `realization-plan.md`:

| Property                                                  | Where proven                     |
|-----------------------------------------------------------|----------------------------------|
| Content hash of every Artifact matches its fields          | constructor + property test      |
| ProofStatus only advances forward (or to terminal)          | type-state + property test       |
| ΔDL is non-negative for every accepted event               | invariant in StatisticalProver   |
| `V(epoch) = Σ event.delta_dl()`                             | unit test                        |
| Replay produces the same registry bytes                     | integration test                 |
| Different policy on same corpus produces different registry | integration test (shape only)    |
| No Discovery event can appear under `Action::Reinforce`     | `EpochTrace::finalize` assertion |
| Every MigrationReport has a causally-linked PromotionSignal | `from_promotion` constructor     |

These become property tests (proptest) once Phase B of the plan lands.

## How to build it in the cleanest order

See `realization-plan.md`. Shortest summary:

- Phase A: theory (this commit)
- Phase B: cascade types into `mathscape-core` — `Event`, `Artifact`
  sealing, `ProofStatus` lifecycle, `Allocator`, `RewardEstimator`,
  `RealizationPolicy`
- Phase C: role impls in existing crates (thin adapters)
- Phase D: refactor `run_epoch` to `Epoch::step`
- Phase E: `RegimeDetector` (level-4 ladder)
- Phase F: adaptive policy (level-5 ladder)
- Phase G: promotion gates 4 + 5
- Phase H: `mathscape-axiom-bridge` (gates 6 + 7)
- Phase I: migration + coverage validation
- Phase J: demotion
- Phase K: multi-corpus support
- Phase L: climb the ladder only if required

At the end of Phase I, one continuous run produces a Rust primitive via
all ten gates, the library contracts, and the trajectory is
hash-stable. That's success.
