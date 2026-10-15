# In-Memory Convergence Layering — The Recursive Pattern

> "Understand that pattern within in-memory convergence computing,
> because it is a generic pattern of expressing solidifying
> understanding and then using as a layered platform for exploration
> of convergence computing itself."

This document names the recursion. `fixed-point-convergence.md`
positioned mathscape as one convergence operator alongside Nix,
FluxCD, Pangea, kenshi. This document states the stronger claim:
**mathscape's output changes the substrate the rest of the
convergence stack operates on.** It is not just another process in
the stack — it is the process that *grows the stack*.

## The convergence stack under ordinary operation

From `pleme-io/CLAUDE.md`:

| Layer          | Converges on          | Checkpoint       |
|----------------|-----------------------|------------------|
| Nix evaluation  | expression → store path| content hash     |
| System build    | store path → closure  | derivation hash  |
| AMI creation    | packer → snapshot     | AMI id           |
| Infrastructure  | declared → running    | S3 state file    |
| GitOps          | git commit → live K8s | Kustomization    |
| Compliance      | policy → attestation  | OSCAL chain      |

All six layers **operate on a fixed substrate** — the set of types,
primitives, and operators available to them is declared out of band
and doesn't change during their operation. FluxCD cannot invent new
K8s resource kinds. Nix cannot invent new builtin functions during a
`nix eval`. Each layer converges *within* its given type system.

## What mathscape does differently

Mathscape **converges on the type system itself.** The substrate
(the set of primitives mathscape can reason about) is precisely the
thing mathscape extends. Each trap reached is a new primitive in
the `Term` enum that subsequent epochs can use.

Every other convergence layer uses a *static* type system; mathscape
uses a *dynamic* one — but dynamic in a constrained, gate-controlled
way that preserves the knowability claim.

This is the *recursion*: mathscape is convergence computing applied
to convergence computing's substrate. Every trap produces a new
primitive; every new primitive changes what mathscape can converge
on next; the process compounds until the primitive set is expressive
enough that no new compression is available under the current policy.

## The layered platform

The pattern generalizes. Any convergence layer whose output is a
new primitive for the layer itself is a *substrate-extending
convergence layer*. Three we have:

1. **Mathscape** — axiom set for mathematical expressions. Grows
   `Term` enum.
2. **axiom-forge** (via mathscape) — extends any closed Rust enum
   it can emit into. Today: `Term`; future: `IacType`, `UOp`,
   `Control`.
3. **arch-synthesizer** (prospectively) — extends the typescape
   itself with new AST-domain primitives. Already catalogues 19
   domains; each new domain is a substrate extension.

Each layer above consumes the output of all lower ones. When
mathscape mints a new `Term` variant, arch-synthesizer registers it
as a typescape leaf, axiom-forge's future proposals can reference
it, and downstream renderers (ruby-synthesizer, etc.) pick it up
via their Morphism impls.

Crucially, **the new primitive is available within the same running
process** — not a multi-day rebuild-redeploy cycle. `Term` enum
extension in production does require a rebuild, but the proposal +
verification + emission happens in seconds in memory.

## Why in-memory matters

The substrate-forge vision (`iac-forge/docs/SUBSTRATE_VISION.md`)
completes the circle: WASM-WASI-compiled verified programs
materialized in memory, run, deallocated. When mathscape mints a new
primitive, the emitted Rust source can be:

1. Compiled to WASM via a small rustc-to-wasm pipeline
2. Loaded into substrate-forge's wasmtime runtime
3. Executed against the current corpus to validate behavior
4. Retained (its content hash joins the registry) or deallocated

All of this happens without spawning processes, without disk
writes, without committing to git. The convergence cycle runs in
memory, at thought-speed.

This is what makes **"in-memory convergence computing"** distinct
from its batch ancestors. Nix builds to disk; FluxCD deploys to
clusters; mathscape + substrate-forge can converge an idea into a
callable function and back again in seconds.

## The generic pattern

Abstractly, a **substrate-extending convergence process** is:

```
P : Substrate × Corpus × Policy → Substrate' × Artifacts
```

where `Substrate' ⊇ Substrate` — the new substrate is a monotonic
extension of the old (modulo demotion, which removes only those
primitives whose evidence collapses).

Compare with ordinary convergence:

```
Q : Substrate × State × Declaration → State'
```

`Q` lives within `Substrate`; `P` modifies it. `Q` is the common
case (FluxCD, Nix, Pangea); `P` is rarer (mathscape today, arch-
synthesizer's typescape future).

The hard part is not the Q→P generalization — it's the gates.
Without gates, a substrate-extending process produces noise and
the trust model breaks: every layer above would have to accept
whatever substrate it's given, which means the whole stack becomes
untrustworthy. With the ten-gate lattice, every substrate change
is proven-by-construction and all existing layers inherit the
guarantee.

## What this means for implementation

Three concrete implications:

1. **Typescape binding is not optional** — it is the integration
   point at which the substrate extension becomes visible to the
   rest of the stack. Without it, mathscape-minted primitives are
   invisible beyond mathscape itself.

2. **substrate-forge is the natural runtime** — when mathscape mints
   a `Term` variant that expands the type system, the immediate
   next use is executing the variant's semantic rules. A
   recompile-everything rebuild is too slow for the epoch cadence.
   In-memory WASM execution closes the loop in seconds.

3. **arch-synthesizer is the canonical renderer** — because it
   already maps typescape → Ruby / HCL / Python / Rust deterministically
   via Morphism impls. axiom-forge is the gate; arch-synthesizer
   is the emitter. Today mathscape-axiom-bridge does its own
   string-template Rust emission; in the future it should delegate
   to arch-synthesizer's Morphism for the `Term` target.

## Positioning relative to the existing platform

| Pattern                       | Examples                              | Substrate mutability |
|-------------------------------|---------------------------------------|----------------------|
| **Static convergence**        | FluxCD, Nix, Pangea→Terraform, kenshi | fixed                |
| **Substrate-extending**       | mathscape, arch-synthesizer typescape| monotone + demotion  |
| **In-memory extending**       | mathscape + substrate-forge           | in-process WASM      |

The platform already has static convergence at scale. It has the
beginning of substrate-extending convergence with mathscape. It
has the ingredients for in-memory extending convergence
(substrate-forge is in the tree, 40 tests). Combining them is the
next realization-plan chapter, beyond the current A–L.

## The recursive claim stated cleanly

Convergence computing can converge on its own substrate, producing
new primitives that become the next convergence cycle's input. The
ten-gate lattice makes this safe; in-memory execution makes it
fast; axiom-forge's obligations make each extension trustworthy;
arch-synthesizer's typescape makes each extension visible across
the platform.

This is what "layered platform for exploration of convergence
computing itself" means: not a tool that runs on a platform, but a
mechanism by which the platform grows its own capabilities while
preserving the trust guarantees that justify it as infrastructure.

## Action items

Not v1 blockers — these are the *next* chapter:

1. **arch-synthesizer Morphism to emit Rust for `Term`** — replaces
   mathscape-axiom-bridge's string template with a proper Morphism
   impl. Would flow via iac-forge::sexpr.
2. **substrate-forge hookup** — a small function
   `substrate_forge::materialize(emission: EmissionOutput) -> Instance`
   that compiles the Rust to WASM and loads it in-process.
3. **Typescape bridge** — already planned as knowability criterion 5;
   its role is larger than originally scoped: it is the hub of the
   substrate-extension mechanism.

These three together make the recursive claim real. Ahead of them,
the ten-gate lattice already in code provides the trust guarantee
that makes the recursion safe.
