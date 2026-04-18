# Autonomous Mathscape Traversal

**Status**: Milestone established 2026-04-17. Pinned by
`crates/mathscape-axiom-bridge/tests/autonomous_traverse.rs`.

## What this document establishes

The machine traverses mathscape autonomously. Given any sufficiently-diverse
corpus set, it discovers primitives, reinforces them via retroactive reduction
across a shared forest substrate, and climbs the proof-status lattice all the
way to `Axiomatized` on empirical cross-corpus evidence alone. No human
approval is in the loop. No hook faked the cross-corpus gate. No external
prover assisted. The lifecycle does the work.

This document names the milestone, the loop that closes, the invariants that
hold, and the test harness that locks them in.

## The loop

```
┌─── Discover ─────────────────────────────────────────────┐
│  base generator + meta generator propose candidates      │
│  from the current corpus + current library               │
└────────────────────┬─────────────────────────────────────┘
                     ↓
┌─── Prove ────────────────────────────────────────────────┐
│  marginal-ΔCR + novelty + lhs_subsumption reward         │
│  clamped meta_compression; candidate accepted on ΔDL ≥ ε │
└────────────────────┬─────────────────────────────────────┘
                     ↓
┌─── Retroactive attestation ──────────────────────────────┐
│  accepted rule fires against the shared DiscoveryForest  │
│  every due node is tested; hits recorded as morphism     │
│  edges; per-corpus provenance tallied                    │
└────────────────────┬─────────────────────────────────────┘
                     ↓
┌─── Reinforce ────────────────────────────────────────────┐
│  subsumption detection collapses less-general rules      │
│  under more-general ones; W-window status advancement    │
│  climbs stable rules Conjectured→Verified→…→Axiomatized  │
└────────────────────┬─────────────────────────────────────┘
                     ↓
             next corpus, next epoch
```

Each arrow is a mechanical transition. No human judgment enters. The loop
closes when the library reaches saturation — further corpora add zero new
rules; existing rules confirm their stability via growing cross-corpus
support; apex rules reach Axiomatized; everything else is Subsumed.

## Lynchpin invariant

> Every rule that lands in the library earns cross-corpus support.

Formally: after an N-corpus sweep, every rule in the final library must have
retroactively reduced at least one node in ≥ 2 corpora. Rules with support
from < 2 corpora are "fragile" — corpus-local artifacts that the process
should not have preserved. The lynchpin is the first thing to check after
any sweep. Violation is a regression.

## What "autonomous" means here

- **No human approval**: there is no step where a human or external agent
  decides which rules advance. The `Prover::prove` interface accepts or
  rejects based on ΔDL. The reinforcement pass subsumes based on pattern
  containment. The status-advancement mechanic climbs the lattice on
  W-window stability. All deterministic, all reflex-level.

- **No hook fake**: earlier flex tests had a `build_observational_hook` that
  fabricated cross-corpus evidence to exercise gate 5. Autonomous traversal
  does not use that hook. Cross-corpus evidence is *earned* by the forest's
  retroactive reduction — a rule gets credit for a corpus only when it
  actually reduces a node that was inserted from that corpus.

- **No external prover**: `StatisticalProver` is the only prover in the
  loop. It does not call out to Lean, Coq, or egg. ΔDL + novelty +
  `lhs_subsumption` are the only signals.

- **Deterministic**: two sweeps with identical budget and depth produce
  identical reports. `autonomous_traverse_deterministic_replay` pins this.

## What autonomous traversal is NOT

- Not a theorem prover. `Axiomatized` here is the top rank of the mathscape
  proof-status lattice — it means the rule passed every current-generation
  machinery gate. It does not mean a formal proof exists.
- Not semantic validation. The machine finds patterns with structural
  subsumption; it does not verify they hold under evaluation. A rule like
  `(?op ?x ?id) => ?x` subsumes add-identity and mul-identity syntactically
  but is semantically invalid for most `(op, id)` pairs.
- Not commutativity / associativity / distributivity discovery. Those require
  equality saturation (e-graph) or empirical evaluation checks. Both are
  outside this milestone.

## The two apex rules observed

On every scale we've tested (12 corpora, 19, 47), the same two rules climb
to Axiomatized:

| Apex | Source | Cross-corpus reach |
|---|---|---|
| `S_10000` | MetaPatternGenerator (dimensional-discovery meta-rule over operator and identity variables) | 18/19, 46/47 |
| `S_040`   | CompressionGenerator (successor-chain universal from successor-chain corpus) | 14/19, 42/47 |

Their structural shape:

- **`S_10000` (rank-1 meta)**: `(?op (?op ?x ?id) ?id) => S10000(?op, ?x, ?id)`
  — the nested identity-element abstraction with both operator and identity
  value as variables.
- **`S_040` (successor universal)**: reduces successor-chains of various depths
  across the zoo.

All other discovered rules (add-identity, mul-identity, nested variants,
successor depth-specialists) end up Subsumed under these two apex rules —
the reinforcement pass correctly identifies them as specializations.

## Test harness

Four orchestrated tests in
`crates/mathscape-axiom-bridge/tests/autonomous_traverse.rs`:

| Test | Total corpora | Purpose |
|---|---|---|
| `autonomous_traverse_small` | 12 | Smoke-check the loop closes at minimum scale |
| `autonomous_traverse_medium` | 19 (default) or from env | Flagship — the default config the milestone was pinned on |
| `autonomous_traverse_stress` | 47 | Lynchpin holds at 2-3× scale; saturation still clean |
| `autonomous_traverse_deterministic_replay` | 2 × 17 | Identical reports across independent runs |

Each test calls `run_traversal(procedural_budget, max_depth)`, receives a
`TraversalReport`, and pins:

1. Lynchpin invariant: no fragile rules.
2. Apex emergence: ≥ 1 Axiomatized rule.
3. Apex quality: every Axiomatized rule has ≥ half-sweep cross-corpus
   support.

Medium and stress additionally pin saturation: the library must stop growing
strictly before the end of the sweep.

## Invocation via skill

The `mathscape-traverse` skill
(`blackmatter-pleme/skills/mathscape-traverse/SKILL.md`) is the reserved
entry point for triggering traversal. It invokes the test harness and
interprets the output. Do not run traversal by hand for reporting purposes;
use the skill so the narration format is consistent and the invariants are
checked.

## What this unlocks

- **A small, empirically-attested axiom set**. Two rank-1 rules carrying
  real cross-corpus evidence. This is the minimal foundation the machine
  has earned through structure-preservation alone.
- **A reproducible measurement of reach**. We can now ask "how much does
  corpus X change the apex set?" or "what's the saturation depth at scale
  Y?" — both are single `cargo test` invocations.
- **A baseline for the next capability jump**. Any future machinery
  (subterm anti-unification, e-graph equivalence, meta-meta discovery)
  can be evaluated against this milestone: does the loop still close?
  Do the apex rules change? Does saturation depth shift?

## What this doesn't unlock (yet)

- Commutativity, associativity, distributivity. Require equality
  saturation.
- Semantic validity checking. A rule's structural subsumption doesn't
  imply its semantic correctness.
- Meta-meta discovery. Requires anti-unification across multiple
  independent meta-rules — currently one meta-rule dominates and
  subsumes the others before meta-meta could fire.
- Proof export to Lean/Coq. `Exported` status is reachable in the
  lifecycle lattice but no exporter is wired.

Each of these is a named next-phase capability. None undermines the
autonomous-traversal milestone; each extends it.

## Reproducibility

```bash
cd ~/code/github/pleme-io/mathscape
cargo test -p mathscape-axiom-bridge --test autonomous_traverse
```

All four tests must pass. Any failure means the milestone has regressed
and needs investigation. Use `--nocapture` to see the full TraversalReport
narrative for each test.

## Cross-references

- `docs/arch/machine-synthesis.md` — the five architectural objects, ten
  gates, five forces.
- `docs/arch/forced-realization.md` — the control-system framing.
- `docs/arch/promotion-pipeline.md` — where the cross-corpus gate lives in
  the canonical gate lattice.
- `docs/arch/reward-calculus.md` — ΔDL as the single currency.
- `docs/arch/fixed-point-convergence.md` — mathscape as a convergence
  controller; autonomous-traversal is a specific convergence the machine
  now demonstrably reaches.
- `blackmatter-pleme/skills/mathscape-traverse/SKILL.md` — the reserved
  skill for invoking traversal.
