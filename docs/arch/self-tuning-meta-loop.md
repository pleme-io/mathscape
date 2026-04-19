# Self-Tuning Meta-Loop

The machine observes its own learning, leverages that observation to
author its own next training recipe, and executes in complete
linguistic isolation. The system sails itself.

Status: **Phase W landed, 2026-04-18.** The perpetual self-optimizing
streaming trainer now composes with an EventHub pub/sub spine;
`publish_outcome_events` translates motor outcomes into MapEvents that
fan out to every subscribed consumer. RigL phantom gradients, EWC
Fisher-weighted stability, learning-progress intrinsic reward, and
corrupted/stalled pruning all operate continuously on the live event
stream. 895 workspace tests pass.

Earlier status: **Phase V landed, 2026-04-18.** The motor runs end-to-end on a
real derive-laws extractor. Built on R26 (BootstrapCycle), R32
(Sexp-describable spec), R33 (ExperimentScenario chain), R34-R35
(wall-clock observability), R37/R39 (opt-in work elimination),
Phase I (subterm-paired AU), Phase J (empirical validity), Phase H
(rank-2 inception), Phase U (meta-loop), and now **Phase V**
(diet-as-action via `AdaptiveCorpusGenerator` +
`LearningObservation.staleness_score()` +
`SpecArchetype::AdaptiveDiet`).

**Observed motor behavior** (pinned by `fix_point_motor_runs_and_visibly_mutates_diet`):

```
 phase  scenario                          archetype  lib  growth   δπ     stale
   0    seed-default (default)            SEED        4     4     0.37   0.11
   1    proposed-baseline (default)       BASE        4     0     0.02   0.91
   2    proposed-extended-discovery       EXT         4     0     0.01   0.96
   3    proposed-adaptive-diet (adaptive) DIET        5     1     0.44   0.09   ← MOTOR FIRED
   4-9  proposed-adaptive-diet (adaptive) DIET        5     0     0.00   0.98
```

Phase 3 is the payoff: staleness crossed 0.6, proposer picked
AdaptiveDiet, library grew to 5 rules. The 5th rule is one the
default corpus cannot reach.

## The four directives

Three user framings, one week apart, read as one continuous arc:

1. **"Observe what we are learning and maximally leverage creating
   systems and models that create systems and models to leverage all
   of that at optimal depth and dimension to create ever more efficient
   models at discovering math naturally."**
2. **"While letting the model tune its own training along the way and
   let the system carefully sail out since it is self encapsulating."**
3. **"We can probably leverage lisp/wasi/wasm type techniques to truly
   virtualize in a completely linguistic isolation and freedom."**

All three collapse into the same mechanism: the machine produces a
description of its next training step AS A LISP VALUE, observes the
outcome AS A LISP VALUE, and refines. The closure is the loop.

## What we've learned (Phase T observations)

Concrete findings from R34-R39 that shape Phase U's design:

- **Work elimination beats work acceleration.** R37's 1.8× skip-plateau
  dwarfed R38's 9% micro-opts. Per-phase plateau detection compounds
  multiplicatively: R39's 4-phase scenario saw 3.97×.
- **Libraries saturate within 1-2 iterations under CanonicalDeduper.**
  Default M0 adds 3 rules at iter 0, 1 at iter 1, 0 thereafter. The
  scenario that runs 4 phases of 5 iterations each does real work in
  ~6 of 20 iterations; the remaining 14 are pure waste the machine
  doesn't yet KNOW it's doing.
- **Meta-patterns collapse unless the base is shape-diverse.**
  `rank2_inception_probe` surfaces ONE meta-rule (S_10000) because
  every concrete identity discovered falls into the same equivalence
  class. Phase I is the mechanism for surfacing shape-orthogonal
  candidates; Phase J would certify them.
- **Measurement precedes optimization.** R34/R35 revealed the real
  bottleneck (`paired_anti_unify` at 92%) that was NOT the obvious
  suspect (eval). R36's cache looked good on paper; measurement said
  no at this scale.
- **Determinism is load-bearing.** Every gain preserved attestation.
  This lets every observation be a repeatable experiment.

The machine is **efficient at doing known work**, **blind to its own
saturation**, and **singular in its convergence**. Phase U turns each
into an opportunity.

## The meta-loop

```
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│   Sexp: (experiment :name "..." :phases ((...)))             │
│                      │                                       │
│                      ▼                                       │
│            execute_scenario_core                             │
│                      │                                       │
│                      ▼                                       │
│   Sexp: (outcome :library (...) :policy (...) :obs (...))    │
│                      │                                       │
│                      ▼                                       │
│            LearningObservation                               │
│                      │                                       │
│                      ▼                                       │
│            ScenarioProposer                                  │
│                      │                                       │
│                      ▼                                       │
│   Sexp: (experiment ...) ← next phase                         │
│                      │                                       │
│                      └──── loop closes                       │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

Everything inside the box is a pure Lisp value. Execution is Rust.
The loop is a function `Sexp → Sexp` — input describes the next
training step, output describes what was learned. Composing this
into itself is the self-tuning property.

## Components

### LearningObservation (ship next)

A typed projection of an `ExperimentOutcome` into the fields the
proposer needs to decide. Lives on `mathscape-core/src/bootstrap.rs`.
Sexp-roundtrippable via a sibling `observation_to_sexp` /
`observation_from_sexp` pair (in mathscape-proof, parallels the
R32/R33 bridges).

Fields (candidate set; first pass):

- `total_library_size` — how many rules the scenario grew to
- `net_growth_per_phase` — where discovery happened vs plateau
- `saturation_phase_index` — first phase that added 0 rules
- `extract_ns_per_iteration` — where wall-clock goes
- `meta_rule_equivalence_classes` — count of distinct shapes
  (requires meta-rule detection plumbing from Phase H's gate)
- `trained_policy_delta_norm` — how much the model weights moved
- `dimensions_explored` — what feature axes the library moved on

Attestation: observation ⊆ outcome, so observation hash is
derivable from outcome hash. Not independently attested; observations
are views, not artifacts.

### ScenarioProposer trait (ship after)

```rust
pub trait ScenarioProposer {
    fn propose(
        &self,
        history: &[LearningObservation],
        current_policy: &LinearPolicy,
    ) -> ExperimentScenario;
}
```

Default impl: use `current_policy.score(&feature_state)` to rank a
catalog of candidate next-specs. Pick the top-K, chain them.
Fallback when the policy has seen too little: round-robin through
the catalog.

Seed catalog (initial primitives for the proposer to choose from):

- increase `n_iterations` when growth per iteration is high
- decrease when plateau is early
- enable `subterm_au_depth` when meta-equivalence-class count == 1
- swap deduper (Canonical ↔ Subsumption) when candidates are being
  rejected at a rate suggesting wrong granularity

### Everything is Sexp (already true)

`BootstrapCycleSpec` → Sexp (R32). `ExperimentScenario` → Sexp
(R33). `LinearPolicy` → Sexp (R10). What's missing:

- `LearningObservation` → Sexp (Phase U.1 — ship next)
- `ScenarioProposer` catalog → Sexp (Phase U.2)

Then the entire loop is a closed Sexp pipeline. The Rust core is a
single function `(Sexp, Sexp) → Sexp` — (scenario, observation_history)
→ next_scenario. Pure, deterministic, attestable.

### Linguistic isolation via Lisp/WASI/WASM

The natural execution target for `(Sexp, Sexp) → Sexp` is a sandboxed
WASM module:

```
╔════════════════════════════════════════════╗
║  Host (Rust, mathscape-core executor)      ║
║                                            ║
║  ┌──────────────────────────────────────┐  ║
║  │  WASI module: tatara-lisp interpreter│  ║
║  │                                      │  ║
║  │  input:  (experiment ...) Sexp       │  ║
║  │  output: (outcome ...) Sexp          │  ║
║  │                                      │  ║
║  │  capabilities: NONE (pure function)  │  ║
║  └──────────────────────────────────────┘  ║
║                                            ║
╚════════════════════════════════════════════╝
```

The WASM sandbox gets:
- no filesystem access
- no network
- no clock (beyond what the host injects)
- no syscalls beyond the input/output channel

The module is a `tatara-lisp` interpreter that evaluates the Sexp
program. Everything mathscape does at the semantic level — corpus
generation, anti-unification, law discovery, training — is
expressible as Lisp code. A WASM build of the interpreter gives us
complete linguistic isolation: the machine's training runs in a
universe where the only primitives are `cons`, `car`, `cdr`,
`lambda`, and whatever tatara-lisp exposes for Term manipulation.

Benefits:
- **Deterministic by construction**: WASM has no nondeterminism
  (modulo the host's input). Two hosts running the same scenario
  on the same hardware produce bit-identical outputs.
- **Portable**: browser, edge, cluster, any WASI runtime
- **Cheap to snapshot**: WASM linear memory is a single blob;
  checkpoint/resume is memcpy
- **Provably bounded**: memory + cycle limits at the runtime level
- **Security**: a compromised recipe can't exfiltrate data; it
  only has stdout = Sexp output

The tatara-lisp → WASM path is the natural next major architecture
move. Not this session — it's multi-day work. But the prior
artifacts already map cleanly:

| Already exists           | WASM equivalent                   |
|--------------------------|-----------------------------------|
| Sexp spec/scenario form  | WASM module input                 |
| Sexp policy/outcome form | WASM module output                |
| Rust executor            | Host runtime invoking the module  |
| BLAKE3 attestation       | Hash of (input, module, output)   |
| Term type                | tatara-lisp Term primitive        |

## Sail-out semantics

"Let the system carefully sail out since it is self encapsulating"
describes the termination condition. The meta-loop runs as long as:

- `LearningObservation::net_growth_per_phase.last()` > 0 (still
  discovering)
- OR `dimensions_explored` keeps expanding (novelty escape)
- OR `trained_policy_delta_norm` > epsilon (still refining)

When ALL three stabilize, the loop has reached a stable attractor
and the session closes naturally. This is phase L5's "edge-riding":
a sufficiently-rich substrate never truly stops producing novelty,
so the sail-out criterion is "no growth for K phases" where K is
the operator's tolerance. The machine WILL keep finding things if
pushed; the user decides when the current voyage is over.

## What ships in Phase U

Sequenced for landing in order of independence:

1. **U.1: `LearningObservation` struct + Sexp bridge** (today)
   Captures post-scenario state in typed form. Foundation.

2. **U.2: `ScenarioProposer` trait + default impl** (soon)
   Defines the proposer seam. Default uses the trained LinearPolicy
   to score candidate specs from a small seed catalog.

3. **U.3: `MetaLoop::run(seed_scenario, proposer, max_phases)` helper**
   The outer self-tuning loop. One phase per tick: execute → observe
   → propose → execute. Terminates on sail-out criterion.

4. **U.4: Meta-loop Sexp bridge** — the whole loop is one Sexp value

5. **U.5: WASM-executable tatara-lisp variant** (long-horizon)
   The linguistic-isolation substrate. Existing Sexp machinery
   becomes the module boundary; the interpreter becomes the sandbox.

## Invariants Phase U must preserve

- **Lynchpin**: every rule in the library of any spawned cycle
  earns ≥2 corpus cross-support (the autonomous-traversal lynchpin)
- **Determinism**: replaying the same seed scenario produces the
  same outcome + observation + proposer output + next scenario
- **Attestation**: every artifact hashed; meta-loop attestation is
  BLAKE3 over the chain of observations + proposed scenarios
- **Self-encapsulation**: no layer reads from outside its inputs;
  the proposer sees only observations, not raw scenario internals

## What Phase U doesn't do

- Does NOT change the discovery mechanism (Phase I / J / K do that)
- Does NOT add new primitives (R13-R20 compute layer stays)
- Does NOT replace the BootstrapCycle (it wraps it)
- Does NOT require Phase H to work (rank-2 inception is orthogonal)

Phase U is the **orchestrator over what already exists**. The model
already tunes (trajectory → LinearPolicy). The system already
encapsulates (trait seams + Sexp I/O). The sail-out is the loop
that ties them together.

## Phase V extensions (2026-04-18): map, events, certify, stream, benchmark, shed

The V.1–V.5 landmarks above (staleness signal, adaptive corpus,
adaptive-diet archetype, proposer branch, motor) closed the
fix-point loop *at the scenario level*. The Phase V extensions
that followed close a second, faster loop: the **proprioceptive
loop** the streaming policy runs at the granularity of single
events instead of whole phases.

### §V.map — the machine has a map of itself

`MathscapeMap` is the first typed view of the library as a whole.
It partitions rules into `core_rules` (apex, highest-certification)
and `union_rules` (remaining support). It tracks `mutation_edges`
between rules, `seed_info` metadata, per-rule cross-corpus support
counts, and a BLAKE3 Merkle root over canonical rule serialization.
The map is *attestable* (Merkle root changes iff rule set changes),
*persistable* (`save_to_path` / `load_from_path`), and *observable*
via `MapSnapshot` + `MapSummary`.

### §V.events — the machine narrates itself

`MapEvent` is a typed event bus with 7 variants:

- `NovelRoot` — a new primitive term encountered at a corpus root
- `RootMutated` — an existing root re-entered a new reduction path
- `CoreGrew` — a rule crossed the apex threshold
- `StalenessCrossed` — the staleness signal passed a configured bound
- `RuleCertified` — a rule advanced on the certification lattice
- `RuleRejectedAtCertification` — evidence insufficient; demoted or dropped
- `BenchmarkScored` — the machine ran its report card

`MapEventConsumer` is a trait with a single `consume(&MapEvent)`.
Every downstream component (buffered history, certifier, streaming
trainer, benchmark runner) implements it — the consumers compose
into a chain.

### §V.certify — reactive rule promotion

`CertificationLevel` is a 5-state ladder:

```
Candidate → Validated → ProvisionalCore → Certified → Canonical
```

`CertifyingConsumer` watches the event bus, routes every rule-
relevant event through a `Certifier` trait (`DefaultCertifier`
implements the default cross-corpus-support policy), and emits
`RuleCertified` / `RuleRejectedAtCertification` downstream. Unlike
the batch-style lifecycle advancement of autonomous-traversal,
certification here is *reactive*: each new empirical observation
is immediately weighed.

### §V.stream — never-destroy streaming trainer

`StreamingPolicyTrainer` wraps a `LinearPolicy` in `RefCell` and
implements `MapEventConsumer`. Every event flows through:

1. `reward_for(&MapEvent)` — typed scalar reward
2. `features_for(&MapEvent, &MapSnapshot)` — feature vector
3. online SGD step via existing `sgd_step_*` primitives

The trainer never resets between scenarios or phases. It is the
session-long persistent policy head that carries learning across
motor iterations, benchmark cycles, and environment mutations.

### §V.benchmark — labeled-data ingress / the report card

The only signal the machine fundamentally cannot produce by
itself is **whether its math matches the world's**. The
benchmark is the external ground truth pipe.

Two problem sets live in `math_problem.rs`:

- **`canonical_problem_set()`** — 12 problems (nat add/mul, int,
  tensor, float-tensor) evaluable by the kernel alone. The floor:
  every cycle should score 12/12. Regression means the kernel
  broke, not that learning failed.
- **`harder_problem_set()`** — 6 symbolic-identity problems built
  around `Term::Var(100)` as a pattern variable. The kernel
  cannot fold these; they require discovered rules. 0/6 with an
  empty library, 6/6 with the identity rules. This is the
  *delta* the machine can score against itself.

`BenchmarkConsumer::benchmark_now(library, downstream)` runs both
sets against the current library and emits:

```rust
MapEvent::BenchmarkScored {
    solved_count,
    total,
    solved_fraction,
    delta_from_prior,
}
```

The streaming trainer's reward for this event is **asymmetric**:

- `absolute` term: `2.0 × solved_fraction`
- `delta` term: `+3.0 × Δ` when Δ is positive
- `delta` term: `-5.0 × |Δ|` when Δ is negative

The −5× penalty is the rule *"don't break what worked."* A
regression on the report card is five-thirds more costly to the
trainer than an improvement is valuable. Over the stream this
produces a ratchet: gains accumulate, regressions get unwound.

### §V.shed — neuroplasticity on the streaming trainer

Biological networks prune underused synapses while forming new
ones. The streaming trainer does the same. Three new fields on
`StreamingPolicyTrainer`:

- `activation_counts: RefCell<[u64; WIDTH]>` — per-dimension fire count
- `cumulative_contributions: RefCell<[f64; WIDTH]>` — integrated `|w_i × v_i|`
- `pruned: RefCell<[bool; WIDTH]>` — which dimensions are currently shed

Two new operations:

- `prune(magnitude_threshold, min_activations) -> Vec<usize>`
  zeros weights that are both small (below the magnitude
  threshold) and rarely activated (at or below the activation
  threshold). Marks them pruned so future updates skip them.
- `rejuvenate(index, initial_value) -> bool` — un-prunes a
  specific dimension and re-seeds it, so a later rejuvenation
  event (e.g. a novelty signal after staleness) can re-open a
  pathway the trainer previously closed.

**Why both directions matter.** Pure pruning is amnesia; pure
expansion is bloat. Neuroplasticity is the interaction: the
network sheds dimensions whose contribution stayed below the
threshold over a long window, *but* it can rejuvenate a
dimension whose reward signal reappears. The policy head's
effective capacity tracks the learnable signal, not the
allocated feature width.

The shed is invoked externally — the trainer exposes the
operations but does not auto-prune. This is deliberate: the
pruning policy is itself a hyperparameter the outer orchestrator
(MetaLoop, or a future Phase W autopruner) decides. For now the
tests exercise prune + rejuvenate directly, proving the
mechanism is sound.

### What the Phase V extensions collectively produce

A **proprioceptive loop** running at event granularity:

```
┌──────────────────────────────────────────────────────────────┐
│   MathscapeMap (typed self-view)                              │
│        │                                                      │
│        ▼                                                      │
│   MapEvent stream (7 typed variants)                          │
│        │                                                      │
│        ▼                                                      │
│   CertifyingConsumer ──(RuleCertified / Rejected)────┐        │
│        │                                              │        │
│        ▼                                              ▼        │
│   BenchmarkConsumer ──(BenchmarkScored)─► StreamingPolicyTrainer│
│                                              │                 │
│                                              ▼                 │
│                                     online SGD + prune/rejuvenate│
│                                              │                 │
│                                              ▼                 │
│                                     updated LinearPolicy       │
│                                              │                 │
│                                              └── feeds back ──┘│
└──────────────────────────────────────────────────────────────┘
```

The scenario-level loop (V.1–V.5) handles *what environment to
run next*. The event-level loop (V.map–V.shed) handles *how the
policy head adapts to each architectural transition as it
happens*. Together they form a two-timescale adaptive control
system: slow mutations at the corpus/archetype level, fast
gradient updates at the policy level, labeled-data ingress
keeping the fast loop honest, and a pruning mechanism keeping
the representation compact.
