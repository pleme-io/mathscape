# Knowability Criterion — When the Machine Is "Done"

`realization-plan.md` ends Phase I with:

> One continuous run produces at least one Rust primitive promoted
> via the full seven-gate pipeline, migration applied, the system
> continues running in an expanded primitive space.

That's a *completion* criterion — the machine works end-to-end. It
is not the same as a **knowability** criterion, which is stronger:
the machine's output is proven-by-construction and replayable under
different policies.

This document specifies the full knowability criterion. Meeting it
is the true "done" state for mathscape v1.

## The knowability claim (inherited from the platform)

From `pleme-io/CLAUDE.md`:

> A knowable platform is a computing environment where every
> capability is proven by construction. You do not hope it works —
> you KNOW it works because the proof exists.

For mathscape specifically, this means every promoted Rust primitive
must carry, at the time of its promotion, **all** the evidence that
justifies its existence — and that evidence must be independently
verifiable.

## The four components of knowability

### 1. Derivation chain completeness

Every primitive has a complete hash chain from corpus → Artifact →
PromotionSignal → AxiomProposal → Certificate → EmissionOutput →
MigrationReport → TypescapeContribution. No missing links.

Verification: given any of the intermediate hashes, walk the chain
both directions. Every hop resolves to a stored artifact.

### 2. Replayability under identical policy

Given the corpus snapshot sequence + the policy used, running
mathscape from an empty registry reproduces the *exact* same
trajectory (byte-identical content hashes at every step).

Verification: a test run stores `(policy, corpus_sequence, expected_registry_root)`,
a replay run computes the actual registry root, equality check.

### 3. Policy-differentiated trajectories

Running mathscape with a *different* policy on the *same* corpus
produces a visibly different trajectory (different registry root,
different primitives promoted, or different promotion order).

Verification: `(policy_A, corpus) → root_A`; `(policy_B, corpus) →
root_B`; `root_A ≠ root_B` for at least one (A, B) pair.

### 4. External verification of at least one promotion

At least one promoted primitive's Lean 4 export is checked by
`lean 4` (out-of-process, not by mathscape itself), and accepted.

Verification: CI step runs `lean build` on the exported proof file
and asserts exit code 0.

## The knowability test matrix

For mathscape to be declared v1-complete:

| Criterion                          | Test                                                                 | Phase          |
|------------------------------------|----------------------------------------------------------------------|----------------|
| Chain completeness                 | `test_derivation_chain_complete(promoted_primitive)`                  | Phase I        |
| Replayability                      | `test_replay_identical(policy, corpus) → same root`                   | Phase I        |
| Policy differentiation              | `test_policies_diverge([policy_A, policy_B], corpus) → different roots` | Phase I        |
| External Lean 4 verification       | CI job runs `lean build` on exported proof                            | Phase I or J   |
| Typescape leaf binding             | `test_typescape_contribution(promoted) → root changed in typescape`   | Phase H / I    |
| Demotion path works                 | `test_demotion(primitive) → reverse_migration valid`                  | Phase J        |
| Calcification diagnostic fires     | fake corpus with monotone growth → `Regime::Calcified` detected       | Phase L        |

Meeting rows 1-5 is *mathscape v1*. Rows 6-7 are *v1.1*.

## What makes this stronger than Phase I acceptance

Phase I asks: "does it work?"
Knowability asks: "can you prove it?"

The difference:

- **Phase I**: one run, one promoted primitive, manually inspected
- **Knowability**: a property that holds for every future run forever
  under this codebase, verified by automated tests

Once knowability holds, adding new domains (ml-forge, iac-forge)
inherits the property without re-proving it — they just implement
the same five trait parameters (`primitive-forge` Phase L extract).

## The fifth component (nice to have, not required)

A *formal specification* of the machine's behavior as a mathematical
object (possibly as a Lean 4 theory) from which the invariants above
are derived as corollaries, not just tested as properties.

Not v1. This is a research artifact, valuable but not blocking. Flag
for Phase L or as a separate research track.

## Consequences for code

1. **Phase I's acceptance tests must include all four knowability
   criteria as CI tests**, not just the pipeline-runs-end-to-end
   test. Update `realization-plan.md` Phase I to reflect this.
2. **The bridge to Lean 4 is critical**, not optional. `mathscape-proof::LeanExporter`
   must be real, not a stub, by Phase I.
3. **Cross-policy diff must be a first-class tool** — a CLI command
   `mathscape diff policy-A.yaml policy-B.yaml corpus.sexpr` that
   runs both and reports the hash-level diff.

## Honest admission

Until these criteria are met, mathscape is a *plausible* forced-
realization system, not a *knowable* one. The plausibility is
valuable — it lets us iterate fast. But the claim in
`rust-lisp-duality.md` that "this pattern is the mechanism by which
proven axiom sets grow" requires knowability to be literally true,
not just designed for.

Meeting the knowability criterion is the moment mathscape stops
being a design and becomes a fact.
