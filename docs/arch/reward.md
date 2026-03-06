# Reward Function

## The Combined Reward

```
R(C, L, L_new) = alpha * CR(C, L_new)
               + beta  * novelty(L_new - L, L)
               + gamma * meta_compression(L, L_new)
```

Where:
- `alpha` weights compression (exploitation)
- `beta` weights novelty (exploration)
- `gamma` weights meta-compression (dimensional discovery)
- `L_new - L` = newly discovered Symbols this epoch

Default: `alpha = 0.6, beta = 0.3, gamma = 0.1`

## Term 1: Compression Ratio (CR)

```
CR(C, L) = 1 - DL(C, L) / DL(C, {})

DL(C, L) = |L| + sum over e in C: size(rewrite(e, L))
```

Measures how much the library shortens the corpus description. A CR of
0.5 means the library halves total description length. The library
definitions themselves are counted — creating useless abstractions is
penalized.

### MDL Connection

This is a direct implementation of the Minimum Description Length (MDL)
principle: the best model (library) is the one that minimizes the combined
length of the model itself plus the data encoded using the model. The
theoretical foundation is Kolmogorov complexity — the shortest program
that generates the data.

Since Kolmogorov complexity is uncomputable, CR is our practical
approximation. The expression tree node count is the "code length"
in our coding scheme.

## Term 2: Novelty

```
novelty(symbol, L) = generality(symbol) * irreducibility(symbol, L)

generality(s) = |{ e in C : s matches subexpression of e }| / |C|

irreducibility(s, L) = 1  if s cannot be derived by composing L
                        0  otherwise
```

A Symbol is novel if it:
1. Applies to many expressions in the corpus (general)
2. Cannot be expressed as a composition of existing library Symbols (irreducible)

### Irreducibility Check

To check whether Symbol `s` is derivable from library `L`:

1. Insert `s`'s definition into the e-graph
2. Apply all rewrite rules from `L`
3. If any existing Symbol rewrites to `s`, then `irreducibility = 0`

This is exact when the e-graph reaches saturation. In practice, run
equality saturation with a bounded number of iterations — if `s` isn't
derived within the bound, assume irreducible.

### Continuous Irreducibility (Enhancement)

Binary irreducibility (0 or 1) is coarse. A continuous measure:

```
irreducibility(s, L) = 1 - max over l in L: similarity(s, l)

similarity(s, l) = |shared_subtrees(s, l)| / max(size(s), size(l))
```

This rewards Symbols that are "mostly new" — sharing some structure with
known Symbols but not entirely derivable.

## Term 3: Meta-Compression

```
meta_compression(L, L_new) = 1 - |L_new| / |L_expanded|

L_expanded = L_new with all meta-Symbols expanded to base definitions
```

Measures how much the new library compresses *the old library itself*.
A value of 0 means no library-level compression. A value of 0.5 means
library definitions halved in total size.

### What Triggers Meta-Compression

Meta-compression occurs when a new Symbol simplifies existing Symbols:

```
Before:  add-identity: (add ?x zero) => ?x
         mul-identity: (mul ?x one)  => ?x
         (library size = 2 rules, ~10 nodes each = ~20 nodes)

After:   identity-element: (op ?x (identity op)) => ?x
         (library size = 1 rule, ~8 nodes = ~8 nodes)

meta_compression = 1 - 8/20 = 0.6
```

## Three-Phase Dynamics

The reward function creates natural phase transitions:

### Phase 1: Compression-Dominant

```
CR rising, novelty low, meta_compression zero
alpha * CR >> beta * novelty
```

The system extracts patterns within the current domain. Example: finds
add-commutative, add-associative, add-identity. Each discovery increases
CR. This is exploitation — mining the current territory.

### Phase 2: Compression Equilibrium

```
CR plateaus, novelty becomes the only gradient
alpha * CR ≈ constant, beta * novelty >> 0
```

All extractable patterns in the current domain are found. The only way
to increase reward is through novelty — discovering structure that
*cannot be derived* from the current library. This forces escape from
the current domain.

### Phase 3: Novelty-Driven Escape + Dimensional Discovery

```
New domain entered, fresh compression opportunities
meta_compression spikes when cross-domain abstractions appear
```

The system discovers `mul`, finds it interacts with `add` (distributivity),
opening fresh compression opportunities. CR rises again — Phase 1
restarts in the expanded domain.

When meta-compression fires (e.g., discovering `identity-element`), the
system recognizes cross-domain structure. This is dimensional discovery.

## Adaptive Weight Schedule

The weights `alpha`, `beta`, `gamma` adapt based on phase:

```rust
fn update_weights(metrics: &EpochMetrics, weights: &mut Weights) {
    let cr_delta = metrics.cr - metrics.prev_cr;

    if cr_delta > COMPRESSION_THRESHOLD {
        // Phase 1: compression is productive, lean into it
        weights.alpha = 0.7;
        weights.beta = 0.2;
        weights.gamma = 0.1;
    } else if metrics.meta_compression > META_THRESHOLD {
        // Phase 3: dimensional discovery, reward more meta-compression
        weights.alpha = 0.3;
        weights.beta = 0.3;
        weights.gamma = 0.4;
    } else {
        // Phase 2: equilibrium, maximize novelty pressure
        weights.alpha = 0.3;
        weights.beta = 0.6;
        weights.gamma = 0.1;
    }
}
```

### Dimensional Probes

When meta-compression exceeds a threshold, generate dimensional probes:

```
For each meta-Symbol M with parameter slot P:
  For each known operator O not yet tested in slot P:
    Generate probe expression: M[P=O]
    Add to population with high priority
```

Example: after discovering `identity-element(op, e)`, generate probes:
- `identity-element(pow, ?)` — what is the identity for exponentiation?
- `identity-element(compose, ?)` — what is the identity for function composition?

These probes bias the search toward completing the newly discovered
dimensional structure.

## Fitness Function (Per-Individual)

Individual fitness combines the global reward signal with local contribution:

```
fitness(individual) = w1 * compression_contribution(individual, library)
                    + w2 * behavioral_uniqueness(individual, archive)
                    + w3 * evaluation_correctness(individual)
```

Where:
- `compression_contribution` = how much the library's CR improves when
  patterns from this individual are included
- `behavioral_uniqueness` = inverse density of the individual's MAP-Elites
  cell neighborhood (sparse regions score higher)
- `evaluation_correctness` = fraction of test inputs that produce valid outputs

## References

- [Kolmogorov Complexity](https://en.wikipedia.org/wiki/Kolmogorov_complexity) — theoretical foundation
- [MDL Principle](https://en.wikipedia.org/wiki/Minimum_description_length) — practical approximation
- [SyMANTIC: Interpretable Model Discovery](https://arxiv.org/abs/2502.03367) — recent SR with parsimony
