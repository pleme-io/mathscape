# Translation layer — every certified rule carries its mathematics

**Status:** landed 2026-10-14. Code: `crates/mathscape-core/src/translation/`.
Vocabulary: `crates/mathscape-core/src/translation/vocabulary.tlisp`.

## What it guarantees

A `MapEvent::RuleCertified` cannot be built without a `Translation` of its
own rule. The only constructor is `MapEvent::rule_certified(rule, samples)`,
which computes `Translation::of(&rule)`; `Translation` has private fields, so
"a certified rule with no human-mathematics counterpart" is not a value the
event stream can hold. A rule the catalog does not recognise is never left
opaque: it gets a `Characterization` naming its structural properties.

| Surface | What it gives |
|---|---|
| `Translation::of(&RewriteRule)` | canonical name, domain, instance LaTeX, schema LaTeX, semantic meaning, operator bindings, orientation, ΔDL signature |
| `Translation::to_invariant_def(status)` | the `(invariant-def :id … :internal-repr … :canonical-name … :math-domain … :latex … :complexity-reduction … :algebraic-status …)` wire form |
| `CertifiedRule::translation()` | the same for any rule on the certification ladder |
| `TrapTranslation::of(&Trap)` | a trap as a fixed point Φ(L\*) = L\* with its exit reason |
| `structures(&[Translation])` | which algebraic structures the certified laws establish (monoid, group, semiring, ring, lattice, …) |
| `derive(term, library, limit)` | an AST rewrite trace as proof steps: substitution instance + congruence, or kernel computation |
| `TheoryDocument` | Markdown or standalone LaTeX: translation table, definitions and axioms, theorems, corollaries and reductions, traps, derivations |
| `mathscape-cli translate [--latex]` | stdin stream of `(map-event …)` / `(rule …)` forms → that document |

The Lisp event stream (`mathscape-proof::map_event_lisp`) is now lossless for
rule-carrying events: `core-grew`, `rule-certified` and
`rule-rejected-at-certification` encode `:lhs`/`:rhs`, and `rule-certified`
also carries its `(invariant-def …)`. A `rule-certified` form without
`:lhs`/`:rhs` is refused on read, since its translation would describe no
rule.

## The vocabulary

One tatara-lisp file, read once (`translation::vocabulary()`), four forms:

- `(defoperator :name :render :carrier :math-name :axiom)` — one per kernel
  builtin. `:render` is `(constant s)`, `(function s)`, `(prefix s)`,
  `(postfix s)`, `(infix s precedence)`, `(bracket open sep close)` or
  `(frac)`. A test fails the build if a builtin lands without one, or if a
  render form disagrees with the builtin's arity.
- `(definvariant :id :property :canonical-name :math-domain :latex
  :semantic-meaning :internal-repr (=> LHS RHS) …)` — a named law; repeat
  `:internal-repr` for variants (left/right). `:property` is the generic law
  class (`commutativity`, `identity-element`, …) used for structure detection.
  A test instantiates every form and fails if the matcher cannot reach it.
- `(defstructure :id :name :arity 1 :requires (…))` or
  `(… :arity 2 :additive (…) :multiplicative (…) :joint (…))` — listed most
  specific first; the first satisfied one is reported per operator (pair).
- `(defprocedure :id :layer :canonical-name :math-domain :latex
  :implemented-by)` — kernel procedures that are Rust, not certified rules
  (R14 autograd, R15 SGD). They appear in the document's definitions, never
  as theorems.

Pattern language inside `:internal-repr`:

| Form | Matches |
|---|---|
| `?a`, `?f` | any subterm; distinct names bind distinct subterms. In argument position it must mention a rule variable (so `?b` cannot swallow the `0` in `a + 0`) |
| `(const ?e)` | a constant (Number, Point, nullary builtin) |
| `add`, `neg`, … | that kernel builtin |
| `0`, `i0`, `-1`, `0.0` | Nat, Int, Int, Float literal |
| `(h x …)` | an application (or a library Symbol application) |

A rule matches in either orientation, and the arguments of a builtin flagged
commutative match in either order. The most specific match (literals count
2, constants 1) is the primary name; every other matching law is listed as
"also an instance of".

Adding mathematics is a vocabulary edit, not engineering: append a
`definvariant`, run `cargo test -p mathscape-core --lib translation`.

## What the numbers mean

`complexity-reduction` is `lhs nodes -> rhs nodes per rewrite (delta-DL k)`:
the description-length change of one application, mathscape's own currency
(`reward-calculus.md`). It is not an asymptotic O(·) claim; the machine has no
cost model that would justify one.

## Known limits

- The internal representation prints values with `Term`'s `Display`, which
  does not distinguish `Nat(3)` from `Int(3)`; the LaTeX carrier does.
- Structure detection reads the certified laws only. It does not check that
  the identity of `+` is the absorbing element of `·`, so a reported
  semiring rests on the laws listed as its evidence.
- `derive` rewrites leftmost-outermost, with a visited set to stop
  commutativity cycles, and applies commutative matches only by swapping the
  two arguments of the redex. Each such step is marked "modulo commutativity".
- R14/R15 are listed as kernel procedures because no `grad` operator exists in
  the term language; a certified chain rule needs one first.
