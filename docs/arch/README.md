# Architecture Documents

Detailed design documents for each Mathscape subsystem.

| Document | Covers |
|---|---|
| [compression.md](compression.md) | Symbolic compression: description length, library extraction, STITCH-style abstraction learning |
| [search.md](search.md) | Evolutionary search, quality-diversity, MAP-Elites archive, mutation operators |
| [reward.md](reward.md) | Reward function dynamics: compression ratio, novelty, meta-compression, adaptive weights |
| [storage.md](storage.md) | Storage architecture: redb expression store, SQLite metadata, epoch transactions, memory budget |
| [proofs.md](proofs.md) | Proof system: Curry-Howard, e-graph verification, proof certificates, Lean 4 export |

See [CLAUDE.md](../../CLAUDE.md) for the unified project vision and development plan.
