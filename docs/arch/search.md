# Search Architecture

## Overview

Mathscape uses evolutionary search over expression trees as the primary
discovery mechanism. The search is structured as a quality-diversity
algorithm rather than a simple genetic algorithm, ensuring both high
fitness and behavioral diversity.

## Expression Representation

```
Term ::= Point(id)               -- irreducible identity
       | Number(value)            -- irreducible quantity
       | Fn(params, body)         -- irreducible transformation
       | Apply(func, args)        -- function application
       | Symbol(name, arity)      -- compressed pattern (learned)
```

Expressions are hash-consed (blake3) for deduplication and O(1) equality.

## Evolutionary Operators

### Mutation Operators

| Operator | Description | Expected effect |
|---|---|---|
| **Subtree replacement** | Replace random subtree with new random tree | Exploration of novel structures |
| **Operator swap** | Replace one primitive op with another (add<->mul) | Local exploration within operator space |
| **Constant perturbation** | Change a Number value by +/- small delta | Fine-tuning quantitative relationships |
| **Argument reorder** | Swap arguments of a commutative Apply | Discover commutativity/symmetry |
| **Symbol insertion** | Replace a matching subtree with its library Symbol | Exploit known compressions |
| **Symbol expansion** | Expand a Symbol back to its definition, mutate, re-compress | Refine existing abstractions |
| **Hoist** | Extract a deeply nested subtree to top level | Simplification |
| **Wrap** | Wrap an expression in a new Apply node | Build complexity incrementally |

### Crossover

Swap subtrees between two parent expressions at compatible type positions.
Type compatibility is checked structurally — a Number subtree can only
replace another Number subtree.

### Selection

Tournament selection: pick `k` random individuals (default k=7), keep
the one with highest fitness. This balances selection pressure (higher k
= stronger pressure) with diversity preservation.

## Quality-Diversity: MAP-Elites Archive

### Why Not Just a Population?

A standard genetic algorithm converges toward a single fitness peak.
Mathscape needs *diverse* expression structures — discovering `add` and
`mul` simultaneously requires maintaining structurally different
individuals, not just high-fitness ones.

### MAP-Elites Architecture

MAP-Elites maintains a grid of elite solutions where each cell represents
a unique combination of behavioral dimensions. For Mathscape:

**Behavioral dimensions (feature map):**

| Dimension | Discretization | Rationale |
|---|---|---|
| **Expression depth** | Bins: 1-3, 4-6, 7-10, 11+ | Shallow = simple laws, deep = complex theorems |
| **Operator diversity** | Count of distinct operators used | Ensures exploration of different operator combinations |
| **Compression contribution** | Low/medium/high | Separates explorative (novel) from exploitative (compressive) individuals |

Each cell in the 3D grid holds one elite individual — the highest-fitness
expression with that combination of depth, operator diversity, and
compression. When a new individual is generated:

1. Compute its behavioral coordinates
2. If the cell is empty, place it
3. If the cell is occupied, replace only if the new individual has higher fitness

This guarantees:
- Shallow and deep expressions coexist
- Expressions using different operator sets coexist
- Both compressive and novel expressions are maintained

### MAP-Elites + Novelty Interaction

The novelty term in the reward function and MAP-Elites's behavioral
diversity are complementary:

- MAP-Elites ensures *structural* diversity (different tree shapes/operators)
- Novelty scoring ensures *semantic* diversity (irreducible new patterns)

A structurally novel expression (unique MAP-Elites cell) that is
semantically redundant (derivable from library) still scores low novelty.
Both mechanisms are needed.

### Population Sampling

Each epoch samples parents from the MAP-Elites archive with probability
proportional to a curiosity score:

```
curiosity(cell) = time_since_last_improvement(cell) * cell_fitness
```

Cells that haven't improved recently are more likely to be selected —
this naturally directs search pressure toward stagnant regions.

## Parallel Symbolic Enumeration (Complementary)

Recent work on Parallel Symbolic Enumeration (PSE, Nature Computational
Science 2025) shows that systematic enumeration of small expressions,
run in parallel, can outperform evolutionary search for discovering
concise laws. Mathscape can integrate this as a secondary search channel:

- **Evolutionary search**: primary, handles complex multi-step derivations
- **PSE (small-expression enumeration)**: secondary, systematically
  enumerates all expressions up to depth D with current library symbols,
  checking each for interesting properties

PSE is most effective in the early epochs when the library is small and
the expression space is tractable. As the library grows, PSE becomes
the mechanism for testing which combinations of library Symbols yield
new identities.

## RL Policy (Phase 7+)

An optional policy network learns which mutations are productive:

- **State**: expression tree encoded as a token sequence
- **Action**: mutation operator + application site
- **Reward**: change in compression ratio after the epoch

Standard REINFORCE policy gradient. Starts uniform-random, learns to
prefer productive mutation patterns. This is an optimization layer over
the evolutionary search — not a replacement.

## Epoch Loop

```
for epoch in 0.. {
    // 1. Sample parents from MAP-Elites archive
    parents = sample_by_curiosity(archive, batch_size)

    // 2. Generate offspring via mutation + crossover
    offspring = evolve(parents, mutation_rates, library)

    // 3. Evaluate all offspring on test inputs
    results = evaluate(offspring, test_suite)

    // 4. Compress: e-graph saturation, anti-unify, extract Symbols
    (new_symbols, rewritten_corpus) = compress(results, library, egraph)

    // 5. Score fitness = compression + novelty + meta
    fitness = score(offspring, new_symbols, library)

    // 6. Update MAP-Elites archive
    for (individual, fit) in offspring.zip(fitness) {
        archive.try_insert(individual, fit)
    }

    // 7. Update library, persist epoch to storage
    library.extend(new_symbols)
    store.commit_epoch(epoch, archive, library, metrics)
}
```

## References

- [Quality-Diversity: A New Frontier](https://www.frontiersin.org/articles/10.3389/frobt.2016.00040/full) — MAP-Elites and novelty search
- [Parallel Symbolic Enumeration](https://www.nature.com/articles/s43588-025-00904-8) — systematic enumeration for scientific discovery
- [Dominated Novelty Search](https://arxiv.org/html/2502.00593v1) — improved local competition in QD
