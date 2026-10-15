# Mathscape as Inception

The whole machine seen as a ladder: each level produces the next,
until a fixed point where a level produces itself. Past the fixed
point, the game changes — we deploy the self-producing model into
environments and let it discover what mathscape IS across them.

This doc is the compass. When asking "where are we in the stack"
or "what's the next rung", start here.

## Inception layers

Each layer is produced by the layer below. Higher = more abstract.
Convergence = the top layer produces itself without outside input.

```
L9  invariant discovery   "what IS mathscape"   — post-fixed-point
                          ────────────────────────
L8  environment           corpus × mechanism × policy × seed
                          ────────────────────────
L7  policy-generating     meta-policy emits policies
    policy                ────────────────────────
L6  trajectory + policy   model that learns how to discover
                          ────────────────────────
L5  mutation synthesis    ML5 compound mutations
                          ────────────────────────
L4  mechanism             ML4 MechanismConfig mutable parameters
                          ────────────────────────
L3  meta-rules            S_10000 rank-1 abstraction (op-variable)
                          ────────────────────────
L2  library               registry of RewriteRules
                          ────────────────────────
L1  rules                 discovered equalities over terms
                          ────────────────────────
L0  substrate             Term, eval, builtin registry, canonical
                          ────────────────────────
```

### L0 — Substrate

`crates/mathscape-core/src/term.rs`, `eval.rs`, `builtin.rs`,
`value.rs`, `parse.rs`. The irreducible kernel. **Status: stable
after R3–R8 (2026-04-18).** Kernel invariants: genuine, true,
repeatable, no-equal-terms, extensible.

### L1 — Rules

`RewriteRule { name, lhs, rhs }`. A rule says "this LHS reduces
to this RHS." The machine discovers rules by anti-unification
over the corpus. **Status: load-bearing since Phase C.**

### L2 — Library

`InMemoryRegistry` + `Registry` trait. The set of discovered
rules, with status lifecycle (Conjectured → Verified →
Axiomatized → Subsumed → Demoted). **Status: stable since Phase D.**

### L3 — Meta-rules

`MetaPatternGenerator` anti-unifies LHSs of existing rules to
mint meta-rules like `S_10000 :: (?op (?op ?x ?id) ?id) = ...`
— rules where operators themselves are pattern variables. The
gateway to rank-2 abstraction. **Status: S_10000 is apex at
every scale from 12 to 100,007 corpora. R8 tensor detector
recognizes the shape.**

### L4 — Mechanism

`MechanismConfig` — every knob the discovery pipeline uses
(`candidate_max_size`, `corpus_base_depth`, `extract_min_matches`,
etc.) promoted from Rust literals to mutable fields. Subject to
the same evolutionary pressure as rules. **Status: M1 Sexp form
landed; ML4 mutation operators landed.**

### L5 — Mutation synthesis

`MechanismMutation::Compound` lets the machine compose its own
mutation operators and promote them to `discovered_operators`.
The set of ways the mechanism can change grows via discovery.
**Status: ML5 landed; compound mutations get reused.**

### L6 — Trajectory + policy

**Just landed (R9+R10, 2026-04-18).**

`TrajectoryStep (epoch, LibraryFeatures, action, accepted, ΔDL)`
captures every discovery decision. `LibraryFeatures` is the
9-dim state vector with `tensor_density` at index 4.
`LinearPolicy` scores states; `train_from_trajectory` updates
weights; `policy_to_sexp` lifts the model into Lisp.

The self-producing loop at this layer: train in Rust → emit Sexp
→ load into next run → train further. Gen N+1 inherits Gen N's
learning through the Lisp boundary. Proven by
`self_producing_loop_via_lisp` test.

### L7 — Policy-generating policy (NOT YET BUILT)

The meta-move. Instead of one `LinearPolicy` that trains itself,
we have a **policy-generator** that emits new policies. The
generator itself is trained by trajectories that vary across
policy variants.

- Generator output: `(policy ...)` Sexp forms
- Generator's own form: `(policy-generator :architecture ...
  :hyperparams ...)` Sexp
- Training signal: which emitted policies led to fastest tensor
  discovery in downstream runs

This is where "the system that produces the model was produced"
becomes concrete. L7's output is an L6 component; L7 is itself
an L6-shaped object recursively.

### L8 — Environment (NOT YET BUILT; scaffold in `environment.rs`)

An environment is `(corpus_generator, mechanism_config, initial_policy, seed)`.
A single bundle a model can be deployed to.

Deployment = give the bundled model a run budget; it traverses
its own environment, collects a trajectory, updates its policy,
emits a trained model. The environment is the model's world.

Multiple environments = multiple worlds. A model trained in
world A and tested in world B exposes whether its learning is
environment-specific or universal.

### L9 — Invariant discovery (POST-FIXED-POINT)

"What IS mathscape" — the rules, meta-rules, tensor shapes that
emerge across ALL environments. The environment-invariant
discoveries are the mathscape's objective structure; the
environment-specific discoveries are artifacts of the corpus.

Operationally: run `N` environments, record what each discovers,
intersect. The intersection is mathscape; the symmetric
difference is environment noise.

## The fixed-point criterion

When has the machine converged on "a model that produces itself"?

**Operational definition.** A policy `P₀` trained on trajectory
`T₀` produces policy `P₁ = train(P₀, T₀)`. Training `P₁` on a
trajectory `T₁` generated under `P₁` produces `P₂ = train(P₁,
T₁)`. The fixed point is reached when the generation relation
`Pₙ → Pₙ₊₁` has a stable limit:

```
lim_{n→∞} ‖Pₙ₊₁ - Pₙ‖ = 0
```

Concretely: some generation `N` exists where further training
cycles don't change the weights beyond numerical noise. The
policy is self-reproducing.

**Weaker sufficient condition.** The weight updates form a
bounded sequence with diminishing step size — monotone progress
toward a Lipschitz-attractor. This is achievable with learning
rate schedules alone; the stricter "self-producing" condition
additionally requires the trajectory distribution to stabilize.

**Detection.** Add a `fixed_point_distance(P₁, P₂) = ‖weights‖₂`
metric. Log it per generation. When it drops below ε across K
consecutive generations, mark convergence.

## The post-fixed-point game

Once the self-producing model exists, the game shifts. We stop
training the discovery machinery and start using it. Three
simultaneous tasks:

### Deploy

Instantiate many environments. Each is a `(corpus, mechanism,
policy, seed)` bundle. The corpus generator varies:

- Peano-heavy (default zoo)
- Int-heavy (R7's Int domain — untouched so far)
- Mixed-domain (Nat + Int)
- High-arity (operators with 3+ args, via mechanism mutation)
- Sparse (few corpora, forces compositional discovery)
- Dense (many corpora, shows saturation behavior)

Each environment runs autonomously. No human in the loop.

### Tune

Per-environment, the model's policy continues to update. The
policy is frozen at the TYPE level (it's `LinearPolicy` or
whatever the converged architecture is) but its weights keep
tracking environment-specific structure. A deployed model
gradually specializes to its environment.

Meta-tuning: hyperparameters of the policy (learning rate,
feature weighting, tensor-target strength) are themselves
mutable, subject to the L7 policy-generating policy's output.

### Discover "what IS mathscape"

Intersect the discoveries across environments. What's common to
all is universal — this is **the mathscape**. What's specific to
some is corpus structure.

Operationally:

1. Run `N` environments to saturation.
2. Collect the Axiomatized rule set from each: `A₁, A₂, ..., Aₙ`.
3. Compute `A_universal = ∩ Aᵢ` — rules that axiomatized in
   every environment.
4. Compute `A_local = Aᵢ ∖ A_universal` per environment.
5. `A_universal` is the mathscape's invariant content.

The machine's final output: a small set of rules that's
invariant across all investigated environments. These ARE the
mathematical truths the substrate supports, discovered without
human input.

## Where we are on the ladder

Committed:

- L0 through L6: present and tested.
- L7 policy-generating policy: design sketch only; no code.
- L8 environment: scaffold landing with this doc.
- L9 invariant discovery: post-convergence; no code yet.

To reach fixed point:

1. Wire L6's scorer into the live generator — currently the
   scorer exists but doesn't influence candidate prioritization
   during traversal. Without this wiring, trajectory-trained
   policies can't feed back into discovery.
2. Build L7 — a policy-generating policy that varies architectures
   and hyperparameters. Needed because a single LinearPolicy
   won't find the fixed point; the generator searches over
   policy structures.
3. Measure convergence — add `fixed_point_distance` across
   generations and log it. Define ε.

To play the post-convergence game:

4. Build L8 environment type (scaffold landing now).
5. Run multi-environment sweeps — each environment autonomous,
   each emitting a trained model + discoveries.
6. Build L9 intersection tooling — compute `A_universal` across
   environments.

## The recursion's end — L9 is the ceiling

**We do not incept further past L9.** Once we have:

- A model that produces itself (L7 fixed point), AND
- An environment produced and managed by that model (L8 deployment
  from the fixed-point policy), AND
- Invariant discovery across environments (L9 intersection),

there is no meta-meta- level to reach. Adding an L10 "invariant-
invariant" discovery layer would be inception-for-its-own-sake
without a question it answers. The stack closes.

After closure, **the whole ladder becomes a set of primitives for
repeated experimenting.** The inception stops being a build agenda
and starts being infrastructure. Each experiment is:

1. Instantiate an L8 environment with specific `CorpusShape`,
   `MechanismSnapshot`, seed, and the converged L7 policy.
2. Run to saturation. Collect L6 trajectory + L2 Axiomatized set.
3. Intersect across N environments to extract L9's `A_universal`.
4. Vary inputs, rerun. The apparatus is fixed; the experiments
   change.

## Primitives after closure

What the completed stack gives the experimenter:

| Primitive | Source | What it exposes |
|---|---|---|
| `Environment` | `environment.rs` (R11) | a deployment bundle |
| `LinearPolicy` | `policy.rs` (R10) | the trained scorer |
| `Trajectory` | `trajectory.rs` (R9) | run history for analysis |
| `ConvergenceTracker` | `environment.rs` (R11) | fixed-point detection |
| `TensorShape::classify` | `tensor.rs` (R8) | the "tensor emerged" test |
| `MechanismSnapshot` | `environment.rs` (R11) | mechanism knob bundle |
| `policy_to_sexp` | `policy_lisp.rs` (R10.1) | Lisp-resident model |
| `canonical_deployment_suite` | `environment.rs` (R11) | standard 6-env bench |

Any higher-level tool (exploration harness, adversarial
environment generator, cross-env intersection analyzer) is built
FROM these primitives, not alongside them as a new inception
layer.

## Adversarial refutation is an experiment, not a layer

Once the stack closes, "can we refute the discovered mathscape?"
is just another experiment — an adversarial environment in the
L8 sense. It doesn't add a new layer to the inception; it
exercises the existing primitives with inputs designed to
falsify `A_universal`.

Rules in `A_universal` that survive adversarial refutation are
candidate mathematical truths — things no environment the
experimenter can construct falsifies. **That's the mathscape.**

## What "full malleability + observation" looks like

(Per the 2026-04-18 framing: tune the model over time, observe it
as it runs.)

Malleability and observation are **features of the primitives,
not a new layer.** Concretely:

- Every primitive is serializable (serde-derived), so state can
  be snapshotted, inspected, modified, and loaded back at any
  moment.
- `LinearPolicy::train_from_trajectory` is already online — it
  updates weights from a trajectory without freezing the model.
  Training during a run is already supported; the caller just
  hasn't wired it into the loop yet.
- Every component has a `_to_sexp` form (policy today; mechanism
  via ML4) so changes can be made via Lisp rewriting — editing
  configuration without rebuilding the engine.
- Per-epoch events (`Event` enum in `event.rs`) already stream
  the engine's decisions. An external observer can subscribe
  without changing the engine.

The malleability is already structural. What remains is
**exposing it ergonomically** — a single-method live tuning
and observation API — which is a convenience wrapper, not a
fundamental capability. That work belongs with R12 ("experiment
harness"), one of the primitive-consuming tools, not with
further inception.
