# Condensation Reward — MDL with Coverage

The prover's composite score. See `forced-realization.md` for the
surrounding picture.

## Why compression alone is insufficient

A naive "maximize compression ratio" reward has two attack surfaces:

1. **The universal-vacuous-rule attack**: a candidate with a pattern
   like `(?f ?x)` matches everything but compresses nothing — it hides
   complexity in the pattern variables. Compression ratio (measured as
   rewritten-node-count / original-node-count) reports success; no real
   structure was found.
2. **The silent-coverage-loss attack**: a candidate that matches half
   the corpus and "compresses" by narrowing what the library can reach.
   Description length drops — because the library now covers less.

Both attacks succeed under pure compression reward. Both fail under
MDL with coverage preservation.

## The MDL objective

The reward is the change in total description length across library
*and* corpus:

```
reward = ΔDL
       = (|L|_old + DL(C | L_old)) − (|L|_new + DL(C | L_new))
       = (library shrinkage) + (corpus compression)
         subject to: coverage_delta(L_old, L_new) ≥ 0
```

Where:
- `|L|` is the total size of library definitions (you pay for the
  abstractions you create)
- `DL(C|L)` is the corpus size after rewriting with the library
- `coverage_delta` counts matches preserved minus matches lost

This is the Rissanen / Solomonoff shape. The optimum is the *shortest
library that compresses the corpus without losing coverage*.

## Three orthogonal axes

The prover decomposes `ΔDL` into three axes so downstream policy can
reason about each independently:

| Axis                       | Field on `AcceptanceCertificate` | Shape                                         |
|----------------------------|----------------------------------|-----------------------------------------------|
| **Library shrinkage**       | `condensation_ratio`             | `(|L|_old − |L|_new) / |L|_old`, ∈ [0, 1]     |
| **Corpus compression**      | `compression_ratio`              | `1 − DL(C|L_new) / DL(C|L_old)`, ∈ [0, 1]     |
| **Coverage preservation**   | `coverage_delta`                 | `matches_new − matches_old`, ∈ ℤ; must ≥ 0   |

Plus a fourth, *derived*, for dimensional discovery:

| **Meta-compression**       | `meta_compression`               | compression of the library *itself* when its rhs terms are treated as the corpus — flags when the library starts referring to itself |

All four must be individually reported. The prover's scalar `score` is
a regime-dependent weighted sum; the raw axes are kept so policy can
re-weight without re-running.

## Why this prevents gaming

| Attack                                    | Blocked by                                      |
|-------------------------------------------|-------------------------------------------------|
| Universal vacuous rule                    | `coverage_delta ≥ 0` alone isn't enough — but combined with `condensation_ratio > 0` and `compression_ratio > ε`, a vacuous rule contributes nothing to either |
| Silent coverage loss                      | `coverage_delta ≥ 0` rejects it outright        |
| Invented redundancy                       | `condensation_ratio` requires library shrinkage; adding new entries without removing old ones yields 0 |
| Local-only theorem dressed as universal   | `cross_corpus_support` (on `PromotionSignal`, not on the cert) gates promotion to primitive |

The prover enforces gates 1–3. Gates 4–5 use the cross-corpus history.

## Scale invariance

Every axis is a ratio (or a delta normalized against library size). A
rule that halves a 4-entry library vs halves a 100-entry library
scores the same `condensation_ratio = 0.5`. This is load-bearing: early
epochs with small libraries must not dominate the score schedule
permanently.

## Why `AcceptanceCertificate` keeps the raw axes

Downstream uses:

1. **Regime detection** reads the axes to classify the epoch (rising
   compression → Exploration; rising meta-compression → Promotion)
2. **Promotion gate** uses `condensation_ratio` separately from
   `cross_corpus_support` — both must cross thresholds independently
3. **Operator introspection** — a policy maker sees *why* a rule was
   accepted, not just *that* it was

## Implementation location

- Computed in `mathscape-reward::compute_reward_axes(corpus, L_old, L_new) -> RewardAxes`
- Assembled into `AcceptanceCertificate` by the prover in
  `mathscape-reward::StatisticalProver`
- Gate thresholds ε, coverage floor, are fields on
  `RealizationPolicy` (config)

## Open questions (not yet closed)

1. **Weighting the axes per regime.** Fixed weights work for v0
   (see `RealizationPolicy.regime_policy`). Adaptive weights are the
   top of the minimal-model ladder.
2. **Meta-compression depth.** Applying the library to itself is order-
   sensitive. `meta_compression` should be the *fixed point* score.
   Approximation: apply up to 3 iterations, stop at plateau.
3. **Coverage granularity.** Counting matches is coarse. A finer
   signal: match-weighted by match-size. Reserved for v0.1 — the
   coarse version is enough to defeat the two named attacks.
