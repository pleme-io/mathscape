# Collapse-and-Surprise — The Phase-Transition Reward

Four user insights compressed into one theoretical frame:

1. **Reduction is layer-relative.** "Maximally reduced" does not
   mean the same thing at every layer. Layer 0 reduction is
   subsumption + status advancement. Deeper layers include meta-
   compression, cross-operator invariance, and other axioms that
   only make sense in their expanded primitive set.

2. **Collapses are the natural event we optimize for.** Large
   condensation events — many rules deduplicating into one after a
   promotion, a meta-symbol unifying previously-independent rules,
   a migration that contracts the library — are not side-effects
   of the machine running. They *are* the point. Compression
   ratio and novelty are scalar proxies; the collapse is the
   underlying object.

3. **Surprise drives reward.** A collapse the reward estimator
   anticipated should score less than one that exceeded its
   prediction. Information-theoretic surprise — `-log(predicted_
   probability_of_event)` — is the correct scaling.

4. **Collapse rate controls experimentation.** When collapses are
   happening often, the policy is fitted to the current density —
   we can afford bolder thresholds. When they become rare, we
   conserve; we've extracted what we can under this policy and
   should tighten rather than thrash.

## The phase-transition model

The current reward-calculus.md treats library growth as a continuous
gradient. The real dynamics are closer to **critical density** in
statistical physics:

```
Library size
    ▲
    │
    │                  ┌──── (post-collapse contraction)
    │                ╱
    │              ╱
    │            ╱ ╲ ← collapse event
    │          ╱
    │        ╱        ╲ ← collapse event
    │      ╱         ╱
    │    ╱         ╱
    │  ╱         ╱
    └─────────────────────▶ epochs
       linear   collapse   regrowth
       growth   event      post-migration
```

Phases:

1. **Addition**: library grows linearly as discovery lands new
   Conjectured rules.
2. **Pressure buildup**: library density rises; pairwise
   subsumability increases; pattern redundancy rises. Compression
   ratio plateaus — the *lagging* indicator. Pressure is the
   *leading* indicator.
3. **Phase transition**: a promotion signal fires (gates 4–5 clear);
   migration contracts the library; a new primitive expands the
   substrate.
4. **Regrowth in expanded substrate**: linear growth resumes over
   the richer primitive set.

The plateau concept from forced-realization.md is a *symptom* of
phase 2, not the phase itself. `reduction-pressure` is the real
quantity.

## Reduction pressure = unresolved-subsumption density

A computable leading indicator of imminent collapse:

```
pressure(library, policy) =
    |{(a, b) ∈ library² : a subsumes b under policy, both active}|
  / max(1, |active_artifacts(library)|)
```

When pressure exceeds a policy-specific threshold, a collapse is
*scheduled*. The reinforcement pass fires the merges + promotions
that realize it. The pressure measurement in `reduction.rs` is the
same pairwise-subsumption scan that barriers are built from —
`ReductionVerdict::barrier_count()` over the `SubsumablePair`
variant *is* the pressure counter.

## The surprise dimension of ΔDL

Every Event's `delta_dl` can be split into two terms:

```
delta_dl = base_bits + surprise * novelty_multiplier

surprise(event, estimator) =
    | actual_delta_dl - estimator.expected(event.category) |
  / max(estimator.variance, ε)
```

Collapses score both more `base_bits` (they genuinely shrink the
library) *and* higher `surprise` (phase transitions are harder to
predict than incremental events). The allocator naturally directs
more compute toward regimes where surprise-per-cost is highest.

This is Schmidhuber's artificial-curiosity reward applied to the
reward calculus: information gain weights ΔDL, not replacement of
it. The additivity property of `V(epoch) = Σ event.delta_dl()`
survives.

## Collapse-rate control of experimentation

A two-parameter control rule, layered on the RealizationPolicy:

```rust
collapse_rate = collapses_per_window / window_epochs

if collapse_rate > high_threshold:
    // Productive ground. Experiment.
    ε_compression *= loosen_factor
    K_condensation *= loosen_factor
else if collapse_rate < low_threshold:
    // Stuck. Conserve.
    ε_compression *= tighten_factor
    K_condensation *= tighten_factor
else:
    // Stable — no adjustment.
```

This is counter-intuitive relative to classical exploration/
exploitation: *high reward frequency pushes toward MORE exploration*,
not less. The rationale: frequent collapses mean the current policy
is well-matched to the library density; we can afford bigger risks
because another collapse will bail us out if the risk doesn't pay.
When collapses stop, we're off the vein — time to exploit what we
have cleanly.

This inverts a classical assumption but matches the natural
dynamics of mathematical discovery: productive eras yield more
productive eras; fallow periods are when conservation matters.

## Layer relativity formalized

A `ReductionPolicy` is **per-layer**. Layer K's policy:

- Checks the subsumption relation valid at layer K (might include
  cross-operator invariants at deep layers)
- Sets the advance ceiling appropriate to what's provable at that
  layer (Verified at layer 0, possibly Exported at layer 1, etc.)
- Has its own collapse-rate thresholds (deep layers have rarer but
  larger collapses)

Transitioning from layer K to layer K+1 happens exactly when:

```
check_reduction(library, policy_K) == Reduced
```

At that point:
- `check_reduction(library, policy_{K+1})` starts returning
  `Barriers(...)` — the richer layer sees obstacles that layer K
  couldn't see
- The machine's focus shifts to those barriers
- Over time, layer K+1 is reduced in turn; transition to K+2 fires;
  etc.

## The system's ambition in one sentence

> A machine that autonomously detects rising pressure, fires
> collapses as they become available, rewards surprise in those
> collapses, adapts its exploration policy based on collapse rate,
> and does this recursively over deepening layers of primitive
> extension — each layer's reduction relative to its own policy.

## Phase ordering (in priority)

For the multi-layer discovery big step:

1. **Reduction meter** (this module, just landed). Gives us a
   layer-0 "done" condition.
2. **Pressure metric** (trivial add on top of the meter). The
   leading indicator.
3. **Real reinforcement pass** that actually fires merges /
   subsumptions when pressure allows. This is what moves pressure
   to zero and triggers phase 3.
4. **Real library rewriting in `migrate_library`**. Completes
   phase 4.
5. **Collapse-rate controller** adjusting ε/K adaptively.
6. **Surprise-weighted reward** scoring phase transitions.
7. **Multi-layer orchestration**: layer 0 runs until reduced
   under `layer_0_default`; migration fires; layer 1 opens;
   recurse.

Items 1–2 are cheap and already 90% shipped. Items 3–4 are the big
step's real dependencies. Items 5–7 are the optimizer and
orchestrator on top.

## Relation to other docs

- `reward-calculus.md` — surprise is the curvature on top of the
  flat ΔDL currency
- `axiomatization-pressure.md` — reinforcement pass is phase-
  transition scheduler, not a plateau-filler
- `forced-realization.md` — the gates are the *channels* through
  which pressure releases; collapse is what it looks like when
  multiple release at once
- `fixed-point-convergence.md` — traps are the regions *between*
  collapses; collapses are transitions between traps
- `knowability-criterion.md` — replayability becomes "collapse
  sequence is a deterministic function of policy + corpus"; two
  policies produce different collapse histories, which is what
  distinguishes them

## The headline

*Mathscape is not compressing a fixed corpus. It is accumulating
pressure until the next collapse, rewarding surprise in the
magnitude of that collapse, and self-tuning the thresholds based
on how often collapses happen. The layers deepen until pressure
ceases to build under any policy — at which point the mathscape,
for this corpus, is exhausted.*
