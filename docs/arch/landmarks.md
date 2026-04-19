# Mathscape Landmarks

Where the machine is, where it's been, and where it goes next. This doc
is the canonical map. Updated every time a milestone closes.

## Where we are (2026-04-18, post-Phase U)

The machine:
- Traverses mathscape **autonomously** — discovers primitives, reinforces
  via retroactive reduction, climbs the proof-status lattice to
  `Axiomatized` without human approval or external prover
- Reaches **rank-1 dimensional discovery** — the MetaPatternGenerator
  abstracts concrete identity laws (`add(?x, 0) = ?x`,
  `mul(?x, 1) = ?x`) into an operator-identity meta-rule
  `(?op (?op ?x ?id) ?id) = …` with both operator and identity
  variable
- Operates with **self-containing compute** — per-corpus cost stays
  flat at ~0.84 ms regardless of total sweep size, from 10k to 100k
  corpora. More data makes the machine more efficient, not less
- **Lynchpin holds** at every scale tested (12, 19, 47, 507, 2007,
  10007, 100007 corpora): every rule earns cross-corpus retroactive
  support ≥ 2
- **Produces a first trained model M0** — empty library → tensor
  corpus → 4 discovered laws → trained LinearPolicy, fully
  Lisp-describable AND Lisp-producible via `BootstrapCycleSpec` +
  `execute_spec_core`. BLAKE3-attested, bit-identical replay
- **Trains more efficiently** — R37 early-stop cuts ~40% of wall
  time on single cycles; R39 cascades the same pattern to a ~4×
  speedup on 4-phase training chains. Autonomous-traversal
  milestone picks up a ~1.5× bonus from R38 micro-opts inside
  `paired_anti_unify`
- **Surfaces subterm-level laws** — Phase I adds
  `paired_subterm_anti_unify` alongside the root-level primitive,
  enumerating matching subterm positions up to a depth cap.
  Unblocks Phase H rank-2 inception once Phase J certifies
  candidates. Exposed as `derive_laws_with_subterm_au`
- **Self-tunes end-to-end** — Phase U's `MetaLoop` observes each
  scenario's outcome via `LearningObservation`, hands the history
  to a `ScenarioProposer`, executes the proposed next scenario,
  and sails out when no-progress + tiny policy delta persist for
  K consecutive phases. `HeuristicProposer` encodes Phase T as
  static decisions; `AdaptiveProposer` LEARNS per-archetype
  performance at runtime with ε-greedy exploration. All pieces
  Lisp-residential; meta-attestation is BLAKE3 over the chain of
  chain-attestations
- **Rank-2 inception achieved** — Phase H landed 2026-04-18.
  With Phase I surfacing shape-orthogonal candidates at subterm
  positions, Phase J rejecting over-general meta-heads via
  empirical validity checks, and the strict-subsumption gate
  admitting distinct meta-equivalence classes, `MetaPatternGenerator`
  mints the first rank-2 meta-rule:
  `S_30002 :: (?op ?id (?op ?id ?x)) => S_30002(?op ?id ?x)` —
  abstracting across both operator and nested-identity
  structure. Pinned by `phase_h_unblock_pipeline_runs_end_to_end`
- **Fix-point motor active** — Phase V landed 2026-04-18. The
  meta-loop now closes on itself: `LearningObservation.staleness_score()`
  emits a scalar signal when a scenario produces no novelty and
  the policy barely moves; `HeuristicProposer` reads the mean
  staleness over the last 3 observations and, when it crosses
  0.6, picks `SpecArchetype::AdaptiveDiet` — a DIET MUTATION
  archetype that routes through `AdaptiveCorpusGenerator`
  (promoted from test scaffolding to core). The adaptive corpus
  reads current library state and synthesizes residue-inviting
  terms the extractor hasn't seen. Pinned by
  `fix_point_motor_runs_and_visibly_mutates_diet` — observed
  trace: seed discovers 4 rules, two baseline/extended retries
  produce 0 growth, staleness crosses threshold, AdaptiveDiet
  fires, library grows to 5 on the first diet-mutation phase.
  The 5th rule is a rule the default corpus could not reach.

### Apex fingerprint

The same two apex rules climb to Axiomatized at every scale:

| Rule | Shape | Origin | Cross-corpus reach at 100k |
|---|---|---|---|
| `S_10000` | `(?op (?op ?x ?id) ?id)` | rank-1 meta (MetaPatternGenerator) | 93,257/100,007 (93.3%) |
| `S_043` | successor-chain universal | rank-0 concrete | ~47% observed across sweep sizes |

Rule IDs are allocated in discovery order (counter managed by
`propose()` callers), so the specific index of the rank-0 apex
shifts slightly as session history replays with different
instrumentation ordering. The STRUCTURAL signature is stable: a
rank-1 operator-identity meta-rule at S_10000 and a rank-0
successor-family universal at a low-40s ID. Phase T confirmed
S_043 at small (12), medium (19), and stress (47) scales.

### What the apex rules tell us

- **S_10000 is genuinely universal.** 93.3% of random procedural
  corpora contain at least one root-level `op(op(x, id), id)` shape.
  This is the machine saying: "in any corpus where a binary operator
  is applied twice with a shared identity-looking argument, this
  pattern fires."
- **S_040 reflects structural density of successor chains** — not
  majority but not rare. ~47% is the true rate of successor-rooted
  terms in random {add, mul, succ} inputs.

## Where we've been

### Phase A: skeleton (initial commits)
- Crates: `mathscape-core`, `-compress`, `-reward`, `-evolve`,
  `-proof`, `-store`, `-axiom-bridge`, `-config`
- Primitives, hash-consing, evaluator, anti-unification,
  statistical prover, epoch/allocator

### Phase B: forced-realization gates (Feb–Mar 2026)
- Gates 1–10, regime detector, reward axes (ΔCR, novelty,
  meta-compression), status lattice, promotion pipeline
- `run_until_reduced`, `MultiLayerRunner`, `ReductionPolicy`

### Phase C: autonomous discovery (Apr 2026)
- Marginal ΔCR in the prover (fixed orthogonal-family gatekeeping)
- `lhs_subsumption` reward axis (captures meta-rule value)
- Anti-unify var-id collision fix (`max(200, input_max + 1)`)
- `CompositeGenerator<Base, Meta>` (base + meta per propose)
- `MetaPatternGenerator` (anti-unify library LHSs)
- Fresh apex emerged: `S_10000` dimensional-discovery meta-rule

### Phase D: forest as substrate (Apr 2026)
- `DiscoveryForest` — retroactive reducer with O(log n) scheduler
- Typed invariants — `IrreducibilityRate`, `CheckPeriod`, `HitCount`
- `due_corpus_view(epoch)` — live-frontier seam for the generator

### Phase E: cross-corpus attestation (Apr 2026)
- Shared forest threads terms across corpora
- `apply_rules_retroactively(&[...])` batch form — no rule starvation
- Saturation sweep: 19 corpora → 2 apex rules Axiomatized
- **Lynchpin invariant** named as the first-class check

### Phase F: autonomous-traversal milestone (2026-04-17)
- `autonomous_traverse.rs` orchestrated suite: small / medium / stress
  / deterministic-replay
- Reserved `mathscape-traverse` skill (blackmatter-pleme)
- `docs/arch/autonomous-traversal.md` — milestone doc
- Commit sequence: `873cfa6` → `1cfc41a` → `405195e`

### Phase G: self-containing compute (2026-04-18)
- `(node, rule)` memoization — retesting skipped, compute bounded
- Scale-invariant apex support threshold (≥5% of sweep, ≥5 corpora)
- Measured: 100k corpora @ 0.84 ms/corpus — per-corpus cost flat from
  10k upward

### Phase R: kernel reductions — "no equal terms" invariant (2026-04-18)

Kernel-level refactoring per `core-algorithm-review.md`. The machine's
level-above work shouldn't need to recover facts the kernel can
structurally enforce. Each R-landmark closes a gap where
semantically-equal terms had structurally-distinct representations.

All changes preserve the autonomous-traversal milestone (apex
fingerprint unchanged, deterministic_replay passes, per-corpus cost
unchanged).

| Landmark | Closes |
|---|---|
| **C1** (shared anonymize) | commutativity rule collapsing to identity under independent var-map anonymization |
| **C2** (BTreeMap bindings) | `pattern_match` non-determinism from HashMap iteration |
| **R3** (commutative sort) | `add(3, 5) ≠ add(5, 3)` structurally — now sorted by derived Ord |
| **R4** (associative flatten) | `add(add(1,2), 3) ≠ add(1, add(2,3))` — flatten + binary-left-associate |
| **R5** (Builtin registry) | magic operator ids scattered across eval, term, downstream — one source of truth |
| **R6** (constant folding) | `Apply(add, [3, 5]) ≠ Number(8)` — fold reducible Applys via registry's eval rule |
| **C3** (Fn param binding) | anonymization cloned params verbatim while renumbering body vars, breaking lexical bindings |
| **R7** (Value::Int + Int builtins) | `Value` nominally extensible but never extended. Now has Int(i64) second variant, 5 Int builtins (int_zero/succ/add/mul, neg) domain-disjoint from Nat, parser recognizes Int literals (-N, iN) and builtin names |
| **R8** (Tensor shape detector) | Answering "can we detect when the machine naturally develops the tensor?" — YES. Distributivity is the gateway tensor: mul bilinear over add is rank-2 tensor structure. `tensor.rs` pattern-matches rule shapes in the library, reports density + distributive / meta-distributive counts. Traversal report now prints tensor emergence alongside lynchpin. Baseline: Peano-only corpora produce 0 tensor rules (machine hasn't discovered full distributivity yet); a regression pin asserts this. When it flips to nonzero, tensor structure has emerged autonomously |

**R1 (AC-absorbing alpha_equivalent)** probed and deferred
2026-04-18. Canonicalizing both rules before anonymization is the
natural next step — it would close the documented gap in
`mathscape-compress::egraph::check_rule_equivalence`. Shift
observed: apex moves from {S_10000, S_040} to {S_10000, S_042}
with S_042 at 2/12 small-scale support, below the threshold.
Lynchpin still holds; deterministic_replay still passes. Reverted
pending structural investigation of the new apex set.

**R6-value (Value polymorphism)** landed 2026-04-18 as **R7**:
Value became extensible via a second variant rather than a full
trait-based refactor. Int domain now lives alongside Nat at every
kernel level — enum, registry, parser, evaluator, canonical fold,
egraph encoding, Lean export. Cross-domain calls are strictly
rejected (no silent promotion); overflow uses checked arithmetic
(correctness > wrapping per kernel invariant "true"). The machine
can now target a second numeric domain whenever a corpus selects
Int — additive capability, no milestone change.

**R2 (TermVisitor trait)** remains pending — marginal cleanup.
Adding a Term variant already surfaces each site via the compiler's
exhaustiveness check (R7 demonstrated this). A visitor trait would
trade that per-site compiler feedback for uniform dispatch; benefit
is modest relative to the refactor scope (47 match sites across 26
files). Deferred.

**Kernel invariant status after Phase R:**
- Genuine: every operation means what it says mathematically ✓
- True: evaluator produces correct answers; C3 closes the last
  known correctness bug; R7 overflow is checked, not wrapping ✓
- Repeatable: deterministic across runs (C2 + BTreeMap bindings) ✓
- No equal terms: commutative + associative + nullary/unary/binary
  constant applications all collapse to one canonical form ✓
  (modulo AC absorption into alpha_equivalent — R1 deferred)
- Extensible: Value carries two domains (Nat, Int); the registry
  scales by appending Builtin entries with no magic numbers ✓

### Phase S: compute layer + self-producing cycle (2026-04-18)

Session arc building the full ML compute stack and the typed
self-producing discovery loop on top. Preserves the autonomous-
traversal milestone throughout; apex fingerprint unchanged,
deterministic_replay bit-identical.

| Landmark | Closes |
|---|---|
| **R13** (Value::Tensor + 6 builtins) | tensor_add/mul/sum/dot/neg/scale, elementwise + reductions + checked arithmetic |
| **R14** (symbolic autograd) | chain/product/sum rules for scalar ops; Int-valued derivative expressions |
| **R15** (SGD optimizer) | sgd_step as Term composition — no new builtin needed, primitives compose |
| **R16** (2D tensor ops) | matmul / transpose / reshape, classical linear algebra |
| **R17** (autograd through tensor ops) | gradient flow for FT_ADD/MUL/SUM/DOT/SCALE/NEG |
| **R18** (Value::Float + SGD convergence) | IEEE 754 doubles as bit-encoded u64. 50-step SGD on (w-7)² converges to within 0.001 |
| **R19** (FloatTensor) | real-valued parametric models; bit-encoded data Vec<u64>; 8 ft_* builtins |
| **R20** (float-tensor autograd + training) | full end-to-end FT training: derive loss symbolically once, 60 SGD steps converge |
| **R21** (tensor discovery corpus) | R24 + tensor_corpus generator; anti-unification produces tensor-headed candidates |
| **R22** (natural emergence probe) | HONEST finding: apex rules {S_10000, S_043} are library compressions, NOT laws. R12 matches zero |
| **R23** (repeated-arrival observation) | 18 configs → same apex every time; zero variation. Machine has a single fixed convergence target on Peano |
| **R24** (law generator) | paired_anti_unify + derive_laws_from_corpus. MACHINE NOW DISCOVERS tensor_add/mul identity laws autonomously from eval traces |
| **R25** (self-bootstrapping loop) | empty library → discover → grow → repeat. 4 iterations, 10 laws, policy trains |
| **R26** (BootstrapCycle typescape entity) | enshrines R25 as first-class typed generic struct with 3 hijackable layers + BLAKE3 attestation |
| **R27** (refactor + invariants) | 20 invariant tests; self_bootstrap now uses R26 DefaultCorpusGenerator. Found: library grows LINEARLY forever (no saturation) |
| **R28** (LibraryDeduper layer 4) | closes R27 linear-growth gap. CanonicalDeduper saturates cycle at step 3 instead of +3/iter forever. Library 30 → 4 rules |
| **R29** (DomainOps trait in autograd) | 10 parallel simplify_* helpers → 3 generic simplify_X_of + 3 trait impls (IntOps/FloatOps/TensorOps). Adding a domain is one impl |

**Phase S headline findings:**

- **The machine discovers its own tensor primitives.** R24's law
  generator, given concrete tensor-identity corpora, produces
  `tensor_add(zeros, ?x) = ?x` and `tensor_mul(ones, ?x) = ?x` —
  matching the hand-coded R13/R19 reference via paired-AU.
- **The cycle converges with dedup.** Without: linear growth
  forever (30 rules after 10 iter). With: saturation at step 3
  (4 rules, remainder rejected as derivable variants).
- **Self-producing loop works end-to-end.** Empty → tensor →
  model → more tensor → updated model, BLAKE3-attested,
  bit-identical replay. The typed entity encapsulates chaos.

**ML apparatus plan.** `docs/arch/ml-apparatus.md` lays out the
full 4-layer hijack-and-optimize architecture with the
orchestrator as the outermost model. Layers 1–4 shipped; the
orchestrator itself is future work.

**Kernel invariant status after Phase S:**
Phase R invariants all preserved. Phase S adds:
- **Compositional compute**: gradient flow through any registered
  domain; SGD step composes from existing primitives
- **Self-producing cycle**: 4-layer trait-based BootstrapCycle
  with BLAKE3 attestation for deterministic replay at the cycle
  level
- **Convergence**: dedup-enabled cycle reaches a structurally-
  distinct fixed point, not unbounded growth

### Phase T: Lisp-residency + wall-clock efficiency (2026-04-18)

Extends Phase S to make the cycle fully Lisp-describable, fully
Lisp-producible, and genuinely efficient. Closes the 2026-04-18
framing: "from this point on we only think in terms of making
that model exist more efficiently and train more efficiently."

| Landmark | Closes |
|---|---|
| **R30** (SubsumptionDeduper + deduplicate_library) | strongest LibraryDeduper — rejects specializations not just alpha-renames; a post-hoc library-cleaning utility on the side |
| **R31** (First trained model M0 + inspection) | end-to-end demo: cycle produces a trained LinearPolicy, Sexp round-trip, bincode persistence, attestation, score known states |
| **R32** (BootstrapCycleSpec + Lisp executor) | cycle recipe becomes a pure Lisp value; `execute_spec_core` dispatches layer names. Input Lisp → output Lisp |
| **R33** (ExperimentScenario multi-phase chain) | sequence of phases, each phase's final library/policy seeds the next. Chain-level BLAKE3 attestation |
| **R34** (Wall-clock timing instrumentation) | per-iteration corpus/extract/dedup timings + per-cycle total on BootstrapOutcome, scenario-total on ExperimentOutcome. Observational — NOT part of attestation payload |
| **R35** (Extract phase split via LawGenStats) | eval/anti_unify/rank wall-clock inside `derive_laws_from_corpus_instrumented`. Narrowed the bottleneck: **paired_anti_unify = 92% of extract** |
| **R36** (MemoizingAntiUnifier) | pass-through cache for `paired_anti_unify` results. Shipped as library machinery; honestly NOT wired into first_model because per-miss Term clone × 4 beats the per-hit savings at current scale. Future R-something: TermRef-keyed cache |
| **R37** (Early-stop on library plateau) | `BootstrapCycle::run_until_stable(window)` + `BootstrapCycleSpec::early_stop_after_stable` + Lisp bridge. **First real wall-clock win: 1.80× on M0.** Same final library in 3 iterations instead of 5 |
| **R38** (paired_anti_unify micro-opts) | three sub-landmarks: term_key replaces Debug-format+Vec<u8> with Term clone; subset check replaces BTreeSet × 2 with sorted Vec + binary_search; `exceeds_or_equals_floor` early-exits max_var_id when no pattern var needs bumping. ~9% cumulative on single cycle |
| **R39** (Scenario-level early-stop demo) | R37 cascades across 4-phase chains: **3.97× wall-clock speedup**, 14 of 20 iterations eliminated, same final library. Plateau detection compounds multiplicatively |

**Phase T headline findings:**

- **Measurement precedes optimization.** R34/R35 instrumentation
  identified the true bottleneck (`paired_anti_unify` at 92% of
  extract) that was not the obvious suspect (eval). R36's memoization
  cache would've looked like a win on paper; measurement said
  otherwise.
- **Work elimination > work acceleration.** R37's skip-when-plateau
  delivered 1.8×; R38's micro-opts on the hot path delivered ~9%.
  Avoid doing the work beats doing the work faster.
- **Efficiency wins compound across phases.** R39 demonstrates that
  per-cycle gains cascade multiplicatively across multi-phase
  scenarios — 1.8× per phase → ~4× over a 4-phase chain.
- **Autonomous-traversal benefits too.** R38 micro-opts inside
  `paired_anti_unify` carry through to the milestone test:
  medium sweep 96ms (was ~150ms expected), stress sweep 321ms
  (was ~500ms expected) — ~1.5× speedup without changing
  determinism or apex fingerprint.

**M0 identity fingerprint (Phase T baseline):**
- 5-iteration default cycle, 4 rules final library, policy
  generation=1, trained_steps=5
- Wall-clock: ~11 ms (was ~12 ms pre-R38 on identical hardware)
- Under `run_until_stable(1)`: ~6 ms, 3 iterations, identical library
- Attestation: covers library + policy + trajectory; stable across
  runs; does NOT include wall-clock timings

**Kernel invariant status after Phase T:**
All Phase R + Phase S invariants preserved. Phase T adds:
- **Observational timings**: every cycle + scenario carries
  wall-clock breakdown. Does NOT enter attestation — two runs
  with identical inputs produce identical attestations despite
  clock drift.
- **Opt-in work elimination**: `early_stop_after_stable` cuts
  post-plateau iterations without changing the final library on
  plateau-reaching workloads. Pinned invariant: early-stop produces
  bit-identical library to full run.

### Phase I: subterm-paired anti-unification (2026-04-18)

Law discovery no longer limited to root-level pattern matches.
`paired_subterm_anti_unify` in `mathscape-compress/antiunify.rs`
enumerates matching subterm positions in both inputs (up to a
depth cap) and anti-unifies each pair against the whole output.
Asymmetric output pairing keeps the function useful when eval
folds deep inputs to leaf outputs.

| Landmark | Closes |
|---|---|
| **Phase I.1** (subterm AU primitive) | root-only AU → pairs at every matching subterm position; min-depth descent with shape-compatible-arity check |
| **Phase I.2** (`derive_laws_with_subterm_au`) | sibling of `derive_laws_with_cache`, routes through the subterm primitive; `subterm_depth=0` collapses to root-only |

**Unblocks Phase H** (rank-2 meta-inception): the
`rank2_inception_probe` test was stuck at ONE active meta-rule
because every root-level candidate collapsed into S_10000's
equivalence class. Phase I surfaces shape-orthogonal candidates
at inner positions — the precondition for
`MetaPatternGenerator` to mint rank-2 across distinct classes.
Phase J (empirical validity) still needed to certify them.

**Honest limitation** (pinned by
`subterm_au_surfaces_inner_pattern`): eval-fold collapses output
structure so many subterm candidates fail the RHS-subset-LHS
filter. Phase I surfaces; Phase J certifies.

### Phase V: The fix-point motor (2026-04-18)

The meta-loop closes. Signal → detection → intervention →
observation → signal ... with the intervention vocabulary now
including environment mutation (diet), not just model-parameter
mutation. This is the architectural move the session's directive
arc was circling: "the need for a diet and its associated signals
should cause the model to learn the environment, observe why the
signal happened and adjust it and keep going — that's the
fix-point model behavior."

| Landmark | Closes |
|---|---|
| **V.1 Staleness signal** on `LearningObservation` | intrinsic signal detecting "the environment has stopped producing novelty" — growth=0 AND policy barely moving. Scalar in [0,1]. Deterministic, Sexp-bridgeable |
| **V.2 `AdaptiveCorpusGenerator`** in core | promoted from `tests/common/experiment.rs` scaffolding to first-class `CorpusGenerator` impl. Reads library state; synthesizes shelled terms whose outer op is non-reducible and whose children are substrate-rule instantiations. Post-reduction residue has NEW STRUCTURE the library hasn't seen |
| **V.3 `SpecArchetype::AdaptiveDiet`** | fifth archetype in the proposer's vocabulary. Routes through `corpus_generator = "adaptive"`; the `execute_spec_core` now resolves this name |
| **V.4 `HeuristicProposer` staleness branch** | mean staleness over last 3 observations; when ≥ 0.6, picks AdaptiveDiet BEFORE the other branches — staleness is highest-priority signal |
| **V.5 `fix_point_motor` running demo** | end-to-end pinned: seed → default corpus saturates at 4 rules → two retries produce 0 growth → staleness crosses threshold → AdaptiveDiet fires → library grows to 5 rules. The 5th rule is unreachable via the default corpus. Pinned by `fix_point_motor_runs_and_visibly_mutates_diet` + `fix_point_motor_is_deterministic` |

**Phase V headline findings:**

- **The motor works on a real derive-laws extractor**, not just the
  null case. The observed trace shows staleness climbing 0.11 →
  0.91 → 0.96 over three phases, triggering AdaptiveDiet at phase 3,
  which produces library growth and policy movement. The machine
  genuinely reacts to its own saturation signal.
- **Diet mutation produces rules that the default diet cannot**.
  The 5th rule observed in the motor trace is empirical proof that
  environment mutation is not cosmetic — it expands what the
  machine can discover. Intervention at the corpus level reaches
  structural territory the training-recipe tweaks can't.
- **Trained on bare math**. The staleness signal is a pure function
  of observation fields (`made_any_progress`, `trained_policy_delta_norm`).
  The intervention choice is a function of observation history. The
  reward is the delta in observation after intervention. No
  external labels, no human-curated corpora — the machine's own
  mathematical trajectory is the training signal.

**Kernel invariant status after Phase V:**
All Phase R + S + T + U + I + J + H invariants preserved. Phase V adds:
- **Staleness determinism**: `staleness_score()` is a pure function
  of observation fields; two identical observations → identical
  staleness.
- **Motor determinism**: same seed scenario + same proposer + same
  executor → identical meta-attestation + identical scenario-name
  sequence. Pinned by `fix_point_motor_is_deterministic`.
- **Novelty invariant**: `fix_point_motor_runs_and_visibly_mutates_diet`
  asserts ≥1 AdaptiveDiet phase AND ≥1 rule total AND
  max_staleness ≥ 0.5 — a regression test for the closed loop.

**What Phase V unlocks for future arcs:**

- **Corpus archetypes as first-class expansion target**. Today
  there's one diet mutation (`AdaptiveDiet`). The architecture
  makes adding more trivial: `SymbolicPreservingDiet`,
  `EquivalenceClassGrouping`, `InverseOperatorInjection`. Each
  opens a new region of mathscape the machine can now reach.
- **The adaptive proposer can learn archetype effectiveness per
  staleness level**. `AdaptiveProposer` tracks per-archetype
  stats. A future pass can condition those stats on the
  observation state, giving the proposer a learned
  "when-am-I-stuck" model.
- **WASM/linguistic isolation (U.5) stays ready**. Everything
  Phase V added is a Lisp value or a trait seam. The motor's
  full trajectory is a sequence of Sexp values; shipping it
  into a WASI module is still the signature the arc has been
  preparing.

### Phase V.map: the machine has a map of itself (2026-04-18)

`MathscapeMap` is the first *typed view* of the library that the
machine can reason about as a whole — not as a flat list of rules
but as a structured object with apex rules, union rules,
mutation edges, seed metadata, and a BLAKE3 Merkle root over
canonical rule serialization. The map is attestable (the Merkle
root changes iff the rule set changes), persistable
(`save_to_path`/`load_from_path`), and queryable (per-rule
cross-corpus support, core/union partitioning).

| Landmark | Closes |
|---|---|
| `MathscapeMap` struct + `MapSnapshot` | typed view of the library as map-of-itself; core_rules / union_rules / mutation_edges / seed_info |
| `library_merkle_root` | deterministic BLAKE3 over canonical rule serialization — any rule change produces a new root |
| `save_to_path` / `load_from_path` | the map is a first-class serializable artifact; sessions can pick up from a prior map |
| `MapSummary` | compressed one-shot observation suitable for the streaming trainer |

### Phase V.events: the machine narrates itself (2026-04-18)

Every structural transition is an event on a typed bus. The
consumer trait is pluggable (BufferedConsumer, CertifyingConsumer,
StreamingPolicyTrainer, BenchmarkConsumer all implement it).
Architectural transitions (novel roots, mutations, core growth,
staleness crossings, rule certifications/rejections, benchmark
scorings) all pass through the same channel.

| Landmark | Closes |
|---|---|
| `MapEvent` enum with 7 variants | `NovelRoot`, `RootMutated`, `CoreGrew`, `StalenessCrossed`, `RuleCertified`, `RuleRejectedAtCertification`, `BenchmarkScored` — the machine's own narrative stream |
| `MapEventConsumer` trait | single `consume(&MapEvent)` method; pluggable downstreams |
| `BufferedConsumer` | default accumulating consumer; used for testing + history |
| `EventCategory` classification | category() arm for each event; enables filtered consumers |

### Phase V.certify: reactive rule promotion (2026-04-18)

Certification is now a *reactive* state machine watching the
event bus, not a batch operation. Rules climb the lattice as
cross-corpus evidence accrues; failed rules get rejected and the
downstream consumers see both outcomes.

| Landmark | Closes |
|---|---|
| `CertificationLevel` state machine | `Candidate → Validated → ProvisionalCore → Certified → Canonical` |
| `Certifier` trait + `DefaultCertifier` | pluggable certification policy; default fires on support thresholds |
| `CertifyingConsumer` | chainable MapEventConsumer that promotes/rejects rules and emits downstream events |
| `run_certification_step` | one-step driver used by tests and external integrations |

### Phase V.stream: never-destroy streaming trainer (2026-04-18)

The policy head is now an always-on online learner. Every
MapEvent produces a reward; reward × feature vector applies an
online SGD step via the existing `sgd_step_*` primitives. The
trainer never resets — it is the session-long persistent model.

| Landmark | Closes |
|---|---|
| `StreamingPolicyTrainer` | wraps `LinearPolicy` in `RefCell`; implements `MapEventConsumer` |
| `reward_for(&MapEvent)` | typed mapping from event category to scalar reward |
| `features_for(&MapEvent, &MapSnapshot)` | feature vector extraction per event |
| `inject(policy)` / `adjust_learning_rate(lr)` | non-destructive policy replacement and rate tuning |
| `snapshot()` | read-only view that does not freeze the stream |

### Phase V.benchmark: labeled-data ingress / report card (2026-04-18)

The machine knows whether it's improving because it has a report
card. Two problem sets — `canonical_problem_set()` (12 kernel-
solvable problems as a floor) and `harder_problem_set()` (6
symbolic-identity probes that NEED discovered rules to score).
Running the benchmark emits `BenchmarkScored` with `solved_fraction`
+ `delta_from_prior`; the streaming trainer rewards improvement
asymmetrically (+3× gains, −5× regressions — *don't break what
worked*).

| Landmark | Closes |
|---|---|
| `MathProblem`, `ProblemResult`, `BenchmarkReport` | typed labeled-data primitives |
| `canonical_problem_set()` (12 problems) | baseline kernel-solvable floor over nat add/mul, int, tensor, float-tensor |
| `harder_problem_set()` (6 problems) | symbolic-identity probes using `Term::Var(100)` as pattern var; 0/6 empty library, 6/6 with identity rules |
| `solve_problem` / `run_benchmark` | deterministic scoring over a given library |
| `BenchmarkConsumer::benchmark_now(library, downstream)` | scores + emits `MapEvent::BenchmarkScored` into the event bus |
| Asymmetric reward in the streaming trainer | `+3.0 × Δ` for improvement, `−5.0 × |Δ|` for regression |

### Phase V.shed: neuroplasticity on the streaming trainer (2026-04-18)

The policy sheds dead dimensions while it forms new ones. Per-
weight activation counts and cumulative contributions are tracked
for the entire stream. `prune(magnitude_threshold, min_activations)`
zeros weights below both thresholds and marks them pruned; future
updates skip pruned weights entirely. `rejuvenate(index,
initial_value)` reverses the prune — un-pruning a dimension and
re-seeding it so subsequent gradient signal can move it.

This is neuroplasticity: the policy head remains open to novelty
(new dimensions can emerge when rejuvenated) while compressing
away the dimensions that never carried load. Biological neural
networks exhibit the same plastic dynamics; the mathematical
justification is the same — a finite-capacity learner must either
grow or shed to keep pace with a non-stationary reward stream.

| Landmark | Closes |
|---|---|
| `activation_counts` per weight | `RefCell<[u64; LibraryFeatures::WIDTH]>`; incremented whenever `\|v_i\| > 1e-9` contributes to an update |
| `cumulative_contributions` per weight | `RefCell<[f64; LibraryFeatures::WIDTH]>`; integrates `\|w_i × v_i\|` across the stream |
| `pruned` flags per weight | `RefCell<[bool; LibraryFeatures::WIDTH]>` — pruned weights held at 0.0 and skipped on update |
| `weight_stats()` | snapshot of all three arrays for external optimizers / analyzers |
| `prune(magnitude_threshold, min_activations) -> Vec<usize>` | zeros weights below both thresholds; returns newly-pruned indices |
| `rejuvenate(index, initial_value) -> bool` | un-prunes a specific dimension and re-seeds it |
| 5 new tests | prune zeros + marks, pruned skip updates, rejuvenate un-prunes + does-nothing, weight_stats exposes |

**Phase V extension headline findings:**

- **The machine now has a proprioceptive loop**. Map + events +
  certifier + streaming trainer + benchmark + shed together form
  a self-observing, self-rewarding, self-pruning system. Every
  architectural transition is an event; every event carries a
  reward; every reward signal moves weights; dead weights get
  shed while novelty continues to expand the represented space.
  The only external signal is the labeled benchmark — the one
  thing the machine fundamentally cannot know by itself is
  whether its mathematical output matches the world's.
- **Asymmetric reward prevents catastrophic regression**. The
  +3×/−5× weighting means the trainer is slow to accept changes
  that degrade the report card and fast to accept changes that
  improve it. Over the stream, this produces a ratchet: gains
  accumulate, regressions get unwound.
- **Shed + grow over the stream is the plastic regime**. Pruning
  doesn't halt learning — it concentrates capacity. Rejuvenation
  doesn't destroy history — it re-opens a dimension whose
  staleness signal says the environment has changed. Together
  they keep the policy head's representation size tracking the
  learnable signal instead of the feature vector's allocated size.

### Phase W: the perpetual self-optimizing model (2026-04-18)

Four research-grade mechanisms from the continual-learning and
dynamic-sparse-training literature, absorbed into the streaming
trainer. Together they produce a never-off, self-pruning,
self-rejuvenating, stability-aware, intrinsically-motivated
adaptive controller that operates continuously on the live
MapEvent stream.

| Landmark | Closes |
|---|---|
| **W.1 RigL-style phantom gradients** | pruned weights still accumulate \|would-be-delta\|; `auto_rejuvenate(threshold, init)` promotes phantom-active weights back into the active set. The neuroplasticity loop is now fully automatic (Evci et al. 2020) |
| **W.2 EWC-style Fisher stability** | per-weight Fisher EMA; anchor on benchmark improvement; Fisher-weighted pullback on regression-producing events protects load-bearing weights while plastic weights adapt. `ewc_lambda = 0` disables (Kirkpatrick et al. 2017) |
| **W.3 Learning-progress intrinsic reward** | on BenchmarkScored, reward += 4.0 × (current − min(last K scores)). Schmidhuber/Oudeyer intrinsic motivation: the agent is rewarded for improving itself |
| **W.stall Corrupted/stalled pruning** | `prune_dormant_or_corrupted(stall_events, fisher_floor, mag_ceiling)` sheds weights that went silent after being active OR that stay near zero despite high Fisher (pressure-flattened); complements V.shed's dead-at-birth pruning |
| **W.4 EventHub pub/sub + motor translator** | `EventHub` synchronous reentrant-safe pub/sub; `publish_outcome_events(outcome, downstream, staleness_threshold)` translates a `MetaLoopOutcome` into MapEvents on any consumer. The motor now drives the full proprioceptive loop through the hub |

**Phase W headline findings:**

- **The perpetual loop composes end-to-end.** The integration
  test `perpetual_loop_composes_all_phase_v_and_w_mechanisms`
  demonstrates EventHub + StreamingPolicyTrainer + BufferedConsumer
  + BenchmarkConsumer + prune + auto_rejuvenate + EWC anchor +
  Fisher EMA + learning-progress bonus all running in one hub
  subscription. Trained steps monotonic, weights finite,
  no resets, deterministic.
- **The hub is Lisp-morphable-ready.** `MapEventConsumer` is
  object-safe and `&self`; Lisp callbacks can wrap themselves as
  consumers via a thin Rust adapter and subscribe at runtime
  without FFI. Every trainer mutation (`inject`, `prune`,
  `rejuvenate`, `set_ewc_lambda`, `anchor_current_weights`) is
  typed and narrow — direct Lisp bindings land as pure wiring.
- **Determinism preserved throughout.** Two independent trainers
  subscribed to the same hub converge to bit-identical weights
  (`event_hub_is_deterministic_and_non_lossy`). The outcome
  publisher is idempotent across runs
  (`publish_outcome_events_is_deterministic`).

**Kernel invariant status after Phase W:**

All prior invariants preserved. Phase W adds:
- **Monotonic training steps**: `trained_steps` only increases.
  Not reset by `snapshot`, `inject`, `adjust_learning_rate`,
  `prune`, `rejuvenate`, `auto_rejuvenate`, or any `set_*`.
- **Phantom-gradient safety**: phantom gradients stay finite.
  Pruned weights stay at 0.0 until rejuvenated.
- **Hub fan-out equality**: for any sequence of publishes,
  every subscriber receives the same sequence in the same order;
  `published_count == min(subscriber.received_count)` for any
  passive subscriber.
- **Outcome-publish translator determinism**: the same outcome
  published twice produces the same event sequence both times.

**What Phase W unlocks:**

- **Lisp-morphable runtime (Phase W.6).** The Rust infrastructure
  is now shaped exactly as tatara-lisp needs: typed snapshots for
  read, narrow mutation API, reentrant-safe subscribe/publish,
  single-method `&self` consumer trait. A Lisp adapter crate can
  expose the hub + trainer to Lisp with minimal ceremony.
- **Online experimentation (Phase W.5).** The hub makes bandit
  probes trivial to attach: each probe subscribes as a consumer,
  observes event rates, adjusts hyperparameters via the narrow
  mutation API, and measures outcomes through subsequent events.
  The infrastructure is in place; probes are pure policy.
- **Async-ready (Phase W.7).** The current hub is synchronous for
  single-threaded determinism. Swapping `RefCell<Vec<Rc<...>>>` →
  `RwLock<Vec<Arc<...>>>` and wrapping consumers in tokio tasks
  gives the same public API running multi-threaded. No logic
  changes; a drop-in scale-up.

### Phase W.5: online experimentation / bandit probes (2026-04-18)

`BanditProbe<T>` — generic ε-greedy bandit over a finite set of
hyperparameter arms. Subscribes to the hub; on BenchmarkScored,
attributes `delta_from_prior` to the currently-active arm; every
`switch_interval` events, picks the next arm via ε-greedy over
reward EMAs and applies it via a user-supplied closure.

| Landmark | Closes |
|---|---|
| `BanditProbe<T>` | generic probe; constructor takes arms, apply-closure, switch_interval, epsilon |
| SplitMix64 counter-based PRNG | deterministic — two probes seeing the same event sequence pick identically |
| `Plastic` impl (later, W.8) | decays ε toward 0.05; switches to best arm on reinforce |
| `inject` / `set_epsilon` / `set_smoothing` / `set_switch_interval` | runtime tuning surface |

Test pins: 5 unit tests (install-first-arm, attribute-delta,
switch-to-best, deterministic-replay, compose-on-hub-without-interference).

### Phase W.6: task-domain abstraction (2026-04-19)

Closes *"right now we are using math as the only training data."*
`TaskDomain` trait defines a pluggable problem domain
(`Input`/`Output`/`Context`/`solve`/`matches`); the rest of the
system stays domain-agnostic. `MathDomain` is the first
implementation; `SumDomain` (a toy Vec<i64> → i64 with bias)
proves the abstraction generalizes far beyond term-rewriting.

| Landmark | Closes |
|---|---|
| `TaskDomain` trait | object-safe, 4 associated types/methods |
| `Task<D>` / `TaskResult<D>` / `TaskReport<D>` | typed labeled-data primitives |
| `MathDomain` | first impl — wraps existing `eval`/library pipeline |
| `as_math_tasks` | adapter: legacy `MathProblem` → `Task<MathDomain>` |
| `SumDomain` (test-only) | proves domain-agnosticism: different Input/Output/Context types |

Test pins: 5 unit tests (math-domain-name, generic-matches-legacy,
harder-empty-scores-zero, harder-identity-scores-full,
SumDomain-proves-generality).

### Phase W.8: universal Plastic trait (2026-04-19)

Closes *"everything has an algorithm to phase out and choose what
to phase out and choose what to reinforce."* Every adaptive
subsystem implements `Plastic`; an outer `PlasticityController`
ticks shed + reinforce uniformly across heterogeneous components.

| Landmark | Closes |
|---|---|
| `Plastic` trait | `component_name`, `active_count`, `phased_out_count`, `capacity`, `utilization`, `phase_out_stale`, `reinforce_strong` |
| `PlasticityController` | holds `Vec<Rc<dyn Plastic>>`; tick runs shed+reinforce on every registered component; returns `PlasticityReport` |
| `impl Plastic for StreamingPolicyTrainer` | phase_out = dead-at-birth prune + dormant-or-corrupted prune; reinforce = phantom-gradient auto-rejuvenate on top quartile |
| `impl Plastic for BanditProbe<T>` | phase_out = ε decay (sheds curiosity); reinforce = switch to best arm if not current |
| `ComponentTick` / `PlasticityReport` | per-tick metrics |

Test pins: 4 unit tests (controller-ticks-all-components,
tick_count-monotonic, report-summary-format,
utilization-handles-zero-capacity) + 1 integration test
(plasticity_controller_ticks_trainer_and_probe_uniformly).

### Phase W.9: Sexp bridges for live Lisp morphing (2026-04-19)

Closes *"awesome how the lisp system could use this rust
infrastructure in memory, which will be super reliable for the
lisp things morphing at runtime."* The Rust shape was already
Lisp-ready; W.9 lands the Sexp conversions so Lisp can observe
and publish events without the Rust surface changing.

| Landmark | Closes |
|---|---|
| **W.9.1** `map_event_to_sexp` | every MapEvent variant → Sexp; NaN deltas render as `'nan` symbol |
| **W.9.1** `trainer_snapshot_to_sexp` | full StreamingPolicyTrainer state (weights, Fisher, phantom, counts, contribs, pruned flags, benchmark history, anchor, ewc_lambda, etc.) as one Sexp |
| **W.9.1** `plasticity_report_to_sexp` | PlasticityReport including per-component ticks |
| **W.9.2** `map_event_from_sexp` | reverse conversion; malformed Sexp + unknown kinds + missing fields all rejected cleanly |
| end-to-end test | `lisp_publisher_injects_events_into_hub` proves Lisp Sexp → parse → hub.publish → trainer.on_event works |

Test pins: 13 unit tests (7 forward + 6 reverse +
round-trip + malformed-rejection + end-to-end publisher).

### Phase W.10: perpetual-improvement fixed-point demo (2026-04-19)

The closed-loop proof. One integration test composes every Phase
V + W mechanism and demonstrates monotonic measurable improvement
on labeled data:

```
scores:           [0.0, 0.333, 0.5]       ← monotonic ↑
bias trajectory:  [0.0, 0.235, 1.245]     ← trainer learns
trained_steps:    4 (monotonic)
events_seen:      9
has_anchor:       true                    ← EWC active
plasticity ticks: 2
lr picks:         [0.05, 0.2, 0.1, 0.05]  ← bandit tunes live
```

Asserts 8 fixed-point properties:
1. Scores monotonically non-decreasing across cycles
2. Final score strictly > baseline
3. Trainer `trained_steps > 0` (monotonic)
4. `benchmark_history.len() == cycle_count`
5. EWC anchor set on improvement
6. Plasticity controller ran N ticks
7. All weights + bias stay finite (no pathology)
8. Hub fan-out lossless (buffer.len == hub.published_count)

This is the artifact that proves the perpetual-improvement vision
landed: feed the system labeled data and its benchmark score
climbs; feed more and it keeps climbing. **The fixed point of
the design.**

Test pin: `perpetual_improvement_fixed_point_demo` in
`crates/mathscape-core/tests/perpetual_loop.rs`.

### Phase U: Self-tuning meta-loop (2026-04-18)

The outer orchestrator lands. The machine now observes its own
learning, proposes its own next training recipe, and sails out
when the reachable territory for a given seed is exhausted.
Closes the directive arc "observe what we're learning / let the
model tune its own training / leverage Lisp virtualization."

| Landmark | Closes |
|---|---|
| **U.1** (`LearningObservation`) | typed digest of an ExperimentOutcome — total_library_size, seed_library_size, net_growth_per_phase, saturation_phase_index, extract_ns_per_iteration, trained_policy_delta_norm, scenario_total_ns, chain_attestation. Pure projection, no authority |
| **U.2** (`ScenarioProposer` trait) | seam for "emit next scenario from observation history + current policy." `HeuristicProposer` encodes Phase T as a static decision tree (baseline / early-stop-plateau / train-only / extended-discovery) |
| **U.3** (`MetaLoop<E, P>` driver) | executes seed → observes → proposes → executes, ... Terminates on max_phases or sail-out (K consecutive no-progress phases with tiny policy delta). MetaLoopOutcome carries full history + meta-attestation (BLAKE3 over chain of chain-attestations) |
| **U.6** (`AdaptiveProposer`) | SELF-LEARNING proposer. Tracks per-archetype empirical stats (progress rate, rules-per-ms, policy delta), scores archetypes via weighted sum, picks argmax with ε-greedy exploration. Interior mutability for the stats; deterministic replay preserved |
| **arch doc** (`docs/arch/self-tuning-meta-loop.md`) | full Phase U frame, the Lisp/WASI/WASM path for U.5 virtualization |

**Phase U headline findings:**

- **The proposer genuinely picks different archetypes**. Demo
  trace on the default corpus: seed→baseline→baseline→
  extended-discovery→train-only×3 — the heuristic branches fire
  based on observed history, not a fixed loop.
- **Self-encapsulation achieved**. The proposer sees only
  observations (typed projections), not raw scenarios or
  outcomes. The executor sees only scenarios, not the proposer's
  history. Each seam is its own black box. Chaos is in the
  layer, not the interface.
- **Full Lisp-residency within reach**. R10 policy Sexp, R32
  spec Sexp, R33 scenario Sexp, U.1 observation (needs bridge).
  Once the observation Sexp bridge lands, the entire meta-loop
  is a pure `(Sexp, Sexp) → Sexp` function — the exact signature
  of a WASI module. Phase U.5 is the virtualization move.

**Kernel invariant status after Phase U:**
All Phase R + S + T invariants preserved. Phase U adds:
- **Meta-attestation**: BLAKE3 over the sequence of per-phase
  chain-attestations. Stable under identical seed + proposer +
  executor; shifts on any change.
- **Observation purity**: `ExperimentOutcome::observation()` is
  a deterministic projection. Same outcome → same observation
  → same proposer decision → same next scenario.
- **Adaptive determinism**: `AdaptiveProposer` uses a
  count-keyed hash for ε-greedy coin-flips rather than a
  random RNG, so two proposers fed the same observation
  sequence emit identical archetype sequences.

## Where we go next

Ranked by impact-per-effort. Each extension must preserve the lynchpin.

### Phase H: meta-rule diversity + rank-2 inception — LANDED 2026-04-18

**Status: UNBLOCKED + EMPIRICALLY VERIFIED.** Phase I (subterm-paired
AU) surfaces shape-orthogonal candidates; Phase J (empirical validity
on K Nat samples) filters the semantically-wrong ones; Phase H's
strict-subsumption gate admits the rest as distinct meta-equivalence
classes; `MetaPatternGenerator` on the validated library mints a
rank-2 candidate that abstracts across shape families.

**First rank-2 meta-rule observed** (via `phase_h_unblock` test on
a diverse scalar identity corpus):

```
S_30002 :: (?op ?id (?op ?id ?x)) => S_30002(?op ?id ?x)
```

This abstracts BOTH the outer operator (?op ∈ {add, mul}) AND the
nested-identity structure. The validated library it was derived
from contained four concrete laws:

  - `add(0, ?x) = ?x`  (flat add-identity)
  - `mul(1, ?x) = ?x`  (flat mul-identity)
  - `add(0, add(0, ?x)) = ?x`  (nested add-identity)
  - `mul(1, mul(1, ?x)) = ?x`  (nested mul-identity)

Running `MetaPatternGenerator` over those four artifacts produced
2 proposals, 1 of them rank-2 (S_30002, the nested-identity
meta-rule). This is the "inception" signal: a rule whose entire
structure is composed of abstractions the machine itself developed.

**What previously landed (2026-04-18 gate):**
`reduction::detect_subsumption_pairs` enforces the
irreducibility-aware gate: two meta-rules only subsume each other
when one STRICTLY generalizes the other (not pattern-equivalent).
Pinned by `rank2_inception_probe` at the original-pipeline level.

**What landed today (2026-04-18 unblock):**
Phase I's `paired_subterm_anti_unify` + Phase J's
`validate_candidates` together surface the shape-diversity that
MetaPatternGenerator needs. `phase_h_unblock_pipeline_runs_end_to_end`
asserts `rank2_count ≥ 1` — the first Phase-H invariant.

**Post-landing integration** (2026-04-18): the Phase I+J+H
pipeline is now a single public call, not hand-wired demo code.
Public API in `mathscape-compress`:

```rust
derive_laws_validated(corpus, lib, step, min, subterm_depth,
                     k_samples, seed, next_id) -> (rules, stats)
rank2_candidates_from_library(validated, corpus, epoch_id,
                              extract_cfg, symbol_floor) -> Vec<Candidate>
is_rank2_shape(term: &Term) -> bool
```

Pinned by `phase_h_integrated_pipeline_one_call` and
`phase_h_integration_is_deterministic`.

**Phase J.2** (2026-04-18): Phase J's empirical-validity sampler
now auto-detects the rule's algebraic domain from its LHS head
operator and routes bindings through the matching pool:

  0-9 → Nat, 10-19 → Int, 20-29 → Tensor, 30-39 → Float,
  40-99 → FloatTensor, unrecognized → Nat fallback

Before J.2, Int/Float/Tensor rules would falsely fail validation
because the Nat pool's bindings don't type-check through those
operators. After J.2, domain-homogeneous rules across all 5
kernel domains validate correctly. Pinned by
`validation_of_int_identity_uses_int_pool` et al.

**Remaining integration work**: wire the Phase I+J+H pipeline
into the default autonomous-traversal extractor stack so rank-2
rules emerge from the standard traversal without hand-authoring.
The plumbing (public API, tests, determinism) is in place;
hooking it into `BootstrapCycle` / the default `DerivedLawsExtractor`
is a behavior change that warrants a fresh attestation fingerprint.

**What landed.** `reduction::detect_subsumption_pairs` now includes
an irreducibility-aware gate: two meta-rules only subsume each other
when one STRICTLY generalizes the other (not pattern-equivalent). If
they're pattern-equivalent, the arbitrary lower-hash tiebreak is
suppressed — preserves meta-rule diversity for rank-2 anti-unification.

**What DIDN'T happen (and why).** The rank-2 inception probe test
(`rank2_inception_probe`) runs the full zoo, then invokes
`MetaPatternGenerator` over the library. Result: only ONE active
meta-rule survives (`S_10000 :: (?op ?x ?id) => ?x`, the flat
identity-element). All other candidate meta-patterns on this corpus
(nested identity, successor-chain meta variants) got strictly
generalized into S_10000 — legitimate subsumption, not arbitrary
collapse.

This tells us something concrete: **the current corpora produce
meta-patterns that all live in ONE equivalence class after strict
generalization.** For genuinely orthogonal meta-rules to coexist —
which is what rank-2 needs as input — we need either:

- **Phase I (subterm anti-unification)** so meta-patterns can
  express shape at varied depths (e.g. commutativity-shape,
  associativity-shape) beyond root-only matching. Different shapes
  → different equivalence classes → coexistence.
- **Phase J (empirical validity)** so meta-patterns carry semantic
  labels (associative? commutative? idempotent?) that make
  structurally-similar but semantically-distinct rules
  non-subsumable.

The gate is CORRECT. The machinery beneath it needs one of the
below phases to surface enough meta-pattern diversity that the
gate has real work to do.

**Signal for "it landed".** Before phase H, any second meta-rule
would be arbitrarily collapsed into the first. After phase H, if a
future phase I/J produces genuinely orthogonal meta-rules, they
coexist and `MetaPatternGenerator` over the library can mint a
rank-2 candidate that generalizes across them. The gate is a
precondition, not a sufficient condition.

**Deliverables (done).** `is_meta_rule` structural detector in
`reduction.rs`; gate applied only when `pattern_equivalent`;
observational test `rank2_inception_probe` pinned in
`autonomous_traverse.rs`.

### Phase I: subterm anti-unification

**The move.** Anti-unify currently runs at the root of term pairs.
Add recursion into shared subterm positions so patterns can surface
*inside* terms whose roots differ.

**Mechanism.** In `antiunify::anti_unify`, walk both terms in
parallel and record the maximal shared subterm skeleton. When roots
differ, generate a fresh variable — but before that, check if any
child pair shares more than the surrounding context.

**Signal.** The machine finds patterns like `mul(?x, add(?y, ?z))`
(distributivity-shaped) — currently invisible because roots are
different operators.

**Risk.** Combinatorial blow-up on deeply-nested terms. Needs a
subterm-depth cap to stay tractable.

### Phase J: empirical validity check in the prover

**The move.** Before accepting a candidate, evaluate its LHS and RHS
on K random concrete bindings using the built-in evaluator. Reject
if LHS and RHS don't agree numerically.

**Why.** Currently a structurally-general rule like
`(?op ?x ?id) => ?x` subsumes add-identity and mul-identity but is
semantically wrong for most (op, id) pairs. An empirical check
would catch this.

**Tension.** Mathscape's current frame treats library rules as
COMPRESSIONS (renamings) not EQUATIONS. An empirical check forces
the equation interpretation. Deliberate choice needed.

### Phase K: e-graph equivalence saturation (egg) — K1–K4 LANDED 2026-04-18

**Status.** Foundation + dedup wiring + activation probe all green.
Empirical finding: **today's bettyfine is already closed under
commutativity AND associativity.** The probes are correct and wired,
but the machine's anti-unifier + alpha_equivalent collapse has
already reduced every candidate pair that commutativity or
associativity could catch.

**What K1–K3 built.**
- K1: `crates/mathscape-compress/src/egraph.rs` — bridge from
  mathscape's `Term` to egg's `MathscapeLang`, plus
  `check_equivalence(lhs, rhs, rewrites, step_limit)` and the
  canonical `commutativity_probe()` + `associativity_probe()`
  rewrite builders. 7 unit tests.
- K2: `check_rule_equivalence(r1, r2, probes, step_limit)` — rule-
  level equivalence via anonymization-normalized LHS/RHS pairs.
  Strictly more powerful than `alpha_equivalent`: with probes,
  catches commutatively-swapped variants alpha_equiv misses. 4
  unit tests.
- K3: `CompressionGenerator::with_egraph_probes(probes)` — opt-in
  dedup via e-graph. Empty probes = bit-identical pre-K3
  behavior (regression sentinel). 3 adapter tests.

**What K4 probed.** `phase_k_egraph_dedup_probe` (ignored) runs 8
seeds × 4 configs (none / commutativity / associativity / both).
At the default extract config + procedural corpora + ε=0.0 prover
settings: all four configs produce bit-identical library sizes (40
rules), axiomatized counts (18), modal apex (S_10000), fingerprint
distribution (7 distinct across 8 seeds, mode 2/8). Monotonicity
assertion: across 4 additional seeds, probe-enabled totals never
exceed probe-disabled totals — (4,4,4,4), (5,5,5,5), (4,4,4,4),
(7,7,7,7).

**Interpretation.** The bettyfine is the trivial Symbol-naming
fixed point identified in phase M10. Symbol-naming rules of the
form `op(?a, ?b) → S(?a, ?b)` are already alpha-equivalent to
their arg-swapped variants (anonymization canonicalizes var ids),
so commutativity adds no new collapse. Associativity needs
asymmetric nesting in candidates, which the current extract config
+ min-shared-size filters away before AU emits them.

**Next move for real penetration.** Phase K's leverage has to shift
from **dedup** to **prover** or **reinforcement**:
- K5 (prover): accept a rule if it closes a new equivalence class
  in the corpus under saturation — semantic accept, not ΔDL.
- K6 (reinforcement): count a corpus node as "reduced" by rule R
  if any e-graph equivalent of R's LHS matches — inflating R's
  cross-corpus support and accelerating its promotion. This
  changes the bettyfine composition because rules with broader
  *semantic* coverage become more supported.

K5/K6 are architecturally heavier than K1–K4. The K1–K4 chain is
the bench they'll be built on.

### Phase L5 — EDGE-RIDING CONFIRMED (2026-04-18)

**Milestone**: 50-cycle perpetual discovery, 800 theorems, zero stalls.

Run: `cargo test -p mathscape-axiom-bridge --release --test
edge_riding edge_riding_loop -- --ignored --nocapture`

Result:
- 271.9s wallclock, 50 cycles, 800 theorems
- 16 theorems per cycle (maxed RUSTIFY_TOP_K=16 every cycle)
- Substrate: 0 → 12 rules (slow, equivalences don't enter)
- Ledger: 0 → 800 rules (constant 16/cycle pace)
- Correctness check: zero post-bootstrap zero-novelty cycles

The correctness criterion — "any halt is a bug by modus tollens
from Gödel's incompleteness" — held across the full run. The
machine never ran out of theorems to find; it just ran out of
allotted budget per cycle.

What this establishes:

- **Perpetual discovery is implementable.** The L0-L8 stack
  (primitives → bootstrap → enumeration → ledger → composition →
  validation → substrate → adaptive corpus → outer loop) is
  sufficient to generate nonzero novelty indefinitely on the
  current operator basis `{zero, succ, add, mul}`.
- **Self-bootstrapping works.** No hand-coded candidate
  associativity / distributivity / any specific equation. Every
  candidate comes from (bootstrap set) ∪ (term enumeration) ∪
  (ledger composition). The ledger-composition pass is what
  keeps the candidate count growing as the ledger grows.
- **Halt-is-a-bug is load-bearing.** Prior runs stalled at
  layer 1, 2, 3, 4 — each halt pointed at a specific mechanism
  gap (hand-picked candidates, size-3 enumeration, size-5
  enumeration without composition). Fixing each gap pushed the
  machine deeper. At layer 4+ with size-5 + composition cap 30,
  the machine runs the full 50 cycles.

Canonical apex: not a single fingerprint anymore — the ledger
is 800 entries. But the substrate's 12 entries are the
reduction-core:

```
[0] mul(1, x) → x
[1] add(0, x) → x
[2] add(x, 0) → x
[3..8] add-mul compositional identities (6 variants of
       add(mul(x,1), mul(y,1)) → add(x, y) and relatives)
[9] mul(x, 1) → x
[10..11] succ-distribution variants:
    add(mul(x, y), x) → mul(x, succ(y))
    add(mul(x, y), x) → mul(succ(x), y)
```

Rank-2 substrate discoveries at [3-8, 10-11]: the machine
found cross-operator + successor-distribution rules
autonomously.

To push further: raise `RUSTIFY_TOP_K`, raise `CYCLES`, raise
`max_size` to 6+ (reaches true associativity), add higher-order
compositional layers. Each knob is independently informative.

### Phase L: adaptive corpus generation

**The move.** Use the current library to construct corpora that
specifically probe what the library cannot yet reduce at root. The
machine generates its own next frontier.

**Why.** Procedural corpora at scale 100k already saturate in 5
steps. To advance the frontier the corpus distribution needs to
PROBE the gap.

**Prerequisites.** Phase H and J help first — more meta-rules to
sense gaps, empirical check to avoid false probes.

## Regressions to watch for

Each landmark above ships with regression tests. The machine has ONE
canonical fingerprint; deviation from it is either a new landmark or
a bug.

| Signal | Likely cause |
|---|---|
| Lynchpin violated | New generator bypasses forest attestation |
| 0 Axiomatized rules | W-window too wide OR reinforcement broken |
| Apex fingerprint changed | New capability (update this doc) OR silent bug |
| Determinism broken | HashMap iteration leak, float order dependency |
| Per-corpus cost not flat at 100k | Memoization regressed OR forest leaking |
| Saturation step > 10 | Zoo diversity broken OR corpus-artifact minting |

## Update protocol

Every phase listed above lands with:
1. A new `docs/arch/*.md` describing what it unlocks, depends on,
   and how it can fail
2. A flex test pinning the new invariant
3. An update to this landmarks doc moving the phase from "where we go
   next" to "where we've been"
4. An apex fingerprint update if the machine's Axiomatized set shifted
5. Update to `mathscape-traverse` skill if user-facing invocation
   changed

The discipline is simple: **no new capability lands until it passes
the orchestrated suite AND updates this map.** The map is the
machine's memory of itself.
