# Forced Realization — the Control System Behind the Mathscape

> **Canonical architectural picture**: `machine-synthesis.md`. That
> document supersedes apparent discrepancies between architectural
> documents. This document elaborates the *control-system* framing —
> why the machine must exist and what forces shape it. Types, regime
> naming, and gate counts are authoritative in the synthesis doc.

## Thesis

Mathematical discovery can be engineered as a **forced realization
process**: a multi-gate filter with a gradient that biases search
toward gate-passing regions, and where each passage widens the search
space for the next iteration. The process is:

- **forced** — gates reject limbo; a proposal advances or decays
- **controlled** — gate thresholds are tunable policy
- **expansive** — each realization widens the reachable set
- **across the mathscape** — no region is privileged; any reachable
  point is reachable under *some* policy

The mathscape is defined constructively: it is the set of primitives
and compositions that the realization machine can produce under a given
policy. Change the policy, get a different mathscape.

## The five forces (the gradient)

Not axes of reward — *forces* that, together, define where the
generator is pulled. Any one alone is gameable. Together they force
genuine generalization.

| Force                         | What it measures                                                  | Failure mode if absent                                                |
|-------------------------------|-------------------------------------------------------------------|-----------------------------------------------------------------------|
| **Compression pressure**      | corpus description length decreases                               | library grows without exploiting structure                            |
| **Coverage preservation**     | every prior match is retained                                     | rules shrink library by *removing* structure they can't re-derive     |
| **Irreducibility**            | not derivable from current primitives + library (e-graph check)   | symbols duplicate existing structure under different names            |
| **Cross-corpus applicability**| appears in ≥ N distinct corpora                                   | local theorems masquerade as universal tools                          |
| **Axiomatization pressure**   | every rule monotonically advances status (or decays)              | library accumulates Conjectured noise indefinitely                    |

The first two are the MDL objective on proposals (`condensation-reward.md`).
Irreducibility is the local gate-3 check. Cross-corpus applicability
gates promotion to primitive (`promotion-pipeline.md`). Axiomatization
pressure acts on every resident rule every epoch
(`axiomatization-pressure.md`) and is measured in the same ΔDL currency
as the others (`reward-calculus.md`).

## The gates (canonical picture)

The realization gates are organized in three layers — discovery,
reinforcement-advance, and promotion — giving ten total. The
**canonical gate lattice** is in `machine-synthesis.md`; it is the
authoritative source. What follows is a control-system summary:

- **Discovery gates 1–3** (`compression floor ε`, `coverage delta ≥ 0`,
  `irreducibility`) — cheap, local, fire per proposal in the Discovery
  pass
- **Reinforcement advance gates V/X/A** (verified / exported /
  axiomatized) — structural, fire per resident rule in the
  Reinforcement pass
- **Promotion gates 4–7** (`condensation K`, `cross-corpus N`,
  `axiom-forge obligations`, `rustc typecheck`) — expensive, episodic,
  fire in the Promotion pass

All ten together enforce: **local** (gates 1–3, cheap) +
**structural** (V/X/A, cheap) + **temporal** (4–5, per-artifact
history) + **type-theoretic** (6–7, external tooling). Passing all ten
is strong evidence of real structure.

Status transitions are strictly monotone up the lattice
`Proposed → Conjectured → Verified → Exported → Axiomatized → Promoted
→ Primitive`. Nothing skips a gate; nothing returns to a lower status
except via Subsumption or Demotion (below).

## Three regimes (the system self-detects and re-tunes)

The gate thresholds are not constant — they track the regime the
system is in. The canonical regime names are defined in
`machine-synthesis.md` and `reward-calculus.md`; they are named by
which event category dominates ΔDL.

| Regime        | Dominant ΔDL source       | Signal                                               | Gate policy                                          |
|---------------|---------------------------|------------------------------------------------------|------------------------------------------------------|
| **Reductive** | Reinforce                 | steady status advances, merges, subsumptions         | default loop; no discovery                           |
| **Explosive** | Discovery                 | reinforcement plateau; ε clearance rising            | fire discovery burst; loose gates                    |
| **Promotive** | Promote                   | top entries heavy-tailed in corpus-count; gates 4–5 clear | fire axiom-forge; migrate library; contract; reset   |

Transitions are events. An operator (human, or policy network) reads
the event stream and adjusts the schedule. Regime detection is the
cheapest level of the [minimal-model ladder](minimal-model-ladder.md).

## Expansion — the bootstrap mechanic

When a primitive lands at gate 7:

1. The `Term` enum gains a variant. The Lisp proposal space expands
   combinatorially — any proposal can now use the new primitive.
2. Library rules whose rhs matches the new primitive's pattern are
   **rewritten** to reference it directly. The library contracts:
   rules that become structurally equal are deduplicated.
3. The contraction frees "conceptual budget" — the next Exploration
   phase has fewer library entries to compete against, so lower-scoring
   candidates become viable again.
4. The new primitive becomes a first-class search operator — mutations
   can move expressions *into* and *out of* it.

Discoveries **compound**, not accumulate. This is why the process is
expansive: each realization gives the next round access to a richer
primitive vocabulary without paying proportional search cost.

## Contraction — the migration report

Every primitive promotion emits a `MigrationReport`:

```rust
pub struct MigrationReport {
    pub primitive: AxiomIdentity,         // what was added
    pub rewritten: Vec<TermRef>,          // library entries that now use the primitive
    pub deduplicated: Vec<TermRef>,       // library entries removed as redundant
    pub epoch_id: u64,
    pub content_hash: TermRef,            // the report is itself an Artifact
}
```

The report is stored alongside the registry (same Merkle DAG). A
trajectory of the mathscape is reconstructible by replaying the
sequence of Artifacts and MigrationReports.

## Demotion — the symmetric force that prevents calcification

Without demotion the system always grows, never reorganizes. Bad early
primitives would trap all downstream work (see: Lean's lack of
deprecation). The symmetric force is:

- Every primitive has a rolling **usage tally** (how often its pattern
  matches in the corpus across the last W epochs)
- If usage falls below floor M, emit a `DemotionCandidate` event
- Demotion is **manual-approval** initially (the operator reviews)
- On demotion: the primitive becomes `#[non_exhaustive]` deprecated;
  its rewrite rule moves back to the library; dependent rules are
  re-expanded

Demotion is rare — migration costs are real. But its existence
guarantees the mathscape converges toward a stable, minimal primitive
set rather than drifting into mount-of-accumulation.

## The control surface

Five numbers plus one schedule:

```yaml
ε_compression: 0.02       # gate 1
K_condensation: 3         # gate 4
N_cross_corpus: 2         # gate 5
M_demotion_floor: 1       # demotion trigger
W_usage_window: 100       # epochs over which usage is tallied
regime_policy:            # weights per regime
  exploration: { alpha: 0.7, beta: 0.25, gamma: 0.05 }
  consolidation: { alpha: 0.3, beta: 0.3, gamma: 0.4 }
  promotion: { alpha: 0.2, beta: 0.2, gamma: 0.6 }
```

Start fixed. Then let the system fit ε, K, N as functions of library
size (looser early, stricter late). Eventually a small policy network
predicts "which gate profile maximizes reach-per-compute over the next
100 epochs?" — the top rung of the minimal-model ladder.

## The mathscape is replayable

Every realization decision is recorded in the Registry's Merkle DAG.
Every Artifact carries `parent_hashes`; every MigrationReport is itself
an Artifact. This means:

- **Re-runnable**: given the same corpus and policy, the same
  trajectory is produced (deterministic)
- **Replayable under different policy**: the Registry + CorpusLog is
  enough to re-traverse under new ε/K/N/M values
- **Comparable**: two policies yield two Merkle trees; their diff is a
  concrete characterization of policy impact

This is what makes the system scientifically serious: the output is a
*reproducible function of its control schedule*. The mathscape is not
a random walk — it is a trajectory, and trajectories can be compared.

## Consequences for the code

| Concept                | Type in mathscape-core                                  |
|------------------------|--------------------------------------------------------|
| Four forces            | fields on `AcceptanceCertificate`                      |
| Gates 1–3              | `Prover::prove` return value                           |
| Gates 4–5              | `PromotionGate` trait, reads `Registry` history        |
| Gates 6–7              | handoff to axiom-forge (see `promotion-pipeline.md`)   |
| Regime                 | `Regime` enum, emitted on `EpochTrace`                 |
| Expansion              | `AxiomProposal` minted in axiom-forge                  |
| Contraction            | `MigrationReport`, new Artifact kind                   |
| Demotion               | `DemotionCandidate` event                              |
| Control surface        | `RealizationPolicy` struct, loaded from config         |

See `realization-plan.md` for the phased rollout.

## Relation to existing mathscape documents

- **`CLAUDE.md` "Compression as Tractability"** — the Exploration
  regime's justification
- **`CLAUDE.md` "Compression Equilibrium and Novelty Escape"** — the
  Exploration → Consolidation transition
- **`CLAUDE.md` "Recursive Compression and Reward Evolution"** — the
  Consolidation → Promotion transition
- **`docs/arch/reward.md`** — the local gates (1–3); will be upgraded
  to include the coverage axis
- **`docs/arch/proofs.md`** — gate 3 (irreducibility via e-graph) +
  gates 6–7 (axiom-forge)
- **`docs/arch/storage.md`** — Registry as Merkle DAG; MigrationReport
  persistence
- **`docs/arch/epoch-quad.md`** — the *structure* the gates are
  embedded in
