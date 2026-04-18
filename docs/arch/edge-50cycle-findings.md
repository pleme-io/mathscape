# 50-cycle findings — structural vs semantic novelty

Analysis of `docs/arch/edge-50cycle-evidence.log`. Evidence source:
`edge_riding_loop` test with knobs `CYCLES=50, PROBES_PER_CYCLE=4000,
RUSTIFY_TOP_K=16, composition_cap=30, max_size=5`.

Run: 271.9s wallclock, 800 structural theorems, zero stalls, mean
16 theorems/cycle.

## The headline finding

**800 structural theorems. 23 distinct LHS families. ~15 distinct
semantic theorems.**

The 50-cycle loop satisfies the correctness criterion — nonzero
novelty every post-bootstrap cycle — but the novelty it measures
is STRUCTURAL, not SEMANTIC. Most of the "new theorems" are
alternative equivalent expressions of a handful of underlying
equations.

## Distribution of structural theorems by LHS family

| Count | LHS | Semantic content |
|------:|-----|------------------|
| 266 | `mul(2, x)` | one theorem: `mul(2, x) = add(x, x)`, expressed 266 ways |
| 266 | `mul(x, 2)` | commuted version of above, 266 ways |
| 31  | `add(2, x)` | `add(2, x) = succ(succ(x))`, 31 ways |
| 31  | `add(x, 2)` | commuted |
| 26  | `add(x, y)` | commutativity of add, with ledger-composed variants |
| 26  | `mul(x, y)` | commutativity of mul |
| 20  | `add(3, x)` | `= succ(succ(succ(x)))`, 20 ways |
| 20  | `add(x, 3)` | commuted |
| 16  | `add(0, x)` | left-identity on add, 16 ledger-composed forms |
| 16  | `add(x, 0)` | right-identity on add |
| 16  | `mul(1, x)` | left-identity on mul |
| 16  | `mul(x, 1)` | right-identity on mul |
| 14  | `add(mul(x, y), x)` | `= mul(x, succ(y))` — distributivity scaffold |
| 6   | `add(mul(x, 1), mul(y, 1))` | `= add(x, y)` under double-identity |
| 6   | `add(x, mul(y, 2))` | distributivity variant |
| 4   | `mul(3, x)` | `= add(add(x, x), x)` |
| 4   | `mul(x, 3)` | commuted |
| 4   | `mul(add(x, y), z)` | partial distributivity |
| 4   | `mul(x, add(y, z))` | distributivity LHS, found from corpus |
| 4   | `add(mul(x, y), z)` | factoring precursor |
| 4   | `add(x, mul(y, z))` | commuted |
| 2   | `mul(x, add(y, 2))` | `= mul(x, succ(succ(y)))` composite |
| 1   | `add(4, x)` | `= succ(succ(succ(succ(x))))` |
| 1   | `add(x, 4)` | commuted |

## True semantic content — the ~15 distinct theorems

Deduplicating by underlying mathematical equation:

1. `add(0, x) = x` — add left-identity
2. `add(x, 0) = x` — add right-identity
3. `mul(1, x) = x` — mul left-identity
4. `mul(x, 1) = x` — mul right-identity
5. `add(x, y) = add(y, x)` — commutativity of add
6. `mul(x, y) = mul(y, x)` — commutativity of mul
7. `mul(2, x) = add(x, x)` — doubling
8. `mul(3, x) = add(x, add(x, x))` — tripling (and the 2-variants of commutativity)
9. `add(2, x) = succ(succ(x))` — double-succession
10. `add(3, x) = succ(succ(succ(x)))` — triple-succession
11. `add(4, x) = succ^4(x)` — quadruple-succession
12. `add(mul(x, y), x) = mul(x, succ(y))` — mul-succ distributivity
13. `add(mul(x, 1), mul(y, 1)) = add(x, y)` — compositional identity collapse
14. Various LHS patterns from distributivity-scaffold corpus whose RHSs
    are composites, each representing some form of partial distributivity

That's 13-15 DISTINCT mathematical facts. The remaining 785 ledger
entries are alternative expressions of these.

## What this reveals about the loop

The correctness criterion held at the STRUCTURAL level. The novelty
rate stayed at 16/cycle across 50 cycles. But the SEMANTIC rate
collapsed early — after maybe 5-10 cycles, new semantic theorems
stopped emerging. The remaining 40+ cycles produced only equivalent
rewrites of existing discoveries.

Two implications:

1. **Structural dedup is insufficient.** Two theorems that are
   commutatively/associatively equivalent should count as one.
   The existing `theorem_key` uses literal LHS=RHS form, which
   correctly distinguishes `add(x, y) → add(y, x)` from
   `add(x, y) → add(x, y)`, but fails to identify
   `mul(2, x) = add(x, x)` with `mul(2, x) = add(add(x, x), 0)`
   as the same theorem.

2. **Phase K is the next mechanism.** The egg e-graph bridge
   (`mathscape-compress::egraph::check_rule_equivalence`) exists
   and is tested — it was built exactly for this use case. Wiring
   it into `Ledger::insert` would tighten novelty from structural
   to semantic.

## The edge has been riding on structural foam

From the machine's perspective, it's doing its job: finding new
theorem forms every cycle. From an external perspective, the
ledger's semantic content saturated early, then filled with
equivalent restatements.

This is a real bug in the correctness criterion: the criterion
measures structural novelty, but Gödel's guarantee is about
semantic novelty. A satisfying correctness check would say:
"nonzero NEW-SEMANTIC-EQUIVALENCE-CLASSES every cycle."

Fortunately, phase K gives us exactly that.

## Implications for discovery depth

The machine reached:

- **Layer 1**: Identities ✓
- **Layer 2**: Commutativity ✓
- **Layer 3**: Small-constant rewrites (mul(2, x) = add(x, x), etc.)
  ✓
- **Layer 4**: Partial distributivity (`add(mul(x, y), x) = mul(x,
  succ(y))`) ✓ — the genuinely-new layer this run surfaced
- **Layer 5 (associativity)**: NOT REACHED. No theorem of form
  `add(add(x, y), z) = add(x, add(y, z))` appears in the ledger.
  The enumerator goes to size 5; associativity's LHS is already
  size 5 and its RHS is size 5, meaning the generator proposes
  candidates but the anti-unifier never surfaces the LHS shape
  because the DistributivityScaffold corpus doesn't probe that
  structure deeply enough.
- **Layer 5 (full distributivity)**: PARTIAL. We have scaffold
  variants but not the full `mul(x, add(y, z)) = add(mul(x, y),
  mul(x, z))`.

## Next mechanism gap (diagnosed from this run)

**Integrate phase K semantic dedup into the Ledger.**

Mechanism:
- `Ledger::insert(rule)` currently checks `theorem_key(rule) ∉ keys`
- Refine: also check that `rule` is NOT e-graph-equivalent (under
  commutativity + associativity probes) to any existing theorem
- `check_rule_equivalence` from `mathscape-compress::egraph` does
  this directly

Expected effect:
- Ledger size at end of 50 cycles drops from 800 to ~15-30
- Cycle theorem-yield drops to 0-3 after cycle ~10
- Correctness criterion fires at the layer-5 frontier
- Next diagnosis: "need deeper corpus" or "need enumerator size 6"

This replaces a false-positive correctness signal (the machine
thinks it's discovering) with a true signal (the machine will
admit it's not finding new equivalence classes).

## Path forward

1. Wire phase K `check_rule_equivalence` into `Ledger::insert`
2. Re-run 50-cycle campaign
3. Measure where the NEW correctness criterion fires (likely
   cycle 5-15)
4. Apply next mechanism extension (enumerator size 6, richer
   corpus, deeper composition) based on what the halt reveals
5. Repeat

This is the same ride-the-mechanism-edge pattern we've been
doing — each halt diagnosis points at the next specific gap.
Only now the signal is semantic, not structural.
