# Typescape Binding — How Mathscape Primitives Join the Platform

Every Rust primitive promoted by mathscape must land in the
**typescape** — arch-synthesizer's universe of all platform types —
as an attested leaf. Otherwise promotions are local to mathscape's
tree and invisible to the rest of the platform. This doc specifies
the binding.

## What the typescape is

From `pleme-io/CLAUDE.md`:

> The typescape is the root data structure of the platform — the
> universe of all types, queryable, attestable, and composable. Every
> repo is a leaf in the typescape Merkle tree; each carries a
> `TypescapeManifest` recording which dimensions contributed to its
> artifacts, content hashes, and the Merkle path back to the root.

Eight dimensions: vocabulary, domains, lattice, stack, DAG, render,
compliance, modules. A mathscape-promoted primitive is a new
vocabulary entry + a new node in the render + compliance dimensions.

## The mapping

When axiom-forge accepts a promotion (gate 6 clears) and rustc
compiles the emitted source (gate 7 clears), mathscape emits a
`MigrationReport`. That report must also produce a
`TypescapeContribution`:

```rust
pub struct TypescapeContribution {
    pub vocabulary_entry: VocabularyEntry,    // dimension 1
    pub domain_enrichment: DomainEnrichment,  // dimension 2 (which AST domain grew)
    pub render_leaf: RenderLeaf,              // dimension 6 (what ships to where)
    pub compliance_claims: Vec<Control>,      // dimension 7 (if any controls attach)
    pub manifest_delta: ManifestDelta,        // dimension 8 (per-repo manifest)
    pub merkle_path: Vec<TermRef>,            // back to the typescape root
}
```

The contribution is itself content-addressed and enters the registry
alongside the `MigrationReport`. A promotion therefore produces two
linked artifacts: one on the mathscape side (migration), one on the
typescape side (contribution). Both have hashes; both are replayable.

## Why `AxiomIdentity` needs a typescape coordinate

The `AxiomIdentity` defined in `mathscape-core::lifecycle`:

```rust
pub struct AxiomIdentity {
    pub target: String,          // e.g. "mathscape_core::term::Term"
    pub name: String,            // PascalCase variant name
    pub proposal_hash: TermRef,  // axiom-forge's Certificate::proposal_hash
}
```

…is incomplete for typescape lookup. A promoted primitive needs to
be *findable* in the typescape by queries like "show me every
primitive in the mathscape module" or "find primitives that share a
compliance claim". Add a coordinate:

```rust
pub struct AxiomIdentity {
    pub target: String,
    pub name: String,
    pub proposal_hash: TermRef,
    /// The typescape coordinate this primitive occupies.
    pub typescape_coord: TypescapeCoord,
}

pub struct TypescapeCoord {
    /// e.g. "mathscape/core/term"
    pub module_path: String,
    /// Which AST domain this primitive belongs to (one of the 19
    /// catalogued in arch-synthesizer::ast_domains).
    pub ast_domain: String,
    /// Merkle hash of the typescape leaf post-insertion.
    pub leaf_hash: TermRef,
}
```

The bridge (`mathscape-axiom-bridge`) is responsible for:

1. Asking arch-synthesizer to compute the `TypescapeCoord` before
   invoking axiom-forge (read-only query)
2. After rustc accepts, asking arch-synthesizer to *commit* the new
   leaf (write operation with attestation)
3. Embedding the returned `leaf_hash` in the `AxiomIdentity`

## Attestation chain

A successful promotion produces this full chain:

```
corpus hashes                  (fed into mathscape)
    │
    ▼
Artifact.content_hash          (mathscape accepts)
    │
    ▼
PromotionSignal.content_hash   (gates 4–5 clear)
    │
    ▼
AxiomProposal.content_hash     (bridge serializes)
    │
    ▼
Certificate.proposal_hash      (axiom-forge: gates 6)
    │
    ▼
EmissionOutput.content_hash    (axiom-forge: emission)
    │
    ▼
rustc-accepted                 (gate 7)
    │
    ▼
MigrationReport.content_hash   (mathscape: library contraction)
    │
    ▼
TypescapeContribution.hash     (arch-synthesizer: platform leaf)
    │
    ▼
new typescape Merkle root      (attested via tameshi)
```

Every step is a content hash. Given any one, the full chain is
reconstructible. This is the complete answer to "where did this
primitive come from?"

## What arch-synthesizer owes

arch-synthesizer must expose:

```rust
pub trait TypescapeBridge {
    /// Compute the coordinate a primitive would occupy given its
    /// target path. Read-only; does not commit.
    fn propose_coord(&self, target: &str, name: &str) -> TypescapeCoord;

    /// Insert a new leaf with attestation. Returns the post-insertion
    /// leaf hash and the new typescape root.
    fn commit_leaf(
        &mut self,
        coord: TypescapeCoord,
        contribution: TypescapeContribution,
    ) -> (TermRef, TypescapeRoot);

    /// Mark a leaf deprecated (mirror of axiom-forge's #[deprecated]).
    fn deprecate_leaf(&mut self, leaf_hash: TermRef, reason: DemotionReason);
}
```

v0 implementation lives in `arch-synthesizer::typescape::bridge`.
The trait decouples mathscape from arch-synthesizer's storage
implementation.

## Why this isn't scope creep

Without the typescape binding:

- **Knowability is local to mathscape**. Nobody else on the platform
  can query what primitives mathscape has grown.
- **Compliance claims evaporate**. A primitive with invariants
  (e.g., "preserves coverage") would only be known to mathscape's
  own verifier. arch-synthesizer's compliance lattice can't index it.
- **Cross-repo attestation breaks**. tameshi's BLAKE3 Merkle
  composition assumes every leaf is indexed somewhere. Mathscape
  primitives would be orphan leaves.

The binding is the *minimum* integration that keeps the platform
knowability claim intact. Everything downstream (compliance, render
targets, module manifests) flows from it.

## Phased rollout

- **Phase B**: extend `AxiomIdentity` with `TypescapeCoord` (three
  `String` + `TermRef` fields). Stub the coord with defaults for now.
- **Phase H** (bridge): implement `TypescapeBridge::propose_coord` and
  `commit_leaf` in arch-synthesizer; call from `mathscape-axiom-bridge`.
- **Phase I** (migration): include `TypescapeContribution` in the
  `MigrationReport` emission path.

## The minimal test

After Phase I runs end-to-end, there must exist a commit history
where:

1. A mathscape run produces a `MigrationReport`
2. arch-synthesizer's typescape root changed between the pre- and
   post-migration snapshot
3. The new leaf hash in the typescape matches
   `AxiomIdentity::typescape_coord::leaf_hash`
4. A tameshi verification over the new typescape root passes

These four conditions together prove the primitive joined the
platform proper.
