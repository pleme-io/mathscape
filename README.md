# mathscape

Evolutionary symbolic compression engine — discovers mathematical
abstractions by rewarding compression and novelty over expression trees.

## Status: autonomous traversal, self-containing compute, self-tuning meta-loop

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

See `docs/arch/autonomous-traversal.md` for the milestone doc,
`docs/arch/self-tuning-meta-loop.md` for the Phase U+V frame, and
`docs/arch/landmarks.md` for the full canonical map (phases A–V).

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
