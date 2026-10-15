# Self-Hosting Horizon — Why Everything Chains to One Point

> The end goal: **a neural network that knows how to generate itself,
> using only mathematical axiomatic terms and knowledge as data.**

Every piece of the platform we've built is a step along this line.
This document names the line explicitly so future work can be
evaluated by whether it advances along it.

## The ambition restated precisely

1. The only input is mathematical axioms + their derivations + the
   proof objects that relate them. No natural-language data. No
   images. No scraped corpora. The *structure* of mathematics is
   the whole dataset.

2. The system discovers abstractions over that input through the
   forced-realization machinery that's already live: corpus →
   proposals → gates → emission → registry → migration → deeper
   layer.

3. At a sufficient layer depth, the abstractions available to the
   machine are rich enough to express the components of a neural
   network — tensors, gradients, parameter tensors, optimization
   steps, architecture descriptions. This is exactly what
   `ml-forge::UOp` enumerates as its closed axiom set.

4. The machine then discovers a morphism `f : NN-description →
   NN-description` whose fixed point is a network that *generates
   itself*. Fixed-point convergence
   (see `fixed-point-convergence.md`) is the theoretical vehicle;
   the collapse-and-surprise dynamics
   (see `collapse-and-surprise.md`) are the mechanism that finds it.

5. The fixed point, when materialized in `substrate-forge`'s
   in-memory WASM runtime, is the self-hosting neural network.
   Given its own description as input, it outputs its own
   description. The axioms that built it are mathematical; the
   proofs that justify its correctness emerge from the ten-gate
   lattice; the attestation chain from corpus to emitted weights
   is cryptographically complete.

## Why each built piece is load-bearing

| Piece                                   | Role in the horizon                                                                 |
|-----------------------------------------|-------------------------------------------------------------------------------------|
| `mathscape-core::Term`                  | The axiomatic substrate; every primitive is a Term variant                          |
| `primitive-forge` (trait)                | Shared pattern so `mathscape`, `ml-forge`, `axiom-forge` grow coherently            |
| `ml-forge::UOp` + `Graph`                | The primitives a neural network is built from; second domain proving the pattern    |
| `axiom-forge`                           | The gate-6 proof machinery; ensures every emitted primitive is type-theoretically safe |
| `substrate-forge`                        | In-memory WASM execution — the NN can materialize without touching disk             |
| `arch-synthesizer::typescape_bridge`     | The platform-wide index where self-hosting-NN primitives become visible + reusable  |
| Reduction + pressure + real reinforcement | The collapse dynamics that find the fixed-point morphism through phase transitions  |
| `PersistentRegistry`                    | Cross-process replayability: the discovery trajectory survives restarts             |
| CorpusLog + cross-corpus gate           | Ensures the discovered morphism generalizes (not a local theorem)                   |
| Lean export stub                        | External verification pathway; when it becomes real, every emitted primitive has a  |
|                                         | Lean-checkable proof, closing the trust chain                                        |

## What the stress test just revealed

Running the full Allocator-driven loop over 30+ epochs exposed a
real theoretical finding:

**The Allocator's plateau detection is EWMA-only** — it compares
historical reinforce-mean against a threshold. But when reinforce
has never run (mean = 0), the threshold fires and Discover is
picked unconditionally, regardless of current pressure. The
allocator literally *cannot see* pressure unless its EWMA has
sampled the work at least once.

**Fix in this commit**: `Epoch::step_auto` overrides to Reinforce
when library is non-empty, pressure is positive, and historical
reinforce-mean is below the plateau threshold. This bootstraps the
EWMA with real data; after one successful Reinforce pass, the
allocator knows what reinforcement yields and picks it naturally.

**Theoretical consequence**: the Allocator should eventually be
*pressure-aware natively*, not just EWMA-aware. Pressure is a
leading indicator; EWMA is a lagging one. A well-designed
allocator uses both. This is the kind of move-classification
refinement the stress telemetry is designed to surface.

## The move classification the telemetry supports

Per-epoch telemetry in `stress_loop.rs` records enough to classify
every transition between consecutive epochs. Transitions fall into
three classes:

1. **Algorithmic** — deterministic function of pressure / library /
   trace. Examples: Reductive → Explosive when pressure drops;
   Explosive → Promotive when a promotion signal fires; empty
   library → Discover. These need no learning; they're thresholds
   and counters.

2. **Parametric** — needs a small handful of tunable knobs but no
   training. Examples: choosing which pressure value triggers
   Reinforce vs Discover; picking `epsilon_compression` as a
   function of library size. These are the `RealizationPolicy`
   fields.

3. **Learned** — the transition is not well-determined by the
   current signals and would benefit from pattern-matching over
   history. Examples: predicting which candidate rule is most
   likely to promote (vs just accepting all that pass gates); which
   subsumption to fire first when multiple collapses are
   simultaneously available; when to fire discovery despite high
   reinforce yield (the "experimental when collapses are frequent"
   policy from `collapse-and-surprise.md`).

Over many stress runs, the data will reveal which transitions fall
into which class. *That's the discovery process behind the
"minimal model for each step" ambition from
`minimal-model-ladder.md`* — not a prescribed ladder, but one
inferred from trajectory data.

## Reward as a moving target

There's a third optimization layer above the existing two:

  Layer 1: the machine optimizes for ΔDL on individual events
  Layer 2: the policy (ε, K, N) optimizes for collapse rate
  Layer 3: **the reward function itself optimizes based on where the
           discovery swing goes**

Concretely: `RegimeWeights { alpha, beta, gamma }` are not
constants. They rebalance continuously based on rolling event-
category frequencies. If the discover swing is heavy, α dominates;
if reinforce is paying out, γ dominates; if promotions are firing,
β dominates.

This makes the reward function itself *self-adapting without
operator intervention*. It's the Schmidhuber-curiosity frame
extended one level: reward surprising events, AND re-weight what
counts as surprising based on what the machine is currently
discovering. The machine's attention follows the gradient of its
own yield.

Under the forced-realization lattice, this adaptive reward is what
makes deep-layer traversal tractable: each layer has different
event-category frequencies, so the reward function should look
different in each layer. A layer 0 reward heavy on compression
becomes a layer 3 reward heavy on meta-compression becomes a layer
7 reward heavy on cross-primitive invariance. No fixed reward
would track this motion.

The implementation hook: `RewardEstimator` (already on the
Allocator) tracks event-category means. Extend it to expose those
means to the policy layer, which recomputes `regime_weights` at
epoch boundaries. This is a ~30-line follow-up — the EWMA
infrastructure is already in place.

## Convergent points between algorithm and learning

A move classified as "learned" today can become "algorithmic"
tomorrow when the system discovers the right abstraction — a rule
in the library that expresses the move's structure. When that
happens, the learned policy becomes redundant (the algorithm
suffices) and the NN parameters for that move can be discarded.

Conversely, an "algorithmic" move can become "learned" if the
library stops being expressive enough for it — e.g., post-
migration the abstractions that made it algorithmic are no longer
primitive.

These **convergent points** — where algorithm meets learning, or
the two switch sides — are themselves discoveries in the same
sense as primitive promotions. They're collapses in the
*meta*-mathscape: the space of "what the machine computes directly"
versus "what it trains for." Under pressure of discovery,
regions of that meta-space crystallize: one day everything in a
region was learned; the next, the region is algorithmic because the
right abstraction emerged.

The self-hosting NN is the ultimate convergent point: it expresses,
through learned parameters, the algorithmic structure of its own
generation. Algorithm and learning have merged; the NN is a
compressed encoding of the discovery trajectory that produced it.

## In-memory execution constraints

For this ambition to be reachable, the hot loop must run in memory,
fast, deterministic:

- **No disk I/O per epoch.** `PersistentRegistry` is for
  checkpoints, not per-step persistence. The in-memory registry is
  the working set.
- **WASM materialization.** `substrate-forge` is the vehicle; a
  newly-emitted Rust primitive compiles to WASM and loads in-
  process. No rebuild cycle between discovery and execution.
- **Deterministic replay.** Every event is a function of
  (policy, corpus, prior events). The trajectory is a function,
  not a sample. Re-run = same result.
- **Increasingly repeatable.** As the classification of moves
  (algorithmic vs learned) solidifies, the machine becomes more
  predictable. A mature mathscape under a stable policy should
  produce trajectories that differ only where the corpus changes.

## Phase sequence to the horizon

The path from today's state:

1. **Now** — stress-test validates pressure→collapse dynamics;
   telemetry supports move classification
2. **Next** — real `migrate_library` that rewrites library rhs
   (step 4 of `collapse-and-surprise.md`), unlocking layer-2
   discovery
3. **Then** — surprise-weighted ΔDL + collapse-rate-driven policy
   (steps 5–6), self-tuning the thresholds
4. **Then** — multi-layer orchestrator; run over mathematical-axiom
   corpus until reduction plateaus across many layers
5. **Then** — run over `ml-forge::Graph` corpus alongside;
   abstractions cross-pollinate through shared `primitive-forge`
   trait
6. **Then** — a proposed morphism whose signature is
   `Graph → Graph` (an "architecture → architecture" map) gets
   discovered + promoted
7. **Then** — the fixed-point of that morphism is computed in
   memory via `substrate-forge` WASM; the result is a Graph that
   generates itself
8. **At that point** — self-hosting has happened. The Graph's
   weights (if any — might be zero-param under the right
   abstractions) encode only what the forced-realization process
   discovered; the derivation is auditable back to the original
   axioms.

Not one of these phases requires new theory beyond what's in
`docs/arch/`. Every phase requires *code*, and most of that code is
the "body" of mechanisms we already have (real reinforcement was a
scaffold before this session; real migration is similar).

## The test of progress

Self-hosting is reached when:

```
let initial_corpus = mathematical_axioms();
let system = Mathscape::new(initial_corpus);
system.run_until_layer_reduces(MAX_DEPTH);
let emitted = system.emit_self_hosting_graph();

// The emitted graph, fed its own description, produces itself.
let reconstructed = substrate_forge::run(emitted, &emitted.describe());
assert_eq!(emitted, reconstructed);
```

At that moment the system has, strictly from mathematical axioms
and the forced-realization machinery, constructed a
self-generating artifact. The chain from axiom to NN is
content-addressed and replayable. What the machine knows about
itself is *what it proved about itself*.

That's the horizon. Everything we've built points there.

## Self-sustainability as generative-maximality diagnostic

A final framing that unifies everything above:

> Each part's ability to reach a self-sustaining form is evidence
> that the symbol/term set in its domain is maximally generative.

This gives us a **structural test** for maximality that's stronger
than `check_maximally_reduced`:

```
maximally_generative(primitives) ≡
    ∃ non-trivial morphism f : X → X expressible entirely in
    primitives, whose fixed point exists and is reachable from the
    current library.
```

Reducibility is a *local* property (no further compressions under
the current policy). Generative maximality is a *global* one
(the primitive set is rich enough to express self-sustaining
structures). Both matter. Reduction without generative maximality
yields libraries that are clean but sterile — they can't build
anything meaningfully new. Generative maximality without reduction
yields libraries that are productive but bloated — they work but
redundantly.

**Every domain has its own self-sustainability target:**

| Domain             | Self-sustaining form                                                |
|--------------------|---------------------------------------------------------------------|
| mathscape           | Term-level axioms that can regenerate their own corpus              |
| ml-forge            | UOp-level primitives rich enough to express a NN architecture → NN architecture morphism with a non-trivial fixed point |
| axiom-forge         | Proposal verifier that can verify its own verifier's proposal       |
| arch-synthesizer    | Typescape that indexes the types expressing its own indexing logic  |
| substrate-forge     | WASM runtime whose bytecode defines the runtime itself              |
| iac-forge           | IacType set that can express the IacType enum itself                |

When any of these reaches its self-sustainability target, that's
**diagnostic evidence** the primitive set in its domain has hit
generative maximality. The machine can keep growing — more layers,
more primitives — but each additional layer is decoration on a
generatively-complete base, not a necessary extension.

The platform-level self-host (a NN that generates itself) is the
*composition* of per-domain generative maximality: only once each
component reaches its own self-sustainability target can the
platform reach its global one.

**This makes the ambition falsifiable**: if at some primitive-set
expansion the machine *cannot* construct the required self-
sustaining morphism, we learn something specific — that the axiomatic
substrate is missing a structural piece. The reduction dynamics
will then naturally drive discovery toward exactly that missing
piece (pressure concentrates there). The forced-realization loop
becomes an **automated search for generative closure**.

## The unified picture

Five simultaneous optimization layers:

  L1: per-event ΔDL            (the local reward)
  L2: policy thresholds         (fit the reward floor to library density)
  L3: reward weights            (re-balance α/β/γ based on discovery swing)
  L4: algorithm-vs-learning     (which moves are determined, which train)
  L5: generative maximality     (diagnostic: which domains self-sustain yet)

Each layer's optimization is a fixed-point of the layer beneath.
When all five settle simultaneously — the platform has stopped
learning because it has nothing left to discover under the current
axiomatic input — we have reached the mathscape's horizon for this
corpus. From there, enlarging the input corpus triggers the whole
cascade again, at a higher level.

This is the recursive self-refactoring the user asked for: every
part reduces into the next part, and the parts collectively reduce
into a self-sustaining whole. The evidence that we've got it right
is that the *same machinery* applies at every layer, because the
pattern IS the fixed point.
