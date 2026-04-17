# Mathscape

Evolutionary symbolic compression engine that discovers mathematical
abstractions by rewarding compression and novelty over expression trees.

## Forced Realization Frame — read this first

Mathscape is a **forced realization machine**: a control system that
pushes proposals through a ten-gate lattice (discovery + reinforcement
+ promotion), driven by five forces measured in a single currency
(ΔDL), emitting a typed event stream whose aggregate score selects the
next action. Every Rust primitive that exists was reached by this
machine under some policy. Every primitive that does not exist is
*unreached, not unreachable*. The operator controls ε/K/N/M; the
machine produces the trajectory.

**Canonical architecture docs** (read in this order):

1. `docs/arch/machine-synthesis.md` — **the canonical picture**. Five
   architectural objects, ten gates, five forces, three regimes,
   type-level invariants. If you only read one, read this.
2. `docs/arch/forced-realization.md` — why the machine must exist;
   the control-system framing.
3. `docs/arch/axiomatization-pressure.md` — reinforcement is the
   default loop; discovery fires only on plateau.
4. `docs/arch/reward-calculus.md` — ΔDL as the single currency; the
   allocator decides reinforce vs discover on expected ΔDL / cost.
5. `docs/arch/condensation-reward.md` — MDL objective per proposal;
   why coverage preservation is load-bearing.
6. `docs/arch/promotion-pipeline.md` — mathscape → axiom-forge handoff;
   gates 4–7; migration reports.
7. `docs/arch/minimal-model-ladder.md` — "whatever is simplest to get
   to the next step"; levels 0–7.
8. `docs/arch/realization-plan.md` — the phased implementation plan
   (A through L).

The sections below (Core Thesis, Compression as Tractability, etc.)
are preserved because they explain the *philosophy* the machine
implements. Where they and the arch docs disagree, the arch docs win
— in particular, regime names are canonical as Reductive / Explosive /
Promotive.

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

### Determinism, Proof, and the Derivation Record

The primitives are fixed. The evaluation rules are fixed. The rewrite
rules, once discovered, are fixed. This means:

**Every chain of computation from primitives to result is deterministic
and reproducible.** Given the same expression and the same library, you
get the same evaluation trace every time. The randomness lives in the
*search* (which mutations to try), never in the *verification* (whether
a discovered identity actually holds).

This has a profound consequence: **every verified discovery carries an
inherent constructive proof.**

#### The Curry-Howard Lens

The [Curry-Howard correspondence](https://en.wikipedia.org/wiki/Curry%E2%80%93Howard_correspondence)
establishes that programs ARE proofs and types ARE propositions. When
Mathscape evaluates `(add (succ zero) (succ (succ zero)))` and gets
`(succ (succ (succ zero)))`, the evaluation trace IS a constructive
proof that `1 + 2 = 3`. Not a claim, not a test — the computation
itself is the proof object.

For identities (universally quantified equations like `(add x zero) = x`),
the proof structure is inductive because the primitives are inductive:

```
Numbers are inductively defined:
    zero : Number
    succ : Number -> Number

Operations are defined by structural recursion:
    add(zero, y)    = y              -- base case
    add(succ(x), y) = succ(add(x, y))  -- inductive step

Therefore add(x, zero) = x is proved by induction on x:
    Base:      add(zero, zero) = zero            ✓ (by definition)
    Inductive: assume add(x, zero) = x
               add(succ(x), zero)
               = succ(add(x, zero))              (by definition of add)
               = succ(x)                         (by inductive hypothesis)  ✓
```

The evaluation engine performs exactly these steps. The chain of
rewrites it applies IS the induction. If the system discovers that
`(add x zero)` always reduces to `x` by exhaustive evaluation AND the
e-graph confirms the equivalence via rewrite rules derived from the
inductive definitions, then the e-graph derivation is a formal proof.

#### Two Levels of Verification

There is a nuance: testing an identity on finitely many values is
empirical evidence, not a proof. Mathscape operates at two levels:

1. **Empirical discovery** (evolutionary search): test `(add x zero) = x`
   for `x = 0, 1, 2, ..., 100`. This is how the identity is *found*.
   It's strong evidence but not a proof — maybe it fails at `x = 101`.

2. **Structural verification** (e-graph + rewrite rules): insert both
   sides into the e-graph, apply the rewrite rules (which are derived
   from inductive definitions), and check if both sides land in the
   same equivalence class. If they do, the [Church-Rosser property](https://en.wikipedia.org/wiki/Church%E2%80%93Rosser_theorem)
   guarantees that the equivalence holds for ALL inputs, not just the
   tested ones. This IS a proof.

The system should track which level each discovery has reached:

```
Status::Conjectured  -- observed empirically, not yet verified
Status::Verified     -- confirmed via e-graph equivalence (proof exists)
Status::Exported     -- proof certificate emitted for external verification
```

#### What This Means for Storage

The lineage table is not just bookkeeping — **it is a proof database**.
Every row in the lineage table is a step in a derivation. The chain
from primitives to a discovered Symbol, reconstructed from the lineage
table, is a complete proof of that Symbol's validity.

This changes what we store and how:

```sql
-- Evaluation traces: the atomic proof steps
CREATE TABLE eval_traces (
    trace_id      INTEGER PRIMARY KEY,
    expr_hash     BLOB NOT NULL,       -- expression being evaluated
    step_index    INTEGER NOT NULL,     -- position in the trace
    rule_applied  TEXT NOT NULL,        -- which rewrite rule fired
    before_hash   BLOB NOT NULL,       -- expression before rewrite
    after_hash    BLOB NOT NULL,       -- expression after rewrite
    epoch         INTEGER NOT NULL
);
CREATE INDEX idx_traces_expr ON eval_traces(expr_hash);

-- Proof certificates: completed proofs of verified identities
CREATE TABLE proofs (
    proof_id      INTEGER PRIMARY KEY,
    symbol_id     INTEGER NOT NULL REFERENCES library(symbol_id),
    proof_type    TEXT NOT NULL,        -- "inductive", "equational", "compositional"
    status        TEXT NOT NULL,        -- "conjectured", "verified", "exported"
    lhs_hash      BLOB NOT NULL,
    rhs_hash      BLOB NOT NULL,
    trace_ids     BLOB NOT NULL,       -- serialized list of trace_ids constituting the proof
    epoch_found   INTEGER NOT NULL,
    epoch_verified INTEGER,
    lean_export   TEXT                  -- optional Lean 4 proof term
);

-- Proof dependencies: which proofs depend on which
CREATE TABLE proof_deps (
    proof_id      INTEGER NOT NULL REFERENCES proofs(proof_id),
    depends_on    INTEGER NOT NULL REFERENCES proofs(proof_id),
    PRIMARY KEY (proof_id, depends_on)
);
```

#### What We Can Do With It

1. **Independent verification**: replay any proof by loading its trace
   from the database and re-executing the rewrite chain. Deterministic
   inputs + deterministic rules = same result every time.

2. **Proof export**: emit proof certificates in Lean 4 or Coq syntax.
   The e-graph derivation maps directly to equational reasoning steps
   in a formal proof assistant. An external verifier can confirm every
   discovery without trusting Mathscape's implementation.

3. **Proof composition**: if Symbol A is proven and Symbol B is proven,
   a derivation that uses both A and B inherits their proofs. The
   `proof_deps` table tracks this — the proof of `distributivity` depends
   on the proofs of `associativity` and `mul-identity`.

4. **Proof compression**: proofs are themselves expressions. A proof
   that appears repeatedly across different Symbols can be compressed
   into a proof *lemma* — a meta-proof. This feeds back into the
   compression reward: **compressing proofs is discovering deeper
   mathematical structure**.

5. **Searchable proof corpus**: over thousands of epochs, the proofs
   table becomes a growing body of machine-generated, machine-verified
   mathematical knowledge. Query it:
   ```sql
   -- All proven identities involving multiplication
   SELECT l.name, p.proof_type, p.status
   FROM proofs p JOIN library l ON p.symbol_id = l.symbol_id
   WHERE l.name LIKE '%mul%' AND p.status = 'verified';

   -- Proof dependency tree for distributivity
   WITH RECURSIVE deps AS (
       SELECT depends_on FROM proof_deps WHERE proof_id = ?
       UNION ALL
       SELECT pd.depends_on FROM proof_deps pd
       JOIN deps d ON pd.proof_id = d.depends_on
   )
   SELECT l.name FROM deps d
   JOIN proofs p ON p.proof_id = d.depends_on
   JOIN library l ON l.symbol_id = p.symbol_id;
   ```

6. **Proof-guided search**: the structure of existing proofs informs
   the search. If every proven identity about `add` was discovered
   via induction on the first argument, the system can bias mutations
   to try induction on the first argument of `mul` — transferring
   proof strategies across domains.

## Architecture Documents

Detailed subsystem designs live in `docs/arch/`:

| Document | Covers |
|---|---|
| [compression](docs/arch/compression.md) | STITCH-style abstraction learning, incremental e-graphs, anti-unification |
| [search](docs/arch/search.md) | Evolutionary search, MAP-Elites quality-diversity archive, PSE |
| [reward](docs/arch/reward.md) | Adaptive weight schedule, continuous irreducibility, dimensional probes |
| [storage](docs/arch/storage.md) | redb + PostgreSQL (SeaORM), epoch transactions, memory budget, growth estimates |
| [proofs](docs/arch/proofs.md) | Curry-Howard, e-graph verification, Lean 4 export, AI prover integration |
| [mcp](docs/arch/mcp.md) | MCP interface: observe-only tools, security boundary, agent interaction patterns |
| [service](docs/arch/service.md) | Service mode: HTTP endpoints, engine loop, Prometheus metrics, three binaries |
| [deployment](docs/arch/deployment.md) | K8s deployment: Docker image, Helm chart, FluxCD, substrate patterns |
| [discovery](docs/arch/discovery.md) | Discovery mining: epoch analysis, known-math mapping, representation layer for React UI |

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

### Data Representation Design Decisions

The expression representation is the most performance-critical data
structure in the system. Every epoch touches every expression. These
choices were deliberated carefully:

**Two-tier representation (Term vs StoredTerm)**:
- `Term` — in-memory, children inline (owned). Used during evaluation,
  mutation, and pattern matching within a single epoch. Optimized for
  traversal speed (cache locality, no indirection).
- `StoredTerm` — on-disk, children are `TermRef` (32-byte blake3 hash).
  Used in redb. Optimized for deduplication and structural sharing.
- Converting between them is cheap and only happens at epoch boundaries.

**Why `enum` not trait objects**:
- Expressions are small, frequently cloned, and pattern-matched on every
  access. An enum with inline data beats trait object indirection + vtable
  dispatch. The compiler can optimize match arms into jump tables.

**Why `Vec<Term>` args not fixed-size**:
- Mathematical operations vary in arity (nullary constants, unary negation,
  binary add, n-ary sum). Fixed-size would require separate variants for
  each arity, bloating the enum. Vec is heap-allocated but amortizes well
  for the 1-4 arg range typical of mathematical expressions.
- Future optimization: `SmallVec<[Term; 2]>` to inline the common case
  (2 args) and only heap-allocate for 3+ args.

**Why `u32` not `String` for variable IDs**:
- Variables are compared millions of times per epoch (pattern matching,
  substitution). `u32` comparison is 1 instruction vs String comparison
  requiring pointer chase + length check + memcmp.

**Why `blake3` not `sha256`**:
- blake3 is ~5x faster than SHA-256 on modern hardware (SIMD-accelerated).
  Hash computation happens for every new expression. At 10k individuals
  × ~50 nodes each × mutation rate, that's ~500k hashes per epoch.

**Why `bincode` for serialization**:
- Compact binary format (no field names), fast serialize/deserialize.
  Expressions are internal data, not an interchange format. JSON/msgpack
  overhead is wasted here.

**StoredTerm in redb**: write-once immutable. The hash IS the key.
Zero-copy reads via redb's mmap. No serialization on read path for
cached entries.

**PostgreSQL metadata**: relational data (population snapshots, epoch
metrics, lineage) where SQL queries are natural. SeaORM entities provide
type safety. Connection pooling for concurrent access from service + MCP.

**Discovery representation (JSON)**: designed for React UI consumption.
Deliberate denormalization — each Discovery object contains everything
the frontend needs for one card/row without additional queries. This
trades storage for rendering speed.

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

Default: `alpha = 0.6, beta = 0.3, gamma = 0.1` (gamma is the
meta-compression term introduced in "Recursive Compression" below).
See [docs/arch/reward.md](docs/arch/reward.md) for adaptive weight schedules.

## Search Process — Evolutionary with RL Guidance

See [docs/arch/search.md](docs/arch/search.md) for full details including
MAP-Elites quality-diversity archive and parallel symbolic enumeration.

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

See [docs/arch/compression.md](docs/arch/compression.md) for STITCH-style
library extraction (1000x faster than DreamCoder) and incremental e-graphs
(persist across epochs instead of rebuilding).

After each epoch, the `egg` e-graph library performs equality saturation:

1. Insert all evaluated expressions into the e-graph
2. Apply known rewrite rules (from the library)
3. Extract the smallest equivalent expression for each
4. Anti-unify across expressions to find new common patterns
5. Patterns that compress the corpus become new library Symbols

## Storage Architecture

### Principle: Memory is a Window, Disk is the Landscape

The expression space is unbounded. Holding the full population, all
discovered expressions, the e-graph, and the library in memory is
naive — it grows without limit as epochs accumulate. Instead, memory
holds only what the current epoch needs. Everything else lives on disk
in purpose-matched databases. Each epoch is a transaction: load the
working set, compute, write results, release.

### Data Types and Their Storage Characteristics

| Data | Shape | Access pattern | Volume | Lifetime |
|---|---|---|---|---|
| **Expression trees** | Tree (DAG with sharing) | Write-once, read-many by hash | Unbounded, grows every epoch | Permanent |
| **Population** | Set of (root_hash, fitness) | Bulk replace each epoch | Fixed size (e.g., 10k) | Current epoch |
| **Library** | Ordered list of rewrite rules | Append-mostly, full scan for rewrite | Grows slowly | Permanent |
| **E-graph state** | Union-find + hash-cons | Build from scratch each epoch | Large but transient | Single epoch |
| **Epoch metrics** | Time series of scalars | Append-only | One row per epoch | Permanent |
| **Lineage** | DAG of derivations | Write-once, query by hash | Grows every epoch | Permanent |
| **Evaluation cache** | (expr_hash, inputs) -> outputs | Write-once, lookup by key | Large, deduplicates | Permanent |

### Hash-Consing: The Foundation

Every expression tree is **hash-consed** — each unique subtree is
identified by its content hash (blake3). Children are stored as hash
references, not inline. This gives:

- **Deduplication**: a population of 10,000 trees sharing common
  subtrees (e.g., `(add ?x zero)`) stores each unique subtree exactly
  once. In practice, populations share 60-90% of their subtrees.
- **O(1) equality**: same hash = same expression. No tree traversal.
- **Natural content-addressing**: expressions are their own keys.
- **Structural sharing across epochs**: mutations that change one
  subtree reuse all other subtrees by reference.

```rust
struct TermRef(blake3::Hash);  // 32 bytes, points into the store

enum StoredTerm {
    Point(u64),
    Number(Value),
    Fn(Vec<TermRef>, TermRef),     // param hashes, body hash
    Apply(TermRef, Vec<TermRef>),  // func hash, arg hashes
    Symbol(SymbolId, Vec<TermRef>), // symbol id, arg hashes
}
```

A depth-10 expression tree with 1,000 nodes but heavy subtree sharing
may only require 50 unique `StoredTerm` entries in the database.

### Database Selection

Two databases, clean separation by access pattern:

#### 1. redb — Content-Addressed Expression Store

[redb](https://crates.io/crates/redb) is a pure-Rust embedded key-value
store. No C dependencies, no FFI, ACID transactions, crash-safe. Ideal
for the hash-consed expression store because:

- **Write-once semantics**: expressions are immutable once hashed
- **Sequential write, random read**: mutations write new subtrees,
  evaluation reads by hash
- **No query language needed**: pure key-value, the hash is the key
- **Zero-copy reads**: redb supports zero-copy access to values
- **Embeds into the binary**: no external server process

Tables in redb:

```
expressions:    blake3::Hash -> bincode(StoredTerm)
eval_cache:     (blake3::Hash, InputSet) -> OutputSet
```

Why not RocksDB: C++ dependency, complex tuning, overkill for
write-once workloads. Why not sled: stability concerns, unclear
maintenance status. redb is simple, correct, and pure Rust.

#### 2. PostgreSQL — Structured Metadata (via SeaORM)

PostgreSQL via `sea-orm` for everything that benefits from relational
queries, ordering, aggregation, and `WITH RECURSIVE` graph traversals:

**Tables** (managed by SeaORM Rust migrations in `mathscape-migration`):
- `population` — per-epoch population snapshots with MAP-Elites bins
- `library` — discovered symbols (append-only)
- `epochs` — per-epoch metrics (compression ratio, novelty, weights, phase)
- `eval_traces` — atomic proof steps (rule applied, before/after hashes)
- `proofs` — proof certificates with Lean 4 export
- `lineage_events` — derivation DAG (denormalized from redb for relational queries)
- `symbol_deps` — symbol dependency graph (materialized from redb)
- `proof_deps` — proof dependency graph (materialized from redb)

SeaORM entity models live in `mathscape-store/src/entity/`.
Migrations live in `mathscape-migration/src/`.

Why PostgreSQL over SQLite: `WITH RECURSIVE` CTEs for lineage/dependency
graph traversal, JSONB for flexible metadata, concurrent read access from
the service + MCP + CLI binaries without file locking, and natural
integration with K8s (CNPG operator or external managed Postgres).

#### 3. Graph Data Architecture

Graph structure lives in **redb adjacency tables** (source of truth):
- `lineage_forward` / `lineage_reverse` — parent->child derivation edges
- `symbol_deps_forward` / `symbol_deps_reverse` — symbol dependency edges
- `proof_deps_forward` / `proof_deps_reverse` — proof dependency edges

Hot-path graph ops (single-hop: "get parents", "get children") use redb
for O(1) embedded lookup. Cold-path analytics (multi-hop traversals,
ancestor chains) use PostgreSQL `WITH RECURSIVE` over denormalized copies.

No dedicated graph database — redb handles hot-path graph traversal,
PostgreSQL handles cold-path graph analytics. If graph analytics become
a bottleneck, Apache AGE (openCypher extension for PostgreSQL) can be
added without architecture changes.

### Epoch Memory Budget

Each epoch loads into memory only:

| Data | In-memory representation | Size estimate |
|---|---|---|
| Population index | `Vec<(TermRef, f64)>` — hashes + fitness | ~240 KB for 10k individuals |
| Active subtrees | LRU cache of recently accessed `StoredTerm` | Configurable, e.g., 100 MB |
| Library | Full `Vec<Symbol>` — always small | < 1 MB even with 1000s of symbols |
| E-graph | Built from scratch, dropped after compress phase | Transient, bounded by population size |
| Reward state | Scalar accumulators | Negligible |

Total working memory: **~100-200 MB** regardless of how many epochs
have run or how large the total expression store has grown. The redb
file may grow to gigabytes over thousands of epochs, but the memory
footprint stays constant.

### Write Strategy

After each epoch:

1. **Batch-write new expressions** to redb (mutations + crossover
   products). Single transaction, sequential writes.
2. **Bulk-insert population** into SQLite (DELETE old epoch rows,
   INSERT new). Single transaction.
3. **Append library entries** if new Symbols were discovered.
4. **Append epoch metrics** row.
5. **Append lineage records** for all new expressions.
6. **fsync** both databases.

The epoch boundary is the transaction boundary. If the process crashes
mid-epoch, both databases roll back to the end of the previous epoch.
No partial state, no corruption, deterministic resume.

### Queryable History

Because everything is persisted with epoch tags, the full history of
the search is queryable after the fact:

```sql
-- Compression ratio over time
SELECT epoch, compression_ratio FROM epochs ORDER BY epoch;

-- When was associativity discovered?
SELECT epoch_discovered, name FROM library WHERE name LIKE '%assoc%';

-- What was the population diversity when novelty spiked?
SELECT e.epoch, e.population_diversity, e.novelty_total
FROM epochs e WHERE e.novelty_total > 0.5;

-- Trace the lineage of a specific expression
WITH RECURSIVE ancestors AS (
    SELECT * FROM lineage WHERE child_hash = ?
    UNION ALL
    SELECT l.* FROM lineage l JOIN ancestors a ON l.child_hash = a.parent1_hash
)
SELECT * FROM ancestors;
```

## MCP Interface — Observe, Don't Interfere

Mathscape exposes an MCP (Model Context Protocol) server for agent
interaction. The interface is strictly **read-and-trigger** — an agent
can run epochs and query all results, but cannot alter the computation.

See [docs/arch/mcp.md](docs/arch/mcp.md) for the full tool reference.

### What MCP Can Do

- **Trigger epochs**: `step` (one epoch), `run` (N epochs), `run_until`
  (stop on compression plateau, novelty spike, etc.)
- **Query in-memory state**: population, library, MAP-Elites archive,
  reward weights, engine status
- **Query database history**: epoch metrics, symbol discovery timeline,
  expression trees, lineage chains, proof certificates, eval traces
- **Visualization helpers**: compression curves, phase transitions,
  archive heatmaps, pretty-printed expressions and proofs

### What MCP Cannot Do

- Alter the reward function, weights, mutation operators, or selection logic
- Inject expressions into the population or modify library entries
- Compute or simulate an epoch outside the engine
- Override compression, novelty, or meta-compression calculations
- Modify any stored data (expressions, proofs, lineage, metrics)

### Why This Boundary Exists

Mathscape is an observable experiment. The search dynamics — compression
equilibrium, novelty escape, dimensional discovery — emerge from the
fixed algorithm interacting with the fixed primitives. If an external
agent perturbs the computation, the traversal is no longer reproducible
and discoveries lose their inherent proof status. The determinism
guarantee (same inputs + same algorithm = same outputs) breaks the
moment an external actor injects state.

**The MCP interface is a one-way glass: full visibility, zero influence.**

### Security Enforcement

The boundary is enforced at the Rust type level. The MCP handler holds
an `Arc<Engine>` (shared read-only reference) and an `mpsc::Sender` for
`EngineCommand` (step/run signals only). No `&mut Engine` is ever
exposed across the MCP boundary. There is no API surface to reach
internal state mutably — the interface simply doesn't expose the
capability.

## Rust Crate Structure

| Crate | Purpose |
|---|---|
| `mathscape-core` | `Point`, `Number`, `Fn`, `Term` enum, hash-consing, evaluation, substitution, s-expr parser |
| `mathscape-store` | redb expression store, PostgreSQL metadata (SeaORM entities), epoch transaction logic, LRU cache |
| `mathscape-migration` | SeaORM database migrations for PostgreSQL schema lifecycle |
| `mathscape-db` | Database management CLI: migrate, rollback, status, verify, reset |
| `mathscape-proof` | Proof construction, verification status tracking, proof composition, Lean 4 export |
| `mathscape-compress` | Anti-unification, e-graph integration (`egg`), library extraction, rewriting |
| `mathscape-evolve` | Genetic operators, population management, tournament selection |
| `mathscape-reward` | Description length, compression ratio, novelty scoring, meta-compression, combined fitness |
| `mathscape-policy` | Optional RL policy network for guided mutation (Phase 7+) |
| `mathscape-cli` | REPL for step-by-step epoch execution, population/library inspection, history queries |
| `mathscape-mcp` | MCP server (stdio transport) — observe-only tools for agent interaction, `Arc<Engine>` read boundary |
| `mathscape-service` | Long-running HTTP service — engine loop + health/metrics + read-only query API for K8s deployment |
| `mathscape-discovery` | Discovery mining: epoch analysis, known-math pattern matching, representation data for React UI |

## Service Mode

Mathscape runs as a long-running service for unattended operation.
See [docs/arch/service.md](docs/arch/service.md) for full details.

**Three binaries, one engine:**

| Binary | Transport | Use case |
|---|---|---|
| `mathscape-service` | HTTP (8080 health/query, 9090 metrics) | K8s StatefulSet — leave it traversing, check in later |
| `mathscape-mcp` | stdio (MCP protocol) | Local Claude Code agent interaction |
| `mathscape-cli` | Terminal REPL | Interactive human exploration |
| `mathscape-db` | CLI | Database migrations, status, verify, reset |

The service binary auto-starts the engine loop and runs epochs
continuously. All state persists to `/data/` (redb + SQLite).
A crash mid-epoch rolls back to the previous complete epoch.

The HTTP API mirrors the MCP query tools — same read-only semantics,
same observe-don't-interfere boundary. No endpoint can alter the
computation.

## Deployment

See [docs/arch/deployment.md](docs/arch/deployment.md) for full details.

**Pipeline:** Nix build -> Docker image -> GHCR push -> Helm chart ->
FluxCD HelmRelease -> K8s StatefulSet with PVC.

**Docker image**: Built with `pkgs.dockerTools.buildLayeredImage`
(Nix-native, no Dockerfile). Non-root, read-only rootfs, /data volume.

**Helm chart**: `deploy/charts/mathscape/` depends on pleme-lib.
StatefulSet with volumeClaimTemplates (10Gi default). Fully tested
with helm-unittest.

**Substrate patterns used**: `mkRustOverlay`, `mkRustDevShell`,
`mkDarwinBuildInputs`, `mkImageReleaseApp`, `mkHelmSdlcApps`.

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

- **STITCH** (Bowers et al.) — fast abstraction learning via branch-and-bound.
  1000-10000x faster than DreamCoder's compression, 100x less memory.
  Core technique for library extraction in Mathscape.
  [Repo](https://github.com/mlb2251/stitch)

- **MAP-Elites** (Mouret & Clune) — quality-diversity algorithm maintaining
  a grid of elite solutions across behavioral dimensions. Ensures
  structural diversity in the population.
  [Paper](https://www.frontiersin.org/articles/10.3389/frobt.2016.00040/full)

- **Incremental Equality Saturation** (EGRAPHS 2025) — persist e-graphs
  across iterations instead of rebuilding, reusing previously derived
  equalities.
  [Paper](https://rupanshusoi.github.io/pdfs/egraphs-25.pdf)

- **Parallel Symbolic Enumeration** (Nature Comp. Sci. 2025) — systematic
  enumeration of mathematical expressions, complementary to evolutionary
  search for small-expression discovery.
  [Paper](https://www.nature.com/articles/s43588-025-00904-8)

## Development Plan

### Phase 0: Scaffold
Cargo workspace with 10 crates. flake.nix using substrate's rust overlay,
`mkRustDevShell`, `mkImageReleaseApp`, `mkHelmSdlcApps`. Docker image via
`buildLayeredImage`. Helm chart with pleme-lib dependency and helm-unittest.
Verify: `nix develop` enters devShell, `cargo check` succeeds.

### Phase 1: Primitives + Expression Trees
Implement `Point`, `Number`, `Fn`, `Apply`, `Symbol` as a Rust enum.
Hash-consing with blake3. S-expression parser and printer. Simple
evaluator over naturals using Peano arithmetic (`zero`, `succ`, `add`,
`mul`). Property tests for evaluation correctness.

### Phase 1.5: Storage Layer
redb for hash-consed expression store. PostgreSQL (via SeaORM) for
population, library, epochs, lineage, proofs, dependency tables. SeaORM
migration crate (`mathscape-migration`) + DB management CLI
(`mathscape-db`). Epoch transaction logic — load working set, compute,
write, release. LRU cache for expression reads. Verify: write 10k
expressions, restart process, load population from PostgreSQL, resolve
expressions from redb.

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

### Phase 6.5: MCP Server
MCP server over stdio using `rmcp`. Read-only query tools for all
engine state (population, library, metrics, proofs, lineage). Execution
tools limited to step/run/run_until. No mutation tools — the agent
observes the traversal but cannot interfere. Verify: agent can step
through epochs, query library, read proofs, and narrate the discovery
process without altering outcomes.

### Phase 6.75: Service Mode + Docker + Helm
HTTP service binary (`mathscape-service`) with Axum. Health, metrics,
read-only query API. Docker image via `buildLayeredImage`. Helm chart
(StatefulSet + PVC) with pleme-lib. helm-unittest tests. Verify: deploy
to K8s, engine runs unattended, query API returns epoch metrics.

### Phase 6.9: Discovery Mining
MCP tool (`mathscape-discovery`) that iterates over epoch data, mines
discovered symbols, and maps them to known mathematical concepts.
Produces structured representation data (JSON) suitable for a React UI
that presents findings to human mathematicians. Includes:
- Known-math catalog with structural pattern matchers (commutativity,
  associativity, distributivity, identity, inverse, etc.)
- Epoch-by-epoch discovery timeline with confidence scores
- Expression tree visualization data (nodes, edges, labels)
- Proof chain rendering data
- Symbol relationship graph for interactive exploration
- Export formats: JSON API, static JSON files, LaTeX snippets

### Phase 7: RL Policy (stretch)
Small policy network (simple MLP) trained via REINFORCE to guide
mutation selection. State = expression encoding, action = mutation
choice, reward = delta compression ratio.

## Build & Run

```bash
# Development
nix develop                    # devShell (Rust + PostgreSQL + Helm + kubectl)
cargo build                    # build all crates
cargo test                     # run all tests
cargo run -p mathscape-cli     # interactive REPL
cargo run -p mathscape-service # local service (HTTP on 8080)

# Database
cargo run -p mathscape-db -- migrate   # run pending migrations
cargo run -p mathscape-db -- status    # show migration status
cargo run -p mathscape-db -- rollback  # roll back last migration

# Docker
nix build .#image              # build Docker image (Linux only)
nix run .#release              # push multi-arch image to GHCR

# MCP
nix run .#mcp                  # start MCP server (stdio transport)

# Helm
nix run .#lint:mathscape       # lint chart
nix run .#release:mathscape    # lint + package + push chart to OCI registry
```

## Conventions

- **Language**: Rust (2024 edition)
- **Build**: substrate builders via Nix (rust overlay, mkRustDevShell, mkImageReleaseApp, mkHelmSdlcApps)
- **Database**: PostgreSQL via SeaORM (Rust migrations in `mathscape-migration`, entities in `mathscape-store/src/entity/`)
- **Expression store**: redb (embedded, content-addressed, pure Rust)
- **Docker**: `dockerTools.buildLayeredImage` (Nix-native, no Dockerfile)
- **Helm**: pleme-lib library chart dependency, helm-unittest tests
- **Deployment**: FluxCD HelmRelease -> K8s StatefulSet with PVC + PostgreSQL (CNPG or external)
- **Testing**: `cargo test` + `nix flake check` + `helm unittest`
- **Expression format**: S-expression — `(add (succ zero) (succ zero))`
- **Library format**: Named rewrite rules — `add-identity: (add ?x zero) => ?x`
- **Compression metrics**: Reported per-epoch as `CR`, `DL`, `novelty`, `|L|`
