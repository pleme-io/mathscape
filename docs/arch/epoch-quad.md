# Epoch — the generate/prove/emit/register quad

The epoch loop is the unit of progress. Every epoch produces zero or
more **Artifacts** that enter the **Registry** monotonically. Each
Artifact carries a BLAKE3 content hash of its canonical form and a
list of `parent_hashes` pointing to the library entries it was
derived from. The registry is not just a library — it is a
cryptographically-chained derivation DAG.

## The four roles

```
                    corpus
                      │
                      ▼
  ┌───────────┐  Candidates   ┌───────────┐
  │ Generator │ ───────────▶ │   Prover   │ ───→ Verdict
  └───────────┘               └───────────┘      (Accept|Reject)
        ▲                                           │
        │                                           ▼
        │                                    ┌───────────┐
        │ feedback (fitness,                 │  Emitter   │ ───→ Artifact
        │ archive, policy)                   └───────────┘
        │                                           │
        │                                           ▼
        │                                    ┌───────────┐
        └───── reads ─────────────────────── │  Registry  │
                                             └───────────┘
                                                   │
                                               next epoch
```

| Role        | Responsibility                                          | Current impl                        |
|-------------|---------------------------------------------------------|-------------------------------------|
| `Generator` | Propose `Candidate`s each epoch                         | `mathscape-evolve::Population`       |
| `Prover`    | Map candidate → `Verdict` (Accept+Cert \| Reject)        | `mathscape-reward` + `mathscape-proof` |
| `Emitter`   | Materialize accepted candidates as `Artifact`s           | `mathscape-compress::extract`        |
| `Registry`  | Append-only store of artifacts with derivation DAG       | `mathscape-store` (PostgreSQL + redb) |

## Why reify the quad

The current `run_epoch` in `mathscape-cli` interleaves four concerns
imperatively: it mutates the population, the library, and the reward
scores inside one function. Reifying the quad buys three things:

1. **Swappable roles.** The [minimal-model ladder](minimal-model-ladder.md)
   requires different `Generator` / `Prover` pairs per phase
   (compression-dominant vs novelty-escape vs dimensional-discovery).
   The quad is the injection point.
2. **Audit trail.** Each `Artifact` carries its content hash + parent
   hashes. The registry becomes a Merkle DAG; `proofs` and `lineage`
   tables are materializations of that DAG.
3. **Determinism contract.** One `step()` call consumes a corpus and
   returns a `Trace`. Given the same generator state, prover config,
   and corpus, the same artifacts emerge. This is the property that
   makes re-runs reproducible and the proof database meaningful.

## The core types

```rust
pub struct Candidate { pub term: Term, pub origin: String }

pub enum Verdict { Accept(AcceptanceCertificate), Reject(Vec<Rejection>) }

pub struct AcceptanceCertificate {
    pub score: f64,
    pub compression_ratio: f64,
    pub novelty: f64,
    pub meta_compression: f64,
    pub status: ProofStatus,          // Conjectured | Verified | Exported
    pub equivalence_hash: Option<Hash>,
}

pub struct Artifact {
    pub rule: RewriteRule,
    pub epoch_id: u64,
    pub certificate: AcceptanceCertificate,
    pub content_hash: Hash,           // BLAKE3(canonical bincode)
    pub parent_hashes: Vec<Hash>,     // library deps → derivation DAG
}

pub trait Generator { fn propose(&mut self, epoch_id: u64) -> Vec<Candidate>; }
pub trait Prover    { fn prove(&self, c: &Candidate, corpus: &[Term], lib: &[Artifact]) -> Verdict; }
pub trait Emitter   { fn emit (&self, c: &Candidate, cert: &AcceptanceCertificate, epoch_id: u64, lib: &[Artifact]) -> Option<Artifact>; }
pub trait Registry  { fn insert(&mut self, a: Artifact); fn all(&self) -> &[Artifact]; }

pub struct Epoch<G, P, E, R> { pub generator: G, pub prover: P, pub emitter: E, pub registry: R, pub epoch_id: u64 }
```

`Epoch::step(&mut self, corpus: &[Term]) -> EpochTrace` runs the four
roles in sequence, returning an audit record.

## What this changes

- `mathscape-evolve::Population` gains a `Generator` impl — no change
  to mutation/crossover logic, only a thin wrapper that drains
  individuals into `Candidate`s each epoch.
- A new `StatisticalProver` in `mathscape-reward` wraps `compute_reward`
  and applies configurable accept thresholds. Future provers
  (`EgraphProver` in `mathscape-proof`, `LeanProver`) slot in here.
- `mathscape-compress::extract` gains an `Emitter` impl that computes
  the content hash + parent hashes from the rule's `rhs` (walking
  `Term::Symbol` references back to library artifacts).
- `mathscape-store` gains a `Registry` impl over PostgreSQL; an
  `InMemoryRegistry` lives in `mathscape-core` for tests and the
  current in-memory REPL.

## Relation to axiom-forge

Same pattern, different domain.

| axiom-forge                          | mathscape                             |
|--------------------------------------|---------------------------------------|
| Lisp proposal                        | `Candidate` (Term, origin)            |
| 7 structural obligations             | `Prover` (reward + future equivalence)|
| `verify` → `Certificate` or Violations| `Verdict::Accept(Cert) \| Reject(_)` |
| `emit_rust` (string templates)       | `Emitter` (RewriteRule construction)  |
| `FrozenVector` (canonical_text, hash)| `Artifact` (rule, content_hash)       |
| Workspace + tests                     | Registry (PostgreSQL + derivation DAG)|

The user-facing consequence: once both crates expose the same quad,
extracting a shared `pleme-io/primitive-forge` crate becomes a
mechanical lift. We do not extract it yet — we prove the pattern works
in two domains first.
