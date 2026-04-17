# Realization Plan — Advancing the System in Phases

The concrete phased rollout of the forced-realization architecture.
Each phase has: what to build, where, what test proves done, what
unlocks next.

See `forced-realization.md` (theory), `condensation-reward.md`
(reward), `promotion-pipeline.md` (handoff), `minimal-model-ladder.md`
(policy).

## Phase A — Theory lockdown ✓ *this commit*

**What**: Write and commit `forced-realization.md`,
`condensation-reward.md`, `promotion-pipeline.md`,
`minimal-model-ladder.md`, and this plan. Upgrade `CLAUDE.md` and
`epoch-quad.md`. Cross-reference from axiom-forge and iac-forge.

**Where**: `mathscape/docs/arch/`, `mathscape/CLAUDE.md`,
`axiom-forge/docs/MATHSCAPE_HANDOFF.md`,
`iac-forge/docs/METACIRCULAR.md`.

**Test**: every downstream phase references this phase's documents;
no phase is executed without a citation.

**Unlocks**: Phase B — types can now be cascaded against a frozen
design.

## Phase B — Type cascade in `mathscape-core`

**What**: Upgrade `AcceptanceCertificate` with `coverage_delta` and
`condensation_ratio`. Add `PromotionSignal`, `MigrationReport`,
`Regime`, `PromotionGate`, `RealizationPolicy`. Expand `EpochTrace`
to emit `Regime` + pending promotion signals.

**Where**: `mathscape-core/src/epoch.rs`,
`mathscape-core/src/realization.rs` (new), `lib.rs` re-exports.

**Test**:
- All existing tests pass
- New unit tests for `PromotionGate` trait over a synthetic
  `DerivationDag`
- `RealizationPolicy` round-trips via sexpr (if we add ToSExpr to
  config types) or serde JSON

**Unlocks**: Phase C — adapter impls can bind to stable trait
signatures.

## Phase C — Adapters in each role-owner crate

**What**: `CompressionGenerator` in `mathscape-compress` wraps
`extract_rules`. `StatisticalProver` in `mathscape-reward` computes
all three reward axes + returns `Verdict` against
`RealizationPolicy` thresholds. `RuleEmitter` moves from core test
module to public `mathscape-core::epoch::RuleEmitter`.
`InMemoryRegistry` gains `parent_hashes_of(TermRef) -> Vec<TermRef>`
for DAG queries.

**Where**: new `adapter.rs` files in `mathscape-compress`,
`mathscape-reward`.

**Test**: end-to-end epoch integration test that runs 10 epochs over
a synthetic corpus, verifies library grows, verifies artifacts carry
content hashes and parent chains.

**Unlocks**: Phase D — CLI can be refactored without risk.

## Phase D — Refactor `run_epoch`

**What**: Replace imperative `run_epoch` body with
`epoch.step(&corpus)` using the Phase C adapters. Population fitness
update reads `EpochTrace.total_score` / `mean_compression_ratio`
instead of re-computing reward.

**Where**: `mathscape-cli/src/main.rs`.

**Test**: CLI `run 10` produces identical-or-better metrics on the
existing test corpora compared to the pre-refactor commit (measured
by final library size + compression ratio).

**Unlocks**: Phase E — regime detector has a stable trace to consume.

## Phase E — Regime detector (level 4 of the ladder)

**What**: `RegimeDetector` struct in `mathscape-core::realization`
that consumes a window of `EpochTrace`s and emits `Regime`. Pure FSM,
no parameters beyond thresholds. `Epoch::step` calls it and emits the
regime on each trace.

**Where**: `mathscape-core/src/realization.rs`.

**Test**: synthetic trace sequences — rising CR → Exploration;
plateau → Consolidation; heavy-tailed corpus counts → Promotion.

**Unlocks**: Phase F — gate thresholds can now be regime-adaptive.

## Phase F — Adaptive gate thresholds (level 5)

**What**: Replace fixed `ε, K, N` in `RealizationPolicy` with linear
models over `[library_size, epoch_id, rolling_cr]`. Fit by grid search
on a held-out validation corpus. Schedule re-fit every W epochs.

**Where**: `mathscape-core/src/realization.rs` —
`AdaptivePolicy::refit(history) -> RealizationPolicy`.

**Test**: adaptive policy matches or beats a hand-tuned fixed policy
on the reference corpus over 200 epochs.

**Unlocks**: Phase G — promotion pipeline has reliable thresholds.

## Phase G — Promotion gates 4 + 5

**What**: `DerivationDagView` over `Registry`. `PromotionGate` impl
that queries subsumption count (gate 4) and cross-corpus support
(gate 5). Emit `PromotionSignal` on crossing both thresholds.
Persist signals in the registry.

**Where**: `mathscape-store` (for persistent DAG view), trait in
`mathscape-core`.

**Test**: fabricate a library with synthetic subsumption and
cross-corpus evidence; verify the gate emits / withholds signals
correctly; verify signal content hashes are stable across runs.

**Unlocks**: Phase H — axiom-forge handoff.

## Phase H — Axiom-forge handoff (gates 6 + 7)

**What**: `mathscape-axiom-bridge` crate (new). Contains
`signal_to_proposal(PromotionSignal, &Artifact) -> AxiomProposal`.
Contains `proposal_to_migration(&AxiomProposal, &EmissionOutput) ->
MigrationReport`. Provides CLI command `mathscape promote <hash>`
that runs the full pipeline end-to-end and emits Rust source +
migration report.

**Where**: new crate `mathscape-axiom-bridge` in
`mathscape/crates/`. Depends on `mathscape-core`, `iac-forge`
(sexpr), `axiom-forge`.

**Test**: integration test seeds a registry with a fabricated
high-condensation high-cross-corpus artifact; runs `promote`; verifies
axiom-forge produces a non-empty `EmissionOutput` whose canonical
hash matches an expected frozen vector.

**Unlocks**: Phase I — mathscape can actually extend its own
primitive set.

## Phase I — Library migration

**What**: `migrate_library(registry, new_primitive) ->
MigrationReport` runs after a successful promotion. Rewrites library
entries, deduplicates, emits `MigrationReport` into the registry as
an Artifact. Post-migration validation pass (coverage check).

**Where**: `mathscape-core::epoch::migrate_library`,
`mathscape-store::persist_migration`.

**Test**:
- Seed registry with 10 entries, 3 of which reference the same
  pattern; run migration; assert exactly those 3 rewrite, at least
  1 dedups
- Post-migration coverage ≥ pre-migration coverage
- Replay the migration twice → identical hash in both the new library
  state and the MigrationReport

**Unlocks**: Phase J — the loop is closed, can iterate autonomously.

## Phase J — Demotion path

**What**: Usage tally per primitive across a W-epoch window. Below
floor M → emit `DemotionCandidate`. Manual-approval CLI: `mathscape
demotion list` / `mathscape demotion approve <id>`. On approval:
primitive variant gets `#[deprecated]`, rule returns to the library
with status `Demoted`, dependent rules re-expand.

**Where**: `mathscape-store`, `mathscape-cli`, and a small
axiom-forge edit: `demote(AxiomIdentity) -> EmissionOutput` (emits
`#[deprecated]` shim).

**Test**: synthetic window showing usage decay; demotion candidate
fires; on approval, coverage holds.

**Unlocks**: **the system is complete enough to self-reorganize**.

## Phase K — Multi-corpus / cross-domain

**What**: Formalize the corpus type so gate 5 can really count
*distinct* corpora. A `Corpus` carries an id + metadata + source.
Registry indexes matches by corpus id. Cross-corpus support becomes a
well-defined number, not a surrogate.

**Where**: `mathscape-core::corpus::Corpus`, storage updates.

**Test**: feed three unrelated corpora (arithmetic, combinator
calculus, symbolic differentiation), verify that only primitives
appearing in ≥ 2 get promoted.

**Unlocks**: cross-domain promotion — the signature mathematical-
discovery event the whole system is pointed at.

## Phase L — Climbing the ladder (levels 6–7)

**What**: Only after phases B–K are stable and the fixed policy has
plateaued. Add RL policy for mutation bias (level 6). Later, neural
symbol proposer (level 7).

**Where**: `mathscape-policy` (already scaffolded).

**Test**: level-6 must out-perform level-5 on a budget-matched
evaluation. If it does not, level 6 is *removed* — level 5 is the
floor.

## Ordering and gating

Phases A–E are **sequential** — each depends on the previous.

Phases F and G can run in parallel once E is done (gate adaptation
is independent of promotion gates).

Phases H and I are sequential and both gated on G.

Phase J is gated on I.

Phase K is gated on H (needs an actual primitive promoted) but
independent of J.

Phase L is gated on the whole fixed-policy baseline plateauing —
don't start before phase G is actually firing promotions regularly.

## Budget estimate (very rough)

| Phase | Est. cost       |
|-------|-----------------|
| A     | half a day      |
| B     | one day         |
| C     | two days        |
| D     | half a day      |
| E     | one day         |
| F     | two days        |
| G     | two days        |
| H     | three days      |
| I     | two days        |
| J     | two days        |
| K     | one to two days |
| L     | weeks to months |

Phases A–J bring the system to self-reorganizing completeness. Phase
K is the payoff (cross-domain primitives). Phase L is an ongoing
research track, not a fixed-cost phase.

## Success criterion

The system is **realized** when:

1. One continuous run produces at least one Rust primitive promoted
   via the full seven-gate pipeline, migration applied, the system
   continues running in an expanded primitive space
2. The trajectory is replayable — same policy + corpus → same
   Artifact hashes + same MigrationReport hash
3. A second policy produces a *different* but also hash-stable
   trajectory on the same corpus, demonstrating control

At that point the mathscape is no longer a hypothesis — it is a
reproducible function of its policy, as promised in
`forced-realization.md`.
