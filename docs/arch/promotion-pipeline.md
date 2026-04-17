# Promotion Pipeline — Library Entry to Rust Primitive

How mathscape discovers that something is no longer just a rule but
deserves to be a first-class primitive. See `forced-realization.md` for
the surrounding picture.

## Thesis

A library entry becomes a Rust primitive exactly when:

1. (gate 4) it subsumes ≥ K existing library entries (`condensation`)
2. (gate 5) it has appeared in ≥ N distinct corpora (`cross_corpus`)
3. (gate 6) axiom-forge's 7 obligations accept it
4. (gate 7) the generated Rust compiles

None of these is negotiable. Each is cheap-to-expensive in order. The
pipeline is strictly sequential — a failure at any gate halts
promotion, the candidate stays in the library, and the event is
logged.

## Phase A — Temporal gates (mathscape-side)

After gate 3 accepts an Artifact, it enters the library with status
`Conjectured`. A dedicated `PromotionGate` watches the registry across
epochs:

```rust
pub trait PromotionGate {
    fn evaluate(
        &self,
        artifact: &Artifact,
        history: &DerivationDag,
    ) -> Option<PromotionSignal>;
}
```

The `DerivationDag` is a read view of the Registry that supports:
- How many library entries reference this artifact's symbol? (for K)
- Across how many distinct corpora has this artifact's lhs matched?
  (for N)
- Has usage remained high or is it drifting toward demotion?

A `PromotionSignal` is emitted when both K and N are cleared:

```rust
pub struct PromotionSignal {
    pub artifact_hash: TermRef,          // the candidate
    pub subsumed_hashes: Vec<TermRef>,   // library entries it condenses
    pub cross_corpus_support: Vec<CorpusId>,  // ≥ N entries
    pub rationale: String,               // human-readable "why this, why now"
    pub epoch_id: u64,
}
```

Signals are written to a promotion queue (persistent, processed
asynchronously). Multiple signals per epoch are normal.

## Phase B — Serialization (mathscape → axiom-forge)

A `PromotionSignal` maps to an `AxiomProposal`:

| PromotionSignal field              | AxiomProposal field                                                                 |
|------------------------------------|-------------------------------------------------------------------------------------|
| `artifact.rule.name`                | `name` (PascalCased via meimei)                                                     |
| `artifact.rule.lhs` structure       | `fields` (each free variable in lhs becomes a typed field)                          |
| `rationale`                         | `doc`                                                                               |
| `subsumed_hashes`                   | `asserted_invariants` (e.g. "subsumes: S_015, S_019, S_022")                        |
| target path                         | fixed: `"mathscape_core::term::Term"` (the enum we're extending)                    |
| kind                                | `AxiomKind::EnumVariant`                                                            |

The map is executed by `mathscape-axiom-bridge::signal_to_proposal`.
The transform is *total*: any accepted PromotionSignal yields a
structurally-valid AxiomProposal (structurally valid ≠ semantically
accepted by axiom-forge — gates 6–7 still apply).

## Phase C — Axiom-forge handoff (gates 6–7)

axiom-forge receives the proposal. Its 7 obligations run:

1. `NameWellFormed` — PascalCase, ASCII, starts-with-letter
2. `NameNotReserved` — not a known `Term` variant name
3. `FieldsWellFormed` — distinct snake_case fields
4. `FieldCountBounded` — ≤ `MAX_FIELDS` (= 8)
5. `TargetPathValid` — we're extending `Term`
6. `DocNonEmpty` — rationale is non-empty
7. `ContentAddressable` — proposal hash is non-zero

If obligations pass, axiom-forge emits:
- The new enum variant (source text)
- `impl ToSExpr`/`FromSExpr` arms
- A `FrozenVector(canonical_text, b3sum_hex)` for cross-language portability

The Rust source is handed to the operator (or a CI bot) who merges the
patch. `rustc` is gate 7 — if the code doesn't compile, the promotion
is rolled back and the library entry stays `Conjectured`.

## Phase D — Migration (library contraction)

On successful promotion (gate 7 passed), `mathscape` runs a migration:

```rust
fn migrate_library(
    lib: &mut Registry,
    new_primitive: PrimitiveIdentity,
) -> MigrationReport {
    // 1. for each library entry whose rhs contains a pattern equivalent
    //    to the new primitive's lhs, rewrite the rhs to use the new
    //    primitive
    // 2. any entries that become structurally equal after rewriting
    //    are deduplicated (kept: the lowest hash)
    // 3. entries whose sole role was the subsumed pattern are removed
    // 4. emit MigrationReport as a new Artifact in the registry
}
```

The `MigrationReport` is itself content-addressed and enters the
Merkle DAG. Replay is straightforward — re-run the migration against
the pre-migration registry snapshot and verify the same hashes emerge.

## Phase E — Verification (post-migration)

After migration, a validation pass:

- Every original corpus expression still has an equivalent form under
  the new library (coverage must not decrease)
- No rule subsumed during migration had status `Verified` or
  `Exported` (subsumption of verified rules requires explicit
  promotion of the migration itself)

Failures here are alarms — they shouldn't happen if gates 1–5 did
their job, but if they do, the migration is rolled back and the
primitive flagged for human review.

## Demotion — the symmetric path

A primitive's usage is tallied across a sliding window W of epochs. If
the count falls below floor M, a `DemotionCandidate` event is emitted.
Demotion is **manual-approval** in v0 — the operator reviews:

- What would happen if we demoted? (the migration in reverse)
- Are there corpora where this primitive is still load-bearing?
- Did a recent promotion replace this primitive's role?

On approval: the primitive's variant becomes `#[deprecated]` in the
`Term` enum, its rewrite rule moves back to the library with status
`Demoted`, and dependent rules are re-expanded.

Demotion is rare. But without it the system calcifies.

## The PromotionSignal has its own content hash

Because a `PromotionSignal` is a decision event, it belongs in the
derivation DAG. `PromotionSignal::content_hash` is
`BLAKE3(canonical-sexpr(artifact_hash, subsumed_hashes,
cross_corpus_support, epoch_id))` — this is the hash axiom-forge
stores as the proposal's upstream identity, so every Rust primitive
has a chain back to the mathscape event that produced it.

## Replayability

The sequence `[Artifact₀, …, PromotionSignalₖ, MigrationReportₖ,
Artifactₖ₊₁, …]` in the registry is a complete record of the
mathscape's trajectory. Re-running it against the same corpus with
the same policy reproduces the same hashes. Re-running with a
different policy (different ε/K/N/M) produces a different but
equally-hash-chained trajectory — which is what makes the process
scientifically serious.
