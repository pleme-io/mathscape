# Cross-Domain Application — iac-forge as Rust/Lisp Dual

Second test of the universality claim. `pleme-io/iac-forge` is the
platform's IaC axiom layer: `IacType` is a closed enum, `Backend` is
a trait with 7 impls, the IR composes via `Morphism`. This doc
checks whether the forced-realization machine fits iac-forge too.

## The shape today

- **Rust side**: `IacType` (String/Integer/Float/Boolean/List/Set/Map/
  Object/Enum/Numeric/Any; `#[non_exhaustive]`), `Backend` trait,
  `ArtifactKind`, `ResourceOp` transform DSL, `Pattern` / `Rule` /
  `Policy`, `Fleet`. All closed, all typed.
- **Lisp side**: already explicit. Every `IacResource`, `IacAttribute`,
  `Pattern`, `Transform` has `ToSExpr` / `FromSExpr`. Canonical
  emission + BLAKE3 is already in production.

iac-forge is the **most mature** Rust/Lisp dual in the platform. It
just hasn't been formalized as a forced-realization system yet.

## How the machine would grow IaC primitives

Corpus: cloud-provider resource definitions (AWS 448 resources,
Azure 27, GCP 27, Cloudflare 202, Akeyless 122, …).

Candidate: a proposed `IacType` variant or a proposed `Backend` trait
method.

Realistic growth scenarios:

| Scenario | What candidate is proposed | Promotion criterion |
|---|---|---|
| Cloud providers add a new primitive type (e.g., `Duration`) | `IacType::Duration` | condensation K: how many existing resources use string-encoded durations? cross-corpus: appears in AWS + Azure + GCP? |
| Pattern emerges: "resources with `name` + `description` + `tags` share a shape" | Abstract `Taggable` trait shape | condensation: subsumes 300+ resources' tag-handling code; cross-corpus: all cloud providers |
| Compliance pattern emerges: "sensitive-immutable together" | New Policy primitive | condensation: used in 12 compliance baselines; cross-corpus: NIST + CIS + FedRAMP |

Mathscape's machinery handles all three identically. The corpus
changes, the ΔDL formula changes (resource-line count instead of
sexpr node count), the reward weights shift — but the machine is the
same.

## What iac-forge contributes

**Compliance-lattice subsumption as gate 6'**. A primitive that wants
to enter the IaC axiom set must be *compliance-compatible* — it must
slot into at least one existing compliance lattice element (NIST /
CIS / FedRAMP / PCI / SOC2) or declare a new one with mechanical
mapping to ≥ 1 existing one.

Mathscape doesn't have this gate because its "corpus" is pure math —
compliance doesn't apply. But the gate slots in the same way
ml-forge's shape-rule well-formedness slots in: as a domain-specific
extension of gate 6.

**Fleet-level composition as a reinforcement event**. iac-forge has
`Fleet` — a BTreeMap of IacResources with a composite hash. A
Fleet's membership changing is a reinforcement event when two
members merge into one (because one subsumes the other under a
compliance lattice join). This maps cleanly onto mathscape's
`Event::Merge`.

## What this would unlock

Today, growing `IacType` requires a human PR with a new enum variant.
If iac-forge ran the forced-realization machine against a corpus of
cloud-provider resource definitions:

1. Patterns like `Duration`, `Identifier`, `Arn`, `Region` would
   auto-propose
2. The condensation gate would verify they save bytes across the
   corpus
3. The cross-corpus gate would verify they're not provider-specific
4. axiom-forge would emit the variant with all seven obligations
5. Every downstream backend would immediately support the new type
   (via blanket `Backend` impl)

This is the logical endpoint of iac-forge's architecture. The
mathscape machine is the *engine* that makes the automation possible.

## Verdict

Pattern fits without distortion. iac-forge would need:

1. A corpus type for cloud resource definitions
2. A generator proposing IacType variants via anti-unification
3. Compliance-lattice gate as an extra domain gate
4. The bridge to axiom-forge (already exists — axiom-forge doesn't
   care about domain)

All four drop in cleanly. Effort after `primitive-forge` extraction:
~2 weeks.

## Consequence for the mathscape theory

- **No changes required.**
- Gate 6 is now explicitly a parameterization point accepting
  domain-specific obligations. Mathscape: 7 obligations. ml-forge:
  +shape-rule well-formedness. iac-forge: +compliance-lattice
  subsumption. Architectural invariant: gate 6 is the "domain knows
  best" gate.
