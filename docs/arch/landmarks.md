# Mathscape Landmarks

Where the machine is, where it's been, and where it goes next. This doc
is the canonical map. Updated every time a milestone closes.

## Where we are (2026-04-18)

The machine:
- Traverses mathscape **autonomously** — discovers primitives, reinforces
  via retroactive reduction, climbs the proof-status lattice to
  `Axiomatized` without human approval or external prover
- Reaches **rank-1 dimensional discovery** — the MetaPatternGenerator
  abstracts concrete identity laws (`add(?x, 0) = ?x`,
  `mul(?x, 1) = ?x`) into an operator-identity meta-rule
  `(?op (?op ?x ?id) ?id) = …` with both operator and identity
  variable
- Operates with **self-containing compute** — per-corpus cost stays
  flat at ~0.84 ms regardless of total sweep size, from 10k to 100k
  corpora. More data makes the machine more efficient, not less
- **Lynchpin holds** at every scale tested (12, 19, 47, 507, 2007,
  10007, 100007 corpora): every rule earns cross-corpus retroactive
  support ≥ 2

### Apex fingerprint

The same two apex rules climb to Axiomatized at every scale:

| Rule | Shape | Origin | Cross-corpus reach at 100k |
|---|---|---|---|
| `S_10000` | `(?op (?op ?x ?id) ?id)` | rank-1 meta (MetaPatternGenerator) | 93,257/100,007 (93.3%) |
| `S_040` | successor-chain universal | rank-0 concrete | 46,883/100,007 (46.9%) |

### What the apex rules tell us

- **S_10000 is genuinely universal.** 93.3% of random procedural
  corpora contain at least one root-level `op(op(x, id), id)` shape.
  This is the machine saying: "in any corpus where a binary operator
  is applied twice with a shared identity-looking argument, this
  pattern fires."
- **S_040 reflects structural density of successor chains** — not
  majority but not rare. ~47% is the true rate of successor-rooted
  terms in random {add, mul, succ} inputs.

## Where we've been

### Phase A: skeleton (initial commits)
- Crates: `mathscape-core`, `-compress`, `-reward`, `-evolve`,
  `-proof`, `-store`, `-axiom-bridge`, `-config`
- Primitives, hash-consing, evaluator, anti-unification,
  statistical prover, epoch/allocator

### Phase B: forced-realization gates (Feb–Mar 2026)
- Gates 1–10, regime detector, reward axes (ΔCR, novelty,
  meta-compression), status lattice, promotion pipeline
- `run_until_reduced`, `MultiLayerRunner`, `ReductionPolicy`

### Phase C: autonomous discovery (Apr 2026)
- Marginal ΔCR in the prover (fixed orthogonal-family gatekeeping)
- `lhs_subsumption` reward axis (captures meta-rule value)
- Anti-unify var-id collision fix (`max(200, input_max + 1)`)
- `CompositeGenerator<Base, Meta>` (base + meta per propose)
- `MetaPatternGenerator` (anti-unify library LHSs)
- Fresh apex emerged: `S_10000` dimensional-discovery meta-rule

### Phase D: forest as substrate (Apr 2026)
- `DiscoveryForest` — retroactive reducer with O(log n) scheduler
- Typed invariants — `IrreducibilityRate`, `CheckPeriod`, `HitCount`
- `due_corpus_view(epoch)` — live-frontier seam for the generator

### Phase E: cross-corpus attestation (Apr 2026)
- Shared forest threads terms across corpora
- `apply_rules_retroactively(&[...])` batch form — no rule starvation
- Saturation sweep: 19 corpora → 2 apex rules Axiomatized
- **Lynchpin invariant** named as the first-class check

### Phase F: autonomous-traversal milestone (2026-04-17)
- `autonomous_traverse.rs` orchestrated suite: small / medium / stress
  / deterministic-replay
- Reserved `mathscape-traverse` skill (blackmatter-pleme)
- `docs/arch/autonomous-traversal.md` — milestone doc
- Commit sequence: `873cfa6` → `1cfc41a` → `405195e`

### Phase G: self-containing compute (2026-04-18)
- `(node, rule)` memoization — retesting skipped, compute bounded
- Scale-invariant apex support threshold (≥5% of sweep, ≥5 corpora)
- Measured: 100k corpora @ 0.84 ms/corpus — per-corpus cost flat from
  10k upward

## Where we go next

Ranked by impact-per-effort. Each extension must preserve the lynchpin.

### Phase H: meta-rule diversity + rank-2 inception

**The move.** Right now one meta-rule (`S_10000`) subsumes all the
others via the reinforcement pass — there's only ever ONE rank-1 rule
alive at a time. For rank-2 discovery to fire, we need multiple
rank-1 rules coexisting so `MetaPatternGenerator` can anti-unify
*between* them.

**Mechanism.** Gate subsumption by lineage: a rule minted by
`MetaPatternGenerator` (origin tag `compress/meta-antiunify`)
should not be subsumable by another meta-rule. Only rank-0 rules
can be subsumed under rank-1; rank-1 rules coexist.

**Signal.** If rank-2 fires, we see a third apex rule emerge with
operator-variable in a deeper position — e.g. a law about laws.
The user called this "incepting": layer N developed entirely by
layers < N's tools.

**Invariant to preserve.** Lynchpin must still hold. Rank-2 rules
need cross-corpus support too.

**Deliverables.** New flex test `flex_rank2_inception_probe`,
updated `reinforcement` subsumption-detection with lineage gate,
updated apex fingerprint in this doc if it lands.

### Phase I: subterm anti-unification

**The move.** Anti-unify currently runs at the root of term pairs.
Add recursion into shared subterm positions so patterns can surface
*inside* terms whose roots differ.

**Mechanism.** In `antiunify::anti_unify`, walk both terms in
parallel and record the maximal shared subterm skeleton. When roots
differ, generate a fresh variable — but before that, check if any
child pair shares more than the surrounding context.

**Signal.** The machine finds patterns like `mul(?x, add(?y, ?z))`
(distributivity-shaped) — currently invisible because roots are
different operators.

**Risk.** Combinatorial blow-up on deeply-nested terms. Needs a
subterm-depth cap to stay tractable.

### Phase J: empirical validity check in the prover

**The move.** Before accepting a candidate, evaluate its LHS and RHS
on K random concrete bindings using the built-in evaluator. Reject
if LHS and RHS don't agree numerically.

**Why.** Currently a structurally-general rule like
`(?op ?x ?id) => ?x` subsumes add-identity and mul-identity but is
semantically wrong for most (op, id) pairs. An empirical check
would catch this.

**Tension.** Mathscape's current frame treats library rules as
COMPRESSIONS (renamings) not EQUATIONS. An empirical check forces
the equation interpretation. Deliberate choice needed.

### Phase K: e-graph equivalence saturation (egg)

**The move.** Integrate the `egg` crate as an optional `Prover` impl.
Accept rules based on structural equivalence under saturation.

**Unlocks.** Commutativity (`add(a, b) = add(b, a)`), associativity,
distributivity — patterns that syntactic anti-unification cannot see.

**Scope.** Medium architectural — `egg` has its own term type we'd
bridge to; saturation is bounded per candidate via step limits.

### Phase L: adaptive corpus generation

**The move.** Use the current library to construct corpora that
specifically probe what the library cannot yet reduce at root. The
machine generates its own next frontier.

**Why.** Procedural corpora at scale 100k already saturate in 5
steps. To advance the frontier the corpus distribution needs to
PROBE the gap.

**Prerequisites.** Phase H and J help first — more meta-rules to
sense gaps, empirical check to avoid false probes.

## Regressions to watch for

Each landmark above ships with regression tests. The machine has ONE
canonical fingerprint; deviation from it is either a new landmark or
a bug.

| Signal | Likely cause |
|---|---|
| Lynchpin violated | New generator bypasses forest attestation |
| 0 Axiomatized rules | W-window too wide OR reinforcement broken |
| Apex fingerprint changed | New capability (update this doc) OR silent bug |
| Determinism broken | HashMap iteration leak, float order dependency |
| Per-corpus cost not flat at 100k | Memoization regressed OR forest leaking |
| Saturation step > 10 | Zoo diversity broken OR corpus-artifact minting |

## Update protocol

Every phase listed above lands with:
1. A new `docs/arch/*.md` describing what it unlocks, depends on,
   and how it can fail
2. A flex test pinning the new invariant
3. An update to this landmarks doc moving the phase from "where we go
   next" to "where we've been"
4. An apex fingerprint update if the machine's Axiomatized set shifted
5. Update to `mathscape-traverse` skill if user-facing invocation
   changed

The discipline is simple: **no new capability lands until it passes
the orchestrated suite AND updates this map.** The map is the
machine's memory of itself.
