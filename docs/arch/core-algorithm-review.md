# Core Algorithm Review — Reduction + Abstraction of the Kernel

The work scope the human owns going forward, per 2026-04-18
session directive: **we work directly ONLY on the core algorithm.
The machine handles everything above.**

Constraints on any kernel change:
1. **Genuine** — no approximations, no hacks. Every operation
   means what it says mathematically.
2. **True** — evaluation produces the correct answer on every
   input. No edge case swept under a rug.
3. **Repeatable** — deterministic. Same inputs produce identical
   outputs. No hidden randomness, no platform-dependent ordering.
4. **No equal terms** — if two terms are semantically equal, they
   MUST be structurally equal. Redundant representations of the
   same meaning are forbidden at the kernel level.

## The current kernel

`crates/mathscape-core/src` owns the kernel. The load-bearing
files:

```
term.rs        — Term enum + Value enum + substitute
value.rs       — Value::Nat(u64) — only natural numbers currently
eval.rs        — evaluator (zero, succ, add, mul), pattern_match,
                 substitute, subsumes, anonymize_term, alpha_equivalent
hash.rs        — BLAKE3 content hashing, TermRef
reduction.rs   — subsumption-based rule collapse
form_tree.rs   — DiscoveryForest retroactive reduction substrate
```

The data types:

```rust
Term::Point(u64)
    | Number(Value)
    | Var(u32)
    | Apply(Box<Term>, Vec<Term>)
    | Symbol(SymbolId, Vec<Term>)
    | Fn(Vec<u32>, Box<Term>)

Value::Nat(u64)  // single variant — Peano arithmetic only

RewriteRule { name: String, lhs: Term, rhs: Term }
```

## Operations the kernel provides (the irreducible five)

Every higher-level machinery reduces to these:

1. `eval(term, library, step_limit) -> Result<Term, _>` — normal form
2. `pattern_match(pattern, term) -> Option<HashMap<u32, Term>>` — structural matching
3. `term.substitute(var_id, replacement) -> Term` — variable replacement
4. `term == term` (derived `PartialEq`) — structural equality
5. `blake3_hash(term) -> TermRef` — content identity

## Where the kernel currently has REDUNDANCY (reduction targets)

### R1: Two equal-adjacent notions of "the same rule"

```rust
fn subsumes(a: &Term, b: &Term) -> bool   // pattern subsumption
fn pattern_equivalent(a: &Term, b: &Term) -> bool  // mutual subsumption
fn alpha_equivalent(r1: &RewriteRule, r2: &RewriteRule) -> bool
fn anonymize_term(t: &Term) -> Term  // canonicalize var ids
```

Four ways to ask "are these the same rule?" Each with subtly
different semantics. This is the source of bugs like the
anonymization-collapses-commutativity issue we hit.

**Reduction**: pick ONE canonical equality. `alpha_equivalent`
(uses anonymized form on both sides with SHARED var map) is the
true notion of rule equality. Everything else is either weaker
(subsumes is one-way) or can be expressed in terms of it. If
two rules are alpha-equivalent, they're the same rule. If
not, they're different. No third option.

### R2: Two incomparable term-traversal machineries

Pattern match, substitution, evaluation, and anti-unification
all traverse terms. Each has its own recursive visitor. They
share no abstraction.

**Abstraction**: a single `TermVisitor` trait that the four
operations implement differently. Deduplicates ~200 LOC of
recursive match arms. Also makes adding a new term variant
(e.g., when we eventually add `Real` or `Rational`) a 5-location
change instead of a 20-location change.

### R3: Commutative-argument order produces unequal structural forms

`Term::Apply(Var(2), [nat(3), nat(5)])` and
`Term::Apply(Var(2), [nat(5), nat(3)])` are structurally
distinct Terms. Semantically both mean "add(3, 5) = 8." The
kernel treats them as different terms; the machine above has to
spend effort (phase K e-graph, semantic dedup, alpha-
equivalence) recovering the obvious fact.

**Reduction**: the kernel canonicalizes commutative Apply nodes
at construction. `add(3, 5) = add(5, 3)` structurally, because
the construction sorts args for known-commutative operators.

**Cost**: the machine can no longer "discover" commutativity —
it's built in. We keep the discovery evidence from earlier
sessions; we just don't re-discover it each run.

**Benefit**: the entire session's work on phase K, semantic
dedup, ML5 saturation-response on commutative variants — all
becomes unnecessary above the kernel. The "equal terms" problem
dissolves.

This is the biggest reduction available.

### R4: Associativity also produces structural redundancy

`add(add(1, 2), 3)` and `add(1, add(2, 3))` are semantically
equal but structurally distinct. Same class of redundancy as
commutativity.

**Reduction**: flatten AC (associative + commutative) operators
into variadic canonical form. `add(a, b, c)` with args sorted.

**Cost and benefit**: same as R3 but stronger. Layer-5
associativity discoveries become trivially structural.

### R5: Var(u32) with magic-constant meanings

`Var(1)` means succ. `Var(2)` means add. `Var(3)` means mul.
`Var(4)` also means succ (as a non-builtin variable). The
evaluator's `step` function has a `match` on these magic
numbers.

**Abstraction**: replace magic-constant operator ids with an
enum or a `Builtin` trait. The set of builtins is first-class,
extensible without touching magic numbers.

**Cost**: more code upfront, slight performance overhead for
trait dispatch.

**Benefit**: adding `sub`, `div`, etc. no longer requires
editing three separate locations. The user's question "can we
add operators autonomously" becomes tractable.

### R6: Value::Nat(u64) hardcodes the number domain

The evaluator works on `u64`. To represent rationals, reals,
large integers, or modular arithmetic, every match on Value is
a choke point.

**Abstraction**: `Value` becomes a trait with `add`, `mul`,
`succ`, `zero`, `eq` as trait methods. Concrete implementations
for `Nat<u64>`, `Int<i64>`, eventually `BigRat`, etc. The
evaluator is polymorphic over the Value type.

**Cost**: larger upfront refactor.

**Benefit**: unlocks non-Peano domains for the machine to
discover theorems in. Without this, the machine's ceiling is
arithmetic-of-naturals; with this, it can reach algebra-of-
integers, modular arithmetic, etc.

## Where the kernel currently has CORRECTNESS bugs

### C1: Independent anonymization of LHS and RHS

`anonymize_term(&r1.lhs)` and `anonymize_term(&r1.rhs)` are
called separately with independent fresh var maps. This means
`add(?a, ?b) → add(?b, ?a)` anonymizes to `add(?100, ?101) →
add(?100, ?101)` (identity form), losing the commutativity
signal. This is a bug in `alpha_equivalent`.

**Fix**: anonymize with a SHARED var map across LHS and RHS,
preserving cross-side variable identity.

### C2: Non-determinism via HashMap iteration

`pattern_match` returns `HashMap<u32, Term>` — iteration order
is non-deterministic (HashMap is random-seeded per process).
Downstream code that iterates bindings may produce different
results across runs.

**Fix**: use `BTreeMap<u32, Term>` for binding maps so iteration
order is deterministic.

## Proposed reductions, ranked by impact

1. **R3 + R4 (AC canonicalization)** — biggest single reduction.
   Eliminates commutativity and associativity redundancy at the
   kernel. Simplifies everything above.
2. **C1 + C2 (correctness bugs)** — must be fixed before any
   other work. Non-deterministic behavior violates the
   "repeatable" constraint.
3. **R1 (single notion of rule equality)** — collapse 4 funcs
   to 1 canonical. Smaller surface area to maintain.
4. **R5 (Builtin trait)** — enables autonomous operator
   addition. Important for ML5/ML6 work.
5. **R6 (Value trait)** — unlocks non-Peano domains. Long-term
   enablement.
6. **R2 (TermVisitor)** — cleanup, marginal impact.

## What each reduction COSTS us (honesty about tradeoffs)

### R3 + R4 (AC canonicalization)

Cost: the machine's "discovery" of commutativity and
associativity becomes trivial — they're kernel-enforced, not
surfaced as theorems. We lose a category of discovery.

But: we ALREADY have that discovery, documented. Commutativity
and associativity are mathematical facts about the Peano
operators, not contingent discoveries. A purer kernel would
bake them in from the start.

And: the machine can still discover IDENTITY elements,
distributivity, successor-counts (e.g., `add(x, 2) = succ(succ(x))`)
— these are NOT encoded in AC canonicalization.

Net: I think this is the right cost to pay. The kernel is
cleaner; the discovery surface shifts one layer up.

### R5 (Builtin trait)

Cost: evaluator becomes slightly more complex (trait dispatch).

Benefit: unlocks operator discovery — the machine can propose a
new operator `(defbuiltin sub (x y) ...)` which becomes
first-class.

Net: right cost, good leverage.

### R6 (Value trait)

Cost: Value becomes generic over T: Numeric. Every downstream
function that operates on Value needs type parameter.

Benefit: machine can work over multiple number domains.

Net: defer until a specific use case arises. YAGNI for now.

## The canonical-form proposal (R3 + R4 concretely)

Introduce a `CanonicalTerm` type that enforces AC invariants at
construction:

```rust
pub enum CanonicalTerm {
    Point(u64),
    Number(Value),
    Var(u32),
    App(Operator, Vec<CanonicalTerm>),  // args sorted if op is AC
    Sym(SymbolId, Vec<CanonicalTerm>),
    Fn(Vec<u32>, Box<CanonicalTerm>),
}

pub struct Operator {
    pub id: u32,
    pub is_commutative: bool,
    pub is_associative: bool,
}

impl CanonicalTerm {
    /// Constructor enforces the AC invariant: Apply nodes with
    /// commutative operators have args in sorted order; associative
    /// operators are flattened into variadic form.
    pub fn apply(op: Operator, args: Vec<CanonicalTerm>) -> Self {
        let args = if op.is_associative {
            flatten_assoc(op.id, args)
        } else {
            args
        };
        let args = if op.is_commutative {
            let mut a = args;
            a.sort();  // requires CanonicalTerm: Ord
            a
        } else {
            args
        };
        CanonicalTerm::App(op, args)
    }
}
```

After this, `Term::Apply(add, [3, 5]) == Term::Apply(add, [5, 3])`
by derived PartialEq. The whole class of "recover commutativity"
machinery above dissolves.

## The promise of this work

After reduction + abstraction:

- **Kernel**: ~1500 LOC, purely canonical, deterministic
- **Machine above**: no more "is this the same as what I already
  have" logic. Every structurally-distinct term IS a distinct
  theorem. No phase K needed (or phase K becomes a Rust primitive
  baked into the kernel, not a machine-level discovery).
- **Discovery layer**: focuses on what's actually new — identity
  elements, distributivity, successor theorems, domain
  extensions.

The session's entire ML1-ML5 + phase K work was needed BECAUSE
the kernel had redundancy. Fix the kernel and much of that
becomes unnecessary.

## The next move

Start with the correctness bugs (C1, C2). Then tackle R3 + R4
(AC canonicalization) as the biggest reduction. Then R5 (Builtin
trait). R6 deferred.

Each change is a kernel-level refactor with regression tests to
confirm the Peano arithmetic primitives still produce the right
answers on known inputs. The machine above should notice no
regression except the collapse of what were previously distinct
terms.

## What we're NOT doing

- NOT building M4/M5/M6 Lisp ports above the kernel. That's
  machine work.
- NOT adding new primitives (sub, div, pred) directly — those
  come via R5's Builtin trait mechanism, and the machine proposes
  them.
- NOT optimizing performance of the machinery above the kernel.
  That's machine work.
- NOT adding new discovery algorithms. The machine is already
  discovering — we're making sure its discoveries are valid
  (via kernel truth) and unique (via kernel canonicalization).
