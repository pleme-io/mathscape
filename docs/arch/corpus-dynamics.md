# Corpus Dynamics — ΔDL When the Corpus Is Not Fixed

`reward-calculus.md` defines ΔDL against a fixed corpus. Real
mathscape deployments face corpora that **grow, drift, and rotate**.
This document closes the gap: how the reward calculus behaves when
the corpus itself is in motion.

## Three modes of corpus change

| Mode            | Description                                         | ΔDL interpretation                                  |
|-----------------|-----------------------------------------------------|-----------------------------------------------------|
| **Growth**       | new workloads added to an existing domain corpus    | per-epoch ΔDL is computed on the *current* corpus snapshot; growth is accepted as a new baseline |
| **Drift**        | existing workloads change (new variables, updated versions) | retained rules re-checked against drifted workloads; rules that lose coverage get demoted |
| **Rotation**     | corpus swapped for an entirely different domain     | cross-corpus tally (gate 5) accumulates; rules that survive rotation become promotion candidates |

All three are legal motions. The machine accepts them without
special-casing, *provided* ΔDL is computed against snapshots, not
running totals.

## Snapshot semantics

Each epoch fixes a **CorpusSnapshot**:

```rust
pub struct CorpusSnapshot {
    pub id: CorpusId,
    pub epoch_id: u64,
    pub content_hash: TermRef,    // hash of the snapshotted expression set
    pub terms: Vec<Term>,
}
```

ΔDL is computed as:

```
ΔDL(epoch k, snapshot Sₖ) =
    (|L_{k-1}| + DL(Sₖ | L_{k-1})) − (|L_k| + DL(Sₖ | L_k))
```

Notice Sₖ is used for **both** the old-library and new-library DL.
The corpus snapshot is shared across the "before" and "after" of one
epoch. This makes ΔDL monotonic in epoch work even when the corpus
changes between epochs.

## Drift — when old coverage claims break

A rule that matched `(add ?x zero)` 40 times in last epoch's
snapshot may match only 10 times in this epoch's (because the corpus
drifted). The reinforcement pass detects coverage loss:

```rust
if usage_in_window(rule) < rule.prior_usage / 2 {
    // warn: coverage cliff
}
```

A rule whose coverage collapses beyond a threshold is a **drift
casualty**. It doesn't get demoted automatically — drift may be
temporary. Instead the rule's `ReinforcementMetadata.status_since`
is reset, starting a new W-epoch observation window. If coverage
does not recover within W, *then* demotion fires.

This is the right policy because:

- Corpus drift is expected (workloads evolve)
- Transient drift shouldn't cost a Verified proof status
- Persistent drift should cost it — the rule no longer describes
  reality

## Rotation — cross-corpus as a first-class signal

Rotation is the friendliest mode: it's gate 5's raison d'être. When
the corpus rotates from arithmetic to combinator calculus, rules
that match in *both* accumulate cross-corpus support. The machine
doesn't need to know the corpus has rotated; it just tallies.

Implementation:

- Registry tracks `Vec<CorpusUsage>` per active artifact
- `CorpusUsage { corpus_id, epochs_matched, first_match_epoch, last_match_epoch }`
- Gate 5 reads this directly

Operators **should rotate corpora** as a matter of policy. Running
mathscape against a single corpus indefinitely produces lots of
condensation but no cross-corpus support, so promotion never fires.
A minimal rotation schedule (e.g., alternate corpora every 100 epochs)
is sufficient to exercise gate 5.

## Growth — the baseline-shift problem

When new workloads are added to an existing corpus, ΔDL across
epochs is no longer directly comparable:

- Epoch 100: `|C_100| = 1,000`, compression produces `ΔDL = 50 bits`
- Epoch 101: `|C_101| = 5,000`, compression produces `ΔDL = 200 bits`

The second epoch looks 4× better but it is operating on 5× more
corpus. The regime detector must normalize:

```
ΔDL_normalized = ΔDL / |Cₖ|
```

And regime transitions use the normalized slope, not the raw slope.

## Unbounded corpora

A truly unbounded corpus (streaming training data) does not permit
full DL computation per epoch. Approximations:

1. **Sliding window**: compute DL over the last N terms seen
2. **Reservoir sampling**: maintain a fixed-size representative sample
3. **Hash-binning**: cheap approximate coverage via rolling Bloom
   filters

All three preserve the ΔDL sign (whether an epoch improved things)
while losing precision in the magnitude. The allocator still works:
it compares expected ΔDL across actions, not absolute values.

v0 does **not** implement any of these. v0 runs on fixed snapshots
with explicit rotation. Streaming support is a Phase L+ concern.

## Invariants preserved across corpus dynamics

Regardless of growth/drift/rotation:

1. Every Event still carries a ΔDL (measured against the epoch's
   snapshot)
2. The registry is still append-only — prior snapshots stay
   referenced via their content hashes
3. Replay is still deterministic — given the same corpus-snapshot
   sequence + the same policy, the same trajectory results
4. Regimes still detect correctly — they use normalized ΔDL when
   corpus size varies

The key insight: **corpus change is a system input, not a corruption**.
The machine treats a rotated corpus like a new epoch the same way
it treats a new proposal — one more Event, same calculus.

## What this adds to the policy

```rust
pub struct RealizationPolicy {
    // ... existing fields ...

    /// How to handle coverage cliffs due to drift.
    pub drift_tolerance_W: u64,      // epochs before demotion fires
    /// Minimum corpus snapshots per rotation window.
    pub rotation_min_epochs: u64,
    /// Normalization factor denominator.
    pub normalize_by_corpus_size: bool,
}
```

## Action items

None for v0. Document exists to confirm the calculus holds under
realistic corpus dynamics. When Phase K (multi-corpus) lands, these
semantics are already specified.

## Consequence for Phase B

Phase B does not need to implement snapshot semantics explicitly. The
existing `corpus: &[Term]` argument to `Epoch::step` is implicitly a
snapshot. When Phase K arrives, the argument becomes
`corpus: &CorpusSnapshot`; existing code signatures change cleanly.
