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

### Compression as Tractability

The expression space over three primitives is infinite. With just `Point`,
`Number`, and `Fn`, you can construct every computable function — which
means the search space is at least as large as the space of all programs.
Naively enumerating expression trees of depth `d` with `k` operators
produces `O(k^(2^d))` candidates — double exponential growth. This is
why brute-force mathematical discovery is intractable.

**Compression is the mechanism that makes traversal feasible.**

Each time the system discovers a Symbol (a named compression), it
collapses an entire region of the expression space into a single node.
The expression `(add (mul x x) (add (mul y y) (mul z z)))` is 11 nodes.
After discovering `square: (mul ?a ?a) => ?a^2`, it becomes
`(add (square x) (add (square y) (square z)))` — 7 nodes. After
discovering `sum3: (add ?a (add ?b ?c)) => (sum3 ?a ?b ?c)`, it becomes
`(sum3 (square x) (square y) (square z))` — 4 nodes. The search tree
that once branched through all possible mul/add combinations now sees a
single `sum3-of-squares` node.

This is not just memory efficiency — it changes what the search process
can *reach*. Without compression:

```
Depth 1:  ~k expressions
Depth 2:  ~k^2
Depth 3:  ~k^4
Depth 4:  ~k^8        (millions with k=10)
Depth 5:  ~k^16       (intractable)
```

With a library of `n` Symbols, each Symbol compresses `m` nodes into 1.
The effective branching factor drops from `k` to `k + n` but the
effective depth drops faster — a depth-5 raw expression may be depth-2
in the compressed representation. The search at depth 5 in compressed
space reaches expressions equivalent to depth 15+ in raw space.

**This is exactly how human mathematics works.** No one reasons about
calculus in terms of Peano successor operations. The tower of
abstractions — arithmetic, algebra, analysis — is a compression stack
that lets finite minds traverse an infinite space. Mathscape replicates
this: each epoch's compressions become the next epoch's primitives,
and the frontier of reachable mathematics advances.

The central hypothesis: **the rate of useful compression outpaces the
rate of combinatorial explosion**, making unbounded mathematical
exploration feasible with bounded memory. Each abstraction layer
compresses the layer below it more than the new layer's own
definitions cost, yielding net negative growth in working set size.

### Compression Equilibrium and Novelty Escape

Compression cannot improve forever in a fixed region. Eventually the
system extracts every reusable pattern from a domain — addition is
commutative, associative, has an identity, and there's nothing left
to compress. The compression ratio plateaus. This is **compression
equilibrium**: the local structure is fully described.

At equilibrium, the compression term `alpha * CR` stops growing. The
reward function naturally pivots — the only way to increase total
reward is through the novelty term `beta * novelty(s, L)`. This is
the escape mechanism:

```
Phase 1 (compression-dominant):
  CR rising, novelty low → system extracts patterns within the current domain
  Example: discovers add-commutative, add-associative, add-identity

Phase 2 (equilibrium):
  CR plateaus → compression reward flatlines
  Novelty becomes the only gradient → system must find NEW structure
  that doesn't decompose into existing library symbols

Phase 3 (novelty-driven escape):
  System explores outside the current domain
  Discovers mul, discovers it interacts with add (distributivity)
  Fresh compression opportunities open → CR rises again → Phase 1 restarts
  in the expanded domain
```

This creates a natural oscillation: **compress locally until equilibrium,
then escape to novel territory, then compress again**. The system cannot
get stuck in a local optimum because equilibrium itself kills the
compression gradient and forces novelty-seeking.

The irreducibility requirement in the novelty function is critical here.
Without it, the system could score novelty by trivially recombining
existing symbols — `(add (mul x y) z)` uses known symbols but discovers
nothing. Irreducibility demands that a novel discovery cannot be derived
by composing existing library entries. This forces genuine exploration:
the system must find structure that is *fundamentally new* relative to
everything it already knows.

The dynamics mirror the history of mathematics itself: centuries of work
within Euclidean geometry (compression), then the escape to non-Euclidean
geometry (novelty), then decades compressing the new territory into
Riemannian manifolds. Arithmetic to algebra to abstract algebra. Each
transition is a compression equilibrium followed by a novelty escape
into a larger space.

**Locality is impossible when novelty is irreducible.** The system is
algebraically prohibited from scoring points by rearranging what it
already has. It must leave.

### Recursive Compression and Reward Evolution

The most powerful discoveries are not those that compress raw expressions
but those that compress *the library itself*. When a new Symbol
simplifies already-discovered Symbols, it is a higher-order abstraction
— a compression of compressions. These are the discoveries that open
entirely new dimensions of the problem space.

**Example — the discovery of "identity element":**

The library contains:
```
add-identity:  (add ?x zero) => ?x
mul-identity:  (mul ?x one)  => ?x
```

These are two independent Symbols. But a new abstraction can compress
them both:
```
identity-element: (op ?x (identity op)) => ?x
    where add-identity = identity-element[op=add, identity=zero]
    and   mul-identity = identity-element[op=mul, identity=one]
```

This is not just a new Symbol — it *retroactively simplifies the
library*. Two entries become instantiations of one. The library itself
gets shorter. And the new Symbol generates a *search directive*: for
any future operator `op` the system discovers, it should look for an
identity element. It predicts the existence of structure it hasn't
seen yet.

#### Meta-Compression Reward

This recursive compression introduces a third reward term beyond
compression ratio and novelty:

```
R(C, L, L_new) = alpha * CR(C, L_new)
               + beta  * novelty(L_new - L, L)
               + gamma * meta_compression(L, L_new)

meta_compression(L, L_new) = 1 - |L_new| / |L_expanded|

where L_expanded is L_new with all meta-symbols expanded back to
their base definitions
```

`meta_compression` measures how much the new library compresses the old
one. A value of 0 means no library-level compression occurred. A value
of 0.5 means the library definitions themselves halved in total size.

#### Dimensional Escape

Meta-compressions don't just save space — they reveal **new problem
space dimensions**. When `identity-element` is discovered, the system
implicitly learns that operators can be parameterized and that structural
properties (identity, associativity, commutativity) are themselves
objects that vary across operators. This is the jump from arithmetic
to algebra — from "addition has properties" to "operators have
properties."

The reward function should adapt when meta-compressions occur:

```
When meta_compression(L, L_new) > threshold:
  - Increase gamma (reward more meta-compression — there may be more)
  - Generate dimensional probes: for each meta-symbol, instantiate it
    with every known operator and check which instances hold
  - Bias mutations toward the new dimension (e.g., try other operators
    in the slot where identity-element was parameterized)
```

This is **reward function evolution** — not a fixed objective but one
that reshapes itself as the system's understanding deepens. The three
regimes become:

| Regime | Dominant term | What the system does |
|---|---|---|
| **Compression** | `alpha * CR` | Extract patterns within a domain |
| **Novelty escape** | `beta * novelty` | Leave exhausted domains for new ones |
| **Dimensional discovery** | `gamma * meta_compression` | Find cross-domain structure that compresses the library itself |

The third regime is the most powerful because it doesn't just find new
territory — it reveals that territories previously thought separate
are *instances of the same structure*. This is how mathematics discovers
its deepest unifications: group theory unifies symmetries across geometry
and number theory, category theory unifies group theory and topology,
homotopy type theory unifies category theory and logic.

Each dimensional discovery doesn't just compress — it generates a new
**axis of variation** that the evolutionary search can explore. Before
`identity-element`, the system searched over specific operator-value
pairs. After it, the system searches over *structural properties of
operators* — a qualitatively different and more powerful search space.

The compression stack becomes self-reinforcing: compressions power
discovery, discoveries yield meta-compressions, meta-compressions
reshape the reward landscape to seek further dimensional structure.
The system bootstraps its own capacity to explore.

## The Three Computational Primitives

All of mathematics can be explored from three irreducible kinds of object:

### 1. Point — the atom of identity

A point is a thing that *is* — nothing more. It has no internal structure,
no value, no behavior. It exists only to be distinguished from other points.
Points are the ground truth of mathematics: before you can count, compare,
or transform, you need *things* to act on.

In set theory, a point is an element. In geometry, it is a location. In
type theory, it is an inhabitant. The specific formalism doesn't matter —
what matters is that points are the irreducible substrate on which all
structure is built.

```
Point(id)    -- an opaque, distinguishable atom
```

### 2. Number — the atom of quantity

A number is a point with *position* — it encodes magnitude, order, or
cardinality. Numbers emerge the moment you distinguish "how many" from
"which one." The simplest construction: a point is `zero`, and `succ`
applied to a number is the next number. From this, all of arithmetic
follows.

Numbers are not just integers — they are any value that admits comparison
and combination: naturals, rationals, reals, complex numbers, ordinals.
The key property is that numbers carry *quantitative information* that
points do not.

```
Number(value)    -- a quantity: natural, rational, real, or symbolic
```

### 3. Function — the atom of transformation

A function is a *rule that maps inputs to outputs*. It is the only
dynamic primitive — points and numbers are static, functions are active.
Every operation in mathematics is a function: addition maps two numbers
to a number, a proof maps hypotheses to conclusions, a symmetry maps
a structure to itself.

Functions are what make mathematics *generative*. Without them, you have
a static collection of points and numbers. With them, you can build
the entire edifice: arithmetic is functions over numbers, algebra is
functions over functions, calculus is functions over continuous functions.

```
Fn(params, body)    -- a transformation: input -> output
```

### The Expression Tree

These three primitives compose into an expression tree — the universal
representation for mathematical objects in Mathscape:

```
Term ::= Point(id)               -- irreducible identity
       | Number(value)            -- irreducible quantity
       | Fn(params, body)         -- irreducible transformation
       | Apply(func, args)        -- function application
       | Symbol(name, arity)      -- compressed pattern (learned)
```

The first three are the primitives. `Apply` is the act of using a
function. `Symbol` is what Mathscape *discovers* — a named compression
of a repeated pattern, like `+` or `assoc` or `derivative`.

The entire search process is: start with Point, Number, and Fn. Evolve
expressions. Find patterns. Compress them into Symbols. Repeat.

## Symbolic Compression — Formalized

### The Compression Function

Let `L` be a library of symbols (named rewrite rules), and `C` be a
corpus of expressions. The **description length** of the corpus under
the library is:

```
DL(C, L) = |L| + sum over e in C: size(rewrite(e, L))
```

Where:
- `|L|` is the total size of all library definitions (you pay for the
  abstractions you create)
- `size(rewrite(e, L))` is the size of expression `e` after replacing
  all matching subexpressions with their Symbol names
- `size(t)` counts nodes in the expression tree

The **compression ratio** is:

```
CR(C, L) = 1 - DL(C, L) / DL(C, {})
```

A compression ratio of 0 means the library is useless. A ratio of 0.5
means the library halves the total description length. Higher is better.

### The Novelty Function

Not all compressions are equal. Discovering `(add x 0) = x` is more
valuable than noticing that `(add 3 4)` appears twice. Novelty measures
the *independence* and *generality* of a discovery:

```
novelty(symbol, L) = generality(symbol) * irreducibility(symbol, L)

generality(s) = |{ e in C : s matches a subexpression of e }| / |C|

irreducibility(s, L) = 1  if s cannot be derived by composing existing symbols in L
                        0  otherwise
```

A symbol that matches 80% of the corpus and cannot be derived from
existing library entries has `novelty = 0.8 * 1.0 = 0.8`.

### The Reward Function

The total reward for an epoch combines compression and novelty:

```
R(C, L, L_new) = alpha * CR(C, L_new) + beta * sum over s in (L_new - L): novelty(s, L)
```

Where:
- `alpha` weights compression (exploitation — use what you've found)
- `beta` weights novelty (exploration — find new things)
- `L_new - L` is the set of newly discovered symbols this epoch

Default: `alpha = 0.6, beta = 0.4` — slightly favor compression over
novelty, since a compression that doesn't hold generally will be pruned
anyway.

## Search Process — Evolutionary with RL Guidance

### Architecture

```
                    +------------------+
                    |   Expression     |
                    |   Population     |
                    +--------+---------+
                             |
              +--------------+--------------+
              |                             |
     +--------v---------+        +---------v--------+
     |  EVOLVE           |        |  EVALUATE        |
     |  - mutate trees   |        |  - run exprs     |
     |  - crossover      |        |  - collect I/O   |
     |  - guided by      |        |  - check eqs     |
     |    policy net     |        |                  |
     +--------+----------+        +---------+--------+
              |                             |
              +--------------+--------------+
                             |
                    +--------v---------+
                    |  COMPRESS         |
                    |  - anti-unify     |
                    |  - e-graph sat    |
                    |  - extract Syms   |
                    |  - rewrite corpus |
                    +--------+---------+
                             |
                    +--------v---------+
                    |  REWARD           |
                    |  - CR(C, L)       |
                    |  - novelty(s, L)  |
                    |  - update policy  |
                    +------------------+
                             |
                         next epoch
```

### Evolutionary Search (primary)

The population is a set of expression trees. Each epoch:

1. **Selection**: Tournament selection — pick k random individuals,
   keep the one with highest fitness.
2. **Mutation**: Random subtree replacement, operator swap, constant
   perturbation, argument reordering.
3. **Crossover**: Swap subtrees between two parent expressions.
4. **Evaluation**: Run each expression on a set of test inputs,
   record input-output behavior.
5. **Fitness**: `compression_contribution + novelty_contribution`
   where compression_contribution measures how much this individual's
   patterns contribute to the library's compression power.

### RL Policy (optional, Phase 7+)

A small policy network learns which mutations are productive:

- **State**: current expression tree (encoded as a sequence of tokens)
- **Action**: which mutation operator to apply and where
- **Reward**: change in compression ratio after the epoch

This is standard policy gradient RL (REINFORCE). The policy starts
uniform-random and gradually learns to prefer mutations that lead to
compressible patterns. This is an optimization over pure evolutionary
search — not a replacement.

### Compression via E-Graphs

After each epoch, the `egg` e-graph library performs equality saturation:

1. Insert all evaluated expressions into the e-graph
2. Apply known rewrite rules (from the library)
3. Extract the smallest equivalent expression for each
4. Anti-unify across expressions to find new common patterns
5. Patterns that compress the corpus become new library Symbols

## Rust Crate Structure

| Crate | Purpose |
|---|---|
| `mathscape-core` | `Point`, `Number`, `Fn`, `Term` enum, evaluation, substitution, s-expr parser |
| `mathscape-compress` | Anti-unification, e-graph integration (`egg`), library extraction, rewriting |
| `mathscape-evolve` | Genetic operators, population management, tournament selection |
| `mathscape-reward` | Description length, compression ratio, novelty scoring, combined fitness |
| `mathscape-policy` | Optional RL policy network for guided mutation (Phase 7+) |
| `mathscape-cli` | REPL for step-by-step epoch execution, population/library inspection |

## Prior Art

- **DreamCoder** (Ellis et al., MIT) — wake-sleep library learning for
  program synthesis. We adapt the library extraction and compression
  reward but use evolutionary search instead of enumeration.
  [Paper](https://arxiv.org/abs/2006.08381)

- **AlphaProof** (DeepMind) — RL for formal mathematical proof in Lean.
  Demonstrates that RL + formal verification can solve Olympiad-level
  problems. We share the search-and-verify philosophy.
  [Paper](https://www.nature.com/articles/s41586-025-09833-y)

- **egg** (Willsey et al.) — Rust e-graph library for equality saturation.
  Core dependency for finding equivalent expressions.
  [Crate](https://crates.io/crates/egg)

- **Kolmogorov complexity / MDL** — theoretical foundation for the reward
  function. The minimum description length principle: the best model is
  the shortest one that explains the data.
  [Overview](https://en.wikipedia.org/wiki/Kolmogorov_complexity)

- **LILO** — neurosymbolic framework extending DreamCoder with LLM-grounded
  library learning and documentation.
  [Paper](https://openreview.net/forum?id=TqYbAWKMIe)

## Development Plan

### Phase 0: Scaffold
Scaffold the repo using substrate's `rust-library` builder for core crates
and `rust-binary` for the CLI. Standard pleme-io flake structure with
workspace Cargo.toml.

### Phase 1: Primitives + Expression Trees
Implement `Point`, `Number`, `Fn`, `Apply`, `Symbol` as a Rust enum.
S-expression parser and printer. Simple evaluator over naturals using
Peano arithmetic (`zero`, `succ`, `add`, `mul`). Property tests for
evaluation correctness.

### Phase 2: Evolutionary Search
Population of expression trees. Mutation operators (subtree swap, op
change, constant perturb). Crossover. Tournament selection. Fitness =
output correctness for now. Verify: evolve expressions that compute
`add(2,3) = 5`.

### Phase 3: Compression Reward
Description length computation. Anti-unification to find common
subexpressions. Library as a `Vec<(Symbol, RewriteRule)>`. Compression
ratio calculation. Verify: repeated `(add x 0)` patterns get extracted
into an `add-identity` Symbol.

### Phase 4: E-Graph Integration
Integrate `egg` for equality saturation. Insert expressions, apply
library rules, extract minimal forms. Verify: discovers that
`(add a (add b c))` and `(add (add a b) c)` are equivalent.

### Phase 5: Novelty Scoring
Track derivability — can a Symbol be composed from existing library
entries? Score novel discoveries higher. Combined reward function with
alpha/beta weights. Verify: discovering `mul-identity` earns novelty
bonus when library only contains additive laws.

### Phase 6: CLI + Observation
REPL with commands: `step` (one epoch), `run N` (N epochs), `pop`
(inspect population), `lib` (browse library), `stats` (compression
metrics). Step-by-step execution to observe the system bootstrapping
from primitives.

### Phase 7: RL Policy (stretch)
Small policy network (simple MLP) trained via REINFORCE to guide
mutation selection. State = expression encoding, action = mutation
choice, reward = delta compression ratio.

## Build & Run

```bash
nix build              # build all crates
nix run .#cli          # launch the REPL
nix run .#test         # run all tests
```

## Conventions

- **Language**: Rust (2024 edition)
- **Build**: substrate builders via Nix
- **Testing**: `cargo test` + `nix flake check`
- **Expression format**: S-expression — `(add (succ zero) (succ zero))`
- **Library format**: Named rewrite rules — `add-identity: (add ?x zero) => ?x`
- **Compression metrics**: Reported per-epoch as `CR`, `DL`, `novelty`, `|L|`
