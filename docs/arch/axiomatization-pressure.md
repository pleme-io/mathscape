# Axiomatization Pressure — No Rule Rests

Reinforcement is the **default**. Discovery is the exception.

Each epoch begins by pushing every existing library entry toward its
most axiomatic state — advancing proof status, merging duplicates,
subsuming redundant rules, demoting stale ones. Discovery (proposing
new rules from the corpus) fires *only when* reinforcement plateaus —
when no status advances, no merges, no subsumptions, and no demotions
have occurred for W epochs.

**Default cadence**:

```
reduce ─── reduce ─── reduce ─── reduce ─── reduce ─── reduce
                                                       │
                                          reinforcement plateau detected
                                                       │
                                                       ▼
                                           ⚡ EXPLORE (discovery burst)
                                                       │
                                                       ▼
                                                    reduce ─── reduce ─── …
```

This mirrors how mathematicians actually work: most time is spent
cleaning up, simplifying, and canonicalizing existing results. New
axioms are rare and episodic. The system models that.

It is also computationally aligned: reinforcement is cheap (algebraic
operations over the derivation DAG), discovery is expensive (corpus
search + anti-unification + novelty scoring). Running discovery only
when it is justified keeps the compute budget on the highest-marginal-
return work.

## The two pass types

1. **Reinforcement** (always runs) — every existing library entry is
   pushed toward its most axiomatic state: status advances, merges,
   subsumptions, demotions. Zero parameters; deterministic.
2. **Discovery** (fires only on reinforcement plateau) — new candidates
   from the corpus run gates 1–3 (compression, coverage, irreducibility).
   Cost-dominant; only justified when reduction is done.

Without reinforcement, the system accumulates Conjectured rules forever.
With it, every rule either advances its proof status, gets merged with
another rule, gets subsumed by a newer more-general rule, or gets
demoted — before the system ever considers looking for new rules.

## The fifth force

Beyond the four forces in `forced-realization.md`
(compression / coverage / irreducibility / cross-corpus), there is a
fifth:

| Force                         | What it measures                                                  |
|-------------------------------|-------------------------------------------------------------------|
| **Axiomatization pressure**   | every rule monotonically improves its proof status per epoch, or decays |

Where the first four forces act on *proposals*, this one acts on
*residents* of the library. A rule that sits at `Conjectured` for W
epochs without advancing is evidence it was noise.

## The full status lifecycle

The `ProofStatus` enum expands from the original three states
(`Conjectured | Verified | Exported`) to a richer lifecycle that
captures both axiomatization progress and subsumption paths.

```
Proposed
   │  passes gates 1–3 (local)
   ▼
Conjectured ──── fails reinforcement for W epochs ────► Demoted
   │  e-graph equivalence proves lhs ≡ rhs                  │
   ▼                                                         │
Verified ─────── another rule subsumes this one ──────► Subsumed
   │  proof emits to Lean 4 and checks                       │
   ▼                                                         ▼
Exported ─────── stable in canonical form for W epochs ───► Axiomatized
                                                             │  passes gates 4–5
                                                             ▼
                                                          Promoted
                                                             │  axiom-forge accepts + rustc compiles
                                                             ▼
                                                          Primitive
                                                             │  usage falls below floor M for W epochs
                                                             ▼
                                                          Demoted
```

Rules flow down this lattice, not up. The only "upward" move is
**rediscovery** — a subsumed or demoted rule can be re-proposed in a
later epoch, starts again at Proposed, and must re-prove its way.

## The reinforcement pass

Each epoch, before Discovery, the Epoch runs:

```rust
impl<G, P, E, R> Epoch<G, P, E, R> where ... {
    pub fn step(&mut self, corpus: &[Corpus]) -> EpochTrace {
        let reinforcement = self.reinforce();        // pass 1
        let discovery = self.discover(corpus);       // pass 2
        // emit a combined trace
    }

    fn reinforce(&mut self) -> ReinforcementTrace {
        // 1. try to advance each entry's status by one step
        //    Conjectured → Verified  : e-graph equivalence
        //    Verified    → Exported  : Lean 4 proof emission
        //    Exported    → Axiomatized : W-epoch canonicality check
        //    Axiomatized → Promoted  : evaluate PromotionGate
        // 2. for each pair (a, b), check if b subsumes a (a ⇒ b is provable)
        //    if yes, mark a as Subsumed(b.hash)
        // 3. for each entry, check if W epochs have elapsed without a
        //    status advance → mark for Demotion review
    }
}
```

The reinforcement pass is **idempotent under fixed policy**: running
it twice in the same epoch state produces the same result. This is
load-bearing for replayability.

## The new advance gates

Alongside the seven realization gates, there are three advance gates
that push rules *horizontally* along the lifecycle:

| # | Gate                 | Triggers                                        | Output status |
|---|----------------------|-------------------------------------------------|---------------|
| V | Verification         | e-graph saturation → lhs and rhs in same class  | Verified      |
| X | Export               | Lean 4 proof emitted and accepted               | Exported      |
| A | Axiomatization       | canonical form stable across W epochs           | Axiomatized   |

These are cheaper than the realization gates (no cross-corpus tally,
no axiom-forge roundtrip) and are evaluated on every entry every
epoch. They run in parallel with the realization gates.

## Canonical form and rule merging

Axiomatization requires a *canonical form* — a deterministic choice
among equivalent rule presentations. Two rules that saturate to the
same e-graph class should merge into one. The merge is:

```rust
fn merge(a: Artifact, b: Artifact) -> Artifact {
    // deterministic pick: lower content_hash wins as canonical
    let (kept, merged) = if a.content_hash < b.content_hash { (a, b) } else { (b, a) };
    let out = Artifact {
        parent_hashes: union(kept.parent_hashes, vec![merged.content_hash]),
        certificate: strengthen(kept.certificate, merged.certificate),
        ..kept
    };
    // emit a MergeReport as a new Artifact so the DAG records the event
    out
}
```

`strengthen` takes the max of each axis (score, compression_ratio,
novelty, etc.) — the merged rule inherits the strongest evidence from
both parents.

## Demotion from the library (vs. from primitive status)

Two distinct demotion events:

| Event                  | From state | To state  | Trigger                           |
|------------------------|------------|-----------|-----------------------------------|
| Library demotion       | any non-Promoted | Demoted   | W epochs without status advance; or usage falls below M |
| Primitive demotion     | Primitive   | Demoted   | usage across all corpora falls below M for W epochs     |

Both use the same `Demoted` sink status. Both require operator
approval in v0 (see `promotion-pipeline.md`). Both are rare.

## Why the reinforcement pass is per-epoch, not per-action

It could be tempting to advance status opportunistically — every
time a rule's coverage is measured, try to prove its equivalence.
Two reasons not to:

1. **Cost isolation**: e-graph saturation is expensive. Running it
   once per epoch per rule bounds the cost per epoch;
   interleaving it with discovery makes cost analysis hard.
2. **Replay determinism**: the reinforcement pass runs deterministically
   on the post-discovery library state. Running it at arbitrary times
   couples results to scheduling, breaks replay.

So: discovery first, then reinforcement, then emit trace. One atomic
epoch boundary per step.

## Shape of the reinforcement trace

```rust
pub struct ReinforcementTrace {
    pub epoch_id: u64,
    pub advances: Vec<StatusAdvance>,     // Conjectured→Verified etc.
    pub merges: Vec<(TermRef, TermRef)>,  // (kept, merged)
    pub subsumed: Vec<(TermRef, TermRef)>,// (old, subsumer)
    pub demoted: Vec<TermRef>,            // demotion candidates
    pub stable: usize,                    // entries with unchanged status
}
```

Included in `EpochTrace` alongside discovery stats:

```rust
pub struct EpochTrace {
    pub epoch_id: u64,
    pub discovery: DiscoveryTrace,          // renamed from the old fields
    pub reinforcement: ReinforcementTrace,
    pub regime: Regime,
    pub promotion_signals: Vec<PromotionSignal>,
}
```

## Relation to the minimal-model ladder

Reinforcement sits at **levels 1–3** of the ladder:

- **Level 1** (e-graph equivalence): no parameters; pure structural
- **Level 2** (canonical form detection): no parameters; deterministic
  choice function over equivalence-class representatives
- **Level 3** (merge-strengthening): no parameters; inherits
  certificate fields by max/min

Reinforcement does not need neural nets, nor ever will. It is an
algebraic process over the derivation DAG. This is a feature — the
system's self-verification layer has zero trained parameters, so its
soundness does not depend on training data.

## Consequences for the code

New types in `mathscape-core`:

```rust
pub enum ProofStatus {
    Proposed,
    Conjectured,
    Verified,
    Exported,
    Axiomatized,
    Subsumed(TermRef),     // points to the subsumer
    Promoted,
    Primitive(AxiomIdentity),
    Demoted(String),       // reason
}

pub struct StatusAdvance {
    pub artifact: TermRef,
    pub from: ProofStatus,
    pub to: ProofStatus,
    pub evidence_hash: TermRef,   // e-graph saturation id, Lean proof hash, etc.
}

pub struct ReinforcementTrace { ... }  // as above

pub trait Verifier {
    fn advance(&self, artifact: &Artifact, library: &[Artifact])
        -> Option<StatusAdvance>;
}
```

New trait impls:

- `EgraphVerifier` in `mathscape-proof` — handles Conjectured → Verified
- `LeanExporter` in `mathscape-proof` — handles Verified → Exported
- `CanonicalityChecker` in `mathscape-core::realization` —
  handles Exported → Axiomatized

## Every epoch is a pressure cycle

Under axiomatization pressure, every rule is *under load* every epoch.
A rule that survives is a rule that demonstrably advanced, remained
axiomatized, or kept its usage floor. A rule that does none of these
for W epochs is noise — the pressure reveals it and the pipeline
removes it.

This is how the library converges. Not by refusing to add things, but
by ensuring every addition is continuously justified. Equilibrium is
not stasis — it is steady-state reinforcement.
