# Minimal-Model Ladder

"Whatever is the simplest to get to the next step." — the user's
stated objective for stepping through the mathscape.

Each regime has a cheapest learning mechanism that advances it. The
ladder below is *strict*: use the lowest level that still makes
progress; climb only when progress plateaus.

## The principle

A learning model is **minimal for a regime** if:

1. it converts regime-relevant signal into regime-relevant action
2. no simpler mechanism can do (1)
3. a simpler mechanism suffices for all lower-complexity regimes

The mathscape bootstrap gains nothing from adding a transformer to
arithmetic discovery. Random mutation suffices. Introducing the
transformer at the wrong rung is negative-sum: the training cost
dwarfs the signal it extracts.

## The levels

| Level | Mechanism                         | Params | Regime it advances                | Signal consumed                              |
|-------|-----------------------------------|--------|-----------------------------------|----------------------------------------------|
| 0     | random mutation + tournament      | 0      | Exploration (early)               | fitness                                      |
| 1     | anti-unification                  | 0      | Exploration (mid)                 | corpus structure                             |
| 2     | MAP-Elites archive                | 0      | Exploration (diversity)           | behavioral bins                              |
| 3     | heuristic novelty (irreducibility)| 0      | Exploration → Consolidation       | library membership                           |
| 4     | regime detector (finite-state)    | ~5     | all                               | CR slope, library size, top-entry distribution |
| 5     | adaptive gate thresholds          | ~20    | Consolidation                     | rolling library statistics                   |
| 6     | RL policy for mutation bias       | ~10k   | Exploration → Consolidation       | past-epoch reward trajectories               |
| 7     | neural proposer for symbols       | ~1M    | Consolidation → Promotion         | library + corpus joint embedding             |

Levels 0–3 are *already in mathscape's code*. Levels 4–5 are the next
mechanical step (see `realization-plan.md` Phase E). Levels 6–7 are
deferred until 4–5 demonstrably plateau.

## Level-by-level rationale

### Level 0 — Random mutation + tournament
No signal beyond fitness. Sufficient when the corpus is small and
primitive operators are cheap to explore combinatorially. Complexity
`O(pop_size × tree_size)` per epoch. Ceiling: once the population
diversity collapses, progress stops.

### Level 1 — Anti-unification
Structural. Finds common subtrees across a corpus; no parameters.
Sufficient when shared structure exists *above the noise*. Ceiling:
can't find structure that requires reordering or hypothesis about
hidden variables.

### Level 2 — MAP-Elites archive
Keeps diversity by partitioning the population into behavioral bins
(depth, op-diversity, compression contribution). No parameters.
Sufficient when collapse is the failure mode, not missed structure.

### Level 3 — Heuristic novelty / irreducibility
Rejects rules derivable from existing library entries via e-graph.
No parameters (the e-graph itself has configuration but not trained
weights). Ceiling: novelty gets expensive when library is large —
every proposal is saturated against every existing rule.

### Level 4 — Regime detector
A tiny finite-state machine on [CR slope, library growth rate,
top-entry corpus support]. Emits `Regime::{Exploration, Consolidation,
Promotion}`. Fewer than 10 parameters; can be hand-tuned or fit by
bisection on a held-out trajectory. This is the *cheapest* learning
model — use it first before anything with gradients.

### Level 5 — Adaptive gate thresholds
ε, K, N fit as linear functions of [library_size, epoch_number,
rolling_cr]. ~20 parameters. Trained by: simulate K epochs under a
grid of thresholds, pick the schedule that maximizes reach-per-compute
on a validation corpus. Still no neural nets.

### Level 6 — RL policy for mutation bias
Policy gradient (REINFORCE) over the mutation operator set. State =
expression tokens, action = which mutation to apply. Small MLP (~10k
params). Justified only when level-5 gate adaptation is insufficient
— i.e., when the bottleneck is *search inefficiency*, not *gate
policy*.

### Level 7 — Neural symbol proposer
An encoder over the library + corpus that predicts likely lhs/rhs
pairs. Transformer or graph NN. ~1M params. Justified only if level-6
RL policy is insufficient — typically when the Promotion regime
requires proposing patterns that reference multiple library entries
at once (cross-domain dimensional discovery). Training data is the
derivation DAG: every accepted Artifact is a positive, every rejected
`PromotionSignal` is a negative.

## Climbing the ladder

Rule: **never climb without evidence the current level has
plateaued**. Plateau signal: compression ratio has been within
± 2 % for ≥ 50 epochs *and* no promotion signals have fired.

The ladder itself is subject to demotion. If level 6 fails to
outperform level 5 after a budgeted evaluation, it's removed from
the policy — level 5 is the floor that cannot be demoted (it is the
gate-threshold machinery, not a search heuristic).

## What this is NOT

Not a recommendation for more complex ML. The opposite. The ladder's
purpose is to ensure we don't reach for a transformer when
anti-unification suffices. Each rung is justified by regime transition,
not novelty appetite.

## Connection to forced realization

The ladder answers: *given a regime, what mechanism minimally
advances the gate state of the most-promising artifacts?* The four
forces + seven gates define *what* must happen. The ladder defines
*how* — with the smallest model that produces motion.
