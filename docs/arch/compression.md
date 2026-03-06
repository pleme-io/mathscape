# Symbolic Compression

## Core Idea

Mathematical understanding is compression. Each layer of abstraction
(counting -> arithmetic -> algebra -> category theory) compresses the
layer below it into fewer symbols. Mathscape automates this by rewarding
shorter descriptions of more phenomena.

## Description Length

Let `L` be a library of named rewrite rules (Symbols), and `C` a corpus
of expression trees. The description length of the corpus under the library:

```
DL(C, L) = |L| + sum over e in C: size(rewrite(e, L))
```

- `|L|` = total size of all library definitions (cost of abstractions)
- `size(rewrite(e, L))` = size of `e` after applying all library rewrites
- `size(t)` = node count in the expression tree

The compression ratio:

```
CR(C, L) = 1 - DL(C, L) / DL(C, {})
```

CR = 0 means useless library. CR = 0.5 means half the description eliminated.

## Library Extraction: STITCH-Inspired Approach

DreamCoder's original compression algorithm is O(n^3) and memory-hungry.
STITCH (Bowers et al., ICLR 2024) achieves 1000-10000x speedup and 100x
memory reduction via branch-and-bound search over candidate abstractions.

### Key STITCH principles to adopt:

1. **Top-down enumeration with pruning**: enumerate candidate abstractions
   from most general to most specific, pruning branches that cannot improve
   the best compression found so far.

2. **Corpus utility score**: for each candidate abstraction `a`, compute
   `utility(a) = (match_count * body_size) - definition_cost`. Only keep
   abstractions with positive utility.

3. **Re-derive from scratch**: because STITCH is fast enough, re-derive
   the entire library from the base corpus at every epoch. This allows
   discarding early suboptimal abstractions rather than being stuck with
   them.

4. **Multi-abstraction extraction**: extract multiple abstractions per
   round in order of decreasing marginal utility. Each extraction rewrites
   the corpus, changing what the next extraction can find.

### Adaptation for Mathscape

STITCH operates on lambda calculus terms. Mathscape uses expression trees
with `Point`, `Number`, `Fn`, `Apply`, `Symbol`. The adaptation:

- **Lambda abstractions -> Pattern variables**: STITCH's `#0`, `#1` hole
  variables map to Mathscape's `?a`, `?b` pattern variables.
- **Application nodes -> Apply nodes**: direct correspondence.
- **Utility = compression contribution**: matches STITCH's objective.

The key departure from STITCH: Mathscape's library is *cumulative* — new
Symbols build on old Symbols. STITCH re-derives from scratch; Mathscape
can do this periodically (every N epochs) but keeps the library stable
between re-derivations to allow the evolutionary search to build on it.

## Anti-Unification

Anti-unification finds the most specific generalization of two terms.
Given:

```
(add (mul x x) (mul y y))
(add (mul a a) (mul b b))
```

Anti-unification yields: `(add (mul ?0 ?0) (mul ?1 ?1))` — a pattern
with two holes.

### Higher-Order Anti-Unification

For deeper abstractions, we need higher-order anti-unification — where
pattern variables can represent functions, not just values. This runs in
linear time for simply-typed lambda terms (Cerna & Kutsia, 2016).

Example:
```
(map (fn [x] (add x 1)) xs)
(map (fn [x] (mul x 2)) xs)
```

Higher-order anti-unification: `(map (fn [x] (?f x ?c)) xs)` — the
function and constant are both abstracted.

## E-Graph Equality Saturation

The `egg` crate performs equality saturation: insert expressions, apply
rewrite rules, and discover all equivalent forms. Used after each epoch to:

1. Insert all evaluated expressions into the e-graph
2. Apply library rewrite rules
3. Extract the smallest equivalent expression for each
4. Identify new equivalence classes (potential new Symbols)

### Incremental E-Graphs

Standard egg discards the e-graph after each saturation run. Recent work
(EGRAPHS 2025) on incremental equality saturation shows how to persist
e-graphs across iterations:

- Assign version numbers to e-classes
- Only explore e-classes added or modified since last saturation
- New terms inherit equalities from previously discovered equivalences

This maps directly to Mathscape's epoch structure: the e-graph persists
across epochs, and each epoch only saturates new/modified expressions.
The benefit is that equalities discovered in epoch 50 still accelerate
rewriting in epoch 500 without re-derivation.

### Implementation Plan

1. Start with standard egg (rebuild each epoch) — simpler, correct baseline
2. Profile compression phase — if it dominates epoch time, switch to
   incremental e-graph
3. Use egg's `Analysis` trait for tracking provenance (which rule produced
   each equivalence)

## Compression Stack Dynamics

The compression stack is self-reinforcing:

```
Epoch 1-10:   discover add, mul         (raw patterns)
Epoch 11-20:  discover commutativity     (properties of operations)
Epoch 21-30:  discover identity-element  (meta-property across operations)
Epoch 31+:    discover "algebraic structure" (meta-meta-property)
```

Each layer compresses the layer below it more than its own definitions
cost. This is the central hypothesis: **net compression grows faster than
definition cost**, making unbounded exploration feasible with bounded memory.

## References

- [STITCH: scalable abstraction learning](https://github.com/mlb2251/stitch) — 1000x faster than DreamCoder compression
- [LILO: LLM-grounded library learning](https://openreview.net/forum?id=TqYbAWKMIe) — STITCH + documentation
- [egg: equality saturation](https://egraphs-good.github.io/) — Rust e-graph library
- [Incremental Equality Saturation](https://rupanshusoi.github.io/pdfs/egraphs-25.pdf) — persistent e-graphs (EGRAPHS 2025)
- [Higher-Order Anti-Unification](https://link.springer.com/article/10.1007/s10817-016-9383-3) — linear time algorithm
