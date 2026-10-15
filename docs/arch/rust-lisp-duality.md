# Rust ↔ Lisp — The Solidification Pair

The foundational insight the other docs rely on without naming. This
document makes it explicit. Everything else — forced realization, ten
gates, reward calculus, axiomatization pressure — is a consequence of
the Rust/Lisp division of labor.

> Rust solidifies new ground rules. Lisp runs the epochs that propose
> them. The machine that moves candidates between the two is what any
> forced-realization system fundamentally *is*.

## The two languages, the two affordances

| Affordance                            | Rust provides                                     | Lisp provides                                             |
|---------------------------------------|---------------------------------------------------|-----------------------------------------------------------|
| **Closed axiom set**                  | enum + trait system; once declared, immutable     | (none — Lisp is open)                                     |
| **Exhaustive reasoning**              | `match` + `#[non_exhaustive]` + clippy            | (none — patterns are ad-hoc)                              |
| **Proof-by-typing**                   | Curry-Howard; types ARE propositions              | (none — types are runtime-dynamic)                        |
| **Final gatekeeper**                  | `rustc`: if it doesn't compile, it doesn't exist | (none — anything parses)                                  |
| **Proof inheritance**                 | blanket trait impls over sealed traits            | (none — no trait mechanism)                               |
| **Universal interchange**             | (requires macros / codegen)                       | s-expressions; any domain encodable without new syntax    |
| **Zero-ceremony composition**         | (requires trait design + impls)                   | apply a head to arguments; done                           |
| **Cheap experimentation**             | (expensive — recompile, re-typecheck)             | parse, eval, discard in microseconds                      |
| **Code-as-data**                      | (syn + quote + proc-macros — heavy)               | native; every program is a value                          |
| **Commitment cost**                   | irreversible-ish (refactoring an axiom is costly) | zero — throw away the sexpr                               |

Each language is *missing* the affordances the other provides. Rust
without Lisp is brittle: the axiom set grows only at the rate a human
can type new types. Lisp without Rust is ungrounded: exploratory
motion produces no cumulative progress because nothing is ever
committed.

The pair together is **self-extending**: cheap experimentation in
Lisp, permanent commitment in Rust, a machine that moves proven
experiments from one to the other.

## The solidification machine

```
┌─────────────────── Lisp side (flexible) ───────────────────┐
│                                                              │
│  sexpr proposals ──► reward calculus ──► gates 1–3          │
│       ▲                                       │              │
│       │                                       ▼              │
│       │                                 library entry        │
│       │                                 (Conjectured)        │
│       │                                       │              │
│       │                          reinforcement / merge /     │
│       │                          subsumption / advance       │
│       │                                       │              │
│       │                                       ▼              │
│       │                                 Axiomatized           │
│       │                                       │              │
│       │                                gates 4 + 5            │
│       │                                       │              │
│       │                                       ▼              │
│       │                               PromotionSignal        │
│       │                                       │              │
└───────┼───────────────────────────────────────┼──────────────┘
        │                                       │
        │ migration rewrites                    │ axiom-forge bridge
        │                                       ▼
┌───────┼───────────── Rust side (committed) ──┼──────────────┐
│       │                                       │              │
│       │                               AxiomProposal          │
│       │                                       │              │
│       │                                gate 6                 │
│       │                               (7 obligations)         │
│       │                                       │              │
│       │                                       ▼              │
│       │                               EmissionOutput         │
│       │                                       │              │
│       │                                gate 7                 │
│       │                                (rustc typecheck)      │
│       │                                       │              │
│       │                                       ▼              │
│       │                               new Rust primitive     │
│       │                                       │              │
│       └───────────────────────────────────────┘              │
│                      MigrationReport                         │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

Everything above the boundary is flexible. Everything below is
committed. The boundary is axiom-forge's 7 obligations + rustc.

## Why *Lisp* specifically

Any exploratory substrate would do in principle. Lisp wins on three
counts:

1. **Universal data structure**. S-expressions can encode any domain's
   proposals without new syntax. A mathscape term, an IaC resource, an
   ML UOp, a compliance rule — same surface form. The machinery that
   computes content hashes, diffs, canonical emissions, etc. is
   written once and reused everywhere.
2. **Code = data**. The reinforcement pass rewrites existing rules as
   it merges and subsumes. In Lisp, "a rule" and "an operation on a
   rule" have the same representation. Transformations compose
   without codegen overhead.
3. **Content-addressing is natural**. A canonical printer over
   s-expressions + BLAKE3 gives a stable identity without type-system
   cooperation. The Rust side consumes this hash; neither side owns
   it.

These three together are what lets `iac_forge::sexpr`'s
`ToSExpr`/`FromSExpr`/`ContentHash` be the *universal* interchange
across mathscape, axiom-forge, arch-synthesizer, and every
`*-forge` backend.

## Why *Rust* specifically

Lisp freezes things by convention; Rust freezes things by type. Any
typed, ADT-first, non-garbage-collected, trait-driven language would
fit. Rust wins on four counts:

1. **Exhaustive pattern matching on closed enums**. The moment a new
   `Term` variant is added, every downstream match on `Term` must
   handle it — either explicitly or via `#[non_exhaustive]` + a
   wildcard. This makes primitive additions *mechanically propagatable*:
   the compiler tells you every place that needs attention.
2. **Blanket impls over sealed traits**. A new primitive gains all
   the derived behaviour of its family (sexpr round-trip, content
   hash, diff, cache key) automatically. Proofs travel with types,
   not with call-sites.
3. **Zero-cost abstractions**. The solidified primitive is as fast as
   if it were hand-written at the machine level. There is no
   performance tax for having used the forced-realization pipeline.
4. **No garbage collector, no runtime**. Primitives ship as part of a
   library; they work in WASM, in embedded, in kernels. The
   solidified axiom set is portable to any compute substrate without
   a language runtime tax.

## Where Lisp's flexibility ends (and that's fine)

Lisp is flexible up to **vocabulary**. Once you commit to a sexpr
grammar (head + args, with a type map for primitives), you have a
vocabulary. Candidates must use it; violations are parse errors. This
is not a limitation — it's the *interface* between Lisp and Rust.

The vocabulary itself is defined by Rust's enums (via `FromSExpr`
impls). So every time Rust grows (a new primitive lands), the
vocabulary grows and Lisp's flexibility expands. The two are coupled:
flexibility on the Lisp side is a *function of* the closed axiom set
on the Rust side.

This is the coupling the metacircular loop formalizes. Lisp can
propose anything expressible in the current vocabulary. Accepted
proposals become new vocabulary. Rejected proposals are free.

## The pattern is universal

Any domain where you need:
- an open exploration substrate (propose, test, discard cheaply), *and*
- a closed, proven, type-checked foundation (commit, rely, type-check)

…fits the Rust/Lisp dual.

| Domain               | Lisp-side explores               | Rust-side solidifies                                |
|----------------------|----------------------------------|-----------------------------------------------------|
| **mathscape**         | Term expressions; rewrite rules  | `Term` enum; `ProofStatus` lattice; UOp additions   |
| **iac-forge**         | IacResource compositions         | `IacType`, `Backend` trait, blanket ProvenMorphism  |
| **ml-forge**          | Tensor graphs                    | `UOp` (15 primitives), shape inference              |
| **axiom-forge**       | AxiomProposal sexpr values       | axiom emission + 7 obligations                      |
| **arch-synthesizer**  | typescape queries over a corpus  | TypescapeManifest, cryptographic attestation        |
| **compliance**         | policy sexpr rules              | `Pattern`, `Rule`, compliance lattice               |

None of these was designed as a Rust/Lisp dual on purpose. They
converged on it because the affordance gap between the two languages
*is* the work that needs doing. Any such system has an exploratory
substrate and a solidified axiom set whether or not it names them.

## How this hooks into everything else

- **`forced-realization.md`** — the five forces and ten gates describe
  how candidates cross from flexible to committed
- **`machine-synthesis.md`** — the five architectural objects sit
  entirely on the Lisp side until Promotion fires gate 6
- **`axiomatization-pressure.md`** — the reinforcement pass operates
  on the Lisp side; only Promotion crosses the boundary
- **`reward-calculus.md`** — ΔDL is a Lisp-side measurement; the Rust
  side has no opinion on reward
- **`promotion-pipeline.md`** — the boundary crossing in full detail
- **`METACIRCULAR.md`** (iac-forge) — the canonical precedent research

The deeper claim across all these: **what appears to be a
math-discovery system is really a substrate-extension system applied to
mathematics.** Replace the corpus and the gate thresholds, and the
same machinery extends the IaC axiom set, or the ML UOp set, or any
other closed-set-that-wants-to-grow.

## The solidification pipeline as reusable infrastructure

Because the pattern is universal, the pipeline components can be
reused:

- **`iac_forge::sexpr`** — the universal interchange format
- **`axiom-forge`** — the 7-obligation gate
- **`arch-synthesizer::typescape`** — the index of what exists
- **`tameshi` / `sekiban`** — the cryptographic attestation

Each of these is already a library. A new forced-realization system
doesn't rebuild them; it provides:

1. A corpus source
2. A domain-specific generator (how proposals are produced from the
   corpus)
3. A domain-specific verifier (how structural advance is checked)
4. Gate thresholds

…and inherits the rest. This is what makes mathscape implementable in
~12 phases rather than being a multi-year research project.

## The deep statement

A system that must **solidify new ground rules over time** has two
loads to carry:

1. *Exploration load* — candidates must be cheap to produce, score,
   reject.
2. *Commitment load* — accepted candidates must be permanent,
   inherited, and propagate through every dependent site.

No single language carries both loads well. The Rust/Lisp pair carries
both, with axiom-forge as the rivet between them. Every pleme-io
primitive domain is already an instance of this pattern. Mathscape is
the first to drive the pattern *autonomously*, where the Lisp-side
generator is itself a learning process rather than a human typing
sexpr files.

This is not a contingent architectural choice. It is a consequence of
what it means to run epochs that solidify rules.
