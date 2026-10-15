# Reward Calculus — The Unified Metric

Reinforcement and discovery must be comparable. The same currency —
**ΔDL (change in total description length)** — measures every event
in the system. This is what lets the epoch controller decide whether
the next cycle should reinforce, discover, or promote.

See `condensation-reward.md` for the per-candidate reward shape, and
`axiomatization-pressure.md` for the reinforcement pass.

## One currency, every event

Every event in the system emits a `ΔDL` value. The currency is
**bits of total description length saved** (library + corpus).

| Event                                | ΔDL shape                                                                 |
|--------------------------------------|---------------------------------------------------------------------------|
| New rule accepted (Discovery)         | `(|L| + DL(C|L)) − (|L'| + DL(C|L'))` — MDL objective of `condensation-reward.md` |
| Status advance (Conjectured → Verified) | `log₂(|possible_unverified_rules_matching_lhs|)` — proof narrows the space |
| Status advance (Verified → Exported)  | `log₂(|rival_proof_systems|)` — external check eliminates implementation uncertainty |
| Merge (two rules → one)               | `size(subsumed) − size(bridge_links)` — library shrinks                    |
| Subsumption (rule absorbed by general one) | `size(absorbed_rule)` — library shrinks                               |
| Demotion (entry removed)              | `size(removed) − expected_recovery_cost` — only positive when recovery cost is low |
| Promotion (library → primitive)       | `size(subsumed_entries) − size(primitive_declaration)` — library contracts |
| Migration (library rewrite after promotion) | aggregated deduplication ΔDL                                           |

Each shape is a *reduction in description length under the
information-theoretic interpretation*. The system accepts any event
whose ΔDL > 0 and whose gates are cleared.

## The epoch's unified score

An epoch's total value is:

```
V(epoch) = Σ ΔDL(reinforcement events) + Σ ΔDL(discovery events)
```

Both sums are in bits. `V(epoch)` is the scalar the regime detector
tracks. Its slope across epochs determines regime transitions, not any
individual axis.

## The meta-choice: reinforce vs discover

### Expected value per unit compute

Let:
- `c_R` = compute cost of reinforcement pass — `O(|L|)` in library size
- `c_D` = compute cost of discovery pass — `O(|C| × |L|)` in corpus × library
- `E[ΔDL_R]` = expected ΔDL from next reinforcement pass
- `E[ΔDL_D]` = expected ΔDL from next discovery pass

### The switching rule

Run discovery iff:
```
E[ΔDL_D] / c_D > E[ΔDL_R] / c_R
```

Equivalently:
```
E[ΔDL_D] / E[ΔDL_R] > c_D / c_R ≈ |C|
```

So discovery must promise `|C|`-times more ΔDL than reinforcement to
justify its cost. That's a steep bar when the corpus is large — which
is why the default state is reinforcement.

### Estimating the expectations

Both expectations are **running averages over recent epochs**. The
controller keeps:

```rust
pub struct RewardEstimator {
    /// Mean ΔDL per reinforcement pass, last W epochs
    pub reinforce_mean: f64,
    pub reinforce_var: f64,
    /// Mean ΔDL per discovery burst, last W discovery events
    pub discover_mean: f64,
    pub discover_var: f64,
    /// Epochs since last discovery fired
    pub since_last_discovery: u64,
}
```

`E[ΔDL_R]` is `reinforce_mean`. `E[ΔDL_D]` is `discover_mean`
inflated by an exploration bonus: the longer since the last discovery,
the more the corpus has moved, so the higher the chance a new
discovery will fire.

```
E[ΔDL_D] = discover_mean × exploration_bonus(since_last_discovery)

exploration_bonus(k) = 1 + ρ × log(1 + k)    // ρ ∼ 0.1 to start
```

## Plateau detection

The reinforcement plateau is defined cleanly:

```
plateau :=
    (E[ΔDL_R] < ε_plateau)  AND
    (slope(reinforce_mean, last W epochs) < 0)
```

When plateau holds, the system runs a discovery pass regardless of the
switching rule (guaranteed-fire behavior). This prevents the
controller from getting stuck if the exploration_bonus miscalibrates.

## Three regimes expressed in the calculus

The regimes from `forced-realization.md`, reframed:

| Regime           | Dominant ΔDL source                           | What's happening                     |
|------------------|-----------------------------------------------|---------------------------------------|
| **Reductive**    | status advances + merges + subsumptions       | reinforcement does most of the work   |
| **Explosive**    | new-rule acceptances + coverage gains          | discovery burst fires and lands rules |
| **Promotive**    | promotion + migration ΔDL                     | library contracts around a new primitive |

Regime = `argmax(ΔDL source category over last W epochs)`.

## Why this is a reward calculus, not just a metric

Three properties make ΔDL act as a formal reward:

1. **Additive across events**: `V(epoch) = Σ event ΔDLs`. Each event
   contributes independently; sub-epoch credit-assignment is
   straightforward.
2. **Scale-invariant under log-transform**: `ΔDL` is measured in bits;
   doubling the corpus doesn't distort the ratios of different event
   kinds.
3. **Monotone in "real structure"**: any event that truly reduces
   description length must correspond to genuine structural
   simplification (by the information-theoretic definition). No event
   scores positive ΔDL by fake work.

Compressions cannot be gamed in this calculus *if* the coverage
constraint holds — see `condensation-reward.md`. Reinforcement events
cannot be gamed because the advances are proof-verified (e-graph, Lean).
Merges and subsumptions cannot be gamed because they require
equivalence of the merged rules.

So: ΔDL is not just *a* metric — it is *the* metric, and every
controller decision reduces to maximizing it under a compute budget.

## The allocator

With the calculus in hand, a compute allocator falls out:

```rust
pub struct EpochAllocator {
    pub policy: RealizationPolicy,
    pub estimator: RewardEstimator,
}

impl EpochAllocator {
    /// Choose the next action given budget.
    pub fn choose(&self, budget: ComputeBudget) -> EpochAction {
        if self.plateau_detected() {
            return EpochAction::Discover;                // guaranteed-fire
        }
        let r_val = self.estimator.reinforce_mean / self.c_r();
        let d_val = self.estimator.expected_discover()  / self.c_d();
        if d_val > r_val { EpochAction::Discover } else { EpochAction::Reinforce }
    }
}

pub enum EpochAction {
    Reinforce,                          // default
    Discover,                           // burst
    Promote(TermRef),                   // when a promotion signal cleared gates 4–5
    Migrate(PrimitiveIdentity),         // after axiom-forge accepted
}
```

The allocator is the *level-4 regime detector* of
`minimal-model-ladder.md`, made concrete.

## Calibration

Initial values (`v0` defaults):

| Parameter          | Default | Meaning                                |
|--------------------|---------|-----------------------------------------|
| `ε_plateau`        | 0.5 bit | plateau threshold for forced discovery |
| `ρ` (bonus coeff)  | 0.1     | how fast exploration bonus grows       |
| `W` (window)       | 20 epochs | averaging window                     |
| `c_r / c_d` ratio  | 1 / |C| | cost-of-discovery multiple             |

These are tunable per config. Later, they fit themselves (level 5 of
the ladder).

## What changes in the code

- `AcceptanceCertificate` already carries `compression_ratio`,
  `novelty`, `condensation_ratio`, `coverage_delta`. Add `delta_dl:
  f64` as the canonical bit-valued reward.
- `StatusAdvance`, merge-events, subsumptions each gain a `delta_dl`
  field.
- `EpochTrace` adds `total_delta_dl: f64` and breaks it down by source:
  `discovery_delta_dl`, `reinforce_delta_dl`, `promotion_delta_dl`.
- New `RewardEstimator` in `mathscape-core::realization` with
  `ewma_update(trace)`.
- New `EpochAllocator` wires the calculus into `Epoch::step`.

## Relation to the seven gates

ΔDL is the **continuous cousin** of the gate system:

- Gates are boolean filters (pass / fail)
- ΔDL is a scalar that *ranks* passing candidates

A candidate that clears gates 1–3 with higher ΔDL is prioritized over
one that barely clears. A reinforcement event with high ΔDL (status
advance on a high-use rule) is prioritized over a low-ΔDL one. The
system always spends compute on the highest-ΔDL-per-cost work
available.

## Replayability extends naturally

Because every ΔDL value is a deterministic function of the registry
state + policy + corpus, the entire sequence of `V(epoch)` values is
reproducible. Two runs under the same policy produce the same
trajectory in reward space. Two runs under different policies produce
differently-shaped curves — the diff is a concrete characterization
of policy impact.

This is what makes control tractable. The reward calculus is the
coordinate system in which the mathscape's motion is observed.
