# Apparatus-Universal Rules — the Core Truths

The 6 structural rules that emerge as near-universals across 38
independent apparatus × corpus × scale experiments. These are the
mathscape equivalent of load-bearing axioms: patterns so robust
that most reward-function shapes, corpus families, and scale
settings surface them.

They are the first Rustification candidates — the first layer of
the proven-primitives Merkle tree.

Established 2026-04-18 via `tests/experiment_catalog.rs`.

---

## What "apparatus-universal" means

An apparatus is the combination rule the prover uses to convert
(compression-ratio, novelty, meta-compression, lhs-subsumption)
into a single scalar reward. Each apparatus is a Lisp Sexp form
(phase ML1). Different apparatus shapes promote different rules;
some produce restrictive libraries (1 rule), others permissive
libraries (160+ rules).

A rule is **apparatus-universal** when it emerges under *most*
reward shapes regardless of the specific combination. It's not
an artifact of any particular apparatus — it's a structural
invariant of the discovery space itself.

The cross-experiment aggregation in the catalog runner counts
how many DIFFERENT experiments promoted a given rule as a
near-universal (appearing in ≥ half the apparatuses of that
experiment). High cross-experiment coverage = high confidence
in the rule's fundamentality.

## The 6 Core Truths

Each row: XEPS = number of distinct experiments (out of 38) in
which this rule was near-universal.

| XEPS | LHS | RHS | Interpretation |
|------|-----|-----|----------------|
| 29 | `(?v100 ?v101 ?v102)` | `(S_0 ?v100 ?v101 ?v102)` | 3-ary apply → Symbol wrap |
| 28 | `(?v4 (?v4 ?v100))` | `(S_0 ?v4 ?v100)` | succ(succ(x)) → one-level wrap |
| 26 | `(?v4 ?v100)` | `(S_0 ?v4 ?v100)` | unary apply wrap |
| 20 | `(?v100 (?v4 ?v101) ?v102)` | `(S_0 ?v100 ?v4 ?v101 ?v102)` | apply with unary inner child |
| 17 | `(?v100 ?v101 (?v3 ?v102 ?v103))` | `(S_0 ?v100 ?v101 ?v3 ?v102 ?v103)` | apply with mul inner child |
| 16 | `(?v2 ?v100 ?v101)` | `(S_0 ?v2 ?v100 ?v101)` | add-specific 2-ary wrap |

Reading notes:
- `?v4` is the operator-id 4, i.e. `succ` in the canonical vocab.
- `?v3` is `mul`, `?v2` is `add`.
- `?v100`, `?v101`, `?v102`, … are *fresh variables* (renumbered
  canonically via `anonymize_term`).

### What each truth actually says

**Truth 1: `(?v100 ?v101 ?v102) → (S_0 ?v100 ?v101 ?v102)`**. The
most universal rule — any 3-element `Apply` (a head and two args)
can be wrapped into a 3-ary Symbol. This is the meta-rule that
"currying the operator out of the rule" is always compressive.
29 of 38 experiments — appears under nearly every apparatus we
tried, across every corpus family.

**Truth 2: `succ(succ(?x)) → S_0(succ, ?x)`**. The rank-1
abstraction over *double unary application*. Tight with truth 1
(28/38 experiments) but structurally different because the outer
operator equals the inner operator. This is the successor-chain
compression: every corpus with succ-nesting discovers it.

**Truth 3: `(?v4 ?v100) → (S_0 ?v4 ?v100)`**. Unary apply → 2-ary
Symbol wrap. Strictly weaker than truth 2 but appears in 26/38
experiments. The companion pattern to truth 1 for arity-1
operators.

**Truth 4: `(?v100 (?v4 ?v101) ?v102) → ...`**. Binary apply
where the first argument is a unary application. 20/38 — wherever
a corpus has mixed arities the machine compresses this shape.

**Truth 5: `(?v100 ?v101 (?v3 ?v102 ?v103))`** (apply with mul
inner child) — cross-operator pattern, 17/38. Emerges most
strongly under apparatuses that reward cross-operator compression
(harmonic, max-pair, cr*nov).

**Truth 6: `(?v2 ?v100 ?v101) → (S_0 ?v2 ?v100 ?v101)`**. The
operator-specific version of truth 1, specialized to `add`. 16/38.

### The emergent structure

The universals stratify into layers:

```
  Layer 1 (meta-universal)  : arity-generic apply-wraps        (truths 1,3,6)
  Layer 2 (operator-pattern): succ(succ) + mixed-arity apply   (truths 2,4)
  Layer 3 (cross-operator)  : add with mul inner, etc.         (truth 5)
```

Layer 1 is trivial — it says "the machine compresses any
application into a Symbol." But it's TRUE AS EVIDENCE: every
apparatus under every corpus under every scale converges on it.

Layer 2 is the first non-trivial discovery — the structural
observation that iterated unary application is itself a pattern.

Layer 3 is the most interesting because it's operator-specific
and cross-structural — it tells the machine "when you see mul
nested inside add's argument, compress." These are the beginnings
of distributivity's structural scaffold.

## What this means for Rust promotion

These 6 rules are the first candidates for Rustification — being
absorbed back into the Rust substrate as typed primitives. The
promotion ladder:

```
Lisp-level discovery (many apparatuses promote a rule)
       → cross-experiment attestation (coverage ≥ threshold)
         → Rust primitive (typed, compiled, no longer mutable)
            → richer vocabulary for next Lisp-level discovery
```

Once a universal rule is Rustified, the Lisp apparatus no longer
needs to DISCOVER it — the Rust layer provides it as a built-in.
The Lisp layer can now focus on patterns built ON TOP of it, which
were previously drowned out by the noise of re-discovering the
universal on every run.

**But: we don't promote yet.** The user's direction (2026-04-18):
"I want to have a lot of discoveries before going back to rust."
The strategy is to accumulate a wide catalog of cross-experiment
universals first, so when we do promote, we're promoting a solid
*layer* of structure, not scattered rules.

Near-term near-universals (in 10+ experiments but not yet
reported here): additional variations of the apply-wrap pattern
(4-ary, 5-ary), compound applications with multiple layers of
nesting, and rules composed purely of previously-minted Symbols
(fixed-points of S_0 on itself, e.g. `(S_0 ?v4 (S_0 ?v4 ?v100)) →
(S_0 ?v4 ?v100)` — 12/38 experiments).

## Which apparatuses are productive

Per-experiment discovery rate confirms the grand-sweep finding:

- **Canonical** (default Rust formula): middle of the pack.
- **Harmonic** (`(/ (cr * novelty) (cr + novelty))`): most
  creative — produces 11 unique structural rules per apparatus
  only it finds.
- **Novelty-only**, **Meta-only**, **Sub-only** (single-axis):
  each produces 62-70 apparatus-specific discoveries.
- **Max-pair**: extremely prolific (60+ apparatus-specific).
- **cr-only**, **threshold-cr**, **cr*nov**, **cr-penalty**: highly
  restrictive — 1 rule per seed on average.

The catalog makes this repeatable: re-running confirms the same
productive/restrictive classification.

## Which corpus families expose the most discoveries

- **Procedural**: the most rules overall (160-200 per 8 seeds × 12
  budget). Random-variable terms over {add, mul, succ} stress the
  apparatus broadly.
- **SuccessorChain**: focused — produces fewer rules but higher
  universal rate because every apparatus finds the succ-universal.
- **AsymmetricArith**: narrow — 3 rules total with very high
  universality. The commutative duplicates pattern collapses to a
  single signal.
- **DeeplyNested**: medium — exposes cross-level compressions
  (truth 4, 5 emerge prominently).
- **ZooPlusProcedural**: closest to the autonomous-traversal
  milestone corpus. Mid-range in discovery count.
- **MixedOperators**: broad diversity, similar to procedural.

**Combinatorial implication**: for fastest discovery of novel
structure, use `DeeplyNested` or `MixedOperators`. For fastest
convergence on stable universals, use `AsymmetricArith`. For
maximum coverage, run the full catalog.

## Regression signals

If a future run of the catalog shows:

- **Fewer than 5 cross-experiment universals** (we currently have 6
  with ≥16 experiment coverage): apparatus wiring may have
  regressed; the Lisp ↔ Rust bridge may be mis-binding an axis.
- **Productive rate < 80%**: too many apparatuses going barren;
  prover threshold or extract config may have changed.
- **Different top-3 universal rules**: the discovery topology has
  shifted; investigate whether a new core truth has emerged or
  whether we've lost the canonical fingerprint.

The catalog is intentionally the regression sentinel for apparatus
behavior. Re-run before any change that touches the prover,
generator, or reward pipeline.

## References

- `crates/mathscape-axiom-bridge/tests/common/experiment.rs` —
  harness + catalog.
- `crates/mathscape-axiom-bridge/tests/experiment_catalog.rs` —
  catalog runner.
- `crates/mathscape-reward/src/lisp_reward.rs` — ML1 Lisp reward
  evaluator the apparatus layer sits on top of.
- `docs/arch/bettyfine.md` — the canonical-apparatus fixed point
  that motivated apparatus-level mutation.
- `docs/arch/rust-lisp-duality.md` — the architectural frame
  where ground rules (Rust) meet mutable apparatus (Lisp).

## Next moves

1. **Expand the catalog to 100+ experiments.** The 38 we have
   span 12 hypothesis families. Filling in the crossings
   (apparatus × corpus × extract config full factorial) gets us
   to 100 with the existing harness — no new machinery needed.
2. **ML3: apparatus mutation loop.** The machine proposes its OWN
   Lisp apparatus mutations rather than us hand-picking. Apply
   anti-unification to winning apparatus forms to generate
   candidates. Evaluate via the catalog. Keep variants that
   increase cross-experiment universal count.
3. **Rustification of universal #2** (succ-succ). The
   concretely-named pattern. Smallest-surface promotion test —
   shows the loop from Lisp-discovery → Rust-primitive closes
   cleanly.
