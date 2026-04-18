# The Bettyfine–Complexity Duality

**The steering wheel.** Every equivalence choice that widens the
bettyfine narrows the visible mathscape complexity, and vice versa.
Bettyfine (contraction) and complexity (expansion) are two sides of
one pressure; tuning either one drives the other. Over epochs, the
machine steers its own discovery trajectory by choosing where to
hold the dial.

## The duality stated

At any fixed machinery state, define:

- **Bettyfine(≡)** — the modal attractor basin of the discovery
  moduli space under equivalence relation ≡. A seed "lands in the
  bettyfine" iff its discovery library is ≡-equivalent to the
  canonical form.
- **Complexity(≡)** — the complement: everything in the moduli
  space that is NOT ≡-equivalent to the bettyfine. The long tail,
  the niche attractors, the structural variants, the unknown
  frontier.

Then for the same discovery dynamics:

- Loose ≡ (operator-abstract + shape-abstract + ...) →
  |Bettyfine| up, |Complexity| down — the machine says "most seeds
  discover the same thing"
- Tight ≡ (nominal-only, or minimal equivalence) →
  |Bettyfine| down, |Complexity| up — the machine says "most seeds
  discover structurally distinct things"

Both are TRUE simultaneously for the same runs. They differ only in
what the observer is willing to call "the same."

## Measured data

At 1024 pure-procedural seeds (2026-04-18):

| Equivalence ≡ | |Bettyfine support| | |Complexity basins| |
|---|---|---|
| Nominal (no anonymization) | ~1% modal (many unique fingerprints) | ~530 |
| Structural (anonymize fresh-ids) | 43.7% modal (bimodal) | 82 |
| Operator-abstract (anonymize ops too) | 89.0% modal | 69 |

Each row is the SAME 1024 runs viewed under different equivalence
discipline. The steering is: which row is "the truth"? The answer
is "all of them, simultaneously, for different questions."

## The steering wheel in epochs

Discovery across epochs has two failure modes:

1. **Stagnation** — library converges early, no new candidates land,
   the machine is "stuck in the bettyfine." Remedy: loosen ≡ at
   classification time. Rules that the machine was treating as
   distinct merge into the bettyfine; freed vocabulary space opens
   up for new attractors in the complexity region. Effectively: the
   machine realizes it had been discovering the same thing over
   and over, and can now look for new patterns.

2. **Explosion** — library grows unboundedly without consolidation,
   every seed produces a unique fingerprint. Remedy: tighten ≡.
   Force the machine to commit to coarser equivalence classes so
   what IS canonical gets crystallized rather than lost in
   long-tail variants.

The dial is typically held at a specific ≡ throughout a sweep — but
there's no reason it has to be constant. The steering is: **change
the equivalence tolerance over the epoch trajectory** based on
observed productivity.

## Concretely: the four ≡-dials we already have

1. `pattern_equivalent(a.lhs, b.lhs)` — mutual LHS subsumption.
   The weakest equivalence.
2. `proper_subsumes(r1, r2)` — LHS match + RHS agrees under
   substitution. Mid-strength; detects semantic equivalence in
   simple cases.
3. `alpha_equivalent(r1, r2)` — identical under fresh-id anonymization.
   Strong; the eager-collapse invariant.
4. Operator-abstract equivalence — additionally anonymizes concrete
   operators. Strongest; merges rules that differ only by which
   specific op.

Currently the reinforcement pass uses alpha_equivalent eagerly for
the meta-rule collapse gate. The other dials are test-instruments.
Phase M5 would wire them as runtime-selectable equivalence classes
for different kinds of discovery.

## Steering strategies

### Production mode (minimize complexity, maximize bettyfine reuse)
Serve the bettyfine directly via `bettyfine_library(vocab)`. No
discovery run needed. The machine's answer for "what does this
corpus vocabulary discover" is pre-computed.

### Exploration mode (maximize complexity, minimize bettyfine)
Use nominal equivalence only. Every seed's library is treated as
distinct. Every rule is a potential frontier candidate. Expensive
but surfaces the widest variety of potential structural
differences.

### Research mode (oscillate)
Alternate tight and loose ≡ across epochs. Tight epochs expand
what's visible as frontier; loose epochs consolidate. The
oscillation itself is the "pressure" that keeps new discoveries
landing.

### Bootstrap mode (start from bettyfine)
Load the canonical bettyfine library before running discovery. The
machine starts with the canonical layer-0 compression already in
place and only needs to discover what's ABOVE it (identity laws,
associativity, etc. — phases I/J/K territory).

## Why this is the steering wheel

Every mature discovery system faces the same tension:
- Unchecked diversity = overfitting to noise, nothing canonical
  surfaces
- Aggressive collapse = overgeneralization, real distinctions lost

Most systems pick ONE equivalence discipline and commit. The
bettyfine-complexity duality makes the choice a PARAMETER the
machine can steer. That's the steering wheel. The specific dial
settings are the tires; what direction the car goes is determined
by which equivalence the machine holds for this epoch.

## Phase M5 deliverables (proposed)

1. Add a `DiscoveryMode` enum parameter to the runner:
   `{Production, Exploration, Research, Bootstrap}`. Each selects
   a different default ≡ at the reinforcement and candidate-dedup
   sites.

2. Add a `steering_controller` that observes productivity per
   epoch (rate of new rule landings, rate of alpha-equivalent
   collapses, rate of proper-subsumption collapses) and adjusts
   the runtime ≡ accordingly. Research-mode oscillation driven by
   this feedback loop.

3. Lock-in tests pinning the bettyfine content for each
   (vocabulary, mode) pair. Any drift is either a new landmark to
   record or a regression to fix.

4. Update `mathscape-traverse` skill with bettyfine-mode
   documentation.

## Bettyfine features as hyperparameters

Because the bettyfine has MEASURABLE features — modal support,
basin count, Shannon entropy, rule cardinality, rank-0/rank-1
ratio, cross-basin frequency distribution — each becomes a
candidate objective for hyperparameter optimization across the
machine's tunable parameters.

Tunable hyperparameters (the "car's settings"):

- **Equivalence dial** (M5 steering): pattern_equivalent /
  proper_subsumes / alpha_equivalent / operator-abstract
- **Operator vocabulary**: which operators enter the corpus
- **Budget × depth** for procedural corpora
- **ExtractConfig**: min_shared_size, min_matches, max_new_rules
- **RewardConfig**: alpha (ΔCR), beta (novelty), gamma (meta
  compression), delta (lhs subsumption)
- **Zoo composition**: which hand-crafted corpora + ratio to
  procedural

Measurable bettyfine features (the "car's instrument panel"):

- `modal_support ∈ [0, 1]`: how dominant the top basin is
- `basin_count`: how many distinct attractors exist
- `rule_cardinality`: how many rules in the canonical library
- `shannon_entropy_bits`: distributional spread
- `rank1_fraction`: proportion of meta-rules (dimensional-
  discovery quality)
- `cross_basin_coverage`: what fraction of rules reach ≥ K
  basins (the universals)

Objectives for optimization:

- **Maximize modal support at minimum rule count** — shortest
  canonical library that dominates
- **Maximize rank1_fraction** — drive the machine toward
  dimensional abstraction
- **Minimize basin_count at fixed modal threshold** — simplest
  possible moduli space that still captures the mode

Standard HPO machinery applies:
- Grid search across {equivalence_dial × budget × depth × ...}
- Bayesian optimization over the continuous parameters
  (reward weights)
- Evolutionary search over vocabulary composition

Phase M6 deliverable: one HPO sweep test that varies ONE
hyperparameter (say, reward `alpha`) and plots bettyfine features
across the sweep. That gives us the first empirical map of "how
does reward shape the bettyfine." From there, multi-dim sweeps
and automated HPO follow.

## The core insight, stated

The machine's discoveries exist in a moduli space. Viewed under
tight equivalence, the space is large and most of it is frontier.
Viewed under loose equivalence, the space contracts to a few
attractor basins — the bettyfines. Which view the machine holds
determines what it treats as "known" vs "unknown" and therefore
what it does next. The equivalence dial IS the discovery steering
wheel.

And because the bettyfine's features (modal support, entropy,
rule count, etc.) are all measurable, the steering wheel itself
can be OPTIMIZED — the machine's hyperparameters become
first-class design variables whose fitness is evaluated on the
observable shape of the bettyfine. That's not just steering; it's
a feedback-controlled meta-optimizer whose loop closes on the
geometry of its own discoveries.
