# Self-Tuning Meta-Loop

The machine observes its own learning, leverages that observation to
author its own next training recipe, and executes in complete
linguistic isolation. The system sails itself.

Status: **design frame, 2026-04-18.** Phase U. Built on R26
(BootstrapCycle), R32 (Sexp-describable spec), R33 (ExperimentScenario
chain), R34-R35 (wall-clock observability), R37/R39 (opt-in work
elimination), and Phase I (subterm-paired AU unblocking meta-diversity).

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
