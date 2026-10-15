# Full Lisp Port — No More Hardcoded Levels

The architectural commitment made at 2026-04-18, during the ML5
landing session. After ML5's hand-coded `Compound(Vec<MechanismMutation>)`
variant, the pattern of "saturation detected → human writes Rust to
add level N+1" becomes structurally unacceptable. From here on,
every architectural gate must be representable as a Lisp Sexp form
the machine can manipulate.

## The invariant this plan enforces

> From M1 onward, when the machine detects saturation at level N,
> the response does NOT involve human intervention in Rust.
> If the current mutation space can't break the saturation, the
> response is (a) Lisp-form mutation of the mutation operators
> themselves, (b) Lisp-form synthesis of new operator TYPES, or
> eventually (c) Lisp-form synthesis of entirely new levels.
> Each of these IS the machine's own discovery process applied
> one level higher.

## What stays in Rust (the kernel)

The Rust layer retains only what CANNOT be made mutable without
breaking correctness:

- **Primitives**: `Point`, `Number`, `Fn` enum variants — the
  irreducible substrate of the expression language.
- **Evaluator**: `zero`, `succ`, `add`, `mul` — Peano arithmetic
  builtins. These define the semantics everything else is
  validated against; mutating them would destroy the validation
  signal.
- **Identity types**: `TermRef` (BLAKE3 hash-cons), `Artifact`
  content hash — the Merkle tree's roots.
- **tatara-lisp runtime itself**: the terreiro that evaluates
  Lisp forms. Parts of this could eventually be Lisp-expressed,
  but that's an ML∞-scale migration.
- **Phase J validator**: runs primitive eval on random
  substitutions. Its logic uses the evaluator directly; the
  candidate generator (which produces what it tests) is Lisp.
- **DiscoverySession state**: substrate, ledger, trajectory —
  these are pure data records. The data itself stays in Rust;
  what VARIES is the Lisp-form apparatus above them.

Everything else — reward forms, mutation operators, fitness
functions, saturation policies, pool management — moves to Lisp.

## The migration order (topological, with test gates at each step)

### M1: MechanismConfig → Sexp struct

**Current**: `struct MechanismConfig { candidate_max_size: usize, ... }`
— 13 fields, Rust struct, `MechanismMutation::apply(&self)` produces
mutants.

**Target**: `(defconfig mechanism :candidate-max-size 5 :composition-cap 30 ...)`
as a Lisp Sexp, with tatara-lisp-derive generating the Rust
bridge. Mutations operate on the Sexp.

**Test gate**: baseline run (no mutations) produces identical
trajectory as Rust-only version. Sexp round-trip preserves all
fields. Gold test asserts parsed-then-serialized form equals
input.

### M2: Mutation operators → Sexp functions

**Current**: 17-variant `MechanismMutation` enum, each variant
implements `apply(&config) -> MechanismConfig`.

**Target**: each operator is a Lisp function:
```lisp
(defop bump-candidate-max-size (delta)
  (lambda (config)
    (update config :candidate-max-size (clamp (+ (:candidate-max-size config) delta) 3 8))))
```

The operator POOL is a Lisp list. Mutation sampling picks
randomly from the pool. New operators can be appended at
runtime.

**Test gate**: same as M1 but with the full atomic-mutation
trajectory. Compound operators (ML5 equivalent) are Lisp lists
of operators composed via a Lisp `compose-ops` function.

### M3: Fitness functions → Sexp forms

**Current**: `delta_novelty > 0` hardcoded as the mechanism-level
fitness signal.

**Target**: fitness at each level is a Lisp form:
```lisp
(deffitness mechanism-trial (trial-result)
  (> (:delta-novelty trial-result) 0))
```

Alternatives become available as mutations:
```lisp
(deffitness mechanism-trial-weighted (trial-result)
  (* (:delta-novelty trial-result)
     (:diversity-weight trial-result)))
```

The machine picks among fitness forms. Higher-level loops can
mutate the fitness form below them.

**Test gate**: same trajectory at baseline fitness. Additional
fitness forms produce different but plausible trajectories.

### M4: Saturation-response policy → Sexp form

**Current**: Rust code in `respond_to_saturation` with hardcoded
escalation schedule (arity 1 → 1 → 2 → 3 → 3+).

**Target**: policy is a Lisp form:
```lisp
(defpolicy saturation-response (pool history escalation-budget)
  (let* ((round (round-of history))
         (arity (escalation-arity round))
         (n (* (base-mutations-per-round) (compound-factor round))))
    (propose-mutations pool n arity)))
```

**Test gate**: baseline behavior preserved. Mutations to the
policy form produce different escalation patterns.

### M5: Machine synthesizes new mutation operator TYPES

**Current**: new operator types come from me hand-editing the
Rust enum.

**Target**: saturation-response has a rarely-used path that
proposes a NEW Lisp operator form by rearranging elements of
existing operator forms. Successful syntheses join the operator
pool. This is the first concrete case of automated architectural
gate creation.

**Test gate**: given a scenario where no existing operator breaks
saturation but a specific new one would (rigged in the test),
the machine synthesizes and accepts it.

### M6: Machine synthesizes new ML levels

**Current**: new levels come from me hand-editing the architecture.

**Target**: when level N saturates AND the operator-synthesis path
saturates too, machine proposes a new Lisp form that looks like
"another level." This requires the machine to understand the meta-
pattern (pool + mutation + fitness + promotion) well enough to
instantiate new ones.

**Test gate**: the machine genuinely proposes a new `(deflevel ...)`
form that, when instantiated, breaks a synthetic deep saturation.
This is the architectural endpoint.

## Test strategy at each milestone

Every migration step has TWO test gates:

1. **Correctness regression**: baseline behavior (default config, no
   mutations) produces the same trajectory as before the port. Same
   cycle counts, same theorem counts, same stall pattern.
2. **Expressiveness expansion**: the ported representation enables
   a NEW behavior that wasn't possible in the Rust-hardcoded form.
   For M1, this is "MechanismConfig can be serialized and reloaded."
   For M2, "new mutation operators can be added at runtime."
   Etc.

If gate 1 fails, the port has a bug. If gate 2 fails, the port
was cosmetic and didn't buy new behavior.

## Runtime performance budget

Lisp-form evaluation is slower than Rust. For each migration, we
measure:

- How many times the Lisp form is evaluated per cycle
- The marginal cost compared to the Rust version

If a port adds >10% per-cycle overhead at default settings, we
cache/compile — but the ARCHITECTURAL commitment remains that
the form is Lisp-expressed. Cached Rust code derived from the
Lisp form is acceptable as long as the form remains mutable.

## Risks and mitigations

- **Risk**: Infinite regress in level-synthesis.
  **Mitigation**: hard cap on depth of self-reference (ML6 can
  propose ML7; ML7 can't propose ML8 without an explicit flag).
- **Risk**: tatara-lisp lacks a primitive the port needs (e.g.,
  struct update syntax, pattern matching).
  **Mitigation**: extend tatara-lisp minimally to cover the need.
  Track what features are added so we know the full language
  surface the migration requires.
- **Risk**: Lisp evaluation bugs produce silently-wrong results.
  **Mitigation**: gold tests at every port. Rust path available
  as fallback for debugging.
- **Risk**: The machine mutates its own apparatus into a
  non-terminating form.
  **Mitigation**: step budgets on every Lisp evaluation. Terreiro
  enforces hard time/step limits.

## Correctness invariants preserved across the port

These must hold at every milestone:

- **Halt-is-a-bug correctness**: every mutation's trial still
  measures delta-novelty. A trial's verdict is either Valid,
  Invalid (with counterexample), or Undetermined.
- **Phase J purity**: validator uses only primitive evaluator,
  never the ledger. Prevents circular proofs.
- **Substrate strict contraction**: only reducing theorems
  enter the substrate. Enforced regardless of whether the check
  is Rust or Lisp.
- **Merkle-tree identity**: every accepted theorem gets a BLAKE3
  content hash. Never varies.
- **Self-feeding topology documented**: every new mutable layer
  gets added to the catalog in `docs/arch/edge-riding.md` so we
  know where the system feeds into itself.

## Non-goals

This port is NOT about:
- Replacing the Rust evaluator. That stays forever.
- Porting existing theorems to Lisp. Theorems are typed data.
- Performance optimization beyond what correctness requires.
- Making the system faster. Making it *more mutable*.

## Connection to tatara-lisp-derive

tatara-lisp-derive is the Rust↔Lisp bridge. The migration uses it
to go both directions:

- Rust struct → TataraDomain → Lisp representation (for M1, M2)
- Lisp form → derived Rust code → compiled apparatus (for ML5+
  discovered operators that prove stable)

This is the same promotion ladder as theorem Rustification: Lisp
discovers, Rust absorbs the stable winners as typed primitives.
The MIGRATION itself is the first large-scale exercise of this
pattern applied to the apparatus, not to theorems.

## How this changes the next session's rhythm

The pattern before:
```
1. Observe machine stall at level N
2. Human designs level N+1 in Rust
3. Human writes the enum / struct / function
4. Human threads it through
5. Rerun, observe next stall
```

The pattern after:
```
1. Observe machine stall at level N
2. Machine proposes Lisp-form mutations to level N's apparatus
3. If insufficient, machine proposes Lisp-form mutations to
   level N+1's fitness/mutation spec
4. If insufficient, machine proposes a whole new (deflevel ...)
   form via search over Sexp shapes
5. Each proposal is validated by trial
6. Winners are promoted; failures fill the graveyard
7. The machine does ALL of the above without human Rust edits
```

The role of the human is now ARCHITECTURAL OBSERVATION, not
compiler. When the machine's own self-mutation loop can't break
a stall, the human's job is to analyze WHY and possibly extend
tatara-lisp itself (or the primitive set). But the mechanism
layers above primitives are the machine's domain.

## Status — 2026-04-18 session landing

- ML0-L5 as Rust: DONE (this session)
- Full Lisp port M1-M6: planned, tasks #87-92 created
- Next action: implement M1 (MechanismConfig as Sexp)

Starting M1 in the current session. Subsequent milestones come as
dedicated work blocks. Each produces empirical evidence (baseline
trajectory preserved; new behavior enabled) before merging.
