# Storage Architecture

## Principle: Memory is a Window, Disk is the Landscape

Memory holds only what the current epoch needs. Everything else lives on
disk in purpose-matched databases. Each epoch is a transaction: load the
working set, compute, write results, release.

## Hash-Consing Foundation

Every expression tree is content-addressed via blake3 hashing. Children
are stored as hash references, not inline.

```rust
struct TermRef(blake3::Hash);  // 32 bytes

enum StoredTerm {
    Point(u64),
    Number(Value),
    Fn(Vec<TermRef>, TermRef),      // params, body
    Apply(TermRef, Vec<TermRef>),   // func, args
    Symbol(SymbolId, Vec<TermRef>), // symbol, args
}
```

Benefits:
- **Deduplication**: populations share 60-90% of subtrees
- **O(1) equality**: same hash = same expression
- **Structural sharing across epochs**: mutations reuse unchanged subtrees

## Database Selection

### 1. redb -- Content-Addressed Expression Store

[redb](https://crates.io/crates/redb) is a pure-Rust embedded KV store.
ACID transactions, crash-safe, zero-copy reads. No C dependencies.

Tables:

```
expressions:    blake3::Hash -> bincode(StoredTerm)
eval_cache:     (blake3::Hash, InputSet) -> OutputSet
```

Why redb:
- Write-once semantics match immutable expressions
- Sequential write, random read pattern
- Pure key-value — hash IS the key
- Embeds into binary, no server process

### 2. PostgreSQL -- Structured Metadata (via SeaORM)

PostgreSQL via `sea-orm` for relational queries, ordering, aggregation,
and graph traversals via `WITH RECURSIVE`.

**Tables** (managed by SeaORM Rust migrations in `mathscape-migration`):

| Table | Purpose | Access pattern |
|---|---|---|
| `population` | Per-epoch population snapshot with MAP-Elites bins | Bulk-replace per epoch |
| `library` | Discovered symbols | Append-only |
| `epochs` | Per-epoch metrics | Append-only, time-series queries |
| `eval_traces` | Atomic proof steps | Append per epoch, join with proofs |
| `proofs` | Proof certificates with Lean 4 export | Append, status updates |
| `lineage_events` | Derivation DAG (denormalized from redb) | Append, recursive traversal |
| `symbol_deps` | Symbol dependency graph (materialized) | Bulk update on library changes |
| `proof_deps` | Proof dependency graph (materialized) | Bulk update on proof changes |

Why PostgreSQL over SQLite:
- `WITH RECURSIVE` CTEs for lineage/dependency graph traversal
- Concurrent read access from service + MCP + CLI (no file locking)
- JSONB for flexible metadata extensions
- Natural K8s integration (CNPG operator or managed Postgres)
- Connection pooling for the service binary

### 3. Graph Data Architecture

Graph structure lives in **redb adjacency tables** (source of truth):

```
lineage_forward:     blake3::Hash -> Vec<blake3::Hash>  // parent -> children
lineage_reverse:     blake3::Hash -> Vec<blake3::Hash>  // child -> parents
symbol_deps_forward: SymbolId -> Vec<SymbolId>
symbol_deps_reverse: SymbolId -> Vec<SymbolId>
proof_deps_forward:  ProofId -> Vec<ProofId>
proof_deps_reverse:  ProofId -> Vec<ProofId>
```

**Hot path** (evolution loop): redb O(1) edge lookups — "get parents of
X", "get children of X". In-process, zero network latency.

**Cold path** (analysis): PostgreSQL `WITH RECURSIVE` over denormalized
copies — "all ancestors of X", "longest derivation chain", "dependency
closure of symbol S".

No dedicated graph database. Apache AGE can be added if needed.

## SeaORM Migration Pattern

Migrations are Rust code in `crates/mathscape-migration/`:

```
mathscape-migration/src/
  lib.rs                              -- Migrator with migration list
  m20250101_000001_initial_schema.rs  -- population, library, epochs, traces, proofs
  m20250101_000002_graph_metadata.rs  -- lineage_events, symbol_deps, proof_deps
```

Entity models in `crates/mathscape-store/src/entity/`:

```
entity/
  mod.rs
  population.rs    library.rs       epochs.rs
  eval_traces.rs   proofs.rs        lineage_events.rs
  symbol_deps.rs   proof_deps.rs
```

Database management via `mathscape-db`:

```bash
mathscape-db migrate     # run pending migrations
mathscape-db status      # show migration status
mathscape-db rollback    # roll back last migration
mathscape-db verify      # check schema matches expectations
mathscape-db reset       # drop all + re-migrate (destructive)
```

## Memory Budget Per Epoch

| Data | In-memory form | Size estimate |
|---|---|---|
| Population index | `Vec<(TermRef, f64)>` | ~240 KB for 10k |
| Active subtrees | LRU cache of `StoredTerm` | Configurable, ~100 MB |
| Library | Full `Vec<Symbol>` | < 1 MB (1000s of symbols) |
| E-graph | Built per epoch (or incremental) | Transient, bounded by pop size |
| Reward state | Scalar accumulators | Negligible |

**Total: ~100-200 MB** regardless of epoch count or expression store size.

## Write Strategy

After each epoch, in a single transaction:

1. Batch-write new expressions to redb (mutations + crossover products)
2. Bulk-insert population into PostgreSQL (DELETE old, INSERT new)
3. Append library entries for new Symbols
4. Append epoch metrics row
5. Append lineage records
6. Write eval traces and proof records if applicable
7. Commit both transactions (redb + PostgreSQL)

The epoch boundary IS the transaction boundary. Crash mid-epoch rolls
both databases back to end of previous epoch. No partial state.

## MAP-Elites Archive Persistence

The MAP-Elites archive (see [search.md](search.md)) is persisted as
part of the population table with bin columns:

- `depth_bin` — expression depth bin
- `op_diversity` — distinct operator count bin
- `cr_bin` — compression contribution bin

On restart, the archive is reconstructed from the latest epoch's
population rows. Each cell's elite is the row with highest fitness
for that (depth_bin, op_diversity, cr_bin) triple.

## Expression Store Growth

Estimated growth rate:
- Per epoch: ~500 new unique subtrees (after deduplication)
- Per subtree: ~100 bytes average (bincode serialization)
- Per epoch disk cost: ~50 KB
- After 10,000 epochs: ~500 MB
- After 100,000 epochs: ~5 GB

redb handles this scale easily. For very long runs (1M+ epochs),
implement garbage collection: prune subtrees not referenced by any
population member, library entry, or proof trace.

## Queryable History

All state is epoch-tagged. Full search history is queryable:

```sql
-- Compression ratio over time
SELECT epoch, compression_ratio FROM epochs ORDER BY epoch;

-- When was associativity discovered?
SELECT epoch_discovered, name FROM library WHERE name LIKE '%assoc%';

-- Diversity at novelty spikes
SELECT epoch, population_diversity, novelty_total
FROM epochs WHERE novelty_total > 0.5;

-- Lineage of a specific expression (recursive CTE)
WITH RECURSIVE ancestors AS (
    SELECT * FROM lineage_events WHERE child_hash = $1
    UNION ALL
    SELECT l.* FROM lineage_events l
    JOIN ancestors a ON l.child_hash = a.parent1_hash
)
SELECT * FROM ancestors;

-- Symbol dependency closure
WITH RECURSIVE deps AS (
    SELECT depends_on FROM symbol_deps WHERE symbol_id = $1
    UNION ALL
    SELECT sd.depends_on FROM symbol_deps sd
    JOIN deps d ON sd.symbol_id = d.depends_on
)
SELECT DISTINCT depends_on FROM deps;
```
