# The Emergent Substrate

Status, 2026-10-16. **Built and tested:** the kernel, the motor, the
certifier and the stream profile. They are the default since this
change. **Designed only:** library compression (anti-unification over
held definitions), proof by induction, retroactive demotion, and moving
the driver into tatara-lisp. The measured results are in §8, and every
number there has a command that reproduces it.

Operator directive, verbatim: *"the rust should be absolutely minimal
and lisp fully emergent from only the 3 primitives"*, *"we need to start
from the smallest number of primitives"*, *"the laws should have been
found emergently too even if they were found again"*.

## 1. Decision: the smallest primitive set is Fn alone

**Choice.** The kernel has one primitive: pure untyped λ (abstraction,
application, variables). Point/Number/Fn is the upper bound, and λ meets
it by encoding:

- **Number contributes no reduction rule.** A Church numeral
  `c_n = λf x. fⁿ x` *is* its own eliminator. Applying it to a step
  function and a base is iteration, which is the non-dependent recursor
  of the inductive type (Martin-Löf's `natrec` without the predecessor
  argument). Number leaves one thing in the kernel: the *observation
  sort*. The certifier samples numeric variables from `c_0 … c_6`, and
  observation decodes a normal form `λf x. fⁿ x` back to `n`. That is a
  sort of test inputs, not an operator.
- **Point is not a separate primitive.** An opaque atom with equality
  only is a free variable. Observation η-expands a term to two
  arguments, `obs(t) = nf(λf x. t f x)`, which puts the atoms `f` and `x`
  in scope. Equality of atoms is then α-equality of normal forms, and in
  de Bruijn form that is structural equality.

**Measured comparison** (`cargo run --release -p mathscape-emergence
--example basis -- <basis> <max>`). Closed normal forms are enumerated
in size order (λ, application and variable each count 1; a combinator
counts 1). Each is tabulated as a binary function on numerals 0..3, and
the run stops at the first term matching each target. Ranges are
ordering-independent: *before* is the count of all smaller sizes,
*through* includes the size that holds the hit.

| basis | add | mul | m^n | n^m | notes |
|---|---|---|---|---|---|
| λ (chosen) | size 13; 48,036 < n ≤ 186,642 | size 8; ≤ 337 | size 5; ≤ 17 | size 2 (`λ0`, identity) | add = `λλλλ. 3 1 (2 1 0)` |
| SK | size 17; 11,418 < n ≤ 46,426 | size 7; ≤ 58 | size 13; ≤ 2,874 | size 5 | mul = `S(KS)K`, which is composition `B` |
| ι (single combinator) | **not reached** in 3,707,852 terms (size ≤ 29, 130 s at size 29) | size 19 | not reached | size 3 | normal-form pruning is impossible: every `ι x` is a redex |
| λ + Nat (zero, succ, iter as atoms) | size 4; ≤ 55 | size 5; ≤ 176 | size 5 | size 1 | cheapest search |

**Why λ over the others:**

- **ι** is rejected by measurement: addition is out of enumerative
  reach.
- **λ + Nat** is rejected on the directive, not on cost. It is the
  cheapest, but it makes Number a primitive with a hand-written
  eliminator. That is the larger kernel the directive rules out. It also
  measures an advantage that comes from the encoding: inputs are already
  Church numerals, so `iter` duplicates what each numeral does.
- **SK** reaches addition in about 4× fewer terms than λ, but pays 200×
  more for `m^n`. In the full emergent loop it names `+` about 6× sooner
  (§8). It also needs observation atoms (a Point) to read a
  result, because it has no binder to η-expand with. So SK plus Point is
  two primitives where λ is one. λ also keeps definitions readable as
  Lisp and matches DreamCoder and Stitch, which both learn libraries over
  λ-terms. Above all, the λ grammar starts from nothing written in Rust,
  while SK needs the two combinators as hand-written definitions. SK is
  kept as a configured profile (`stream.emergence.basis: combinators`)
  and runs against λ in §8.

The literature agrees on the shape of this trade.

- Bracket abstraction from λ to SK is worst-case quadratic in term size
  (Kiselyov, FLOPS 2018), which matches SK's loss on `m^n`.
- Church numerals make arithmetic non-recursive and the predecessor hard.
  Scott numerals invert that and need a fixpoint for `+`
  (Church encoding; Mogensen 1994).

The kernel therefore needs no recursion operator for the operators that
emerge first.

## 2. Destination

Rust is a minimal, auditable kernel:

- the term representation over the chosen basis;
- a step-bounded normalizer;
- persistence (hash-consed, content-addressed snapshots behind HEAD);
- a certifier.

Everything else comes out of the machine's own epochs, with rediscovery
counted as success:

- every operator (`+`, `×`, …);
- every constant;
- every law (identity, associativity, commutativity);
- the laws over operators.

The epoch is: generate → evaluate → observe → name → conjecture →
certify. Operator variables can be sampled because operators are terms
the model holds.

The search driver is Rust today (`mathscape-emergence`). Its
destination is tatara-lisp, so that Rust shrinks to `mathscape-kernel`
plus persistence (§10, E6).

## 3. The governing law

Stated once, in `CLAUDE.md` ("Governing law: mathematics emerges, Rust
only checks it"). Enforced by
`mathscape-config::substrate::the_default_stream_grows_mathematics_from_the_primitives_and_keeps_builtins_and_the_frontier_off`
and `mathscape-stream/tests/emergent.rs`.

## 4. Inventory of today's Rust

| Rust | Class | Where it goes |
|---|---|---|
| `mathscape-kernel` (new): `Term` (de Bruijn), call-by-need normal-order normalizer with fuel / node / depth budgets, `numeral`, `as_numeral`, `observe` | **kernel** | stays; ~400 lines, `#![forbid(unsafe_code)]`, no dependencies |
| `mathscape-emergence::certify` — `KernelCertifier`, `to_core` / `to_kernel` | **kernel** (the certifier) | stays |
| `mathscape-stream` store, snapshots, HEAD, deltas, health | **kernel** (persistence) | stays |
| `mathscape-emergence::{enumerate, behavior, law, engine}` — the motor, naming, conjecture, closure | **must move to Lisp** | E6: tatara-lisp driver over kernel primitives exposed to Lisp |
| `mathscape-core::builtin` (22 hand-written operators), `eval` builtin dispatch, `value` (Nat/Int/Float/Tensor) | **configure off** | `stream.substrate: builtin` |
| `mathscape-core::certification::DefaultCertifier` (number sampling, head-variable refusal) | **configure off** | used only by the builtin profile |
| `mathscape-compress::Frontier`, `entailed_by_library` | **configure off** | `stream.frontier.enabled: true` (default false) |
| `mathscape-compress::{Motor, law_generator, antiunify, meta_gen}`, `adaptive_corpus`, `bootstrap` corpora | **configure off**; anti-unification is **reusable** | first-order anti-unification works unchanged on SK trees; for λ it needs binder-aware matching (§7) |
| `mathscape-proof::mechanism` (ML4 pool, mutations) | **configure off** | drives only the builtin profile |
| `mathscape-core::{translation, mathscape_map, snapshot, streaming_policy}` | neutral | the emergent stream reuses them unchanged |

Nothing was deleted. The builtin model is one config key away:
`StreamConfig::builtin`, or `stream.substrate: builtin` with
`stream.frontier.enabled: true`.

## 5. The kernel

- **Terms.** `Var(i)` (de Bruijn index), `Lam`, `App`, and `Sym(k)`, a
  reference to the model's k-th held definition. Size counts each node
  as 1.
- **Normalizer.** Call-by-need normal-order normalization by evaluation:
  lazy argument thunks, closures, readback under binders. Its head-spine
  loop is iterative, so long β-chains do not deepen the Rust stack. It
  finds a normal form whenever one exists within budget. Budgets are fuel
  (β-steps and δ-unfoldings), output nodes and nesting depth, and each
  produces a typed `Halt`: `Fuel`, `Size`, `Depth`, `Open` or `Unbound`.
- **δ-unfolding.** `Sym(k)` evaluates its definition once per
  normalization (cached). Definitions are closed and may only refer to
  earlier symbols, which the certifier enforces.
- **Observation.** `observe(t) = nf(λλ. t 1 0)`, then decode
  `λλ. 1(1(…0))` as a numeral.

Rust names nothing beyond these four constructors.

## 6. The certifier, redesigned for operator-sorted variables

`KernelCertifier` (`mathscape-emergence/src/certify.rs`) implements the
same `Certifier` trait the stream's gate already uses. It handles three
kinds of rule.

**Definitions.** A definition is `def <kind><k>: Symbol(k) => λ-term`.
It is certified only if all of the following hold:

- `k` is the next fresh symbol;
- the body is closed over earlier symbols;
- the body is built only from kernel constructors;
- the claimed kind holds: `k` (basis) needs nothing more, `c` must
  observe to a numeral, and `u`/`b` must be numeral-total and
  operator-shaped on the grid.

A rule that mentions a builtin id, a `Number` or a `Point` is rejected as
outside the substrate.

**Laws.** Every pattern variable gets a sort, inferred from where it
occurs:

- a variable in head position of an application is
  `Operator(arity)`;
- any other variable is `Numeral`.

Numerals are sampled from `c_0…c_6`. **Operator variables are sampled
from the model's held definitions of that arity.** The old blanket
refusal (`DefaultCertifier::head_pattern_var`) is replaced by a scoped
one: a rule is refused only when no held operator of the needed arity
exists ("no held operator of arity 2 to sample ?f from"). A certified
operator-variable law therefore claims *"for every operator this model
holds"*, which is exactly the population the model can rewrite with.

Test: `an_operator_variable_ranges_over_the_held_operators_of_its_arity`.
It was shown red with the pool emptied and green with the held pool.

- `?f(y,x) = ?f(x,y)` certifies over {add, mul};
- it is refuted once exp is held;
- it is refused when nothing is held.

**Evidence, not proof.**

- **Exhaustive small box.** When the sample space is at most
  `certify_exhaustive` (4,096), every assignment is checked. That is all
  343 triples over `c_0…c_6`, not a random 64. Before this was added, 64
  random samples certified `x^(s19 y z) = s13(y, x)`, which fails only at
  y = z = 0.
- **Consistency gate.** A law is also refused if adding it to the
  congruence closure would make two distinct atoms (variables or
  constants) equal. A theory that proves x = y is false somewhere, so
  this check costs nothing and catches what sampling misses.
- **What a certificate claims.** It claims "holds on every assignment
  in the box, and is consistent with the held theory". It does not claim
  a proof:
  - β-equivalence is undecidable (Church 1936, Scott–Curry).
  - Commutativity of Church `+` is not even a λ-equation:
    `λmn.add m n` and `λmn.add n m` have different βη-normal forms, so by
    Böhm's theorem some non-numeral argument separates them.

  The law holds on the numeral sort only, which is why the sort is part
  of the certifier. Proof needs an induction principle over that sort
  (HipSpec, Zeno, TheSy). Under Church encoding it is absent from the
  calculus, and it is phase E5.

## 7. How operators and laws emerge

One epoch of the default stream (`Writer::emerge`, `Engine::step`):

1. **Generate.** Unrank closed β-normal forms of the current size from a
   cursor `(size, index)`. Unranking is deterministic and resumable, and
   memory is O(size²) counts instead of stored term lists.
   `terms_per_epoch` bounds the slice.
2. **Observe.** For each term, compute the following:
   - **Nullary:** if it observes to a numeral not yet held and fewer than
     `max_constants` are held, name it a constant.
   - **Unary/binary:** its table on the grid (0..6 unary, 0..3² binary).
     It is an operator *candidate* if all of the following hold:
     - the table is total;
     - it depends on every argument;
     - it is not a projection;
     - its image has at least `min_image_*` values;
     - no held operator has the same table.
3. **Conjecture (QuickSpec over the model's own vocabulary).** For a
   candidate `k`, build the universe of terms up to `law_max_size` over
   the held constants and operators plus `k` and three variables, with
   `k`'s terms first. Fingerprint every term on seeded numeral
   assignments, and make a term that collides with an earlier class
   representative a conjecture `term = representative`. Prune with
   congruence closure (seeded with every held law), then certify, then
   run the consistency gate.
4. **Name.** `k` is named only if all of the following hold:
   - it is **irreducible**: no certified law states `k(x̄) = t` with `t`
     free of `k`. This is the novelty clause of CLAUDE.md's reward,
     `irreducibility(s, L)`, made exact.
   - it carries at least `min_laws` certified non-ground laws.

   The definition goes through the stream's gate first, then each law.
5. **Persist.** Definitions and laws are ordinary `RewriteRule`s in the
   library, as `Symbol` heads, `Fn`/`Apply` bodies, and `Var ≥ 100`
   pattern variables. They are logged as `RuleCertified` events in
   `deltas.lisp` and snapshotted behind HEAD. Snapshot metadata
   (`stream.emergence.*`, `stream.substrate`) holds the cursor and the
   failed-candidate set.

**How this differs from QuickSpec, Hipster, TheSy and the old Frontier.**
All four take the *signature* as input: the user's functions or our 22
builtins. Here the signature is the *output* of steps 1–4. The names `+`,
`×`, `^` and `succ` never appear in Rust, and the law search runs over
symbols the machine minted from λ-terms. That is the difference between
looked up and found. Generation is still enumeration, as it is in
DreamCoder and Stitch. What emerges is the vocabulary and the theory, not
the search order.

**The prior emergent path, read before designing.** It is reused
where it applies; where it does not, this section says so.

- **Landmarks Phase H/I/J** (`MetaPatternGenerator`,
  `phase_h_unblock_pipeline_runs_end_to_end`) found laws by
  anti-unification. It minted `(?op (?op ?x ?id) ?id)` and the rank-2
  `S_30002`. Its corpora were built from builtin operators (Nat ADD/MUL,
  and tensor ops 20/21 for M0 in `first_model.rs`). So its laws emerged
  *over hand-written operators*, which is the gap this design closes.
- **Phase J never validated an operator-variable rule.** It substituted
  a `Number` into the head, so `eval` returned a stuck `Apply` and the
  rule failed. Rank-2 meta-rules were minted but never certified,
  because their RHS is an irreducible `Symbol`. There was no
  operator-variable validation path to reuse. Head-variable sorting and
  held-operator sampling (§6) are new. The anti-unification idea is
  reused at the design level for E4.

**Missing, and planned.**

- **Library compression (E4).** Anti-unify the held λ-definitions
  (Stitch-style, with binder-aware matching via slotted e-graphs, PLDI
  2025) and rank-1 law schemata (`?f(x, ?e) = x`). The schema is then
  certified with `?f` sampled from held operators, and its instances
  become its support.
- **Retroactive demotion.** When a later operator makes an earlier one
  reducible, demote the earlier one (`demotion.rs`) without deleting it.
- **A ranking agenda.** Name the most productive candidate first, not
  the first that passes (HR's interestingness measures, Colton 2002).

## 8. Measured results

Single scratch runs on this Mac in release mode, deterministic from the
seed (`cargo run --release -p mathscape-emergence --example emerge --
<epochs> [combinators]`, seed 7, `EmergenceConfig::default()`). They are
throwaway measurement runs, not models (governing law 4). The stream
reproduces the same sequence (`tests/emergent.rs` pins determinism and
resume).

Seed 7, `EmergenceConfig::default()` (λ grammar, generative filter
on, library atoms off). The writer itself was run from an empty
`MATHSCAPE_STREAM__DATA_DIR` under a scratch directory, and the
standalone runner produced the same sequence. The writer named `+` at
epoch 54, 294 s after a fresh start; the runner took 316 s.

**Timeline of the default (λ) model.** *Size* is the λ-term size being
enumerated; laws are certified and in the library. `s0` is the constant
1, `s1` is 0.

| epoch | wall s | size | named | what it is (read off its laws and table) |
|---|---|---|---|---|
| 0 | 0.00 | 2 | `c0 := λ0` | the constant **1** (the identity, read by η as `λf x. f x`) |
| 1 | 0.00 | 3 | `c1 := λλ0` | the constant **0** |
| 2 | 0.02 | 5 | `b2 := λλ. 0 1` | **exponentiation** `m^n`: `x^1 = x`, `x^0 = 1`, `1^x = 1` |
| 3 | 0.08 | 7 | `u3 := λλ. 1 (1 0)` | **square** |
| 5 | 0.84 | 8 | `b5 := λλλ. 1 (2 0)` | **multiplication**: `x·x = sq x`, `x·1 = x`, `x·0 = 0`, `x·y = y·x`, `x·(y·z) = y·(x·z)`, `sq x · sq y = sq (x·y)`, `(0^x)^y = 0^(x·y)` |
| 8 | 4.45 | 10 | `u8 := λλλ. 1 (2 1 0)` | **successor**: `x^(S 1) = sq x`, `x^(S S 1) = x · sq x`, `x^(sq (S 1)) = sq (sq x)` |
| 54 | 316 | 13 | `b52 := λλλλ. 2 1 (3 1 0)` | **addition**: `x + 1 = S x`, `x + 0 = x`, **`x + y = y + x`**, **`S (x + y) = x + S y`**, `x · 2 = x + x`, **`x + x·y = x · S y`**, `x + (y + z) = y + (x + z)` |

So by epoch 54 the model holds 0, 1, successor, addition,
multiplication, squaring and exponentiation. All were found as λ-terms
and none were given. Among the laws:

- the **Peano recursion equations** for `+` and `·`: `S(x+y) = x + S y`
  and `x·S y = x + x·y`;
- commutativity of `+` and `·`;
- left-commutativity, which with commutativity gives associativity;
- the identity and annihilator elements;
- instances of the exponent laws.

Each law was conjectured from the model's own observations and
certified on every assignment in `{0..6}³`. Rediscovered, as the
directive allows, not looked up.

**What else emerged.** Between multiplication and addition the model
named about 40 other binary operators. Typical ones are `λm. m (λ_. m)`
("first non-zero"), operators that test for zero, and mixtures of
those. They are real, total, law-bearing functions on numerals.
Schwichtenberg (1976) showed that simply-typed λ over Church numerals
defines exactly the **extended polynomials**: polynomials combined with
zero tests. The zoo is the zero-test half of that class, and the motor
rediscovers it as faithfully as it rediscovers `+`. The interestingness
knobs exist to order that zoo, not to steer toward `+`:

- `generative` (default on): an operator must produce, somewhere on the
  grid, a value that is neither one of its arguments nor a held
  constant. It drops pure selectors.
- `definable_filter`: skip any table that a first-order composition of
  held symbols of size ≤ 5 already has.

**Variants measured** (same seed and machine; the λ grammar unless
stated; epoch and wall time to the named event):

| variant | × named | + named | symbols at + | note |
|---|---|---|---|---|
| **default** (λ, generative, no library atoms) | epoch 5, 0.8 s | **epoch 54, 316 s** | 53 | the shipped default |
| λ, generative off | epoch 7, 1.8 s | epoch 116, 769 s | 120 | every zoo selector is named first |
| λ + held symbols as grammar atoms (DreamCoder-style) | — | — | after 34 epochs, 435 s: no commutative operator | atoms let iterated compositions of held symbols (such as `λx. x s3`) crowd the small sizes |
| SK grammar (`basis: combinators`), generative | epoch 3, 0.13 s (`S(KS)K`) | **epoch 16, 52 s** | 18 | addition is `S (K S) (S (K (S (K S) K)))`, with commutativity, `x + 0 = x` and `x + 1 = S x` |
| SK grammar, generative off | epoch 3 | epoch 25, 106 s | 27 | |

SK reaches `+` about 6× sooner. It stays a profile, not the default,
because its grammar rests on two λ-terms written in Rust (S and K, the
basis definitions in `engine.rs`), and that is exactly the given
vocabulary the governing law forbids. The λ grammar starts from nothing
the driver wrote. Measured cost of that purity: 264 s to `+` on this
machine.

**Memory.** The stream writer's RSS was sampled every 30 s with
`/bin/ps -o rss=` (`rss.tsv` in the scratch run). It held at 41–50 MiB
through epoch 34 with 530 library rules. The motor stores no term lists
(it unranks), so the bounded parts are the law universe
(`law_universe_max` = 20,000 terms) and per-name law intake
(`max_laws_per_name` = 16).

**Not emerged yet, measured honestly:**

- predecessor and subtraction, which under Church encoding need
  Kleene's pairing trick: size ≥ 20, past these runs;
- distributivity as a single law: its right side has size 7, past
  `law_max_size` 5. Only instances appeared, such as
  `x + x·y = x·S y`;
- any certified operator-variable law: the motor does not yet propose
  them (E4). The certifier supports them (§6).

## 9. Success criteria

| criterion | measure | status |
|---|---|---|
| a held symbol is extensionally `+` on the grid and on the certification box | `b52`, epoch 54 (seed 7) | **met** |
| commutativity of that symbol is certified | `law (s52 x y) = (s52 y x)` | **met** |
| a held symbol is `×` with comm, identity (`c0 = 1`) and annihilator (`c1 = 0`) | `b5`, epoch 5; pinned by `multiplication_and_its_commutativity_emerge_from_lambda_terms_alone` | **met**, CI-pinned |
| successor is held, and `x^(S 1)` = square is a law | `u8`, epoch 8 | **met** |
| the default stream holds no builtin, Number or Point anywhere | `tests/emergent.rs` | green |
| kill-9 resume converges on the uninterrupted chain | `tests/emergent.rs` | green |
| memory stays bounded | stream RSS 41–50 MiB through epoch 34 (§8) | **met** for the measured window |

## 10. Phases

| phase | content | status |
|---|---|---|
| E1 | kernel: λ terms, normalizer, observation, numeral sort | **built**, `mathscape-kernel` tests |
| E2 | `KernelCertifier`: definitions, sorted laws, operator variables from held definitions, exhaustive small box | **built**, red→green for the operator-variable test |
| E3 | emergent motor and stream profile as default; builtin and Frontier configured off; substrate pinned per HEAD | **built**, `tests/emergent.rs`, config enforcement test |
| E4 | library compression: anti-unify held definitions and laws; rank-1 schemata certified over held operators | designed |
| E5 | induction over the numeral sort: certify a law by a recursor-shaped proof, so certificates become proofs | designed |
| E6 | driver in tatara-lisp: enumerate, observe, name and conjecture as Lisp over kernel primitives; Rust keeps E1, E2 and persistence | designed |
| E7 | retroactive demotion and an interestingness agenda | designed |

## References

- Ellis et al., *DreamCoder*, PLDI 2021, arXiv:2006.08381 — λ-calculus base, wake-sleep library learning, MDL objective.
- Bowers et al., *Top-Down Synthesis for Library Learning* (Stitch), POPL 2023, arXiv:2211.16605.
- Cao et al., *babble*, POPL 2023, arXiv:2212.04596 — library learning modulo a *given* equational theory.
- Claessen, Smallbone, Hughes, *QuickSpec*, TAP 2010; Smallbone et al., JFP 2017; Johansson et al., *Hipster*, CICM 2014, arXiv:1405.3426; Singher & Itzhaky, *TheSy*, CAV 2021, arXiv:2009.04826; Claessen et al., *HipSpec*, CADE 2013.
- Kiselyov, *λ to SKI, Semantically*, FLOPS 2018 — quadratic worst case of bracket abstraction.
- Tromp, *Binary Lambda Calculus and Combinatory Logic*, 2006.
- Barendregt, *The Lambda Calculus*, 1984 — Böhm's theorem; Scott–Curry undecidability.
- Willsey et al., *egg*, POPL 2021 (λ example: explicit substitution); Schneider et al., *Slotted E-Graphs*, PLDI 2025.
- Rissanen, *Modeling by shortest data description*, Automatica 1978; Grünwald, *The MDL Principle*, MIT Press 2007.
- Lenat, *AM*, Stanford 1976; Lenat & Brown, *Why AM and Eurisko Appear to Work*, AAAI-83 / AI 23(3) 1984; Ritchie & Hanna, AI 23(3) 1984.
- Schwichtenberg, *Definierbare Funktionen im λ-Kalkül mit Typen*, Arch. Math. Logik 17, 1976 — the extended polynomials (polynomials with zero tests), the class the binary operators in §8 come from.
- Colton, *Automated Theory Formation in Pure Mathematics*, Springer 2002 (HR).
- Jackson, *Software Abstractions* (Alloy), 2006 — the small-scope hypothesis behind exhaustive small-box checking.
