# Cross-Domain Application — ml-forge as Rust/Lisp Dual

The duality doc claims the pattern is universal. This document tests
the claim by walking through `pleme-io/ml-forge` — a tensor-IR
library with 15 primitive UOps — as a forced-realization system.
If the pattern fits without distortion, it's probably universal. If
it doesn't, we learn what mathscape-specific assumptions leaked in.

## ml-forge today

ml-forge defines 15 closed UOps on a typed tensor graph. The enum
`UOp` is `#[non_exhaustive]`. Any ML computation is a composition of
UOps — element-wise (Neg, Exp, Log, Sin), arithmetic (Add, Mul, Max,
Cmp), reductions (Sum, MaxReduce), movement (Permute, Reshape),
and the atypical (Cast, Input, Const, MatMul, Index).

The set is *small and closed by design* — this is tinygrad's
philosophy. Complexity ships as *compositions*, not as new UOps.
15 is not a ceiling; it's a floor — below which composability breaks.
Above, it grows only when composition alone cannot express a
workload without painful redundancy.

## Where the Rust/Lisp dual shows up

Today:
- **Rust side**: `UOp` enum, `Graph` struct with typed shape
  inference, `TensorType`, `Shape`, `Dim`, `Dtype`. Closed, proven,
  type-checked.
- **Lisp side**: (implicit) — compositions of UOps expressed as
  `Graph`s. Every graph is a sexpr-expressible value via
  `iac_forge::sexpr::{ToSExpr, FromSExpr}`.

The explicit Lisp side doesn't exist yet. Graphs are constructed by
Rust code. But the *potential* for a Lisp-side generator is there —
the graph representation is already sexpr-compatible; the UOps are
already content-addressable.

## How the forced-realization machine would grow UOps

Imagine running the mathscape machine over an ml-forge corpus. The
corpus is training workloads (transformers, convnets, graph NNs).
Each workload is a Graph, and the library is... currently empty
(there's no compression layer yet).

| Step | What happens                                              | Gate involvement             |
|------|-----------------------------------------------------------|------------------------------|
| 1    | Generator proposes sub-graph patterns (anti-unification across workloads) | gates 1–3 in mathscape-like prover |
| 2    | A pattern like "Mul(Sum(_,_), Const(x))" matches 40 % of the corpus | compression ratio clears ε |
| 3    | Pattern stable across workloads; coverage holds           | reinforcement advances       |
| 4    | Pattern subsumes three existing "FusedLinearBias" entries | gate 4 (K=3) clears          |
| 5    | Pattern appears in every major workload class             | gate 5 (N≥2) clears          |
| 6    | axiom-forge receives a `PromotionSignal` mapping the pattern to a new UOp variant | gate 6 (7 obligations) runs  |
| 7    | axiom-forge emits `UOp::FlashAttention { causal: bool, head_dim: u64 }` into the Rust source | gate 7 (rustc) runs          |
| 8    | ml-forge's library gets rewritten: three FusedLinearBias rules deduplicate into one FlashAttention call | migration report emitted     |

The pattern fits cleanly — no distortion.

## What ml-forge contributes that mathscape doesn't

**Typed shape inference as an additional gate.** Mathscape's Term
enum is untyped. ml-forge's UOp carries shape invariants. A promoted
UOp must carry *shape rules* — how output shape relates to input
shapes. This is a new gate between 6 and 7:

- Gate 6' (proposed, ml-forge-specific): **shape-rule well-formedness**.
  Given the proposed UOp's arity and carried parameters, does a
  total shape function exist? Can it be inferred automatically from
  the rhs pattern?

The gate slots in naturally. axiom-forge's structural obligations
(name well-formed, target path valid, etc.) are a superset that
lets each domain add its own checks. ml-forge adds shape-rule
checks; iac-forge adds compliance-lattice checks; compliance-
controls adds baseline-subsumption checks. The 7 obligations in
axiom-forge today are *mathscape's* instantiation — they're a
baseline every domain extends.

## What this tells us about the abstraction

The mathscape machine is not mathscape-specific. What it is:

- A **PrimitiveGrowth** system parameterized by
  - `Candidate` shape (mathscape: RewriteRule; ml-forge: UOp shape pattern; iac-forge: IacResource pattern)
  - `Generator` (mathscape: anti-unification; ml-forge: graph pattern mining; iac-forge: Terraform state diffing)
  - `Verifier` (mathscape: e-graph + Lean; ml-forge: shape inference + numerical agreement; iac-forge: plan-apply equivalence)
  - `PromotionGate` (domain-specific, but always temporal)
  - `AxiomForgeBackend` (domain-specific emit target; mathscape: `Term`; ml-forge: `UOp`; iac-forge: `IacType`)

These five parameters specialize the pattern. Everything else is
shared: Event stream, ΔDL currency, reinforcement loop, allocator,
ten-gate lattice, type-state lifecycle, migration reports.

## The crate-structure consequence

If we accept this, the target architecture is:

```
primitive-forge (new, Phase L or later)
  ├── Event / Artifact / EpochTrace / EpochAction / Registry
  ├── ProofStatus lattice
  ├── PromotionSignal / MigrationReport
  ├── Regime / RewardEstimator / Allocator / RealizationPolicy
  └── traits: Generator, Prover, Emitter, PromotionGate, Verifier

mathscape-core
  impl PrimitiveGrowth<Candidate = RewriteRule, Emit = Term>

ml-forge (future)
  impl PrimitiveGrowth<Candidate = UOpPattern, Emit = UOp>

iac-forge (future)
  impl PrimitiveGrowth<Candidate = IacPattern, Emit = IacType variants>
```

Today mathscape owns everything. The extract to `primitive-forge` is
Phase L in the plan — and the plan says "don't extract until the
pattern works in two domains."

This document is the *theoretical* validation that the second
domain (ml-forge) would fit. We don't need to code it yet. We just
need to know that, when the time comes, the extraction will be
mechanical.

## What ml-forge would need to fit today

Not much:

1. A corpus of ml workloads as `Graph` values (exists informally;
   needs a corpus type)
2. A generator that proposes UOp patterns (doesn't exist; would be
   the equivalent of `mathscape-compress::extract_rules` for tensor
   graphs)
3. A shape-preserving verifier (exists — graph shape inference)
4. The PromotionGate / bridge machinery (comes free once
   `primitive-forge` is extracted)

Effort estimate: ~3 weeks once `primitive-forge` exists. The hard
part (the machine itself) is already being built for mathscape.

## Why this test matters

If mathscape is the *first* instance of a pattern that also fits
ml-forge, iac-forge, compliance, and arch-synthesizer — then every
line of work on mathscape is really work on a platform-wide
capability. The value of solidifying the pattern cleanly compounds
across five domains.

If mathscape turned out to be domain-specific — if the pattern
didn't fit ml-forge without distortion — then the work would have a
natural ceiling at mathscape's domain. We would still build it, but
the extraction to `primitive-forge` would not be justified.

This document's claim: the pattern fits. The main ml-forge-specific
need (shape-rule well-formedness) slots in as an additional
domain-gate between 6 and 7 without disturbing anything else.

## Consequence for the locked mathscape theory

- **No changes required.** The ten-gate lattice, the five forces,
  the three regimes, the reward calculus all survive the cross-
  domain test without modification.
- **One clarification**: gate 6 ("axiom-forge obligations") is really
  "domain-specific structural obligations". Mathscape uses the 7
  generic ones. ml-forge would add shape-rule well-formedness.
  iac-forge would add compliance-lattice checks. This is a
  parameterization point, not a revision.

- **Plan phases unchanged.** The ml-forge extension is a Phase L+
  concern. The current plan (A–K) is mathscape-only.

## Action items

None. This document exists to *test* the theory, not to produce new
work. If Phase I of the mathscape plan succeeds, a sibling doc
`domain-iac-forge.md` and an actual `primitive-forge` extraction
become the logical next step. Until then, mathscape carries the
pattern.
