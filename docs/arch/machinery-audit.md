# Core Machinery Audit — What We Already Have

A review of the mathematical laws and simplifications already
encoded in the core machinery, so we stop re-discovering them
in eval / test / corpus land when they exist elsewhere.

Written 2026-04-19 after a near-miss: the "zero-absorber"
frontier in the 92-problem curriculum looked like a gap the
motor needed to close. Review revealed the law was **already
encoded in `autograd::simplify_mul_of`** since R29 (2026-04-18),
just never plumbed into eval. Plumbing it removed a frontier
the motor should legitimately discover — settling, not
progress.

## The pattern to avoid

When a curriculum subdomain reveals a gap:

1. **Audit first**: is the law already expressed somewhere?
2. If YES → is it **intentionally** scoped to that place? Don't
   duplicate it into eval just because eval would benefit.
3. If NO → let the motor try to discover it. Kernel short-
   circuits are the last resort, not the first.

## The catalog

### Canonicalization (R3 + R4 + R6)

`Term::canonical()` produces an AC-canonical form. Commutative
operators get sorted args; associative nestings get flattened.

**Used in:**
- `CanonicalDeduper` — rules dedupe on canonical form
- `eval::step` (Phase Z.8) — input term + rule LHS both
  canonicalized before pattern match, so left- and right-
  oriented identity rules both reduce
- `Term::canonical` itself, callable anywhere

**DON'T re-implement.** Don't add "commute args before match"
in a new place — use the existing canonicalization.

### Domain-simplification (R29 — `autograd::simplify_*_of`)

`simplify_add_of::<D>`, `simplify_mul_of::<D>`, `simplify_neg_of::<D>`
encode:

| Law | Expression | Domains |
|---|---|---|
| Additive identity | `zero ⊕ b = b`, `a ⊕ zero = a` | Int, Float, Tensor, FloatTensor, Nat (Z.9) |
| Multiplicative identity | `one ⊗ b = b`, `a ⊗ one = a` | same |
| Zero absorber | `zero ⊗ _ = zero`, `_ ⊗ zero = zero` | same |
| Double negation | `neg(neg(x)) = x` | same |
| Zero negation | `neg(zero) = zero` | same |

**Used in:** derivative construction (`symbolic_derivative`).

**NOT used in eval.** Deliberately. These laws aren't kernel
arithmetic — they're symbolic simplifications that should emerge
as motor-discovered rules. Plumbing them into eval removes the
motor's frontier without the motor doing the work.

**DON'T plumb into eval.** If a curriculum probe exposes a gap
for `mul(0, ?x) → 0`, the path to closing it is motor discovery
OR curriculum acknowledgement that the gap is real — not a
one-line eval hack.

### Pattern matching (`eval::step` + `pattern_match`)

`pattern_match(pattern, term)` produces variable bindings OR
None. Phase Z.8 extended eval to try both the raw and canonical
forms so left- and right-oriented rules both match.

**DON'T add new "match the commuted form" logic.** The
canonical form already handles that.

### AC normal form in `Term::canonical`

The kernel's canonical form re-orders args of commutative
operators. `add(x, 0)` becomes `add(0, x)`. `mul(3, 2)` becomes
`mul(2, 3)` (sorted by a canonical key).

**This is why `right-identity` closed at Z.8**: the stored
rule `add(0, ?x) → ?x` matches canonicalized input
`add(0, ?x) ← canonical(add(?x, 0))`.

### Constant folding (`eval_add`, `eval_mul`, `eval_int_*`, `eval_float_*`)

Each arithmetic builtin folds its own concrete values:
- `eval_mul`: `Nat × Nat` → product
- `eval_int_mul`: `Int × Int` → checked product
- `eval_float_mul`: `Float × Float` → product
- Analogues for add, neg, sub (where defined)

**Kept minimal on purpose.** These are PURE arithmetic — not
symbolic simplification. `mul(0, ?x)` where `?x` is a Var is
NOT within this scope; it's a symbolic law.

### What the motor legitimately discovers

| Law | Subdomain | Discovered by |
|---|---|---|
| `add(0, ?x) → ?x` | symbolic-nat | motor |
| `mul(1, ?x) → ?x` | symbolic-nat | motor |
| `tensor_add(zeros, ?x) → ?x` | tensor-algebra | motor |
| `tensor_mul(ones, ?x) → ?x` | tensor-algebra | motor |
| `add(?x, 0) → ?x` | right-identity | canonicalization (Z.8) |
| `mul(?x, 0) → 0` | zero-absorber | OPEN FRONTIER (honest) |
| `mul(0, ?x) → 0` | zero-absorber | OPEN FRONTIER (honest) |

The last two are the current curriculum frontier. The motor
doesn't discover them today. That's genuine research direction:
- Better anti-unification over constant-output patterns
- E-graph equivalence class unification
- Explicit zero-absorber probe in the corpus + compression
  relaxation

### NatOps (Z.9, completing R29)

`NatOps` impl of `DomainOps` was added because Int/Float/Tensor
were already there. NatOps lives in `autograd.rs` alongside
the others — pure infrastructure completion.

**Not currently plumbed into eval**, deliberately. Available
for the derivative path if/when it flows through Nat. Available
for future motor-layer or coach-layer decisions that want
domain-aware reasoning.

## Summary: three layers of laws

```
  KERNEL            →  pure arithmetic constant folding only
                       (add, mul, neg of concrete values)

  TERM CANONICAL    →  AC normal form (commutative reordering,
                       associative flattening). Applied inside
                       eval's library-match step.

  AUTOGRAD / SIMPLIFY → domain-aware simplification laws (zero,
                       identity, double-neg). Used in derivative
                       construction. NOT in eval.

  MOTOR / RULES     →  everything else. The legitimate frontier.
                       When the curriculum exposes a gap, the
                       motor discovering the rule is progress.
                       Plumbing simplify_* into eval is settling.
```

## Going forward

When a curriculum subdomain exposes a gap:

1. Audit this doc for relevant existing machinery.
2. If the law is in the CANONICAL layer → curriculum probe is
   likely already served; check test setup.
3. If the law is in AUTOGRAD → decide deliberately: is this
   discovery territory or eval-layer infrastructure? Usually
   the former.
4. If nothing covers it → real frontier. Let the motor attempt.
   Use the finding to inform research on the extractor / corpus
   / e-graph direction.
