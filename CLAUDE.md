# Mathscape

Evolutionary symbolic compression engine that discovers mathematical
abstractions by rewarding compression and novelty over expression trees.

## Core Thesis

Mathematical understanding is compression. When humans discover that
`a + b = b + a`, they compress infinitely many concrete observations into
a single symbolic law — commutativity. When they name that pattern `+`,
they compress a binary successor-counting operation into one symbol.
Every layer of mathematical abstraction is a compression step:
arithmetic compresses counting, algebra compresses arithmetic,
category theory compresses algebra.

**Mathscape asks: can we automate this process?**

Given a minimal computational substrate and a reward signal that favors
shorter descriptions of more phenomena, can a search process rediscover
known mathematics — and find new compressions humans haven't seen?

## Foundational Primitives

### Why Not "Point, Number, Function"?

The intuition that math reduces to three kinds of objects (atoms, values,
transformations) is sound as a *computational basis*, but the standard
mathematical foundations use fewer primitives:

| Foundation | Primitives | Everything else is... |
|---|---|---|
| **Set theory (ZFC)** | Sets + membership (`in`) | Sets of sets |
| **Lambda calculus** | Variable, Abstraction, Application | Lambda terms |
| **Type theory (Martin-Lof)** | Types + terms | Typed constructions |
| **Category theory** | Objects + morphisms | Compositions |

Lambda calculus is the closest match: three syntactic forms, and all of
computable mathematics emerges from them. Church numerals encode numbers,
combinators encode logic, fixed-point operators encode recursion.

### Mathscape's Computational Substrate

We adopt a typed expression tree that unifies the user's intuition with
lambda calculus:

```
Term ::= Atom(id)             -- irreducible elements (the "point")
       | Lit(value)            -- numeric/boolean literals (the "number")
       | Fn(param, body)       -- lambda abstraction (the "function")
       | App(func, arg)        -- function application
       | Op(name, args)        -- built-in operator (add, mul, eq, ...)
       | Sym(name)             -- named abstraction (compressed pattern)
```

`Atom`, `Lit`, and `Fn` are the three irreducible *kinds* — everything
else (`App`, `Op`, `Sym`) is sugar for applying functions to atoms and
literals. This preserves the "three primitives" insight while being
computationally precise.

## The Compression-Novelty Reward

### Insight: Comprehension = Compression

From Kolmogorov complexity theory: the complexity of an object is the
length of the shortest program that produces it. A "theory" or "law" is
a short program that produces many observations. The shorter the program
relative to the data it explains, the better the compression — and the
deeper the understanding.

This gives us a principled reward function:

```
reward(library, corpus) = compression_ratio + novelty_bonus

compression_ratio = 1 - (description_length(corpus, library) / raw_length(corpus))

novelty_bonus = sum over each new identity i:
    generality(i) * irreducibility(i)
```

Where:
- **description_length**: size of the corpus when rewritten using the
  library's abstractions (Sym nodes replace repeated subexpressions)
- **raw_length**: size of the corpus as flat expression trees
- **generality**: how many distinct expressions an identity applies to
- **irreducibility**: the identity cannot be derived from existing library entries

### What Gets Rewarded

1. **Finding common subexpressions** — if `(add x (add y z))` and
   `(add (add x y) z)` appear often, discovering associativity and
   compressing both to `assoc(add, x, y, z)` is rewarded.

2. **Finding identities** — if `(mul x 1) => x` holds for all x,
   naming this `mul-identity` compresses every occurrence.

3. **Building abstraction towers** — using `assoc` and `mul-identity`
   together to derive distributivity earns compound rewards because the
   new law compresses *further* on top of existing compressions.

4. **Novel discoveries** — an identity that cannot be decomposed into
   known library entries gets the highest novelty bonus.

## Search Architecture

### Why Not a Neural Network Alone?

A neural network is a function approximator — it maps inputs to outputs
via learned parameters. What the user described ("trying different things
across epochs with a reward function") is **reinforcement learning** or
**evolutionary search**, where the neural network is one component:

| Approach | Strengths | Weaknesses |
|---|---|---|
| **Pure neural net** (supervised) | Fast inference | Needs labeled data we don't have |
| **Reinforcement learning** (AlphaProof-style) | Proven on formal math | Needs a formal verifier (Lean/Coq) |
| **Genetic programming** | Natural fit for expression trees | Slow convergence on large spaces |
| **DreamCoder (wake-sleep)** | Library learning + compression | Complex, needs domain-specific DSL |
| **Hybrid: evolutionary + neural guide** | Best of both worlds | Implementation complexity |

### Mathscape's Approach: Evolutionary Library Learning

We combine genetic programming with DreamCoder-style library extraction:

```
Phase 1: EVOLVE (wake)
  - Maintain a population of expression trees
  - Mutate: random subtree replacement, crossover, simplification
  - Evaluate: run expressions, collect input-output behaviors
  - Select: fitness = compression_ratio + novelty_bonus

Phase 2: COMPRESS (sleep-abstraction)
  - Scan population for repeated subexpressions (anti-unification)
  - Extract common patterns as new Sym entries in the library
  - Rewrite population using new symbols (shorter trees = higher fitness)
  - Use e-graph equality saturation to find equivalent forms

Phase 3: DREAM (sleep-fantasy)
  - Generate synthetic problems using the current library
  - Attempt to solve them, discovering new compositions
  - Failed attempts guide future mutations

Repeat for N epochs.
```

### Key Rust Components

| Crate | Purpose |
|---|---|
| `mathscape-core` | Expression tree, evaluation, substitution |
| `mathscape-compress` | Anti-unification, e-graph integration, library extraction |
| `mathscape-evolve` | Genetic operators, population management, selection |
| `mathscape-reward` | Compression ratio, novelty scoring, fitness |
| `mathscape-cli` | REPL for step-by-step execution and observation |

### Prior Art This Builds On

- **DreamCoder** (Ellis et al., MIT) — wake-sleep library learning for
  program synthesis. We adapt the library extraction but replace the
  neural recognition model with evolutionary search for simplicity.
  [Paper](https://arxiv.org/abs/2006.08381)

- **AlphaProof** (DeepMind) — RL for formal mathematical proof via
  Lean. We share the "search + verify" philosophy but operate on
  expression trees rather than formal proof terms.
  [Paper](https://www.nature.com/articles/s41586-025-09833-y)

- **egg** (Willsey et al.) — Rust e-graph library for equality
  saturation. We use this for finding equivalent expressions efficiently.
  [Crate](https://crates.io/crates/egg)

- **Kolmogorov complexity / MDL** — the theoretical foundation for our
  reward function. Compression = understanding.
  [Overview](https://en.wikipedia.org/wiki/Kolmogorov_complexity)

- **LILO** — neurosymbolic framework extending DreamCoder with LLM-
  grounded library learning.
  [Paper](https://openreview.net/forum?id=TqYbAWKMIe)

## Development Plan

### Phase 0: Substrate (scaffold)
Scaffold the repo using substrate's `rust-library` builder for the core
crate and `rust-binary` for the CLI. Standard pleme-io flake structure.

### Phase 1: Expression Trees + Evaluation
Implement `Term` enum, pattern matching, substitution, and a simple
evaluator over natural numbers. Verify: `(add (succ zero) (succ zero))`
evaluates to `(succ (succ zero))`.

### Phase 2: Genetic Operators
Implement mutation (random subtree swap, constant perturbation, operator
change), crossover, and tournament selection. Verify: population evolves
toward expressions that produce target output sequences.

### Phase 3: Compression Reward
Implement anti-unification to find common subexpressions. Implement
description length computation. Verify: when many trees contain
`(add x (add y z))`, the system extracts `assoc` and rewriting shortens
the corpus.

### Phase 4: Library + E-Graphs
Integrate the `egg` crate for equality saturation. Build the library as
a set of rewrite rules. Verify: the system discovers commutativity and
associativity of addition from examples.

### Phase 5: Novelty Bonus + Identity Discovery
Add the novelty component to the reward. Track which identities are
derivable from the library. Verify: the system discovers multiplicative
identity `(mul x 1) = x` and gets a novelty bonus because it's not
derivable from additive laws.

### Phase 6: REPL + Observation
Build the CLI with step-by-step epoch execution, population inspection,
library browsing, and compression metrics visualization.

## Build & Run

```bash
# Build all crates
nix build

# Run the REPL
nix run .#mathscape-cli

# Run tests
nix run .#test
```

## Conventions

- **Language**: Rust (2024 edition)
- **Build**: substrate `rust-library` / `rust-binary` via Nix
- **Testing**: `cargo test` + nix check
- **Expression format**: S-expression style `(op arg1 arg2)`
- **Library format**: Named rewrite rules `name: lhs => rhs`
