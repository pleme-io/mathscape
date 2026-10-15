# Stream-training infrastructure — destination and path

**Status:** step 1 built 2026-10-15 (`crates/mathscape-stream`, see
[Built](#built-2026-10-15)); steps 2–4 remain. The "today" table below was
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

## Build order

1. ~~`mathscape-stream` binary: explicit seed in config, the writer loop
   (EventHub → trainer → certification → snapshot every K), `HEAD` file,
   resume from `HEAD` on start. Fix the chart to the env the binary reads.~~
   Built 2026-10-15.
2. Scout mode of the same binary: `--seed --scenario`, emits
   `(epoch-delta …)` on stdout or to the writer's socket.
3. Run on rio under engenho or systemd; measure certified-invariants/hour
   and $/invariant.
4. Only when rio is saturated: the spot scout tier and an S3 object backend
   through arch-synthesizer's spot suite.
