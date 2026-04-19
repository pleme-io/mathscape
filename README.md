# mathscape

Evolutionary symbolic compression engine — discovers mathematical
abstractions by rewarding compression and novelty over expression trees.

## Status: autonomous traversal, self-containing compute, self-tuning meta-loop, perpetual self-optimizing model

As of 2026-04-18 (post-Phase U), the machine traverses mathscape
**autonomously**, with **self-containing compute**, and now a
**self-tuning meta-loop** on top that observes its own learning,
proposes its own next training recipe, and sails out when the
reachable territory for a seed is exhausted:

- Given any sufficiently-diverse corpus set, it discovers primitives,
  reinforces them via retroactive reduction across a shared forest
  substrate, and climbs the proof-status lattice to `Axiomatized` on
  cross-corpus empirical evidence alone. No human approval is in the
  loop. No external prover assists. No hook fakes the gate.
- More input makes the machine **more efficient**, not less.
  `(node, rule)` memoization + content-addressable hash-consing turn
  per-corpus cost into O(new_nodes × library_size) — measured at
  ~0.84 ms/corpus from 10,000 corpora up through 100,000, on a
  commodity darwin-arm64 workstation.
- The **lynchpin invariant** holds at every tested scale: every rule
  in the final library earns cross-corpus retroactive support ≥ 2.
  No corpus artifacts survive.
- **Phase T** (wall-clock efficiency): R37 early-stop on library
  plateau cuts post-saturation iterations automatically — 1.80× on
  single M0, 3.97× on 4-phase scenarios; autonomous-traversal
  medium 96ms (vs 150 expected), stress 321ms (vs 500 expected).
- **Phase I** (subterm-paired anti-unification): law discovery no
  longer limited to root-level pattern matches. Unblocks Phase H's
  rank-2 meta-inception when paired with Phase J's empirical
  validity (pending).
- **Phase U** (self-tuning meta-loop): `MetaLoop<Executor, Proposer>`
  drives an observe → propose → execute → observe cycle.
  `LearningObservation` captures what each scenario taught;
  `HeuristicProposer` encodes Phase T's findings as decisions;
  `AdaptiveProposer` LEARNS per-archetype performance over time
  and biases its picks toward empirical winners. Fully Lisp-residential
  (R32 spec, R33 scenario, R10 policy, U.1 observation), meta-loop
  attestation is BLAKE3 over the chain of chain-attestations.
- **Phase V** (fix-point motor): the loop closes on itself.
  `LearningObservation.staleness_score()` detects when the
  environment has stopped producing novelty;
  `SpecArchetype::AdaptiveDiet` mutates the DIET (routes through
  `AdaptiveCorpusGenerator`, which reads library state and
  synthesizes residue-inviting terms). Observed running:
  default corpus saturates at 4 rules over 3 phases, motor fires
  AdaptiveDiet on phase 3, library grows to 5 — a rule the
  default corpus could not reach. The model reacts to its own
  saturation by expanding its environment. Trained on bare math:
  staleness, intervention, and reward are all pure functions of
  the observation stream.
- **Phase V.map** (the machine has a map of itself): `MathscapeMap`
  is a typed, attestable view of the library — `core_rules`,
  `union_rules`, `mutation_edges`, seed metadata, per-rule
  cross-corpus support, and a BLAKE3 Merkle root over canonical
  rule serialization. `save_to_path` / `load_from_path` persist
  it; `library_merkle_root` produces deterministic attestation.
- **Phase V.events** (the machine narrates itself): `MapEvent`
  enum with typed variants — `NovelRoot`, `RootMutated`, `CoreGrew`,
  `StalenessCrossed`, `RuleCertified`, `RuleRejectedAtCertification`,
  `BenchmarkScored`. `MapEventConsumer` trait + `BufferedConsumer`
  implementation form the default event bus. Every architectural
  transition is an event the downstream consumers see in order.
- **Phase V.certify** (reactive promotion): `CertificationLevel`
  state machine (`Candidate → Validated → ProvisionalCore →
  Certified → Canonical`) with `Certifier` trait, `DefaultCertifier`,
  and `CertifyingConsumer` — a chainable consumer that watches the
  event stream, promotes rules through the lattice on confirmed
  empirical support, and emits `RuleCertified` /
  `RuleRejectedAtCertification` downstream.
- **Phase V.stream** (never-destroy streaming trainer):
  `StreamingPolicyTrainer` wraps `LinearPolicy` in `RefCell` and
  implements `MapEventConsumer`. Every event produces a reward
  signal; reward × feature vector applies an online SGD step.
  The trainer never resets between phases — it is the persistent
  policy head across the whole session.
- **Phase V.benchmark** (labeled-data ingress / report card):
  `MathProblem`, `ProblemResult`, `BenchmarkReport`,
  `canonical_problem_set()` (12 problems), `harder_problem_set()`
  (6 symbolic-identity problems needing discovered rules),
  `run_benchmark()`, and `BenchmarkConsumer::benchmark_now(library,
  downstream)` that scores the library and emits
  `MapEvent::BenchmarkScored { solved_fraction, delta_from_prior }`.
  The streaming trainer rewards improvement asymmetrically: +3×
  for gains, −5× for regressions — *don't break what worked*.
- **Phase V.shed** (neuroplasticity): per-weight activation counts
  and cumulative contributions tracked inside the streaming
  trainer. `prune(magnitude_threshold, min_activations)` zeros
  weights that are both small and rarely-activated and marks
  them pruned; `rejuvenate(index, initial_value)` un-prunes and
  re-seeds. Pruned weights are skipped on future updates. The
  policy sheds dead dimensions while the stream continues to form
  new ones — neuroplasticity applied to the policy head.
- **Phase W** (perpetual self-optimizing model): four
  research-grade mechanisms absorbed from continual-learning and
  dynamic-sparse-training literature.
  - **W.1 RigL phantom gradients**: pruned weights still
    accumulate `|would-be-delta|`; `auto_rejuvenate` picks up
    phantom-active weights automatically. The shed+grow loop is
    now fully autonomous (Evci et al. 2020).
  - **W.2 EWC Fisher-weighted stability**: per-weight Fisher EMA;
    anchor on benchmark improvement; Fisher-weighted pullback on
    regression events protects load-bearing weights from drift
    while marginal weights stay plastic (Kirkpatrick et al.
    2017).
  - **W.3 Learning-progress intrinsic reward**: benchmark events
    get a +4× bonus for `current - min(last K scores)` positive
    improvement — the agent is rewarded for improving itself
    (Schmidhuber / Oudeyer).
  - **W.stall Corrupted/stalled pruning**:
    `prune_dormant_or_corrupted` sheds weights that went silent
    after being active or that stay zero despite high Fisher
    (pressure-flattened).
  - **W.4 EventHub + motor translator**: synchronous reentrant-
    safe pub/sub spine; `publish_outcome_events` translates
    `MetaLoopOutcome` → `MapEvent` stream fanned out to every
    subscriber. The full perpetual loop runs end-to-end in one
    subscription (`tests/perpetual_loop.rs`).

- **Phase X** (Mathematician's Curriculum): 32 problems across
  6 subdomains — arithmetic-nat, arithmetic-int, symbolic-nat,
  tensor-algebra, compound, generalization — with per-subdomain
  scoring. Observed real-motor result: 56% → 88% (Δ +31.2%), 4
  subdomain masteries, 2 regressions surfacing rule-vs-kernel
  priority issue the coarse benchmark hid.
- **Phase Y.0** (LiveInferenceHandle): query the running model
  without freezing the stream. `infer`, `current_competency`,
  `policy_snapshot`, `library_snapshot`, `library_size` — all
  non-blocking, dashboard-friendly, Lisp-morphable.
- **Phase Y.1** (OpenAPI spec + normalized pipeline):
  `apis/mathscape-inference/openapi.yaml` — 3.0.3, 5 paths, 23
  schemas — single source of truth for REST, gRPC, GraphQL, MCP,
  SDKs, docs, completions. Sekkei → takumi → forge-gen renders
  every transport.
- **Phase Y.2** (mathscape-inference-api crate): typed DTOs
  matching the spec field-for-field, `InferenceService` trait
  all backends route through, `HandleAdapter` default impl
  wrapping `LiveInferenceHandle`. JSON-over-wire full round-trip
  tested (11/11).

See `docs/arch/autonomous-traversal.md` for the milestone doc,
`docs/arch/self-tuning-meta-loop.md` for the Phase U+V+W frame,
`docs/arch/perpetual-improvement.md` for the fixed-point synthesis,
and `docs/arch/landmarks.md` for the full canonical map
(phases A–Y).

## Quickstart

```bash
cd mathscape

# Run the orchestrated autonomous-traversal suite (4 tests):
cargo test -p mathscape-axiom-bridge --test autonomous_traverse

# Scale probe — 10,000 procedural corpora, ~8s in release:
MATHSCAPE_TRAVERSE_BUDGET=10000 \
  cargo test -p mathscape-axiom-bridge --test autonomous_traverse \
    autonomous_traverse_medium --release -- --nocapture

# Mega probe — 100,000 procedural corpora, ~85s in release:
MATHSCAPE_TRAVERSE_BUDGET=100000 \
  cargo test -p mathscape-axiom-bridge --test autonomous_traverse \
    autonomous_traverse_medium --release -- --nocapture
```

Or via the reserved skill (from any repo):

```
/mathscape-traverse
```

## Core thesis

Mathematical understanding is compression. Mathscape automates that
compression: given a minimal computational substrate and a reward
signal that favors shorter descriptions of more phenomena, a search
process rediscovers known mathematics — and may find new compressions
humans haven't seen. The **self-containing compute** property means
this is tractable at arbitrary input scale: the machine's developed
tools (symbols, meta-rules) compact each new corpus using work
already done, so the effective traversal cost stays bounded as the
reachable territory expands.

## Documentation

- `CLAUDE.md` — conventions, architecture, crate structure.
- `docs/arch/machine-synthesis.md` — the canonical picture of the
  five architectural objects, ten gates, five forces, three regimes.
- `docs/arch/autonomous-traversal.md` — the milestone + self-containing
  compute findings.
- Full arch-doc list in `CLAUDE.md`.

## Invocation boundaries

- Read-only observation: use the `mathscape-traverse` skill.
- Code changes: edit crates directly, then rerun
  `autonomous_traverse` to confirm the lynchpin invariant still holds.
- Anything that breaks `autonomous_traverse_deterministic_replay`
  is a serious regression — some source of non-determinism has leaked
  into the loop.
