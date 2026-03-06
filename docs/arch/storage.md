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

### 2. SQLite -- Structured Metadata

SQLite via `rusqlite` for relational queries, ordering, aggregation.

```sql
-- Population snapshot (bulk-replaced each epoch)
CREATE TABLE population (
    epoch       INTEGER NOT NULL,
    individual  INTEGER NOT NULL,
    root_hash   BLOB NOT NULL,
    fitness     REAL NOT NULL,
    cr_contrib  REAL,
    novelty     REAL,
    PRIMARY KEY (epoch, individual)
);

-- Library of discovered Symbols (append-only)
CREATE TABLE library (
    symbol_id        INTEGER PRIMARY KEY,
    name             TEXT NOT NULL,
    epoch_discovered INTEGER NOT NULL,
    lhs_hash         BLOB NOT NULL,
    rhs_hash         BLOB NOT NULL,
    arity            INTEGER NOT NULL,
    generality       REAL,
    irreducibility   REAL,
    is_meta          BOOLEAN DEFAULT FALSE
);

-- Epoch metrics (one row per epoch, append-only)
CREATE TABLE epochs (
    epoch                INTEGER PRIMARY KEY,
    compression_ratio    REAL NOT NULL,
    description_length   INTEGER NOT NULL,
    raw_length           INTEGER NOT NULL,
    novelty_total        REAL NOT NULL,
    meta_compression     REAL NOT NULL,
    library_size         INTEGER NOT NULL,
    population_diversity REAL,
    alpha                REAL NOT NULL,
    beta                 REAL NOT NULL,
    gamma                REAL NOT NULL,
    duration_ms          INTEGER
);

-- Lineage (derivation DAG)
CREATE TABLE lineage (
    child_hash    BLOB NOT NULL,
    parent1_hash  BLOB,
    parent2_hash  BLOB,
    mutation_type TEXT NOT NULL,
    epoch         INTEGER NOT NULL
);
CREATE INDEX idx_lineage_child ON lineage(child_hash);
CREATE INDEX idx_lineage_epoch ON lineage(epoch);

-- Evaluation traces (proof steps)
CREATE TABLE eval_traces (
    trace_id      INTEGER PRIMARY KEY,
    expr_hash     BLOB NOT NULL,
    step_index    INTEGER NOT NULL,
    rule_applied  TEXT NOT NULL,
    before_hash   BLOB NOT NULL,
    after_hash    BLOB NOT NULL,
    epoch         INTEGER NOT NULL
);
CREATE INDEX idx_traces_expr ON eval_traces(expr_hash);

-- Proof certificates
CREATE TABLE proofs (
    proof_id       INTEGER PRIMARY KEY,
    symbol_id      INTEGER NOT NULL REFERENCES library(symbol_id),
    proof_type     TEXT NOT NULL,
    status         TEXT NOT NULL,
    lhs_hash       BLOB NOT NULL,
    rhs_hash       BLOB NOT NULL,
    trace_ids      BLOB NOT NULL,
    epoch_found    INTEGER NOT NULL,
    epoch_verified INTEGER,
    lean_export    TEXT
);

-- Proof dependency graph
CREATE TABLE proof_deps (
    proof_id   INTEGER NOT NULL REFERENCES proofs(proof_id),
    depends_on INTEGER NOT NULL REFERENCES proofs(proof_id),
    PRIMARY KEY (proof_id, depends_on)
);
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
2. Bulk-insert population into SQLite (DELETE old, INSERT new)
3. Append library entries for new Symbols
4. Append epoch metrics row
5. Append lineage records
6. Write eval traces and proof records if applicable
7. fsync both databases

The epoch boundary IS the transaction boundary. Crash mid-epoch rolls
both databases back to end of previous epoch. No partial state.

## MAP-Elites Archive Persistence

The MAP-Elites archive (see [search.md](search.md)) is persisted as
part of the population table, with additional columns:

```sql
ALTER TABLE population ADD COLUMN depth_bin    INTEGER;
ALTER TABLE population ADD COLUMN op_diversity INTEGER;
ALTER TABLE population ADD COLUMN cr_bin       INTEGER;
```

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

All state is epoch-tagged. Full search history is queryable post-hoc:

```sql
-- Compression ratio over time
SELECT epoch, compression_ratio FROM epochs ORDER BY epoch;

-- When was associativity discovered?
SELECT epoch_discovered, name FROM library WHERE name LIKE '%assoc%';

-- Diversity at novelty spikes
SELECT epoch, population_diversity, novelty_total
FROM epochs WHERE novelty_total > 0.5;

-- Lineage of a specific expression
WITH RECURSIVE ancestors AS (
    SELECT * FROM lineage WHERE child_hash = ?
    UNION ALL
    SELECT l.* FROM lineage l
    JOIN ancestors a ON l.child_hash = a.parent1_hash
)
SELECT * FROM ancestors;
```
