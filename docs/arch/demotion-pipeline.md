# Demotion Pipeline — the Symmetric Path

Promotion is documented in `promotion-pipeline.md`. This document
specifies the **symmetric path**: how a Rust primitive falls back
to a library entry, and how a library entry falls out of the active
lifecycle entirely.

Without demotion the system calcifies. Bad early promotions would
trap all downstream work. With demotion the machine can reorganize.

## Two distinct demotion events

| Event                    | From state             | To state  | Trigger                                                 |
|--------------------------|------------------------|-----------|---------------------------------------------------------|
| **Library demotion**     | `Conjectured`/`Verified`/`Exported`/`Axiomatized` | `Demoted(reason)` | W epochs without status advance; or usage below M  |
| **Primitive demotion**   | `Primitive(identity)`   | `Demoted(RetiredPrimitive)` | usage across all corpora below M for W epochs |
| **Subsumption** (adjacent) | any active state      | `Subsumed(hash)` | a more-general rule subsumes this one (symmetric to merge) |

All three use the same sink statuses. The differences are in
trigger and in the reverse-migration cost.

## Library demotion (cheap)

A library entry sitting at `Conjectured` for W epochs without
advancing is probably noise. The reinforcement pass tracks:

```rust
pub struct ReinforcementMetadata {
    /// Epoch the entry entered its current status.
    pub status_since: u64,
    /// Usage tally in the rolling W-epoch window.
    pub usage_in_window: u64,
}
```

After each epoch:

- `now - status_since > W_usage_window` AND `usage_in_window < M` →
  emit `Event::Demote { reason: StaleConjecture }` (or `UnusedExport`
  depending on which status the entry sat at)
- The entry's `ProofStatus` transitions to `Demoted(reason)`
- The registry keeps the entry (append-only) but excludes it from
  future reinforcement and discovery context

Library demotion is **automatic** — it does not require operator
review. The cost of being wrong is cheap: if the corpus rotates and
the demoted rule becomes useful again, a future epoch can re-propose
it (it will enter at `Proposed` and start over).

## Primitive demotion (expensive)

A `Primitive` variant that stopped being used is a different story.
Demoting it means:

1. The Rust enum variant must be marked `#[deprecated]` (emission
   change in axiom-forge)
2. Every downstream crate that matched on the primitive gets a
   deprecation warning at the match site
3. A reverse migration re-expands the primitive's rewrite rule back
   into the library as a `Demoted(RetiredPrimitive)` entry
4. Future builds that upgrade past the `#[deprecated]` grace period
   will fail to compile until the variant is removed

This is **operator-gated** in v0 via:

```rust
pub struct DemotionCandidate {
    pub primitive: AxiomIdentity,
    pub usage_history: UsageWindow,
    pub cross_corpus_history: Vec<CorpusUsage>,
    pub reverse_migration_plan: ReverseMigrationPlan,
    pub content_hash: TermRef,
}
```

The operator reviews:

- How much downstream code matches on this variant today?
- What corpora still exercise it? Is there recovery evidence?
- Is the reverse migration correct (does re-expanded rule hold
  coverage)?

On approval, the bridge:

1. Asks axiom-forge to emit a `#[deprecated(since = "...", note = "...")]`
   shim of the existing variant (new emission, old proposal hash
   preserved for chain integrity)
2. Re-inserts the primitive's expansion into the library with
   status `Demoted(RetiredPrimitive)`
3. Emits a `ReverseMigrationReport` into the registry

## Subsumption (semi-automatic)

Subsumption is not technically demotion (the entry remains addressable
via its `Subsumed(subsumer_hash)` status pointing at the winner).
Mechanics:

- During reinforcement, every pair `(a, b)` is checked for
  subsumption — is every match site of `a` also matched by `b`
  (with b's pattern variables consistently bound)? If yes, `a` is
  subsumed by `b`.
- The entry stays in the registry. Its status becomes
  `Subsumed(b.content_hash)`. Coverage does not decrease because `b`
  covers everything `a` covered.
- Subsumption is automatic; operator approval is not required
  because the subsumer is provably at least as general.

Subsumption and demotion differ in one critical respect:
**subsumption preserves coverage; demotion may not.** That is why
demotion below primitive level requires either (a) W-epoch silence
AND usage below M (automatic because the signal itself is "nothing
depends on this") or (b) operator approval (for the risky primitive
case).

## The ReverseMigrationReport

Mirrors `MigrationReport` but runs the other direction:

```rust
pub struct ReverseMigrationReport {
    pub demoted_primitive: AxiomIdentity,
    pub re_expanded: Vec<TermRef>,        // library entries restored
    pub call_sites_updated: Vec<String>,  // downstream files / functions
    pub deprecation_emitted: AxiomIdentity, // the new #[deprecated] shim
    pub epoch_id: u64,
    pub content_hash: TermRef,
}
```

Entered into the registry as an Artifact. The derivation DAG
therefore records demotions as first-class, replayable events — a
different policy could choose not to demote, producing a visibly
different trajectory from the same corpus.

## When demotion fires vs when it doesn't

**Fires automatically:**

- `Conjectured` entries idle for W epochs with zero coverage
  contribution
- `Verified`/`Exported`/`Axiomatized` entries with usage below M for W
  (these entries exist but aren't doing work)
- Subsumption by a more-general rule (symmetric to merges)

**Fires only with operator approval:**

- Primitive demotion (migration cost across downstream crates is
  real)
- Demotion of a rule that has `ProofStatus::Exported` and an
  external Lean proof (we don't want to lose proven theorems just
  because they became unfashionable)

**Never fires (invariant):**

- A rule whose subsumer is itself `Subsumed` or `Demoted`
  (would create a dangling pointer)
- The last rule providing coverage for some corpus fragment
  (coverage floor is a hard constraint)

## Calcification detection

How do we know the demotion force is *working*, not just theoretical?
Two indicators:

1. **Demotion-to-promotion ratio over time**. If promotion is 20:1
   over demotion for > 1000 epochs, either the machine is genuinely
   discovering great primitives, or demotion is broken. The operator
   should periodically audit.
2. **Library-size trajectory**. A healthy mathscape shows a library
   size that grows, plateaus, contracts (via promotion-migration or
   demotion), then grows again. If the curve is monotonically
   increasing for > W epochs, calcification may be setting in. This
   is a `Regime::Calcified` warning — not a regime for normal
   operation but a diagnostic one.

## Interaction with the knowability claim

Demotion is what makes the knowability claim *honest*. A system
that only grows is a system whose axiom set accumulates noise, and
the "every capability is proven by construction" claim becomes a
statement about a growing pile with no reverse pressure. With
demotion:

- Every primitive currently in `Primitive` status was demonstrably
  used across W epochs; it didn't get grandfathered in.
- Every primitive currently `Demoted(RetiredPrimitive)` was
  demonstrably *not* used; the system admits the promotion was wrong
  and reverses it.
- The *active* axiom set is always the set that passes all gates
  *today*, not the set that passed them once in the past.

This is what makes the forced-realization machine self-correcting
rather than only self-extending.

## Phase alignment

| Demotion capability            | Phase in `realization-plan.md` |
|--------------------------------|-------------------------------|
| Library demotion (automatic)    | **Phase J** (Demotion path)    |
| Subsumption detection           | **Phase G** (rolled in with promotion gates) |
| Primitive demotion (manual)     | **Phase J**                   |
| Reverse migration execution     | **Phase J**                   |
| Calcification diagnostic regime | **Phase L** or later           |

## Summary

Demotion is the symmetric force that makes the forced-realization
machine convergent rather than just cumulative. Library demotion is
cheap and automatic. Subsumption is a safe side-channel. Primitive
demotion is expensive, operator-gated, and rare by design — but it
*must* exist, or the axiom set calcifies.

The existence of the demotion path is what lets the `knowability`
claim hold indefinitely, not just at promotion time.
