# Oscillating Irreducibility — Phase M

**Status**: Phenomenon empirically observed 2026-04-18. Instrumentation
in place (`oscillation_probe_seeded_variance`). Research direction, not
yet operationalized.

## The user's insight

> "my instinct says there probably isn't a reliable constant signal for
> irreducibility so there is some oscillating seed that probably keeps
> things from sticking if there isn't a single point its a wave or
> probably distribution at the core of the whole thing that breaks
> symmetry, its much like symmetry breaking and information theory"
>
> "and if we identify the behavior of that oscillation and optimize it
> we can optimize discovery"
>
> "I bet the core object here that drives the unstickiness is something
> that looks like quantum object behavior"
>
> "the oscillation probably behaves like quanta"

Translation into machine terms:

- Irreducibility is **not a single-point observation**. Classical
  subsumption checks (`pattern_match`, `pattern_equivalent`,
  `proper_subsumes`) all produce point answers — yes/no for a given
  (r1, r2) pair. That's insufficient when the true behavior is
  distributional.
- The **oscillation** comes from the input distribution. Which corpora
  the machine sees, in what order, with what procedural variation —
  each sample is one measurement of a standing wave.
- **Symmetry-breaking** at the core: when two possible discovery
  trajectories are equally valid (both preserve the lynchpin, both
  converge), the tiny asymmetries in the seed/corpus decide which
  trajectory the machine takes. The DISTRIBUTION of outcomes across
  seeds reveals the set of possible attractors.
- **Quantum-like behavior**: the outcomes are not a continuum but a
  DISCRETE SET of attractor basins. Each seed "measures" one basin.
  The count of distinct basins across K seeds gives a finite
  multiplicity — the number of orthogonal convergence states.

## Empirical observation

`oscillation_probe_seeded_variance` runs the discovery pipeline with 8
different procedural seeds and records the apex-rule fingerprint each
lands on.

**Zoo-anchored (7 hand-crafted + 15 procedural corpora per seed)**:

```
 seed    apex[0]    apex[1]    sat    elapsed
    1      S_040    S_10000      5       23ms
    7      S_040    S_10000      5       17ms
   42      S_040    S_10000      5       16ms
   …     [all identical]
  9999     S_040    S_10000      5       15ms

Distinct apex fingerprints : 1
Modal support              : 8/8 (100%)
```

UNIFORM — the zoo dominates the structural signal so heavily that seed
variation is invisible. This is the regime of "classical" autonomous
traversal.

**Pure-procedural (no zoo, only 15 seed-driven corpora per seed)**:

```
 seed    apex[0]    apex[1]    sat    elapsed
    1      S_040    S_10000      5       23ms
    7      S_046    S_10000      3       15ms
   42      S_002      S_003      —       11ms
  100      S_008      S_009      —       13ms
  256      S_009      S_010      —        9ms
  500      S_014      S_018      1       19ms
 1024      S_017      S_020      1       17ms
 9999      S_004      S_005      —       11ms

Distinct apex fingerprints : 8
Modal support              : 1/8 (12%)
```

**OSCILLATING.** Eight seeds, eight distinct basins. The phenomenon
exists. It only becomes visible when the procedural corpus distribution
has enough weight relative to the hand-crafted zoo to actually
influence which attractor the machine lands in.

## What this means

The machine has many stable convergence states. At the deterministic
level — fixed seed, fixed corpus — it always goes to the same one. But
the SPACE of states is large, and which one the machine chooses is a
function of the input distribution.

In classical mechanics this would be called "initial-condition
sensitivity." In quantum mechanics, "eigenstate selection under
measurement." In information theory, "symmetry-breaking under
channel noise." The user named the phenomenon before it was
instrumented; the instrumentation confirms it.

## What "surrounds" it vs what "controls" it

We **surround** the phenomenon — we can now observe it from many angles:

| Instrument | What it reveals |
|---|---|
| `oscillation_probe_seeded_variance` | Number of distinct attractors across a seed set |
| `autonomous_traverse_deterministic_replay` | Confirms per-attractor determinism |
| `autonomous_traverse_medium/stress` | Proves the dominant attractor at zoo scale |
| `rank2_inception_probe` | Shows which attractor the subsumption gate preserves |
| `DiscoveryForest::edges` + cross-corpus map | Per-attractor structural signature |

We do NOT yet **control** it. Control would mean:

- Steering the machine deliberately toward a specific attractor
- Forcing transitions between attractors (phase transitions)
- Enumerating the FULL set of possible attractors (not just sampling)
- Quantifying the "distance" between attractors in rule-space

These are open research problems. The user was right: "I just don't
know how we can take control of it."

## Next moves toward control

Ranked by how decisively each advances toward steering.

### Phase M1: density of attractors

Sweep many more seeds (100, 1000) and plot the fingerprint
distribution. Is the count of distinct attractors bounded (truly
quantized) or unbounded (continuum approximated by discreteness)?
If bounded, we've enumerated the finite state space of possible
discovery outcomes.

### Phase M2: seed-space clustering

For each seed, record the FULL library composition (not just apex).
Cluster by similarity. The clusters ARE the attractor basins.
Compute their volumes (how many seeds fall into each). This gives
a probability distribution over attractors — the "wavefunction" of
discovery outcomes.

### Phase M3: transition between attractors

Take two seeds s1, s2 that land in different attractors. Blend
their corpora incrementally (50% s1 corpora + 50% s2). Observe
whether the machine lands in attractor-1, attractor-2, or a
third. If third, we've observed an INTERMEDIATE attractor —
evidence that attractors have adjacency structure.

### Phase M4: oscillation-driven discovery

If attractors form a graph, run discovery deliberately across
multiple seeds and take the UNION of their libraries. Rules that
appear in many attractors are universal. Rules in few attractors
are niche but real. A library composed of union-across-attractors
is RICHER than any single-attractor library.

This is optimization by oscillation: instead of trying to converge
to one answer, sample the distribution and take the UNION of
robust discoveries. The user's "identify the behavior of that
oscillation and optimize it" — this is the concrete proposal.

## Tie to proper subsumption (phase H refinement)

`proper_subsumes` (the RHS-aware subsumption check added alongside
this doc) is deterministic — it resolves LHS-ambiguous subsumptions
by checking whether the substituted RHSs agree. That's a
**point measurement** of irreducibility.

The oscillation framing says: even proper-subsumes gives a point
answer when the true behavior is distributional. A rule that's
properly-subsumed under one seed's corpus distribution might fail
to be properly-subsumed under another's (because the set of
concrete instances against which the check would be empirically
applied differs).

The gates we build can ONLY be as deterministic as the subsumption
check itself. Oscillating gates — gates whose decision depends on
a sampled distribution — are a future research direction. The
current phase-H gate is the correct deterministic floor; phase M
would add a distributional layer on top.

## Glossary

- **Attractor basin** — a stable convergence state the machine
  lands in. Defined empirically by (apex rule set, library
  composition, final forest stats).
- **Modal support** — the fraction of seeds that land in the
  most-frequent attractor. High = uniform; low = oscillating.
- **Symmetry breaking** — the mechanism by which small seed
  differences select between multiple equally-valid attractors.
- **Eigenstate-like behavior** — the hypothesis that attractors
  are discrete rather than continuous; a seed "collapses" the
  system into one of them.
- **Oscillating irreducibility** — the idea that
  rule-subsumption is a distributional property that only reveals
  itself across seeds, not at any single evaluation point.

## References

- `crates/mathscape-axiom-bridge/tests/autonomous_traverse.rs` —
  `oscillation_probe_seeded_variance` and
  `run_traversal_pure_procedural`
- `crates/mathscape-core/src/eval.rs` — `proper_subsumes`, the
  deterministic irreducibility check
- `crates/mathscape-core/src/reduction.rs` — where the check is
  applied in the reinforcement pass
- `docs/arch/landmarks.md` — phase-M listing in the roadmap
