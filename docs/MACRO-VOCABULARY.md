# mathscape — macro vocabulary (design, not executed)

> Same discipline as `pleme-io/sui/docs/MACRO-VOCABULARY.md` and the
> org ★★ EMITTER SUBSTRATE rule: **macros pay only on genuine recurring
> shapes.** Two places pay — *Layer-B* domain/dispatch tables (one
> algorithm transcribed N times against a table of cases → collapse the
> whole *drift class*) and *Layer-A* real hand-kept impl tables (an
> enum's `ALL`/`rank`/`slug` match already exists → an existing farm
> derive collapses it). Everything else is **LEAVE-ALONE**, called out
> loudly. Reuse over new; a one-off is inlined and left alone until the
> pattern ripens.
>
> Grounded in a hands-on read of every crate + live runs (2026-07-11,
> HEAD `cf5a17a`). This is a **design doc — no refactor is executed
> here.** Each row cites `file:line` and a real site count.

## TL;DR — the one drift class that dominates

**25 hand-written `*_to_sexp` / `*_from_sexp` functions across 10 files**
are the same shape transcribed by hand, and the source already *admits
it*: `bootstrap_lisp.rs:4` says *"Pairs with R10.1's `policy_to_sexp`
and R12.1's `primitive_to_sexp` — same pattern, same level of
fidelity."* That is the definition of a drift class. The single
highest-leverage move in the repo is a **`#[derive(SexpForm)]`** (a new
TataraDomain-style derive) that emits the `(head :kw val …)` writer +
the keyword-args reader + the field accessors from one struct/enum
declaration. It kills the drift class *and* deletes the 9 duplicated
field-accessor helpers *and* removes the already-observed drift between
the two hand-written Sexp arithmetic evaluators.

The three highest-leverage families, ranked:

| Rank | Family | Layer | Sites | Kills |
|---|---|---|---|---|
| 1 | `#[derive(SexpForm)]` — keyword-record ↔ Sexp | B | 25 fns / 10 files | the whole to/from_sexp drift class + 9 dup accessor helpers |
| 2 | `sexp_arith!` — one tiny arithmetic-Sexp interpreter | B | 2 evaluators, already drifted | reward↔fitness operator-table drift |
| 3 | `#[derive(LatticeEnum)]` — linear proof-status ladder | A | `ProofStatus` (rank/next/advance) + 3 sibling enums | hand-kept rank/order tables |

Everything else surveyed is LEAVE-ALONE (see the rejection list).

---

## Per-crate catalog

### mathscape-proof — the Sexp-bridge epicentre

| Shape | Site(s) | Verdict | Mechanism | Recurrence justification |
|---|---|---|---|---|
| `policy_to_sexp` / `policy_from_sexp` | `policy_lisp.rs:51,69` | **Layer-B → family #1** | `#[derive(SexpForm)]` on `LinearPolicy` | keyword-record ↔ Sexp; field list transcribed twice today |
| `MechanismConfig::to_sexp/from_sexp` | `mechanism.rs:166,218` | **Layer-B → family #1** | `#[derive(SexpForm)]` on `MechanismConfig` | 15 fields hand-pushed then hand-parsed; `clamp()` stays a hand hook |
| `MechanismMutation::to_sexp/from_sexp` | `mechanism.rs:460,525` | **Layer-B → new `(defmutation-op)`** | per-variant `(op-name arg)` catalog | 17 variants each with a `list("kebab", int)` + a `"kebab" =>` arm — a **variant-name↔symbol** table hand-kept in two directions; CATALOG-REFLECTION-shaped |
| `evaluate_fitness_form_inner` | `mechanism.rs:851` | **Layer-B → family #2** | shared `sexp_arith!` interpreter | `+ - * / > >= < <= max min if` dispatch — duplicated (see reward) |
| `primitive_to_sexp` / `primitive_from_sexp` | `primitives_lisp.rs:46,120` | **Layer-B → new `(defprimitive)`** | per-variant kind-tagged catalog | 11 `MlPrimitive` variants, each `Sexp::symbol("kind-slug")` + reader arm — the discovered-abstraction alphabet, a real table |
| `spec_to_sexp` / `spec_from_sexp` + `rule_to_sexp` + `term_to_sexp` | `bootstrap_lisp.rs:49,85,125,159` | **Layer-B → family #1 (+ recursive `term`)** | `#[derive(SexpForm)]` + one hand-written recursive `Term` arm | the file's own header names this a copy of policy/primitive |
| `map_event_*` (see core) | `map_event_lisp.rs` | Layer-B → family #1 | — | 7-variant event record ↔ Sexp |

### mathscape-core

| Shape | Site(s) | Verdict | Mechanism | Recurrence justification |
|---|---|---|---|---|
| `ProofStatus::{rank,next_structural,can_advance_to,is_active}` | `lifecycle.rs:143,156,173,183` | **Layer-A → `#[derive(LatticeEnum)]`** | a small farm derive for a **linear-ordered lifecycle enum** | a genuine hand-kept rank table (0..6 + `u8::MAX`) + successor map; the exact shape a derive fits |
| `Event::{delta_dl,category}` | `event.rs:106,123` | **Layer-A(partial)** — `#[derive(GetterAll)]`-adjacent | reuse `pleme-getter-derive` for `delta_dl` field-extraction; **hand-keep `category`** | `delta_dl` is a pure by-field extract across 8 variants (derive fits); `category` is a *semantic partition* (Discovery/Reinforce/Promote/Meta) — that's authored knowledge, not mechanical. **Split the verdict.** |
| `EventCategory`, `Regime`, `Domain`, `DemotionReason` | `event.rs:22`, `control.rs:20`, `law_generator.rs:527`, `lifecycle.rs:89` | **LEAVE-ALONE** (see #3 note) | — | small closed enums with *semantic* (not mechanical) matches like `dominant_category`; a derive would add no table |
| `Term` `Display` / `canonical` / `substitute` / `size` / `depth` | `term.rs:47,59,114,271,464` | **LEAVE-ALONE** | — | recursive structural algorithms, not a flat table; the AST is *the* domain, not a candidate for a generic derive |
| `MlPrimitive` variant → symbol slug | `primitives.rs:54` + `primitives_lisp.rs` | folded into `(defprimitive)` (family, proof crate) | — | the slug table lives with the Sexp bridge, not the enum def |
| `map_event_*` bridge | `mathscape_map.rs` / `map_event_lisp.rs` | family #1 | — | see proof crate |

### mathscape-reward

| Shape | Site(s) | Verdict | Mechanism | Recurrence justification |
|---|---|---|---|---|
| `evaluate_reward_sexp` | `lisp_reward.rs:62` | **Layer-B → family #2** | shared `sexp_arith!` | the **second** copy of the arithmetic-Sexp interpreter; has `clamp`, lacks the comparison ops the fitness copy has — **already drifted**, which is the whole argument |
| `bindings_from_axes` | `lisp_reward.rs:192` | LEAVE-ALONE | — | an 8-entry hand map; a macro buys nothing, and the axis names are a stable contract |
| `Regime`-weighted reward config | `reward.rs` | LEAVE-ALONE | — | small typed weights, no dispatch table |

### mathscape-evolve

| Shape | Site(s) | Verdict | Mechanism | Recurrence justification |
|---|---|---|---|---|
| `MutationOp` + `ALL_OPS` const + `apply_mutation` dispatch | `mutate.rs:9,24,39` | **Layer-A → reuse `pleme-allvariants-derive`** for `ALL_OPS`; **hand-keep** `apply_mutation` | `pleme-allvariants-derive` | `ALL_OPS: [MutationOp;6]` is exactly the hand table `#[derive(AllVariants)]` collapses; but the `apply_mutation` body dispatches to 6 *distinct hand-written functions* — that's real code, not a table, LEAVE it |
| `MutationPolicy` trait + `Uniform`/`Tracking` | `policy/lib.rs:21` | LEAVE-ALONE | — | two small impls, an ergonomic trait; deriving would regress it |

### mathscape-compress

| Shape | Site(s) | Verdict | Mechanism | Recurrence justification |
|---|---|---|---|---|
| `Domain::{detect,detect_in}` | `law_generator.rs:527` | LEAVE-ALONE | — | head-op → domain is a *structural* match over `Term`, not a flat slug table |
| anti-unify / e-graph / extract | `antiunify.rs`, `egraph.rs`, `extract.rs` | LEAVE-ALONE | — | genuine algorithms; the org rule explicitly excludes "domain algorithm" from generic derives |

### mathscape-cli / -mcp / -service / -api / -inference-api

| Shape | Verdict | Recurrence justification |
|---|---|---|
| clap subcommands, axum handlers, `emit_rust` `format!` blocks | LEAVE-ALONE | one-offs; the CLI's `format!` emitter (`main.rs:237`) is a genuine ★★ TYPED-EMISSION target *only if* it recurs — it's a single call site today, so inline it and wait |
| DTOs matching `openapi.yaml` field-for-field | LEAVE-ALONE | forge-gen already owns this generation path (Phase Y.1/Y.2); a local macro would fork it |

---

## The three macro families, specified

### Family #1 — `#[derive(SexpForm)]` (Layer-B, highest leverage)

**The shape it collapses.** Every `*_to_sexp` writes
`items.push(Sexp::keyword("kebab-name")); items.push(<encode field>)`
per field, and every `*_from_sexp` re-lists the same fields as
`"kebab-name" => out.field = <decode>?`. The field list is authored
**twice per type, in two directions**, plus a hand-written
`parse_keyword_args`/`int_of`/`float_of`/`string_of`/`symbol_val`
helper set re-pasted into each file (9 such helpers counted).

**The derive.** A farm derive (TataraDomain-family, in
`pleme-io/tatara-rust-ast`) that, from
```rust
#[derive(SexpForm)]
#[sexp(head = "policy")]
struct LinearPolicy {
    #[sexp(kw = "generation")]  generation: u64,
    #[sexp(kw = "weights")]     weights: [f64; 9],
    // ...
}
```
emits `to_sexp` (the keyword-record writer) + `from_sexp` (the
keyword-args reader, unknown-keyword-tolerant, wrong-arity-rejecting)
+ the shared accessor helpers *once* in the farm crate. Hand hooks
(`MechanismConfig::clamp` after parse; `Term`'s recursive arm) stay as
`#[sexp(post = clamp)]` / `#[sexp(with = term_codec)]` escape hatches —
the derive covers the mechanical 90%, the domain keeps its 10%.

**Why it earns its keep (the third-use test):** 5 clean keyword-record
consumers today (`policy`, `MechanismConfig`, `BootstrapCycleSpec`,
`trainer-snapshot`, `plasticity-report`) + the 7-variant `map-event` and
11-variant `MlPrimitive` catalogs (via the sum-type arm). Well past the
three-use bar; the pattern is *stated as recurring in the source
comments themselves.*

### Family #2 — `sexp_arith!` (Layer-B)

**The shape.** `evaluate_reward_sexp` (reward) and
`evaluate_fitness_form_inner` (proof) are two hand-written recursive
walkers over `(op args…)` with an operator dispatch that is *supposed*
to be the same and **has already diverged** (reward has `clamp`; fitness
has `> >= < <=`). This is the canonical Layer-B drift: one algorithm,
two transcriptions, silent divergence.

**The primitive.** One authored operator table (a
`macro_rules! sexp_arith!` or a small `OpTable` const in a shared
`mathscape-sexp` helper module) that both crates consume, each declaring
only its *bindings source* (axis map vs `TrialResult` keyword) and its
*allowed operator subset*. The comparison/`clamp` ops become table rows,
not per-copy code — divergence becomes unrepresentable.

**Note (tier-honest):** this is the smaller LOC win but the *correctness*
win — the two evaluators feeding accept/reject and mechanism-fitness must
not silently disagree on shared operators.

### Family #3 — `#[derive(LatticeEnum)]` (Layer-A)

**The shape.** `ProofStatus` hand-keeps `rank()` (a 7-row 0..6 table),
`next_structural()` (a successor map), `can_advance_to()` (a `rank <`
comparison), `is_active()` (a terminal-variant predicate). This is a
*real* hand-kept table — the exact case Layer-A derives pay.

**The derive.** A `#[derive(LatticeEnum)]` where a `#[lattice(rank = N)]`
attribute per variant emits `rank`/`next`/`can_advance_to`/`is_active`.
It fits *only* `ProofStatus` (the linear ladder); the sibling enums
(`Regime`, `EventCategory`, `Domain`) have **semantic** matches
(`dominant_category`, `detect`) that are authored knowledge — those stay
hand-written. Scope tightly: one adopter today, add the derive to the
farm only when a second linear-lifecycle enum appears. **Until then this
is a LEAVE-ALONE-but-noted** — one hand-kept ladder does not yet justify
a new farm derive; the note is the trigger.

---

## Rejection list (LEAVE-ALONE — loud, per the org rule)

Force-fitting a macro onto any of these is over-abstraction debt:

1. **`Term` and every method on it** (`size`/`depth`/`substitute`/
   `canonical`/`apply_canonical`/`Display`/`free_vars`). The expression
   tree is *the domain*, walked by recursive structural algorithms — not
   a flat table. A generic derive cannot express canonicalization.
2. **The compress algorithms** — `antiunify`, `egraph`, `extract`,
   `law_generator::Domain::detect`. Genuine domain algorithms; the org
   rule explicitly excludes these.
3. **`MutationPolicy` / `UniformPolicy` / `TrackingPolicy`** — ergonomic
   two-impl trait; a per-field derive would regress the hand API.
4. **`bindings_from_axes`** — an 8-entry stable-contract map; a macro
   buys nothing.
5. **`Regime`, `EventCategory`, `DemotionReason`, `Domain`** as enums —
   small, closed, **semantic** matches (not mechanical). A derive adds a
   speculative table nobody reads.
6. **`Event::category`** (but NOT `Event::delta_dl`) — `category` is an
   authored 4-way partition; only `delta_dl`'s by-field extraction is
   mechanical. Split, don't blanket-derive.
7. **`apply_mutation` dispatch body** — dispatches to 6 distinct
   hand-written mutation functions; that's code, not a table (only the
   `ALL_OPS` const is table-shaped).
8. **CLI/axum/DTO glue** — one-offs; DTOs are forge-gen's job already.
9. **`emit_rust` / CLI `format!` blocks** — single call sites today.
   TYPED-EMISSION applies *when they recur*; inline and wait for the
   second site.

---

## Phased execution plan (highest drift-class kill first)

**Phase M0 — `#[derive(SexpForm)]` for the 5 keyword-records.**
Author the farm derive; adopt on `LinearPolicy`, `MechanismConfig`,
`BootstrapCycleSpec`, `trainer-snapshot`, `plasticity-report`. Delete the
9 duplicated accessor helpers; centralize them in the derive's runtime.
Gate: every existing `*_sexp` round-trip test stays green (they already
exist and are the byte-pin). Biggest single drift-class kill.

**Phase M1 — sum-type arm of `SexpForm` for the two catalogs.**
Extend the derive to kind-tagged sums; adopt on `map-event` (7 variants)
and `MlPrimitive` (11 variants, via `(defprimitive)`). Same green-test
gate. Adds CATALOG-REFLECTION shape (a variant with source but no arm
becomes a compile error).

**Phase M2 — `sexp_arith!` shared interpreter.**
Lift one operator table into a shared `mathscape-sexp` helper; repoint
`evaluate_reward_sexp` + `evaluate_fitness_form_inner` at it. Reconcile
the drifted operator sets (decide whether comparisons + `clamp` belong to
both). Gate: the reward-form gold test + the fitness-form tests + the
`phase_ml_reward_mutation_probe` behavioural probe (173-rule swing) stay
identical.

**Phase M3 (deferred, noted-not-built) — `MechanismMutation` as
`(defmutation-op)` + `LatticeEnum` for `ProofStatus`.**
The mutation-op variant↔symbol table and the proof-status ladder are real
tables but single-adopter each; land the derive only when the *second*
consumer of each shape appears. Leave a `pending-macro:` note at each
site.

---

## Compliance notes

- **Reuse-first:** family #1/#2/#3 all target the existing
  `pleme-io/tatara-rust-ast` farm + a shared `mathscape-sexp` helper —
  no fresh macro crate is minted for mathscape alone. `SexpForm` is
  fleet-general (any crate serializing to `tatara_lisp::ast::Sexp` wants
  it), so it belongs in the farm, not here.
- **Byte-pin:** every generated `to_sexp` is pinned by an already-shipped
  round-trip test. Adoption is a refactor under a green gate, never a
  behaviour change.
- **The rejection list is load-bearing.** mathscape is a *mature,
  ergonomic* codebase; blanket-deriving its `Term` methods or its
  semantic enum matches is exactly the over-abstraction the org rule caps
  at diminishing returns. Only the three named families (and only where
  cited) demonstrably reuse the same shape ≥3 times.
