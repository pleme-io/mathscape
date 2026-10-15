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

### Phase M1: density of attractors — DONE 2026-04-18

Empirical answer: **intermediate quantization, apparent basin count
large-but-sub-linear at current machinery.**

Stairway sweep (pure-procedural, budget=15, depth=4):

| seeds | basins | singletons | new_basins | basin_rate |
|---|---|---|---|---|
| 128   | 91    | 81    | 91   | 0.711 |
| 256   | 168   | 145   | 77   | 0.602 |
| 512   | 306   | 248   | 138  | 0.539 |
| 1024  | 529   | 399   | 223  | 0.436 |

basin-rate decay: 0.436 / 0.711 = 0.613× over 8× seed growth.
basin/seed ratio at 1024: 0.517 (sub-linear).

Growth ≈ seeds^0.85 at this machinery scale. The attractor space
is FINITE but large — definitely more than 529 distinct apex
fingerprints, but the rate of new-basin discovery is shrinking.
Full saturation would require seeds in the 10,000+ range OR
better basin classification.

**Finding that's bigger than the numbers.** The 529 "distinct"
basins are distinct by **apex rule name** (S_xxxx). Many are
likely structurally equivalent modulo rename — same LHS/RHS
shapes with different fresh symbol ids from different runs. True
STRUCTURAL basin count may be orders of magnitude smaller. Phase
M2 (structural classification) is the test.

Pinned by `oscillation_basin_space_cardinality`
(ignored-by-default, ~12s).

### Phase M2: structural basin classification — DONE 2026-04-18

**Answer: ~80 structural basins at 1024 seeds, strongly bimodal.**

`oscillation_structural_basin_classification` anonymizes fresh
symbol ids and variable ids in each run's library, then classifies
basins by STRUCTURAL fingerprint (lhs/rhs shape, not nominal
S_NNN names).

Result at 1024 seeds, pure-procedural:

  Nominal basins  (by S_NNN names) : 529
  Structural basins (by shape)     : 80  (85% compression)

Top-10 structural basin support

    rank  seeds   fraction
    1      445    43.5%    ← dominant attractor A
    2      431    42.1%    ← dominant attractor B
    3       34     3.3%
    4       10     1.0%
    5        7     0.7%
    6        7     0.7%
    7–10   2–4 each

  singleton structural basins : 64/80
  Shannon entropy             : 2.216 bits
  normalized entropy          : 0.351

This IS the finite object. 80 attractors with strongly bimodal
weight (two dominant at ~43% each, carrying 86% of all seed
outcomes). 64 tail attractors carry the long-tail variations
(<1% support each, likely corpus-specific niche structure).

The LLN+anonymization discipline converts what looked like 529
random distinct outcomes into 80 canonical attractor types at
this machinery scale. The "wavefunction" over discovery is
concentrated, not diffuse.

**The eager-collapse principle landed here.** Alpha-equivalent
rules are detected by `eval::alpha_equivalent` and collapsed by
`reduction::detect_subsumption_pairs` at reinforcement time,
BEFORE they pollute the library with nominal variants. The
structural collapse we observe post-hoc used to happen only in
the test; it now happens during traversal itself.

Pinned by `oscillation_structural_basin_classification`.

### Phase M2+: operator-abstract condensation — GEM MATERIALIZED

**The third layer of condensation, run 2026-04-18.**

After structural classification (85% nominal-to-structural
compression → 81 basins), a further layer abstracts ALSO concrete
operator ids. The classification asks: "ignoring which SPECIFIC
operator got Axiomatized (add vs mul vs succ), what SHAPE of rule
did the machine discover?"

Result at 1024 seeds, pure-procedural:

  Layer                                       Basins
  ────────────────────────────────────────────────────
  Nominal (S_NNN names)                       >500
  Structural (anonymize fresh IDs)              81
  Operator-abstract (anonymize ops too)         69

  Modal operator-abstract basin support: 911/1024 = 89.0%

**At this level of abstraction, 89% of ALL seed-driven runs land
in one canonical discovery shape.** The machine's discovery-space
is essentially: ONE dominant pattern + a tail of 68 structural
outliers summing to 11%.

### The gem's anatomy

The top-3 STRUCTURAL basins revealed what's inside the dominant
operator-abstract basin:

  Basin #1 (43.7%): (?4 ?100) + (?3 ?100 ?101) reducing to named
                    symbols. "succ-universal + mul-universal."
  Basin #2 (41.9%): (?4 ?100) + (?2 ?100 ?101). "succ-universal +
                    add-universal."
  Basin #3 ( 3.3%): (?4 ?100) + (?? ?100 ?101) where ?? is a fresh
                    var (rank-1 meta). "succ-universal + meta-
                    operator universal."

All three share the same succ-universal rule. They differ ONLY in
which binary operator reached Axiomatized. Under operator-abstract
equivalence they MERGE into one canonical shape: "a unary op +
a binary op, each reducing to a named symbol holding the op and
its args."

### What the gem tells us about the machine

At current machinery scale (15-corpus pure-procedural budgets), the
TRUE diversity of mathscape's discovery output is:

- ONE canonical discovery shape holding 89% of outcomes
- 68 rare variants (11% combined) carrying niche structural
  differences
- Under the classical nominal view this looks like 500+ distinct
  "outcomes" — 85% of which was fresh-id noise

This is the finite object, the amplituhedron-adjacent gem the user
pointed at. The oscillation isn't a continuum of infinite outcomes;
it's essentially one canonical pattern that presents itself under
different operator-labelings, plus a measurable long tail of
niche variants.

Pinned by:
  - `oscillation_structural_basin_classification` (phase M2)
  - `oscillation_apex_basin_anatomy` (contents of top basins)
  - `oscillation_operator_abstract_basins` (phase M2+ gem)
  - `oscillation_structural_basin_convergence` (verification at 2048)

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
