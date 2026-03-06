# Service Architecture

## Overview

Mathscape runs as a long-running service that executes epochs
continuously, storing all results to disk. The service exposes
HTTP endpoints for health checks, Prometheus metrics, and a
read-only query API for remote observation.

## Binary: `mathscape-service`

```
mathscape-service
  ├── Engine loop      (runs epochs, writes to storage)
  ├── HTTP server      (health, metrics, read-only queries)
  └── Storage layer    (redb + SQLite in /data/)
```

### Engine Loop

The engine loop runs in a background Tokio task:

```rust
async fn engine_loop(engine: Arc<Engine>, config: EngineConfig) {
    loop {
        engine.step().await;  // one epoch
        // Metrics and storage are updated inside step()
        if engine.should_pause() {
            engine.wait_for_resume().await;
        }
    }
}
```

The engine starts automatically on boot and runs until stopped.
All state is persisted to /data/ after every epoch — a crash
mid-epoch rolls back to the previous complete epoch.

### HTTP Endpoints

| Endpoint | Method | Purpose |
|---|---|---|
| `/healthz` | GET | Liveness probe — returns 200 if process is alive |
| `/readyz` | GET | Readiness probe — returns 200 if engine has completed at least one epoch |
| `/metrics` | GET | Prometheus metrics (epoch count, CR, novelty, duration, library size) |
| `/api/status` | GET | Engine status (running/paused, current epoch, elapsed time) |
| `/api/library` | GET | Full symbol library |
| `/api/epochs` | GET | Epoch metrics (query params: start, end) |
| `/api/expression/:hash` | GET | Resolve expression tree by hash |
| `/api/proof/:id` | GET | Proof certificate by ID |

All `/api/*` endpoints are **read-only**. There is no endpoint to
modify engine state, inject expressions, or alter the computation.

### Ports

| Port | Purpose |
|---|---|
| 8080 | HTTP (health + query API) |
| 9090 | Prometheus metrics |

## Three Binaries

| Binary | Transport | Use case |
|---|---|---|
| `mathscape-service` | HTTP | Long-running K8s deployment |
| `mathscape-mcp` | stdio (MCP) | Local Claude Code integration |
| `mathscape-cli` | Terminal REPL | Interactive exploration |

All three share the same engine core. The difference is the
interface layer:

- **service**: HTTP server, auto-starts engine loop, designed for
  unattended operation
- **mcp**: MCP protocol over stdio, agent triggers epochs explicitly,
  designed for Claude Code interaction
- **cli**: Interactive REPL, human triggers epochs with commands

## Storage Layout

```
/data/
  expressions.redb    -- hash-consed expression store
  metadata.sqlite     -- population, library, epochs, lineage, proofs
```

Both databases use ACID transactions with fsync at epoch boundaries.
The /data/ directory is a PersistentVolume in Kubernetes.

## Configuration

Environment variables:

| Variable | Default | Description |
|---|---|---|
| `DATA_DIR` | `/data` | Path to storage directory |
| `RUST_LOG` | `info,mathscape=debug` | Log level filter |
| `LOG_FORMAT` | `json` | Log format (json or pretty) |
| `HTTP_PORT` | `8080` | HTTP server port |
| `METRICS_PORT` | `9090` | Prometheus metrics port |
| `POPULATION_SIZE` | `10000` | Number of individuals per epoch |
| `ALPHA` | `0.6` | Compression weight |
| `BETA` | `0.3` | Novelty weight |
| `GAMMA` | `0.1` | Meta-compression weight |

## Prometheus Metrics

```
mathscape_epoch_total              counter   Total epochs completed
mathscape_epoch_duration_seconds   histogram Epoch execution time
mathscape_compression_ratio        gauge     Current compression ratio
mathscape_description_length       gauge     Current description length
mathscape_novelty_total            gauge     Total novelty this epoch
mathscape_meta_compression         gauge     Meta-compression this epoch
mathscape_library_size             gauge     Number of symbols in library
mathscape_population_diversity     gauge     Population diversity metric
mathscape_expression_store_bytes   gauge     redb file size
mathscape_proof_count              gauge     Total verified proofs
```
