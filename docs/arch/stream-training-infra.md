# Stream-training infrastructure — destination and path

**Status:** step 1 built 2026-10-15 (`crates/mathscape-stream`, see
[Built](#built-2026-10-15)); the first-epoch saturation it exposed is
broken by ML4.2 (see [Saturation](#saturation-broken-then-the-second-plateau-2026-10-15));
step 3 is declared for ryn's engenho (see [On ryn's engenho](#on-ryns-engenho-2026-10-15));
steps 2 and 4 remain. The "today" table below was
measured 2026-10-14, before step 1.

## Where the machine actually is

| Claim the directives make | What the code does today |
|---|---|
| One canonical model grows continuously | No binary runs the streaming trainer or the meta-loop. Every `StreamingPolicyTrainer::new` / `MetaLoop::new` call is under `#[cfg(test)]` or `crates/*/tests/`. |
| Deterministic, replayable | `mathscape-service` seeds from entropy (`mathscape-service/src/main.rs:573,653`). Only the meta-loop is seed-deterministic (`meta_loop.rs:20`). |
| No checkpoints, only an append-only stream | Model state is `ModelSnapshot` (library + `TrainerSnapshot`, BLAKE3 content hash, bincode "MSCP" file, `snapshot.rs:62-197`). It is content-addressed, so it is a hash-keyed object, not a throwaway checkpoint. The service never writes one. |
| Distributed workers | The trainer, `EventHub` and `LiveInferenceHandle` are `Rc<RefCell<…>>`, not `Send` (`streaming_policy.rs:38`, `inference.rs:29`, `mathscape_map.rs:215`). The only measured parallelism is independent seeded probes: ~10x on 14 cores, 100k probes in ~2 min (`dense_campaign.rs`). |
| Object store, lease bus | No S3, NATS or lease dependency in any `Cargo.toml`. |
| Deployed | No HelmRelease for mathscape exists in any pleme-io repo; the chart sets env vars the binary never reads (`DATA_DIR`, `DATABASE_URL`). |

So the bottleneck is not compute. It is that there is no long-running process
that streams, persists and resumes. Spot capacity bought today would run a loop
that loses everything on eviction.

## Destination

One writer, many stateless scouts, content-addressed everything:

```
scouts (N, stateless, seeded)          canonical model (1 writer)
  seed s, scenario k  ── epoch-delta ──▶  apply delta (idempotent by hash)
  discover, reduce, certify locally       StreamingPolicyTrainer.on_event
  emit (epoch-delta …) S-expressions      every K deltas: ModelSnapshot
                                          ──▶ objects/{h[0:2]}/{h[2:]}
                                          HEAD ← snapshot hash
```

- **Unit of distribution: the seeded scenario.** It is the only unit proven
  embarrassingly parallel, and a lost scout costs one replay of a seed, never
  model state. The single writer keeps the `Rc` trainer as it is; its update
  per event is cheap, and discovery (the expensive part) runs in scouts.
- **Wire format: `(epoch-delta …)`**, built from the existing lossless
  `map_event_lisp` stream. Every `rule-certified` already carries its
  `(invariant-def …)` (`translation.md`), so a delta is self-describing in
  human mathematics. Merging is by rule content hash: applying a delta twice
  is a no-op.
- **State: content-addressed objects + one mutable pointer.** Snapshots and
  deltas are written under their BLAKE3 hash; only `HEAD` moves. Rewind is
  moving `HEAD`; fork is starting a writer from an older hash
  (`fork_from_snapshot` exists).
- **Counter-force for every expansion:** scouts expand; the writer's
  certification gate, `DemotionGate` and `TrapDetector` contract. A scout
  whose seed produced no certified delta within its budget is not
  rescheduled (the eviction is the counter-force on compute).
- **Interruption:** a scout needs no 120-second drain. It holds no state the
  seed cannot regenerate, so eviction is `kill -9`-safe by construction. Only
  the writer needs a durable target, and it writes after every K deltas.
- **Cost signal:** dollars per certified invariant = substrate cost over the
  window ÷ count of `rule-certified` deltas applied. That's the
  `CostBudget` promessa template in `breathe-catalog/src/cost.rs`, given its
  first live input.

## Substrate

| Option | For | Against |
|---|---|---|
| **rio first** (32 threads, owned) | $0 marginal; the measured workload is CPU-bound and fits; one writer + ~28 scouts on one box needs no network object store (local disk, same hash layout) | One machine; no burst |
| AWS spot burst (c7a/m7a via arch-synthesizer `SpotWorkloadDecl`) | Elastic scouts; spot suite gets its first consumer | No EKS cluster since 2026-08-27; Karpenter, bucket and cluster all to build; spend before the runner has produced one certified delta |

Because scouts are stateless and the object layout is the same on disk and in
S3, rio-first is not a detour: the AWS tier adds scouts and swaps the object
backend, nothing else.

## Built (2026-10-15)

- **The motor is a library.** `mathscape_compress::Motor` (+ `Extractor`,
  `UnknownCorpus`) replaced thirteen hand-copied `ScenarioExecutor`s in
  `mathscape-axiom-bridge/tests`; their unknown-corpus behaviours (refuse,
  fall back to core, skip) are the three `UnknownCorpus` values.
  `first_model.rs` keeps its own executor: it dispatches on the extractor
  name, a different shape.
- **`mathscape-stream`** (lib + bin). Config is the `stream:` section of
  `mathscape-config` (`seed` required, no entropy; env
  `MATHSCAPE_STREAM__*`). Each epoch is a scout step
  (`Writer::discover`: the motor seeded by `epoch_seed(seed, epoch)`) then
  a writer step (`Writer::apply`: `CoreGrew` through
  `CertifyingConsumer<DefaultCertifier>` into the `EventHub` the
  `StreamingPolicyTrainer` listens on). Certified rules join the library;
  rejected ones become tombstones, never retried. A changed epoch is an
  `(epoch-delta …)` line in `deltas.lisp` and a content-addressed object.
- **State on disk.** `objects/{h[0:2]}/{h[2:]}` (snapshots under their
  content hash, deltas under the hash of their bytes), `HEAD` (hex), each
  written temp-then-rename with fsync. A snapshot is taken every
  `snapshot_every` certified deltas, every `checkpoint_epochs` epochs, and
  at the budget (`until_epoch`). Snapshot metadata carries seed, next
  epoch, parent HEAD and tombstones, so `Writer::open` resumes exactly.
- **Replay-stable.** `snapshot_handle_at` takes the clock; the stream's
  default `clock: logical` stamps the epoch, so the same seed and config
  give the same hash chain, and a writer killed mid-run converges on the
  uninterrupted chain after resume (`tests/stream.rs`). `clock: wall`
  keeps the old behaviour.
- **Fixed on the way:** `TrainerSnapshot.anchor_bias` was always 0 (now
  read from the trainer), and the term S-expression codec emitted
  `tensor-opaque` / `fn-opaque`, so a tensor rule's `rule-certified`
  event did not read back; both now round-trip.
- **Chart:** env now names what the binaries read
  (`MATHSCAPE_PERSISTENT_PATH`, `MATHSCAPE_DATABASE__URL`); `stream.seed`
  renders the writer's env, run with `command: ["/bin/mathscape-stream"]`
  (the engine image now carries the binary).

**First measurement (this Mac, 14 cores, release, seed 2026, defaults):**
20,000 epochs (plus 62 replayed) in 45.7 s of wall time across a SIGKILL
at epoch 7,678 and a resume from HEAD at epoch 7,616. 2 certified invariants, both in epoch
0; 189 rejected candidates tombstoned; zero certified after epoch 0. So
certified invariants per hour is ~158 over this run and **0 at steady
state**: with this motor configuration discovery saturates in one epoch.
The bottleneck is the motor (corpus and extractor), not compute; buying
spot capacity now would buy more sterile epochs. ML4's mechanism mutation
is the lever.

## Saturation broken, then the second plateau (2026-10-15)

**Why 0.1.7 stopped after epoch 0.** Measured, not guessed:

- The `default` corpus is a fixed tensor zoo with no seed; its two
  identity laws are fully extracted in epoch 0.
- The `adaptive` corpus emits random whole Peano terms. The
  derived-laws extractor anti-unifies whole input→normal-form traces,
  so two random terms generalize to `?x → n` (a constant), which the
  certifier rejects: every later candidate was one of those, tombstoned.
- Each epoch builds a fresh `MetaLoop`, so its proposer never sees a
  history and its staleness branch never fires. Nothing mutates.

**What changed** (ML4.2, `ml4-mechanism-evolution.md` Status):
each epoch also feeds a slice of the `Frontier` (observational-
equivalence equations over `MechanismConfig`'s candidate vocabulary),
an entailment gate skips anything the library already proves, and an
exhausted frontier plus `plateau_epochs` dry epochs triggers
`respond_to_saturation`. `frontier.enabled: false`,
`mechanism.respond: false`, `novelty: off` is exactly 0.1.7
(`tests/stream.rs` pins it).

**Measurement** (this Mac, release, defaults, two seeds run
concurrently, 30-minute cap; both hit the cap, so rates are from the
epoch log via duckdb):

| | seed 7 | seed 2026 |
|---|---|---|
| epochs in 30 min | 1,146 | 2,808 |
| certified, total / after epoch 0 | 188 / 178 | 161 / 151 |
| epochs that certified (after 0) | 87 | 84 |
| last certifying epoch (minute) | 953 (18.1) | 1,178 (8.8) |
| **certified/hour after epoch 0, until the last certification** | **590** | **1,029** |
| certified/hour after epoch 0, whole 30 min | 356 | 302 |
| mechanism mutations / saturated responses | 22 / 4 | 23 / 10 |

Library growth by 100-epoch bucket, seed 7: 45, 67, 104, 114, 124,
136, 158, 161, 170, 188, then flat. Seed 2026: 47, 64, 71, 87, 96, 111,
123, 128, 135, 137, 147, 161, then flat for 1,600 epochs. What it
found, in order: the Nat commutative semiring and its succ laws, then
one domain per mutation as each evaluator operator joined the
vocabulary (float add/sub/mul/div/neg, Int, tensor and float-tensor
elementwise ops): identities, inverses (`x − x = 0`,
`x + neg x = 0`), double negation, commutativity, associativity,
negation through multiplication. Mutations were almost all
`add-candidate-vocab-op`, plus constants 2 and 3.

**The second plateau, named.** Both seeds end mechanism-saturated
with `candidate_max_size` still 5 and 23 of the evaluator's 30
operators of arity 1–2 in the vocabulary. The other seven (tensor
scale, matmul, transpose, reshape, float-from-int, float-tensor scale
and matmul) take or return a second sort or a 2-D shape, which the
single-sort frontier cannot build, so their trials score 0. The size lever is shut: an early
`(bump-candidate-max-size 1)` trial found nothing new inside its
256-equation budget, so `respond_to_saturation` graveyarded it, and the
graveyard is keyed on the mutation alone, never on the base config it
failed against. It is never retried, even after the vocabulary has
grown from 3 operators to 23. Rates are therefore 0/hour at steady state again, one
level up: the frontier at size ≤ 5 over this vocabulary is exhausted
(seed 2026: cursor 8,063/8,063 from epoch ~1,180). Responses then
back off (window doubles per failure: 4, 8, 16 … epochs), so the
sterile tail costs little.

The next lever was in the mechanism layer, not compute; it is the
section below.

## The second plateau opened (2026-10-15, v0.1.9)

**Re-measured first**, on 0.1.8's own plateau state (seed 7, HEAD at
epoch 1,145, library 188): the graveyard held `(bump-candidate-max-size 1)`
and `(add-candidate-vocab-op 11)` keyed on the mutation alone; op 11
had only got back in by riding a compound whose other half
(`set-corpus-seed-from-theorems`) was inert. Re-building that state's
frontier at larger sizes and scanning past the size-5 tail:

| max size | equations | build | scanned past tail | non-entailed | certified | first novel at |
|---|---|---|---|---|---|---|
| 5 | 8,083 | 74 ms | 6,000 (from 0) | 0 | 0 | — |
| 6 | 26,826 | 210 ms | 6,000 | 4 | 4 | 2,901 |
| 7 | 140,577 | 1.1 s | 6,000 | 2 | 2 | 1,399 |

So size 6 held novel laws, but the first one sat 2,901 equations past
the tail; a 256-equation trial could never reach it. The entailment
gate costs ~6.7 ms per equation and dominates a trial.

**What changed:**

- `mechanism.graveyard: context` (default): a failed mutation is
  recorded with the context it failed in, `blake3(config ‖ library
  root)`, and retried once either moves. The context is logged as
  `:context` on every `(mechanism-delta …)` and persisted with the
  pool. `mutation` keeps 0.1.8's mutation-only key.
- `mechanism.trial-scale: opened` (default): a size bump trials every
  equation it opens, `trial_budget ≤ opened ≤ trial_ceiling`
  (32,768); `trial-stop-after: 1` ends a trial at its first certified
  law. `fixed` and `0` restore 0.1.8.
- `mechanism.drive-motor: true` (default): the motor's corpus
  (`corpus_*`), extractor (`extract_min_matches` → `min_support`, then
  `extract_min_shared_size` and `extract_max_new_rules` as filters) and
  semantic projections (`candidate_max_size`, `composition_cap`,
  `validator_*`) are read from `MechanismConfig`, and a mutant that
  moves them gets a motor trial. `StreamMechanism::v0_1_8()` is the
  whole prior surface; `tests/stream.rs` pins it.
- **Certifier soundness.** Driving the motor surfaced `(?f ?x) → ?x`
  ("every function is the identity") certifying against any library
  that holds a double-negation law: a sampled value in head position
  reduces through it. `DefaultCertifier` now refuses a pattern
  variable in operator position by name; every other rule is judged
  as before. Neither 0.1.8 library held one (0 of 188, 0 of 161).

**Measurement** (release, two seeds concurrently, 30-minute cap; both
hit it; duckdb over the epoch logs):

| | seed 7 | seed 2026 |
|---|---|---|
| epochs in 30 min | 1,388 | 1,635 |
| library at 30 min (0.1.8) | 173 (188) | 146 (161) |
| certified after epoch 0 | 163 | 136 |
| last certifying minute (0.1.8) | **26.8** (18.1) | **26.4** (8.8) |
| certified/hour after epoch 0, whole 30 min (0.1.8) | 354 (356) | 272 (302) |
| mutations / saturated responses (0.1.8) | 21 / 0 (22 / 4) | 18 / 0 (23 / 10) |
| size bump adopted at epoch | 518 | 305 |

Certified per 5-minute window, minutes 0–30:

| | 0–5 | 5–10 | 10–15 | 15–20 | 20–25 | 25–30 |
|---|---|---|---|---|---|---|
| seed 7, 0.1.8 | 126 | 25 | 25 | 2 | 0 | 0 |
| seed 7, 0.1.9 | 115 | 20 | 2 | 17 | 7 | 2 |
| seed 2026, 0.1.8 | 125 | 26 | 0 | 0 | 0 | 0 |
| seed 2026, 0.1.9 | 76 | 30 | 10 | 8 | 0 | 12 |

The dead tail is gone: no saturated response in either run, and both
still certify past minute 26. At the 30-minute mark the libraries are
8–9% smaller, because the size bump is now adopted early and every
later vocabulary addition rebuilds a size-6 frontier, about three
times longer, from cursor 0. Every law certified after epoch 0 came
from the frontier: the driven motor and its projections added
nothing beyond the two epoch-0 laws in any of the four runs.

**Where it plateaus next, and why.** Not within 30 minutes. Two
measured limits bound it:

- Frontier density falls with size: about 4 certified per 6,000
  non-known equations at size 6 and 2 per 6,000 at size 7, all through
  a ~6.7 ms entailment check. At 64 equations an epoch the frontier
  supplies roughly 0.04 laws per epoch at size 7. Seed 7 ended with its
  size-6 frontier nearly walked (cursor 19,136 of 19,184).
- Canonical fitness takes the first mutant with any yield, so a
  sparse size bump can win over a dense vocabulary addition. A
  yield-per-equation fitness (ML4.3's diagnostics) would order them,
  and memoizing entailment verdicts would make a rebuild from 0 cheap.
  Neither is built.

## On ryn's engenho (2026-10-15)

**Where it runs.** ryn's engenho runs pods as native host processes
from Nix closures (`kubeletBackend: native`), so the writer ships as the
`mathscape-stream` closure, not an image: no registry, no push, no spend.
nix `pleme.mathscape.stream` renders this repo's chart (`engine.enabled:
false`, `stream.enabled: true`, seed 7) with substrate's `mkHelmRender`
into `/etc/engenho/manifests.d/mathscape-stream.yaml`; engenho's
node-manifests driver applies it and prunes it when the declaration
goes. The model lives in `~/.local/share/mathscape/stream` (a hostPath
mounted at itself, inside ryn's `hostPathAllowlist`). Chart values and
refusals: [deployment.md](deployment.md#stream-writer-streamenabled).

**Health surface.** `stream.health_addr` serves `/healthz`, `/readyz`
(503 until HEAD is open) and `/metrics` (epoch, library rules,
certified/rejected/candidate counters, mechanism mutations and
saturations, frontier cursor and size, and the timestamps of the last
epoch, snapshot and certification). The chart probes it and, where
VictoriaMetrics runs, scrapes it and alerts on Down, Stalled,
SnapshotStale and Plateau.

**The slice, measured** (this Mac, release, seed 7, one process per
run, `/usr/bin/time -l`; CPU is user+sys over wall):

| code, mechanism | run | epochs | CPU | max RSS | frontier |
|---|---|---|---|---|---|
| 0.1.8, default | 5 min | 568 | 0.93 core | 337 MiB | 5,024 |
| 0.1.8, default | 10 min | 780 | 0.95 core | 1.0 GiB | 7,038 |
| 0.1.8, default | 20 min | ~990 | 0.96 core | 2.6 GiB | 8,083 |
| 0.1.8, default | 40 min | 1,346 | 0.94 core | 2.6 GiB | 8,083 |
| 0.1.9, default | 20 min | 1,155 | 0.96 core | 0.43 GiB | 19,184 |
| 0.1.9, default | 40 min | 1,670 | 0.97 core | **12.3 GiB** | 21,296 |
| 0.1.9, `v0.1.8` profile | 20 min | 1,001 | 0.96 core | 2.6 GiB | 8,083 |
| 0.1.9, `v0.1.8` profile | 40 min | 1,389 | 0.97 core | 2.7 GiB | 8,083 |

CPU is one core whatever the mechanism. The memory reading in this
table was wrong about its cause: it is not the frontier, and the 12.3
GiB is a transient peak inside a mechanism trial, not growth. The cause
and the bound are in [Memory bounded](#memory-bounded-2026-10-16).

**ryn ran the `v0.1.8` profile until 2026-10-16** (`pleme.mathscape.stream.mechanism`,
which sets `mechanism.graveyard: mutation`, `trial_scale: fixed`,
`trial_stop_after: 0`, `drive_motor: false`): a darwin engenho has no
cgroups, so no limit is enforced there, and the profile is the bound
that holds. That is 1 of ryn's 14 cores and ~2.7 of its 48 GiB. The
alert layer and breathe bands are off there, like the pangea stack and
the llm server beside it, because neither VictoriaMetrics nor
breathe's CRDs run on that cluster; `/metrics` is the observation
surface until they do. Since 2026-10-16 ryn runs the default
mechanism, its memory bounded ([below](#memory-bounded-2026-10-16)).

**Live on ryn (2026-10-16 00:14Z).** Applied by switching ryn to the
pushed system (its closure diff against the running one was exactly
`mathscape-stream` and its manifest); engenho's node-manifests driver
created `mathscape/mathscape-stream-0`, Ready, `/readyz` 200 on
127.0.0.1:9473, HEAD advancing (`d8d6f2…` → `005957…` → `ee1025…`).
Deleting the pod brought it back with `resumed from HEAD
head=005957… epoch=137` and `mathscape_stream_resumed 1`; the 5 epochs
since that snapshot replayed. **It runs at ~8% of a core, not one**:
engenho's LaunchDaemon (`io.pleme.engenho.daemon`) is `ProcessType:
Adaptive`, and its native children inherit that throttled QoS, so the
writer does ~6 epochs a minute where an unthrottled process does ~50.
Certified invariants per hour on ryn: ~120 after epoch 0 in the first
21 minutes, ~73 in the 9 minutes after the resume, against 590 for the
same seed unthrottled. The slice is smaller than the chart asks for;
raising it is the daemon's `ProcessType`, a node-wide choice for every
engenho workload.

**The 0.1.8 20-minute run, scraped once a minute from `/metrics`:** library 76 at
minute 1, 136 at 5, 161 at 9, 186 at 14, 188 at 18, then flat;
22 mechanism mutations. That is the second plateau above, reproduced
through the new surface.

## Memory bounded (2026-10-16)

**Cause, measured.** The peak lives in the motor's corpus, not the
frontier. `Frontier::build` with ryn's 20-operator vocabulary peaks at
22 MiB at size 5, 52 MiB at 6, 272 MiB at 7, 680 MiB at 8 (the clamp's
ceiling). A bisect of the four 0.1.9 mechanism switches (eight runs,
seed 7, RSS every 20 s) showed every profile spiking, the default
highest (4.7 GiB at minute 34, back to 69 MiB a minute later), so the
growth is a spike, not a leak. Replaying the default stream from its
epoch-1347 snapshot reproduced a 12.9 GiB peak; per-trial logging put it
in two motor trials of epoch 1388's saturation response (157 s and 58 s)
whose corpus vocabulary had lost operator 4, and the motor alone on
that input took 163 s and 7.6 GiB. A call-stack sample put the time in
`derive_laws_with_cache` → `eval`, and a probe in `eval` showed the
input terms themselves at 1.2M and 2.5M nodes.

`AdaptiveCorpusGenerator` nests to `base_depth + library.len()/3`, and
each node stops with probability 1/3, so a vocabulary of binary
operators is a supercritical branching process: expected term size
grows exponentially in library size. Measured over 256 terms per cell
(base depth 4):

| corpus vocab | library | p90 nodes | max nodes |
|---|---|---|---|
| 2-3-1 (v0.1.8's fixed corpus) | 30 / 120 / 180 | 133 / 6,092 / 48,664 | 531 / 26,275 / 179,028 |
| 2-3-5-7-6 (a default-mechanism trial) | 30 / 120 / 180 | 343 / 92,805 / 4,396,990 | 871 / 254,197 / 10,381,902 |

**Fix.** Every corpus term has a node budget (`max_nodes`, default
2048, `stream.corpus_max_nodes`), threaded through the residue, shell
and rule-instantiation builders: an application is built only while the
budget can still close it, and a rule's pattern variables share the
budget by occurrence count. A corpus that never reaches the budget is
generated exactly as before, and `usize::MAX` restores the unbounded
generator. Pinned by `a_large_library_cannot_grow_a_corpus_term_past_the_node_budget`
(red without the budget: a 9,255-node term) and
`a_large_library_keeps_the_motor_corpus_inside_its_node_budget`
(150-rule library, debug build: 3 s and 0.7 GiB; budget off, 308 s and
4.0 GiB).

**After** (release, three streams at once, 65 minutes, RSS every 5 s):

| run | peak RSS, min 0-30 | peak RSS, min 30-65 | epochs | library | certified after epoch 0 | last certification |
|---|---|---|---|---|---|---|
| default, seed 7 | 136 MiB | 526 MiB | 2,027 | 216 | 206 | minute 62.6 |
| default, seed 2026 | 134 MiB | 437 MiB | 2,581 | 159 | 149 | minute 43.8 |
| v0.1.8, seed 7 | 119 MiB | 117 MiB | 73,541 | 188 | 178 | minute 6.8 |

The minute-40 peaks are size-7 trial frontiers (272 MiB alone) and fall
back by minute 60 (246 and 289 MiB). At 30 minutes the default reaches
186 rules for seed 7 and 147 for seed 2026, against 173 and 146 under
0.1.9; seed 7 passes 0.1.9's 188-rule ceiling and is still certifying
at minute 62. The v0.1.8 profile, no longer spending minutes on
million-node corpora, reaches its 188 rules in 6.8 minutes instead of
about 18. The chart now asks 768Mi and limits at 2Gi.

## Build order

Donated compute and the commit ledger the stream grows into: [model-ledger.md](model-ledger.md).

1. ~~`mathscape-stream` binary: explicit seed in config, the writer loop
   (EventHub → trainer → certification → snapshot every K), `HEAD` file,
   resume from `HEAD` on start. Fix the chart to the env the binary reads.~~
   Built 2026-10-15.
2. Scout mode of the same binary: `--seed --scenario`, emits
   `(epoch-delta …)` on stdout or to the writer's socket.
3. ~~Run under engenho~~ Declared for ryn's engenho 2026-10-15
   (`pleme.mathscape.stream`, seed 7; the default mechanism since
   2026-10-16): one writer, one core, under 0.6 GiB, $0. Measure certified-invariants/hour from its `/metrics` once it is
   running; rio stays the scout box for step 2.
4. Only when rio is saturated: the spot scout tier and an S3 object backend
   through arch-synthesizer's spot suite.
