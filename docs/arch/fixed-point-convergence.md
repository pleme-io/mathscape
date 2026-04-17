# Fixed-Point Convergence — Mathscape as a Convergence Controller

> "We can trap / reorganize / trap as epochs of learning and
> efficiency which mirror fixed-points in convergence computing."

This doc places mathscape inside pleme-io's convergence-computing
framework. The placement is not metaphorical — mathscape is
literally a fixed-point operator whose convergence target is *the
mathscape of reachable primitives*. Every trap is `f(s) = s`; every
reorganization is `f(s) ≠ s` again until a new trap forms.

## The trap/reorganize/trap cycle

```
                             reorganize
                          (plateau / promotion
                           / demotion / rotation)
   ┌─────────────────────────────────────────┐
   │                                         │
   ▼                                         │
trap ───────────────► reorganize ────────────┘
(f(s) = s)            (state changes)
```

Every mathscape run is a walk along this cycle:

1. **Trap 1.** Initial library forms. Reinforcement advances rules
   through the lifecycle. Fixed point when no gate V/X/A transition
   and no discovery hit.
2. **Reorganize.** Reinforcement plateau triggers `Regime::Explosive`.
   New candidates arrive. Library grows.
3. **Trap 2.** Richer library, still converging. Reinforcement
   catches up.
4. **Reorganize.** Cross-corpus evidence triggers `Regime::Promotive`.
   axiom-forge mints a primitive. Migration contracts the library.
5. **Trap 3.** New primitive in play. Reinforcement begins again in
   an expanded search space.
6. **Reorganize.** Demotion fires after W epochs of inactivity.
7. …

Each trap is a **registry root hash** that stays stable across at
least W epochs. Each trap is a canonical checkpoint, shareable and
replayable.

## Formal correspondence with the convergence stack

From `pleme-io/CLAUDE.md`:

```
Layer           Engine              Checkpoint           Verification
─────           ──────              ──────────           ────────────
Nix evaluation  nix eval            Store path           Content hash
System build    nix build           Closure              Derivation hash
AMI creation    Packer+kindling     AMI snapshot         ami-test
Infrastructure  Pangea→Terraform    S3 state file        RSpec synthesis + InSpec
Bootstrap       kindling-init       server-state.json    ami-integration-test
GitOps          FluxCD              Git commit           Kustomization status
Compliance      kensa/tameshi       OSCAL attestation    Continuous verification
```

Mathscape adds one row:

| Layer         | Engine     | Checkpoint    | Verification                           |
|---------------|------------|---------------|----------------------------------------|
| **Axiom set** | Mathscape  | Registry root | MigrationReport chain + Lean 4 export  |

Each row is a fixed-point operator. Mathscape's output feeds the
next row (the Rust source axiom-forge emits is consumed by `nix
build`, which produces a store path, etc.). The stack is a chain of
fixed-point operators whose outputs compose.

## What makes mathscape a convergence controller

The convergence-controller crate at pleme-io defines the
`ConvergenceController` trait: a process with a declared desired
state, a deterministic transformation to reach it, a checkpoint
mechanism, and drift detection. Mathscape implements all four:

| `ConvergenceController` requirement | Mathscape implementation                                |
|--------------------------------------|---------------------------------------------------------|
| Declared desired state               | `RealizationPolicy` — the gate thresholds              |
| Deterministic transformation          | `Epoch::step(corpus) → EpochTrace`                     |
| Checkpoint                            | Registry root hash after a trap is reached              |
| Drift detection                      | Reinforcement pass running each epoch                   |

Mathscape therefore *should* register as a `ConvergenceProcess` in
the platform's process table — it becomes PID-N in the ProcessTable,
addressable as `mathscape.{hash}.{pid}.k8s.quero.lol`. This is not
optional infrastructure; it's the standard integration every
pleme-io convergence process carries.

## Tameshi attestation — every trap is signed

Every trap's `registry_root` is a BLAKE3 content hash over the
post-trap registry state. That puts traps *directly* in the scope
of `tameshi` — the platform's deterministic integrity attestation
layer.

On reaching a trap, mathscape emits an `AttestationLayer` record:

```rust
pub struct TrapAttestation {
    pub trap_hash: TermRef,                // the registry_root
    pub policy_hash: TermRef,              // RealizationPolicy content hash
    pub corpus_chain: Vec<TermRef>,        // CorpusSnapshots that fed the trap
    pub parent_trap: Option<TermRef>,      // previous trap in this run
    pub tameshi_layer_hash: TermRef,       // BLAKE3 composition of the above
}
```

`tameshi_layer_hash` is the standard 11-layer Merkle composition
mathscape contributes to the platform attestation DAG. This means:

- **Sekiban** (the K8s admission webhook) can gate deployments on
  "this image was built from mathscape trap H" — same machinery it
  already uses for Pangea-generated Terraform.
- **Kensa** (the compliance engine) can register mathscape traps as
  evidence artifacts for NIST / CIS / FedRAMP controls just like
  any other infrastructure artifact.
- **Inshou** (the Nix gate CLI) can pre-verify a trap's hash before
  accepting a downstream nix-build that depends on a primitive
  promoted by that trap.

Every Rust primitive's provenance chain therefore reaches tameshi
attestation at the trap boundary, independently of mathscape's own
verification. The knowability claim becomes an *external* property
once tameshi signatures cover the trap.

Integration is cheap: tameshi already accepts arbitrary BLAKE3-hashed
layer records. Mathscape adds one new `LayerType` variant (via
axiom-forge, naturally — the metacircular loop applies here too).

## Traps as first-class artifacts

Since every trap is a registry root, every trap is content-
addressable. We name them:

```rust
pub struct Trap {
    pub registry_root: TermRef,          // identity
    pub epoch_id_entered: u64,           // when converged
    pub epoch_id_left: Option<u64>,      // when reorganization fired
    pub policy_hash: TermRef,            // which policy was active
    pub corpus_hash_sequence: Vec<TermRef>, // corpora fed into this trap
    pub exit_reason: Option<TrapExitReason>,
}

pub enum TrapExitReason {
    ReinforcementPlateau,
    PromotionFired(TermRef),            // the PromotionSignal that triggered
    DemotionFired(TermRef),             // the DemotionCandidate
    CorpusRotation(CorpusId),
    PolicyChange(TermRef),              // new policy hash
}
```

Every trap enters the registry as an Artifact. The derivation DAG
therefore records *the sequence of traps* as a first-class trajectory,
which is exactly what makes replayability work — the same trap
sequence under the same policy produces the same registry.

## Operator commands that fall out

With traps as artifacts, the CLI grows:

```
mathscape traps list                                # show all traps in the registry
mathscape traps show <hash>                         # inspect one trap
mathscape traps diff <hash-a> <hash-b>              # compare two trap registries
mathscape rollback <hash>                           # reconstruct registry at that trap
mathscape replay --from <hash> --policy <new>.yaml  # re-run from a trap forward
```

These are standard convergence-computing operations. The trap is the
unit of rollback / comparison / replay. Without naming traps
explicitly, these operations would have no grounding.

## Calcification as failed convergence

A system stuck in one trap for an unbounded number of epochs is in
`Regime::Calcified` — reinforcement fully converged, no discovery is
firing, no demotion is firing, policy is static. Calcification is
*valid fixed-point behavior* but is also a diagnostic signal: either

- the corpus is truly exhausted for this policy (fine, rotate)
- the policy is too tight (raise ε_plateau)
- the demotion force is broken (usage tally not declining for rules
  that should be demoted)

The convergence-controller framework makes calcification visible
naturally: a process whose PID never reconciles is observably stuck.

## Mathscape in the process tree

Under the Unix-process model from the platform's CLAUDE.md:

```
seph.1 (init — convergence-controller)
├── kaze.2 (child — prod cluster)
│   └── mathscape.{hash}.5 (mathscape instance running on kaze)
└── drill.3 (DR cluster)
    └── mathscape.{hash}.7 (a shadow instance for reproducibility tests)
```

Each mathscape instance is a PID-tagged convergence process.
Multiple instances can run in parallel against different corpora or
policies, each producing its own trap sequence. All instances share
the typescape (arch-synthesizer's commitment is globally visible) but
differ in local policy.

## Consequences for code

**None for Phase B.** The `Trap` type and ops are Phase K+
concerns (after multi-corpus support lands). But:

- Every `EpochTrace` already records the registry root implicitly
  (via the sequence of Accept/Migrate events); extracting Traps from
  the trace stream later is mechanical.
- `RealizationPolicy::content_hash()` should exist on day 1 (Phase
  B). Its hash is what every Trap references in `policy_hash`. Add a
  `content_hash()` method when we define the struct.

## The big picture

Mathscape is not a standalone system. It is the convergence-
computing layer for the *axiom set*. Its traps compose with every
other convergence layer above and below:

- Corpus changes → new proposals → new traps (from above)
- Traps → new Rust primitives → Nix derivations → AMIs (to below)
- Downstream systems using the new primitive converge too (cascading)

When a mathscape trap produces a Rust primitive that becomes part of
iac-forge, FluxCD reconciliation on K8s fleets running iac-forge-
generated Terraform automatically picks up the new type in the next
sync. The convergence cascades.

This is the correct mental model. Mathscape is not an AI that
discovers math; it is a convergence operator whose output is
*the set of primitives the platform can currently reason about*.
Its "discoveries" are fixed points reached by the machinery, and the
machinery is the same machinery every other convergence process in
the platform uses.

## Action items

1. **Trap type and ops**: Phase K.
2. **`RealizationPolicy::content_hash()`**: Phase B.
3. **Register mathscape-service as a ConvergenceProcess**: Phase H
   (alongside the axiom-forge bridge).
4. **Document trap-based replay in `realization-plan.md`**: done
   implicitly; add an explicit note in Phase I acceptance criteria.

## The theorem we are claiming

**Mathscape is a fixed-point operator whose fixed-points form a
monotone-lattice sequence of axiom sets, each of which extends the
previous while preserving coverage under the current policy.**

This is testable (gate 5's coverage check). It is the formal
statement behind the trap/reorganize/trap cycle. Once proven in the
code (Phase I), mathscape is recognized by the platform as a
first-class convergence operator.
