# Perpetual Improvement

The fixed point of this system is a **perpetually improving model, given
any task.** This document synthesizes the shape that pulls the live
proprioceptive loop, the event-driven infrastructure, the task-agnostic
ingress, and the universal neuroplasticity under one coherent frame.

Status: **landed through Phase W.6, 2026-04-19.** The substrate is in
place; the live async runner (W.7), universal plasticity trait (W.8),
and Lisp-morphable runtime (W.9) are queued.

## The invariant the system targets

Every component of the adaptive pipeline follows the same neuroplastic
pattern:

> **Every thing phases out what isn't working and reinforces what is.**

This is not a metaphor — it is the literal mechanism. Weights shed
when their Fisher information times magnitude stays near zero;
library rules get demoted when cross-corpus support erodes; bandit
arms lose reward EMA and get avoided; corpus archetypes that stop
producing growth get rotated out. Everything has the same shape.

And every thing also grows: pruned weights get rejuvenated when the
phantom gradient says reward signal wants them back; new rules emerge
from anti-unification; new arms can be added to probes; new corpus
archetypes fire on saturation signals.

Shed and reinforce, continuously, over the live event stream.

## The five composed loops

The machine is not one loop. It is five loops running at different
timescales, all sharing the same event bus.

```
           ┌─────────────────────────────────────────────────┐
           │           Perpetual Improvement Machine          │
           └─────────────────────────────────────────────────┘

  ┌──────────────────── Ingress ────────────────────────────┐
  │ Task<D> streams — benchmarks, external data, motor      │
  │ outcomes, operator-authored challenges                  │
  └──────────────────────────────┬──────────────────────────┘
                                 │
                                 ▼
                      ╔══════════════════╗
                      ║    EventHub      ║   ← W.4
                      ╚══════════════════╝
                                 │
            ┌────────────────────┼────────────────────┐
            ▼                    ▼                    ▼
   ┌────────────────┐   ┌────────────────┐   ┌──────────────────┐
   │StreamingPolicy │   │ CertifyingCon- │   │ BanditProbe(s)   │
   │   Trainer      │   │    sumer       │   │                  │
   │ W.1 RigL       │   │ V.certify      │   │ W.5 online       │
   │ W.2 EWC        │   │ lattice        │   │ experimentation  │
   │ W.3 LP reward  │   │                │   │                  │
   │ W.stall prune  │   │ demotes eroded │   │ sheds bad arms   │
   └────────────────┘   │ rules          │   │ amplifies good   │
            │           └────────────────┘   └──────────────────┘
            │                    │                    │
            └────────────────────┼────────────────────┘
                                 ▼
                      ╔══════════════════╗
                      ║  Derived events  ║   (BenchmarkScored,
                      ║  republished to  ║   RuleCertified, etc.)
                      ║      hub         ║
                      ╚══════════════════╝
                                 │
                                 ▼
                         next iteration
```

### Loop 1 — Motor (scenario timescale)

`MetaLoop::run` orchestrates scenario → outcome → observation → next
scenario. `AdaptiveProposer` learns per-archetype performance; on
staleness, `AdaptiveDiet` mutates the corpus. `publish_outcome_events`
translates outcomes into MapEvents published to the hub.

Timescale: seconds to minutes per phase.

### Loop 2 — Streaming trainer (event timescale)

Every MapEvent → reward → feature vector → online SGD step. Shed
weights automatically via phantom-gradient auto-rejuvenation (W.1);
anchor on benchmark improvement, resist regression via Fisher pullback
(W.2); reward the agent's own learning progress (W.3); shed stalled
and corrupted weights on the neuroplasticity mechanism (V.shed +
W.stall).

Timescale: microseconds per event.

### Loop 3 — Certifier (rule-lifecycle timescale)

CertifyingConsumer watches the event stream and advances rules through
`Candidate → Validated → ProvisionalCore → Certified → Canonical` as
cross-corpus evidence accrues. Rules that fail to re-accrue support
get demoted.

Timescale: minutes (rules mature across many corpus exposures).

### Loop 4 — Bandit probes (hyperparameter timescale)

BanditProbe cycles through hyperparameter arms (learning rate,
ewc_lambda, prune thresholds, switch intervals), attributes benchmark
delta to the active arm, picks the next via ε-greedy over reward EMA.
Multiple probes compose without interference — each owns its knob.

Timescale: dozens to hundreds of events per arm.

### Loop 5 — Benchmark (report-card timescale)

BenchmarkConsumer periodically evaluates the current library/model
against labeled task sets (canonical + harder for math; future
domains plug in via `TaskDomain`). Every scoring emits
`BenchmarkScored` to the hub; every downstream loop reacts — trainer
rewards improvement, anchors on gains, auto-rejuvenates dormant
weights the benchmark signal is trying to move.

Timescale: seconds (one scoring pass per benchmark trigger).

## Task-agnostic ingress

The user-framed observation: *"right now we are using math as the only
training data."* The `TaskDomain` trait (W.6) closes this gap:

```rust
pub trait TaskDomain: 'static {
    type Input: Clone;
    type Output: Clone + PartialEq;
    type Context: ?Sized;
    fn name() -> &'static str;
    fn solve(ctx: &Self::Context, input: &Self::Input, step_limit: usize)
        -> Option<Self::Output>;
    fn matches(expected: &Self::Output, actual: &Self::Output) -> bool;
}
```

`MathDomain` is the first implementation. Adding a new domain (code
synthesis, NLP completion, tensor regression, image classification) is
one struct + one trait impl. No changes to the hub, the trainer, the
probes, or the certifier — they're already domain-agnostic.

## Universal neuroplasticity

The directive: *"everything has an algorithm to phase out and choose what
to phase out and choose what to reinforce."*

Today's state:

| Component | Phase out | Reinforce |
|---|---|---|
| StreamingPolicyTrainer weights | `prune`, `prune_dormant_or_corrupted` | `rejuvenate`, `auto_rejuvenate` (W.1) |
| Library rules (certification) | demote on support erosion (V.certify) | promote on evidence (V.certify) |
| Corpus archetypes | staleness-triggered rotation (V.1–V.5) | reuse via AdaptiveProposer stats |
| Bandit probe arms | low-reward arms lose selection probability (W.5) | high-reward arms get picked more (ε-greedy) |
| Trainer anchor | overwritten on improvement (W.2) | anchor stabilizes recent winners (W.2) |
| Benchmark history | rolling window (W.3) | recent high-water marks drive intrinsic reward (W.3) |

The future shape: a `Plastic` trait that every component implements,
so an outer controller (Loop 0?) can drive periodic phase-out and
reinforce passes uniformly:

```rust
pub trait Plastic {
    fn component_name(&self) -> &'static str;
    fn active_count(&self) -> usize;
    fn phased_out_count(&self) -> usize;
    fn phase_out_stale(&self) -> usize;    // returns count shed
    fn reinforce_strong(&self) -> usize;   // returns count strengthened
}
```

Each adaptive subsystem implements the trait in its own terms; the
outer loop calls them on a cadence.

## The async / live shape (Phase W.7, queued)

The hub is synchronous today. For true live operation — data streams
arriving asynchronously, probes running concurrently, multiple
trainers on different domains running in parallel — the roadmap is:

1. Swap `RefCell<Vec<Rc<...>>>` → `RwLock<Vec<Arc<...>>>` in
   `EventHub`. Public API unchanged.
2. Make `MapEventConsumer: Send + Sync`. Existing consumers all
   satisfy this with `RwLock`/`atomic` wrappers.
3. Run each consumer as a tokio task; the hub's `publish` becomes a
   broadcast channel `send`.
4. Add `TaskStream<D: TaskDomain>` producers that pull from external
   sources (file watchers, HTTP endpoints, stdin) on their own tokio
   tasks and publish into the hub.

This is a refactor, not a redesign. The pieces exist; they just have to
be moved from single-threaded determinism to multi-threaded
determinism + atomicity.

## The Lisp morphing axis (Phase W.9, queued)

The user: *"how the lisp system could use this rust infrastructure in
memory, which will be super reliable for the lisp things morphing at
runtime."*

The Rust infrastructure is already Lisp-shaped:
- `MapEventConsumer` is object-safe + single-method + `&self`. A Lisp
  closure wraps as a consumer via thin Rust adapter.
- All mutations are typed, narrow, and atomic: `inject`,
  `adjust_learning_rate`, `set_ewc_lambda`, `prune`, `rejuvenate`,
  `set_epsilon`, `set_switch_interval`. Each can have a Lisp binding
  added without changing the Rust surface.
- Snapshots (`snapshot`, `fisher_snapshot`, `phantom_gradients`,
  `weight_stats`, `benchmark_history`, `arm_reward_ema`, `arm_trials`)
  are pure typed data, directly serializable to Sexp via existing
  bridges.

Adding Lisp morphing is additive wiring, not a refactor. Future
tatara-lisp-adapter crate exposes:
- `(subscribe-consumer hub lisp-callback)` — attaches a Lisp closure
  as a MapEventConsumer.
- `(publish-event hub event)` — injects an event from Lisp.
- `(snapshot-trainer trainer)` → Sexp of current policy + stats.
- `(inject-trainer trainer sexp-policy)` — writes a Lisp-authored
  policy back to the trainer.
- `(prune-trainer trainer mag act)` → Sexp list of pruned indices.

With this, a Lisp script running inside the same process can:
1. Subscribe probes that enact Lisp-defined policies.
2. Mutate the trainer mid-stream.
3. Add new consumers at runtime.
4. Swap out the motor's proposer with a Lisp-authored alternative.

The Rust side's invariants (determinism, attestation, typed
snapshots) protect Lisp morphings from breaking the loop's
correctness.

## The fixed point

With all five loops running on the same event bus, the system's
behavior tends toward:

- **Libraries grow as long as new rules hold cross-corpus**;
  subsumption + certification prune overgrowth.
- **Policy weights self-size** to the complexity of the active reward
  signal — phantom gradients pull back dormant dimensions only when
  signal demands them, so allocated width tracks actual learnable
  signal.
- **Hyperparameters converge** to the settings that produce best
  benchmark improvement (bandit probes identify winners).
- **Corpus expands along the machine's own saturation frontier** —
  the AdaptiveDiet keeps introducing structures the current library
  cannot reduce until the library grows to cover them.
- **Benchmark score monotonically improves in the long run** —
  regressions get unwound by the asymmetric reward + EWC anchor;
  only improvements survive into the anchor state.

All without human intervention in the loop, on labeled training
data of any task type, with cryptographic attestation of every
transition.

This is the fixed point. The machine does not halt; it asymptotes
toward "best model for the task(s) you've shown it so far, ready to
integrate the next task you give it."

## Roadmap

| Phase | Name | Status |
|---|---|---|
| V.map | MathscapeMap typed view | ✓ landed |
| V.events | MapEvent + consumer trait | ✓ landed |
| V.certify | CertificationLevel lattice + reactive consumer | ✓ landed |
| V.stream | StreamingPolicyTrainer | ✓ landed |
| V.benchmark | Labeled-data ingress (math) | ✓ landed |
| V.shed | Prune + rejuvenate | ✓ landed |
| W.1 | RigL phantom gradients + auto-rejuvenate | ✓ landed |
| W.2 | EWC Fisher-weighted stability | ✓ landed |
| W.3 | Learning-progress intrinsic reward | ✓ landed |
| W.stall | Corrupted/stalled pruning | ✓ landed |
| W.4 | EventHub pub/sub + motor translator | ✓ landed |
| W.5 | Online experimentation (bandit probes) | ✓ landed |
| W.6 | TaskDomain abstraction (beyond math) | ✓ landed |
| W.7 | Async hub + tokio tasks per consumer | queued |
| W.8 | Universal `Plastic` trait + outer controller | queued |
| W.9 | Tatara-lisp adapter (in-memory Lisp morphing) | queued |

After W.7–W.9 the machine is the live perpetual self-optimizing
system targeting *any task* with *any training data* streamed in
*asynchronously* and *Lisp-morphable at runtime*. That is the
fixed point the design converges on.
