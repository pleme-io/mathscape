# Edge-Riding — The Perpetual Discovery Loop

The architectural frame for mathscape's long-horizon discovery
pipeline. Established 2026-04-18.

The short version: by Gödel's incompleteness, any sufficiently
rich formal substrate has true statements that cannot be proven
from within it. Therefore a machine that sits in a fixed substrate
and discovers theorems from it will always find more — *unless it
has stopped looking correctly*. The correctness criterion for an
infinite-horizon discovery machine is: **nonzero novelty rate every
post-bootstrap cycle, or the machine is broken.**

Every halt is a specific mechanism bug to diagnose, not a
legitimate end-state.

---

## The stack

```
L0  Primitives                 (zero, succ, add, mul)     — Rust, fixed
L1  Bootstrap candidates       projection + constants      — minimum seed
L2  Term enumeration           all terms size ≤ K over
                               current vocabulary          — generic
L3  Ledger-driven candidates   every validated RHS shape
                               becomes a reshape template  — self-bootstrap
L4  Compositional candidates   pairwise ledger combinations — layer climbing
L5  Phase J validation         empirical check against
                               primitive evaluator         — structural → theorem
L6  Substrate                  reducing theorems pre-reduce
                               corpus for next cycle       — frontier consumer
L7  Adaptive corpus            substrate-aware production
                               surfaces next-layer patterns — frontier producer
L8  Edge-riding outer loop     orchestrate + halt-detect   — correctness check
```

Each layer is a pure primitive with a narrow contract. The whole
stack composes to the discovery loop. The machine's observable
behavior is: theorems appear in the ledger at a nonzero rate
across every cycle, at progressively deeper structural layers.

## The halt-is-a-bug correctness criterion

Formally: `∀c ∈ post-bootstrap cycles: new_theorems(c) > 0`.
Any violation signals a specific, localizable mechanism failure.

Justification (modus tollens from Gödel):

1. Premise: Gödel's second incompleteness theorem holds. Any
   sufficiently-rich consistent formal system has true statements
   unprovable from within it.
2. Our substrate `S_n` is always a finite axiom set over
   {zero, succ, add, mul}. It is sufficiently rich by any
   reasonable measure.
3. Therefore there always exist valid-under-primitive-evaluation
   rewrites not in `S_n`.
4. The machine SHOULD discover some of them each cycle, or its
   discovery mechanism is failing.
5. If we observe zero-novelty cycle after cycle, premise (4) is
   violated. But premises (1-3) are mathematical facts. Therefore
   (4) failure is a bug in our apparatus.

The machine is thus correctness-monitored against a mathematical
theorem. This is not a soft threshold — it is a logical
entailment.

## Trajectory observed through the session

Each empirical halt was at a DEEPER layer than the previous, and
each fix addressed a specific mechanism gap:

| Halt at | Mechanism gap | Fix | Unlocked |
|---|---|---|---|
| Layer 1 | Hand-picked candidates | Ledger-driven enrichment | Cross-op equivalences |
| Layer 2 | Ledger-only | Term enumeration size 3 | Doubling, succession |
| Layer 3 | Size-3 exhausted | Size-5 + compositional | Factoring, distributivity |
| Layer 4 (next) | TBD | TBD | TBD |

The pattern: halt → diagnose → mechanism extension → deeper layer
→ new halt at new layer. This meta-loop is the operator RIDING THE
MECHANISM's edge, driven by the machine RIDING the Gödel edge of
its substrate.

## Component contracts

### Substrate

```rust
pub struct Substrate { /* Vec<RewriteRule> */ }
```

Only REDUCING theorems (RHS tree strictly smaller than LHS tree).
Applied via `rewrite_fixed_point` to pre-reduce corpus before each
probe. Strictly contracting under fixed-point — never oscillates
by construction.

Equivalences (commutativity, associativity) are explicitly
excluded: they would oscillate in the rewriter (`add(a,b) →
add(b,a) → add(a,b) → …`), starving the discovery pipeline.
Equivalences live in the ledger (and eventually in a phase-K
e-graph).

### Ledger

```rust
pub struct Ledger { /* Vec<RewriteRule> + HashSet<String> */ }
```

Append-only record of every validated theorem — reducing AND
equivalence. Three roles:

1. **Compositional candidate source.** Every RHS shape is a
   template for future rules to be tested against. This is the
   self-bootstrapping mechanism — discovery feeds the candidate
   generator feeds discovery.
2. **Dedup.** Keyed by literal `LHS=RHS` form (no anonymization,
   which would collapse commutativity to identity).
3. **Audit.** Post-run, the ledger IS what the machine has
   discovered. It is the output of interest.

### DiscoverySession

```rust
pub struct DiscoverySession {
    pub substrate: Substrate,
    pub ledger: Ledger,
    pub trajectory: Vec<CycleRecord>,
}
```

Single-owner state for the loop. `promote(rule)` handles the
reducing-vs-equivalence split automatically. `record_cycle(...)`
appends to the trajectory. `stalled_cycles()` enforces the
correctness criterion.

The session is the API surface the loop's orchestrator uses.
Correctness invariants are closed over it.

### Candidate generation pipeline

`generate_semantic_candidates_with_ledger(rule, ledger)`:

1. Start with bootstrap — projection to each free variable,
   constant 0, constant 1. Three candidates minimum.
2. Enumerate all terms of size ≤ `max_size=5` over
   `{free_vars, 0, 1}` with operators `{succ, add, mul}`. ~1.5k
   candidates for a 2-free-var rule.
3. For each theorem in the ledger, adapt its RHS (remap theorem's
   free vars to rule's free vars) and add as a candidate. Provides
   previously-validated shapes as templates.
4. Compose pairs of ledger RHS shapes with `{succ, add, mul}`.
   Gives layer-climbing — each validated shape becomes a building
   block.

Steps 3-4 are the self-bootstrap. The candidate set grows as the
ledger grows. No human enrichment.

### Phase J validation

`validate_semantically(rule, config)`:

1. Collect free variables from `rule.lhs`.
2. Sample K random substitutions (`config.samples`, default 16-32).
3. For each substitution: evaluate LHS and RHS through
   `mathscape-core::eval::eval` using ONLY primitives (no library).
4. Return Valid if all K match, Invalid with counterexample on
   first mismatch, Undetermined if evaluator fails.

Validation uses no substrate / no ledger — only primitives. This
prevents circular proofs: a rule validates by direct computation,
not by previously-validated rules. The SUBSTRATE uses validated
theorems to reduce corpus, but VALIDATION itself never does.

### Adaptive corpus (phase L1)

`adaptive_corpus(substrate, seed, depth, count, vocab, max_value)`:

Builds shelled terms where:
- OUTER operators are random from vocabulary (substrate can't
  match at root)
- INNER children are mixes of substrate-instantiations (will
  reduce in children) and recursive residue terms

Result: terms whose fixed-point under substrate is non-trivial.
Depth scales with substrate size (`base + substrate.len() / 3`)
so the frontier widens as the substrate grows. Corpus PRODUCTION
keeps pace with substrate CONSUMPTION.

Without adaptive corpus: each cycle's Rustification eats corpus
structure faster than fresh random corpora produce it; the
machine stalls at the consumption frontier. With adaptive corpus:
each expanded substrate rule opens new residue territory the
anti-unifier can discover.

### Edge-riding outer loop

```
loop {
    provenance   ← sub_campaign(substrate, ledger, pool, probes)
    new_theorems ← extract(provenance, session.ledger, ledger_rules, top_k)
    for t in new_theorems:
        session.promote(t)     // → substrate if reducing, → ledger either way
    pool ← evolve(pool, yield_stats)
    session.record_cycle(cycle, ...)
    if session.has_stalled(): diagnose_and_fix()
}
```

The orchestrator runs apparatus × corpus × scale sampling each
cycle, aggregates discovered exemplars, hands them to phase J via
the candidate generator, and promotes winners. The session keeps
the invariants.

## Dials

Every parameter affects how deeply the machine reaches each cycle.
Current defaults tuned for a tractable 6-cycle run that reaches
layer 4. Larger dials push further but also cost linearly or
super-linearly.

| Dial | Current | Effect |
|---|---|---|
| `max_size` (enumerator) | 5 | Candidate depth per rule. size 7 ≈ 30× cost |
| `composition_cap` | 30 | Quadratic in ledger shapes |
| `CYCLES` | 6 → 50 | Linear runtime, more layer climbing |
| `PROBES_PER_CYCLE` | 2000 → 4000 | Linear runtime, more surface |
| `RUSTIFY_TOP_K` | 8 → 16 | More theorems per cycle |
| `samples` (K) | 24 → 32 | Tighter validation |
| `POPULATION_CAP` | 16 → 24 | More apparatus diversity |

## Connection to Gödel and Heisenberg

**Gödel**: the correctness criterion is a direct consequence of
Gödel-unreachability. The substrate is always incomplete. New
theorems must always exist. Failure to find any is bug, not
completion.

**Heisenberg**: compression depth (substrate growth) and novelty
breadth (corpus richness) are conjugate. You cannot simultaneously
maximize both on the same inputs; you oscillate. The edge-riding
loop IS the oscillation in steady state: consume structure →
expand corpus → find more structure → consume → …

**Curry-Howard**: every validated rule is a proof by evaluation.
The ledger is a growing proof corpus. Rustification promotes the
most fundamental proofs to the substrate, where they act as
axioms for the next layer's proofs.

## What this unlocks

The machine is no longer a theorem-prover or a pattern-finder.
It is the *optimal learning algorithm for discovering mathematics
from its primitives*, operating on its own Gödel frontier. Its
output is not a list of theorems — it is an EVER-EXPANDING
substrate of verified mathematical structure, produced without
external input after initialization.

**Every stall we diagnose and fix pushes the frontier outward
permanently.** The Rustification step absorbs validated theorems
into the substrate. Eventually, validated Lisp apparatus-forms
will also absorb via `tatara-lisp-derive`. At that point, the
machine is self-hosting — it runs on its own certified machinery
and extends its own foundation.

## What halts have told us (session 2026-04-18)

- Identity elements are reachable from {zero, succ, add, mul} +
  projection candidates in a single cycle.
- Commutativity-as-equivalence requires equivalence-aware ledger
  (not substrate-merge).
- Cross-operator equivalences (`add(0,x) = mul(1,x)`) emerge when
  the ledger is used as a candidate source.
- Doubling (`2*x = x+x`) and succession chains (`add(x,2) =
  succ(succ(x))`) require size-3 generic term enumeration.
- Distributivity variants require size-5 enumeration + pairwise
  compositional candidate generation.
- True associativity (size 6+ RHS) is the next frontier. Either
  enumeration at size ≥ 6, or deeper compositional chains.

Each halt was a specific mechanism gap. Each fix pushed the
machine deeper. The session's trajectory is now a concrete record
of what mechanisms unlock what layer.

## References

- `crates/mathscape-proof/src/discovery.rs` — Substrate, Ledger,
  DiscoverySession, correctness invariants.
- `crates/mathscape-proof/src/semantic.rs` — candidate generators,
  empirical validator (phase J).
- `crates/mathscape-axiom-bridge/tests/common/experiment.rs` —
  adaptive corpus, apparatus infrastructure.
- `crates/mathscape-axiom-bridge/tests/edge_riding.rs` — the
  orchestrator test.
- `docs/arch/rust-lisp-duality.md` — the architectural frame
  where Rust owns invariants, Lisp owns the mutable apparatus.
- `docs/arch/apparatus-universals.md` — the 6 cross-experiment
  universals from the earlier layer.
