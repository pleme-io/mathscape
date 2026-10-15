# Model ledger: one model, every change a commit, any donor anywhere

**Status:** DESIGN, written 2026-10-15. No ledger code exists yet; this doc
records what already exists that it stands on, what is missing, and the
order to build it. Citations into mathscape are pinned to `3fc66ec`
(v0.1.8); tiers are BUILT (code + tests on main), PARTIAL (code exists but
does not yet carry the claim), DESIGN (prose only).

This is a descriptive doc, not a new primitive, so it mints no name. The
pieces it names that become code get named through the `naming` skill when
they land.

## 1. Destination

mathscape grows one model. Not one per run, one per machine or one per
experiment: one canonical Lisp model whose every change is a commit, the way
a git repository is one history. The ledger gives that model five
properties:

- **Persistent.** Training never restarts from zero. Every certified rule,
  demotion and mechanism change lands on top of what was there.
- **Reversible.** Any change can be undone by a later commit that says so;
  nothing is rewritten.
- **Time-travelling.** Any past state can be checked out by hash and run.
- **Reroutable.** Any past state can be branched with a different seed or
  mechanism and grown in a new direction, then merged back.
- **Open to donors.** Anyone with spare compute (a laptop, a cluster, a spot
  node, a third party we have never met) can search for rules and submit
  what they find. The model absorbs what is true and refuses what is not,
  without trusting who sent it.

The one load-bearing fact that makes the last property safe is an
asymmetry: **finding a rule is expensive and checking one is cheap.** The
frontier, the mechanism trials and the e-graph saturation are the cost.
Re-evaluating a proposed equation on fresh samples is microseconds. So the
search can be handed to anyone, and the check never is.

## 2. What exists today

| Piece | Where | Tier | What it already gives the ledger |
|---|---|---|---|
| Content-hashed model snapshot | `ModelSnapshot` `mathscape-core/src/snapshot.rs:93`; hash `snapshot.rs:152` (BLAKE3 over version, clock, library, trainer, metadata); verified on load (`load_from_path` `snapshot.rs:180`) | BUILT | A tree object: the whole model at one point, addressed by hash |
| Replay-stable clock | `StreamClock::Logical` default (`mathscape-config/src/lib.rs:52`) | BUILT | Same seed gives the same hash chain, so commits are reproducible |
| Fork from any snapshot | `fork_from_snapshot` `snapshot.rs:294` | BUILT | Checkout: a live, independent model from any hash |
| Object store + `HEAD` | `mathscape-stream/src/store.rs:13` (`objects/{h[0:2]}/{h[2:]}`, temp-then-rename, `HEAD` staged and committed at `:178-190`) | BUILT | Git's object directory and one ref, kill-9 safe |
| Parent pointer | `stream.parent` metadata key (`writer.rs:25`, written at `writer.rs:581`) | PARTIAL | A commit chain exists, but the parent is a metadata string inside the tree, not a first-class commit field, and nothing walks or verifies it |
| Delta log | `EpochDelta` (`delta.rs:107`: epoch, seed, parent root, new root, events) and `MechanismDelta` (`delta.rs:24`) appended to `deltas.lisp` | BUILT | The per-commit change set, as Lisp |
| Idempotent merge by rule hash | `Writer::merge` `writer.rs:409`; `rule_content_hash` `certification.rs:138` | PARTIAL | Re-applying a delta is a no-op, but `merge` **trusts** the delta's `RuleCertified` events and pushes the rule without re-checking it (`writer.rs:415-421`) |
| Tombstones | rejected rule hashes persisted (`writer.rs:26`, `:390`) | BUILT | A refused rule is never re-tried or re-admitted |
| Certification levels | `CertificationLevel` `certification.rs:68` (Candidate → Validated → ProvisionalCore → Certified → Canonical); `DefaultCertifier` `:182` | PARTIAL | The gate exists, but its samples are public and tiny (section 4) |
| Proof status lattice + demotion | `ProofStatus` `lifecycle.rs:106` (Proposed … Axiomatized, Demoted, Subsumed); `DemotionGate` `demotion.rs:34`; `demote_artifact` `demotion.rs:109`; `docs/arch/demotion-pipeline.md` | BUILT | Revert already has its semantics: demotion keeps the entry and excludes it |
| Equational proof object | `prove_equational` `mathscape-proof/src/lib.rs:101` | PARTIAL | A normal-form proof, but relative to a context library that may itself contain unproven rules |
| Lean export | `export_rule_as_lean` `mathscape-proof/src/lean_export.rs:54` | PARTIAL | Theorem statements only; the proof body is `sorry` |
| Library root | `library_merkle_root` `bootstrap.rs:846` | PARTIAL | Order-independent hash of the library, but a sorted concatenation, not a tree: no inclusion proofs |
| Meta-attestation | `MetaLoop` `meta_loop.rs:670-681` (BLAKE3 over per-phase chain attestations) | BUILT | Precedent for hashing a run's whole history into one value |
| Benchmark signal | `MapEvent::BenchmarkScored` `mathscape_map.rs:117` | BUILT | The regression signal bisect needs |
| Attestation chain | tameshi `HeartbeatChain` `src/heartbeat.rs:184` (hash-linked entries, `verify_integrity` `:285`) | BUILT, public, crates.io 0.1.5 | The signed, append-only log of `HEAD` moves |
| Append-only proof | tameshi `ConsistencyProof` `heartbeat.rs:426`, `verify_consistency_proof` `:497` | BUILT | Certificate-Transparency-style proof that history only grew |
| Merkle tree with proofs | tameshi `Blake3Algorithm` (RFC 9162 domain separation) `src/merkle.rs:45`, `merkle_proof` `:232`, `verify_proof` `:256` | BUILT | Inclusion proofs: "rule r is in the model at commit c" |
| Public-key signatures | tameshi `LocalSigner` (Ed25519) `src/signing.rs:133`, `verify_signed_root` `:101` (pubkey-only, `verify_strict`) | BUILT | Writer and donor signatures that anyone can check without a secret |
| Git-shaped typed DAG | galho `GalhoTree` `galho-storage/src/galho_tree.rs:101`, `advance_ref` `:164` refusing a non-descendant with a typed `Divergence` `:45`; `OutcomeChain` `outcome_chain.rs:106`; `three_way_merge` `galho-types/src/merge.rs:46`; `TypedConflict` `conflict.rs:15` | BUILT for IaC, private repo | The commit DAG, compare-and-swap ref moves and a typed conflict algebra, already written once |
| Donated-compute substrate | `theory/MUTIRAO.md` §VI (quinhão, the donated share), §VII (trust and abuse), §III.4 (eito, distributed jobs); `mutirao-convergent` `GrowSet` | DESIGN, with BUILT CRDT core (private repo) | The swarm, identity, quotas and attested give/take ledger donors run under |

The short version: the object store, the tree, the delta log, the hash
chain and the signatures all exist. What does not exist is the thing that
binds them into a commit, and the one check that makes donation safe.

## 3. Object model

Four object kinds, all content-addressed under the same `objects/` layout
the stream writer already uses, all Lisp on the wire.

**Tree.** A `ModelSnapshot`, as today: library, trainer state, mechanism
pool and frontier cursor (in metadata), with its BLAKE3 hash. A tree says
*what the model is*.

**Commit.** A tree plus where it came from and why it is trusted:

```lisp
(model-commit
  :parents   (<commit-hash> ...)
  :tree      <snapshot-hash>
  :library   <library-root>
  :deltas    ((epoch-delta ...) (mechanism-delta ...) (demotion ...))
  :receipts  ((certified :rule <rule-hash>
                         :context <library-root the check ran against>
                         :sample-seed <u64> :samples <n>
                         :level certified))
  :donors    ((<rule-hash> :donor <pubkey-hash> :proposal <proposal-hash>))
  :writer    <pubkey-hash>
  :signature <ed25519 over the commit hash>)
```

A commit has zero parents (genesis), one (an ordinary step) or two or more
(a merge). Parents are a first-class field, so the parent hash lives inside
the commit's own hash, not in a metadata string the tree happens to carry.
The receipts are the point: a commit does not say "these rules are true", it
says "these rules passed this check, against this library, on samples drawn
from this seed", and anyone can re-run that check.

**Ref.** A name pointing at a commit: `HEAD` (today's only ref), branch refs
(`refs/branches/<name>`) and the canonical ref (`refs/canonical`). A ref
moves only by compare-and-swap against its expected tip, the way galho's
`advance_ref` refuses a node that does not descend from the tip.

**Ref log.** Every ref move is appended to a tameshi `HeartbeatChain`
(sequence, previous hash, entry hash, signed by the writer). That makes the
history Certificate-Transparency-shaped: anyone holding an old head can
demand a `ConsistencyProof` that today's head descends from it. A writer
that rewrote history would be caught by any peer holding an older head.

## 4. Trust model

### The load-bearing rule

**The writer never trusts a donor's verdict.** A donor's proposal is a
claim. The writer re-certifies every proposed rule itself, against the
current library, on samples it draws **after** the proposal arrives, from a
seed the donor cannot know in advance:

```
sample-seed = BLAKE3(writer-salt ‖ HEAD ‖ rule-hash)
```

Only the writer's own receipt goes into the commit.

### Why this is not optional: today's samples are public

`DefaultCertifier::try_certify` (`certification.rs:309-345`) runs seeds
`0..seed_count` (default 3), and each variable's value comes from a fixed
pool indexed by the sample number. The Nat pool is seven values,
`[0, 1, 2, 5, 17, 3, 8]` (`certification.rs:263`); Int has seven, Float six, Tensor and
FloatTensor four (`:266-305`). A donor who reads the
source knows every point the check will ever evaluate. Any pair of terms that
agree on those seven naturals passes, however false the equation is
everywhere else. That is fine while the only proposer is our own motor. It is
a forgery recipe the moment a stranger can propose.

So the first change is twofold: the certifier takes its sample seed as an
input and draws from wide ranges rather than fixed pools (the pools stay, as a
configured value), and `Writer::merge` stops trusting `RuleCertified` events
and runs the certifier on every incoming rule.

### Path to proof

Sampling is evidence, not proof. The climb is the proof-status lattice that
already exists:

1. **Sampled** — passes the writer's post-submission samples (the floor for
   any donated rule).
2. **Verified** — both sides reach one normal form (`prove_equational`) or
   one e-class. The receipt records the context library root, because a
   normal-form proof is only as good as the rules it rewrote with.
3. **Exported** — a Lean 4 proof that Lean accepts. Today the export emits
   the statement with `sorry`; filling the body turns donated rules into
   proof-carrying code: a donor can ship the proof, and the writer checks
   the proof instead of sampling.

`Canonical` (the level that seeds every future run) should require
`Exported` once proofs exist. Until then it requires Verified plus
post-submission samples.

### Each attack, refused at the entry, siblings proceed

A proposal is a batch of rules. Every refusal below names one rule (or one
proposal) and lets the rest of the batch through. A batch is never refused
because one entry in it is bad.

| Attack | What it looks like | Refusal | Scope |
|---|---|---|---|
| Poisoning | A false rule, built to pass known samples | Re-certified on fresh samples; on disagreement it is tombstoned with the donor and the failing sample recorded | That rule; the rest of the batch is checked independently |
| Subtle poisoning | A false rule that survives sampling anyway | It sits at Sampled, below Verified, and cannot reach Canonical; a later counterexample demotes it, and every receipt whose context contained it is re-checked (section 5) | That rule and what was certified on top of it |
| Replay | The same proposal sent again, by anyone | Idempotent by rule hash: a known or tombstoned rule is a no-op before any work is done | That entry |
| Spam | Many proposals to burn writer CPU | Per-donor rate limit at ingress (samba `LeakyBucket`), and a bounded step limit per check | That donor's excess; other donors unaffected |
| Sybil | Many identities | Identity buys **credit**, never **acceptance**: a rule is accepted on its own check, whoever sent it. So sybils can only inflate credit or spread spam across keys; the answer is invitation-gated swarms first, as `theory/MUTIRAO.md` §VII states | Credit accounting, and per-identity quotas at admission |
| Forged identity | A proposal signed with a key that does not verify | Refused before parsing its rules (`verify_signed_root` returns false, never errors) | That proposal |
| Writer misbehaviour | The writer rewrites history | Any holder of an older head gets a failing `ConsistencyProof` | Detection, by anyone |

## 5. Operations on an equational library

The library is not a text file, so git's line-based semantics do not apply.
It is a set of equations, each certified against a context. That makes most
operations simpler than git's and one of them harder.

**Checkout (time travel).** Load any commit's tree and `fork_from_snapshot`
it. The result is an independent live model; nothing about the checked-out
commit changes. BUILT at the tree level; the commit layer adds walking the
parent chain.

**Branch / reroute.** Start a writer from any commit with a different seed or
mechanism and give it its own ref. Today `Writer::open` refuses a seed that
differs from `HEAD`'s (`writer.rs:145`, `StreamError::SeedMismatch`). That
refusal is right for resume and wrong for branching, so the seed moves from a
global property of the store to a property of the ref.

**Revert.** Revert is demotion, recorded as a commit. The rule stays in
history and in the registry with status `Demoted(reason)`, excluded from
rewriting and discovery, exactly as `demotion-pipeline.md` already specifies
for library demotion. It is configured off, never deleted. Revert has one
consequence git does not have: certification is relative to a context. Every
receipt whose `:context` library contained the reverted rule is re-checked
against the library without it, and anything that no longer holds is demoted
in the same commit. The receipts make that walk exact rather than a guess.

**Merge.** Two branches grew from a common ancestor; the merge commit has both
as parents. For the library this is close to a union, because **true
equations cannot contradict each other**: if both sides only hold identities
that are valid in the evaluator's semantics, their union is still valid.
So a merge is:

1. Union the certified rules, and union the tombstones and demotions (all
   grow-only sets, a `GrowSet`-shaped CRDT).
2. Re-certify every incoming rule against the merged library, on fresh
   samples. A rule that fails was never true. Its branch's certification was
   wrong, or rested on a context rule that is false. That is the real
   conflict, and it resolves by demotion, with the receipts showing which
   branch admitted it.
3. Drop rules the merged library already entails (`entailed_by_library`,
   `mathscape-compress/src/frontier.rs:259`). That is redundancy, not
   conflict.
4. Normalize orientation. `a → b` on one side and `b → a` on the other are
   the same equation but make rewriting loop. That is an operational
   conflict, not a logical one, and it is resolved by one canonical
   orientation (a term order, e.g. smaller right-hand side), recorded in the
   receipt.

The **trainer is the hard part**, because policy weights are not a lattice:
two diverged weight vectors have no meaningful union. The default resolution
is replay: the merged library's event stream is replayed through a fresh
trainer from the common ancestor's weights. That is deterministic, because
the trainer is seed-deterministic, so a merge commit's tree is reproducible.
"Keep ours" and "keep theirs" stay as configured policies. The mechanism pool
is per-branch state, and the merge keeps the one its policy names.

**Rebase.** Replay a branch's deltas onto a new base: each delta's rules are
re-certified against the new base's library (fresh samples, new receipts),
and the trainer replays. Because deltas carry rules rather than diffs,
rebase never has a textual conflict. It has only the merge's two real ones:
a rule that fails against the new base, and orientation.

**Bisect.** A benchmark regression (`BenchmarkScored` with negative
`delta_from_prior`) between two commits is bisected by checking out the
midpoint, scoring it with `canonical_problem_set` / `harder_problem_set`, and
halving. Deterministic replay makes each probe exact. The culprit is a commit,
and its receipts name the rule and donor.

## 6. Donor protocol

**What a donor sends:**

```lisp
(model-proposal
  :base      <commit-hash the search started from>
  :rules     ((rule :lhs ... :rhs ...) ...)
  :mechanism <mechanism-config hash the search used>
  :evidence  (optional: the donor's own samples, an e-graph proof, a Lean proof)
  :donor     <ed25519 pubkey>
  :signature <ed25519 over the proposal hash>)
```

The proposal is content-addressed, so the same proposal is the same object
wherever it comes from. `:base` lets the writer tell a stale search from a
fresh one, but it is never a precondition: a rule found against an old base
is still checked against the current library. `:evidence` can speed the
check up (a valid Lean proof is decisive) but can never replace it.

**What the writer does with it, per rule:** verify the signature (refuse the
whole proposal if it fails), drop known or tombstoned hashes, drop entailed
rules, re-certify on post-submission samples, then commit the survivors with
receipts and the donor's key in `:donors`. Credit, meaning which donor
contributed which accepted rule, is read straight off the commit history,
which is the attested give/take ledger `theory/MUTIRAO.md` §VI describes.

**How it maps onto what exists:** today's scout is a donor that happens to be
local and trusted. The stream design's scout mode
(`stream-training-infra.md`, build-order step 2: `--seed --scenario`, emits
`(epoch-delta …)`) becomes "emit a `model-proposal`". Our own scouts go
through the same check as a stranger's. One path, no trusted side door.

**Transport,** in order of arrival: the writer's local socket and stdout
lines (step 2 as designed); an HTTP or gRPC ingress on the writer for remote
donors on engenho, rio or spot; mutirão's swarm (maré CAS replication, eito
jobs) once it exists, at which point proposals are just CAS objects any peer
can fetch. The object layout is the same on local disk, S3 and the swarm
CAS, so moving between transports changes the plumbing, not the protocol.

**What stays a single writer, and why.** Search is embarrassingly parallel.
Committing is not: the trainer's update is order-dependent, `HEAD` is a total
order, and the ref log is a single chain so that consistency proofs mean
something. So there is exactly one writer per ref, which is the design the
stream already has. Writers on different branches run concurrently, and merge
is how their work meets.

## 7. Build order

Each phase extends a primitive that exists. None starts a new one. Each ends
with a test that would fail without it.

**L1. The writer re-certifies; samples are drawn after submission.**
`Certifier` takes a sample seed; `DefaultCertifier` samples wide ranges, and
today's fixed pools stay as a configured value. `Writer::merge` runs the
certifier on every incoming rule instead of trusting its events.
*Test:* a constructed rule that agrees on the fixed Nat pool and nowhere else
is certified by the fixed-pool certifier and rejected by the salted one; a
batch of one forged and one true rule commits the true one and tombstones the
forged one.

**L2. The commit object and the signed ref log.** Add the public `tameshi`
crate. Commits become objects with first-class parents and receipts. `HEAD`
moves append to a `HeartbeatChain` signed with a `LocalSigner`. Resume
verifies the chain, not just the tree hash.
*Test:* a commit with a tampered receipt is refused on load; a rewritten
ref log fails `verify_consistency_proof` against an older head; same seed
still yields an identical commit chain.

**L3. A library tree with inclusion proofs.** The library root becomes a
tameshi Merkle tree over sorted rule hashes (RFC 9162 domain separation)
instead of a sorted concatenation.
*Test:* a donor verifies "rule r is in commit c" from the root and a proof
path alone; the proof fails against a commit where r was reverted.

**L4. Operations.** Seed per ref (branching); revert as a demotion commit
that re-checks dependent receipts; merge as union + re-certify + entailment
drop + orientation + trainer replay; rebase; bisect over `BenchmarkScored`.
*Test:* per operation: a branch with a different seed coexists with the
original; reverting a rule demotes a rule that was certified only against
it; merging two branches is the same tree in either order; a planted
regression is found by bisect.

**L5. The donor protocol.** The `model-proposal` object, writer ingress
(socket first), per-donor rate limits, credit read from history. Our own
scouts switch to proposals.
*Test:* two donors proposing the same rule produce one library entry and two
credit records under the configured policy; a proposal with a bad signature
is refused while a valid proposal in the same tick commits.

**L6. Extract the DAG, once mathscape is the third consumer.** galho's
`GalhoTree` is almost shape-free; only `NodeMeta.iac_system`
(`galho_tree.rs:21-27`) ties it to IaC. mathscape-stream carries its own
object store (`store.rs:13`), the third CAS in the fleet beside galho's
`acervo` and mutirão's maré. That is the extraction trigger the prime
directive names: lift the DAG and CAS into one shared crate and make
mathscape-stream, galho and mutirão consume it. One constraint shapes this:
mathscape is public and its CI runs only because it is, so it can only depend
on crates published from public repos. galho and mutirão are private today,
so the extraction lands in a public crate.
*Test:* the stream's existing resume, kill-mid-write and hash-chain tests pass
unchanged on the shared crate.

**L7. Proof certificates.** Lean export emits real proofs for rules whose
proof is a normal-form or e-graph derivation. `Canonical` requires
`Exported`. Donors can submit proofs as evidence.
*Test:* `lean` accepts the exported proof of a known identity and rejects a
proof script for a forged rule.

**L8. The swarm.** Proposals and objects travel over mutirão when its
transport exists. Donors are quinhão-bounded peers.
*Test:* mutirão's own; the ledger tests above run unchanged over it.

L1 comes first because it is the one that makes everything after it safe: a
ledger that records forgeries faithfully is worse than no ledger.

## 8. Against the "Executive Synthesis" directive

The pasted directive (One Model, Merkle universe, epoch-delta schema,
FormalSum terms, spot drain, two-tier engine) and this design agree on the
shape and differ in three places.

**Agrees:** one canonical model; content-addressed state in a Merkle
structure; append-only epoch deltas as the unit of change; cheap workers and
one durable model; determinism as the reason replay and audit work. All of
that is section 3 and the stream writer.

**Differs:**

- **Its `Term` enum is not mathscape's.** It proposes `Atom | Apply |
  FormalSum`. mathscape's `Term` is `Point | Number | Fn | Apply | Var |
  Symbol` (plus the value domains). A `FormalSum` is additive to that enum,
  not a replacement, and nothing in the ledger depends on either. The ledger
  hashes whatever `Term` is.
- **Scouts need no 120-second drain.** A drain protects state, and a scout or
  donor holds none: losing one costs one replay of a seed. Only a writer
  holds state, and it writes atomically after every K deltas, so even the
  writer survives `kill -9` (measured 2026-10-15). A drain is at most a
  courtesy that saves the replay.
- **Universal linearity is not an axiom.** `f(c₁A + c₂B) = c₁f(A) + c₂f(B)`
  is false for any nonlinear `f` (`f(x) = x²` breaks it at once). In this
  ledger it is a **law class**: a rule of that shape is certified per `f`
  like any other proposal, and the set of `f` it holds for is what the model
  learns. Taking it as an axiom would admit a false rule at genesis, which
  every commit after would inherit.

## 9. Open questions

- **Writer salt custody.** The post-submission seed needs a secret the donor
  cannot see. Whether that is a per-writer key under cofre or derived from a
  signed beacon (so a third party can re-run the exact check later) decides
  whether receipts are publicly replayable. Deriving it from `HEAD` plus the
  rule hash alone would be replayable but predictable once `HEAD` is known.
- **Credit policy.** First-accepted only, or every donor who found it?
  The ledger records both; the policy is a config value, like mutirão's
  per-swarm fairness knob.
- **Trainer merge.** Replay is deterministic but costs a full replay from the
  ancestor. Whether a cheaper merge (e.g. averaging only weights whose
  activations are disjoint) is ever sound is unmeasured.
- **Canonical gate.** How much evidence a donated rule needs before it may
  seed every future run, before Lean proofs exist.
