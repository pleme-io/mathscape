# ML Apparatus on the BootstrapCycle Platform

A full machine-learning apparatus built on the R26 `BootstrapCycle`
typescape entity. The core idea: because the cycle exposes three
**hijackable layers** (corpus generation, law extraction, model
update) AND carries **BLAKE3 attestation** at every boundary,
each layer can be independently optimized at runtime while the
outer cycle remains a stable, typed, reproducible object.

This is the architecture that lets the model hack the model that
is running it all.

## The apparatus, at a glance

```text
    ┌─────────────────────── Orchestrator ────────────────────────────┐
    │                                                                  │
    │   observes attestation deltas, reroutes layer implementations,   │
    │   rolls back on regression, promotes proven swaps                │
    │                                                                  │
    │   ┌──────────── BootstrapCycle<C, E, M> ─────────────────────┐   │
    │   │                                                           │   │
    │   │   C: CorpusGenerator   ←── hijackable                      │   │
    │   │   E: LawExtractor      ←── hijackable                      │   │
    │   │   M: ModelUpdater      ←── hijackable                      │   │
    │   │   D: LibraryDeduper    ←── hijackable (R28)                │   │
    │   │                                                           │   │
    │   │   attestation: BLAKE3(library, policy, trajectory)        │   │
    │   │                                                           │   │
    │   └───────────────────────────────────────────────────────────┘   │
    │                                                                  │
    │   output: BootstrapOutcome with iteration snapshots + attestation │
    │                                                                  │
    └──────────────────────────────────────────────────────────────────┘
```

The **orchestrator** sits above the cycle. It can:
- Run a cycle with a chosen `(C, E, M)` triple.
- Read the attestation, compare against history.
- Swap one or more layers for alternative implementations.
- Re-run and compare outcomes.
- Keep the swap if attestation improved; revert if it regressed.

Over time, the orchestrator converges on the best layer
implementations for the problem class it's working on.

## The layers and their optimization seams

### Layer 4 (R28): LibraryDeduper

**Default behavior.** `NoDedup` — every candidate is accepted.
Backward-compatible; `BootstrapCycle::run` uses this.

Two stronger defaults are shipped:

- `CanonicalDeduper` — catches exact duplicates after
  canonicalization (R3/R4/R6 fold). Two rules with identical
  (canonical LHS, canonical RHS) are duplicates.
- `AlphaDeduper` — also canonicalizes pattern-variable ids via
  `anonymize_rule`. Catches alpha-renamed duplicates that
  `CanonicalDeduper` misses.

**Empirical proof the layer works.** Same 10-iteration deep
bootstrap:

| Deduper            | Final library | Saturation |
|--------------------|---------------|------------|
| `NoDedup`          | 30 rules      | never      |
| `CanonicalDeduper` | 4 rules       | step 3     |

The linear-growth pathology (R27's deep exploration finding) is
closed. The cycle now **converges to a structurally-distinct
core**.

**Optimization axes.**

- *Faster dedup*: index the library by canonical hash; dedup
  becomes O(1) per candidate instead of O(library_size).
- *Proper-subsumption*: use `eval::proper_subsumes` to reject
  candidates whose (LHS, RHS) is strictly generalized by an
  existing rule — stronger than equality.
- *E-graph saturation*: union-find over candidate + library to
  detect semantic equivalence beyond syntactic shape.
- *Empirical equivalence*: run the candidate and an existing
  rule on K random instances; if they agree on all of them, treat
  as duplicate.

**Hijack surface.** Any `impl LibraryDeduper for MyType`.

### Layer 1: CorpusGenerator

**Default behavior.** `DefaultCorpusGenerator` produces tensor-
identity corpora at iteration 0, nested compositions on later
iterations. Mirrors R25's hand-crafted strategy.

**Optimization axes.**

- *Memoization by (iter, library_hash)*: if the library hasn't
  changed structurally, corpus selection is stable. Skip
  regeneration.
- *Adaptive selection*: pick corpora that target gaps in the
  current library's coverage (identified via R12 primitive
  census).
- *Curriculum learning*: iteration-index-scheduled corpus
  difficulty. Start simple, escalate.
- *Domain-specific corpora*: domain modules (tensor, modular
  arithmetic, combinatorial) each implement CorpusGenerator. The
  orchestrator picks based on the library's current focus.

**Hijack surface.** Any `impl CorpusGenerator for MyType`. Swap
via `BootstrapCycle::new(MyType, ...)`.

### Layer 2: LawExtractor

**Default behavior.** `DerivedLawsExtractor` wraps R24's
`derive_laws_from_corpus` — paired anti-unification of
evaluation traces.

**Optimization axes.**

- *Trace caching*: if corpus is unchanged AND library is
  unchanged, reuse prior trace set. Only re-derive when inputs
  change.
- *Incremental AU*: pair only NEW traces with existing ones;
  avoid recomputing pairs that appeared last iteration.
- *Subterm AU (R21)*: enable when the default root-AU saturates.
  More candidates at higher cost.
- *E-graph saturation (Phase K)*: use equational reasoning to
  recognize laws beyond syntactic shape.
- *Neural candidate generator*: a trained model proposes
  candidate `(lhs, rhs)` patterns; the extractor verifies.

**Hijack surface.** Any `impl LawExtractor for MyType`.

### Layer 3: ModelUpdater

**Default behavior.** `DefaultModelUpdater` calls
`train_from_trajectory` with lr=0.05 on R10's `LinearPolicy`.

**Optimization axes.**

- *Learning rate schedule*: decay over iterations, warm-up, or
  adaptive (learning rate chosen by the orchestrator based on
  gradient norm).
- *Batch training*: accumulate trajectories across cycles before
  updating, for smoother gradients.
- *Nonlinear policies*: MLP or transformer-backed; the `PolicyModel`
  trait from R10 is the abstraction seam.
- *Actor-critic*: value estimate for each library state, policy
  gradient from advantage.
- *Meta-learning*: the updater itself is parameterized and
  learned (MAML-like).

**Hijack surface.** Any `impl ModelUpdater for MyType`.

## The orchestrator — the outermost model

The orchestrator is the piece not yet built. Its job is:

1. Maintain a history of `(layer_triple, attestation, outcome_metrics)`.
2. Given current state, choose the next `(C, E, M)` to try.
3. Run the cycle, observe the new attestation + metrics.
4. Compare: did we improve (more laws discovered, higher tensor
   density, faster convergence)?
5. Keep or revert based on comparison.
6. Over time, the orchestrator's choice policy converges to the
   best layer combinations.

**The orchestrator is a `LinearPolicy` (or richer model) trained
on the attestation history.** Its input is summary metrics from
the last N cycles; its output is a distribution over layer-triple
choices for the next cycle.

**Self-hacking.** Because the orchestrator is itself a model, and
the platform allows its own model to be swapped, the orchestrator
can be replaced too. This is where "the model hacks the model
running it" becomes literal:

- Gen 1 orchestrator: hand-coded greedy (always pick most-novel
  layer triple that hasn't regressed in N cycles).
- Gen 2 orchestrator: learned from Gen 1's history.
- Gen 3: learned from Gen 2's history.
- ...

Each generation's model is trained on the prior generation's
attested trajectory. The system bootstraps its own meta-level.

## Runtime optimization via layer boundaries

Because every layer is:

- A trait (interface, not implementation)
- Serializable (outcomes are bincode-roundtrippable; R10.1
  policy_to_sexp for Lisp-level inspection)
- Attested (BLAKE3 covers the whole)

...three runtime tricks become possible:

### Trick 1: Hot-swap mid-run

At any iteration boundary, the orchestrator can swap a layer
implementation. The current policy + library + trajectory carry
through because they're plain data — the swap affects only what
happens next iteration.

*Use case:* swap to a more expensive LawExtractor when the
cheaper one stops finding new laws.

### Trick 2: Roll back on regression

Every outcome carries an attestation. If a new layer triple
regresses performance, the orchestrator reverts to the last
known-good triple (identifiable by its attestation). No state
corruption — the rejected outcome is simply discarded.

*Use case:* a neural LawExtractor that hallucinates invalid
rules gets rolled back the moment attestation shows declining
library quality.

### Trick 3: Parallel layer exploration

The typescape entity is Send + Sync (derived via field types).
Run N cycles in parallel with N different layer triples, compare
attestations, keep the best. The orchestrator owns the
exploration budget; each cycle is independent.

*Use case:* A/B testing LawExtractor variants across multiple
problem domains simultaneously.

## Efficiency story

Because each layer is cleanly encapsulated:

1. **Caching.** A layer's output depends only on its inputs. Cache
   by input hash. If the orchestrator is considering swap-and-
   compare, cached layers mean only the changed piece re-runs.

2. **Skip.** If attestation tells us the cycle outcome hasn't
   changed, don't re-train, don't re-score, don't re-persist.

3. **Parallelize.** Layers within a cycle are sequential, but
   multiple cycles are independent. Exploit cores.

4. **Specialize.** Each domain (tensor, modular, combinatorial)
   can ship its own optimized LawExtractor tuned for that domain's
   rule shapes.

5. **Incremental.** When only the trajectory grows between
   cycles (no library change), the ModelUpdater can train on the
   new steps alone rather than re-processing the full trajectory.

Compare to a monolithic ML system where optimizations have to
thread through everything: the layered cycle isolates the work.

## Invariants the platform preserves

Regardless of which layers are hijacked or how aggressively the
orchestrator optimizes, the following hold:

1. **Determinism.** Same layer triple + same seeds → identical
   attestation. The platform is a pure function of its inputs;
   only the orchestrator introduces state (deliberately, for
   learning).

2. **Attestation coherence.** The cycle-level hash covers
   everything. Tampering with library, policy, or trajectory
   invalidates it.

3. **Type safety.** All swaps are typechecked at compile time
   (generics) or at serde deserialization time (for Lisp-loaded
   variants). No runtime class-errors.

4. **Rollback safety.** Reverting to a prior attestation is just
   restoring a bincode payload. No side effects to undo.

5. **Serialization.** Every piece of cycle state goes to and from
   bincode. For Lisp-resident orchestration, R10.1's
   policy_to_sexp pattern extends to bootstrap outcomes.

## The big picture

Before R26, mathscape was a discovery engine with discoveries
hard-wired into tests. After R26, mathscape is a platform where
discovery is a reusable, swappable, attestable entity.

With that entity in hand, efficiency flows from standard systems
engineering: cache, skip, parallelize, specialize. The
orchestrator — a model that runs the apparatus — becomes the
thing that learns how to operate the apparatus efficiently.

And because the orchestrator is ALSO a model on this platform,
it can be the subject of the same cycle: the platform produces
a better orchestrator over time, without human intervention.

That's the apparatus.
