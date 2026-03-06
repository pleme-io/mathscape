# MCP Interface

## Design Principle: Observe, Don't Interfere

The MCP (Model Context Protocol) interface is a **read-and-trigger**
boundary. An agent connected via MCP can:

1. **Trigger** epoch execution (the engine runs the epoch internally)
2. **Observe** all results — population, library, proofs, metrics, lineage

An agent **cannot**:

1. Alter the reward function, mutation operators, or selection logic
2. Inject expressions into the population
3. Modify library entries or rewrite rules
4. Compute or simulate an epoch outside the engine
5. Override compression, novelty, or meta-compression calculations
6. Modify stored data (expressions, proofs, lineage, metrics)

**Why:** Mathscape is an observable experiment. The search dynamics —
compression equilibrium, novelty escape, dimensional discovery — emerge
from the fixed algorithm interacting with the fixed primitives. If an
external agent can perturb the computation, the traversal is no longer
reproducible and discoveries lose their inherent proof status. The
determinism guarantee (same inputs + same algorithm = same outputs)
breaks the moment an external actor injects state.

The MCP interface is a one-way glass: full visibility, zero influence
on the process.

## Tool Categories

### Execution Tools (trigger only)

These tools start epoch execution. The engine does all computation
internally — the MCP layer just signals "go."

| Tool | Description |
|---|---|
| `step` | Run exactly one epoch. Returns epoch summary on completion. |
| `run` | Run N epochs. Params: `count: u32`. Returns final epoch summary. |
| `run_until` | Run until a condition is met. Params: `condition: StopCondition`. Returns epoch at which the condition triggered. |

`StopCondition` variants:

```
StopCondition::EpochCount(n)           -- stop after n epochs
StopCondition::CompressionPlateau {    -- stop when CR delta < threshold
    threshold: f64,                       for `window` consecutive epochs
    window: u32,
}
StopCondition::NoveltySpike {          -- stop when novelty > threshold
    threshold: f64,
}
StopCondition::MetaCompression {       -- stop when meta_compression > threshold
    threshold: f64,
}
StopCondition::LibrarySize(n)          -- stop when |L| >= n
```

### Query Tools (read-only)

All query tools are pure reads — they never mutate state.

#### In-Memory State

| Tool | Description | Returns |
|---|---|---|
| `get_status` | Current engine state (running/idle, current epoch, elapsed time) | `EngineStatus` |
| `get_population` | Current epoch's population snapshot | `Vec<Individual>` with hash, fitness, depth, operators |
| `get_library` | Full symbol library | `Vec<Symbol>` with name, arity, epoch discovered, generality, is_meta |
| `get_archive` | MAP-Elites archive grid | `HashMap<(depth_bin, op_div, cr_bin), Individual>` |
| `get_weights` | Current reward weights | `{alpha, beta, gamma}` |

#### Database Queries (Historical)

| Tool | Description | Params |
|---|---|---|
| `get_epoch_metrics` | Metrics for a specific epoch or range | `epoch: u32` or `range: (u32, u32)` |
| `get_metrics_series` | Time series of a specific metric | `metric: MetricName, range: (u32, u32)` |
| `get_symbol_history` | When and how a symbol was discovered | `name: String` or `symbol_id: u32` |
| `get_expression` | Resolve an expression tree from its hash | `hash: blake3::Hash` |
| `get_lineage` | Full derivation chain for an expression | `hash: blake3::Hash, depth: Option<u32>` |
| `get_proof` | Proof certificate for a symbol | `symbol_id: u32` |
| `get_proof_tree` | Recursive proof dependency tree | `proof_id: u32` |
| `get_eval_trace` | Step-by-step evaluation trace | `expr_hash: blake3::Hash` |
| `search_library` | Search library by name pattern or properties | `query: LibraryQuery` |
| `search_proofs` | Search proofs by type, status, or dependencies | `query: ProofQuery` |

`LibraryQuery`:
```
name_pattern: Option<String>     -- SQL LIKE pattern
epoch_range: Option<(u32, u32)>  -- discovered in this range
min_generality: Option<f64>
is_meta: Option<bool>
```

`ProofQuery`:
```
proof_type: Option<String>       -- "inductive", "equational", "compositional"
status: Option<String>           -- "conjectured", "verified", "exported"
symbol_name: Option<String>
epoch_range: Option<(u32, u32)>
```

#### Visualization Helpers

| Tool | Description | Returns |
|---|---|---|
| `get_compression_curve` | CR over all epochs | `Vec<(epoch, cr)>` |
| `get_discovery_timeline` | Library symbols ordered by discovery epoch | `Vec<(epoch, symbol_name, is_meta)>` |
| `get_phase_transitions` | Detected compression/novelty/meta phase boundaries | `Vec<(epoch, phase)>` |
| `render_expression` | Pretty-print an expression as S-expr | `String` |
| `render_proof` | Pretty-print a proof certificate | `String` |
| `render_archive_heatmap` | MAP-Elites archive occupancy and fitness | Grid data for visualization |

## Security Boundary

The MCP server runs in the same process as the engine but exposes only
the tools above. The boundary is enforced at the Rust type level:

```rust
/// MCP handler receives a read-only reference to the engine.
/// No &mut Engine is ever exposed across the MCP boundary.
pub struct McpHandler {
    engine: Arc<Engine>,  // shared read access only
    trigger: mpsc::Sender<EngineCommand>,  // step/run signals only
}

/// The only commands an MCP client can send.
pub enum EngineCommand {
    Step,
    Run(u32),
    RunUntil(StopCondition),
}
```

`Engine` exposes only `&self` query methods. Mutation happens exclusively
inside the engine's own run loop, which consumes `EngineCommand` from
the channel. There is no API surface to reach internal state mutably.

## Interaction Patterns

### Agent as Observer

The primary use case: an agent watches the traversal unfold, asks
questions about what it sees, and reports insights to the user.

```
Agent: step           -> epoch 47 complete, CR=0.42, 2 new symbols
Agent: get_library    -> [..., add-commutative, add-associative]
Agent: get_proof(5)   -> inductive proof of add-commutative, verified
Agent: run_until CompressionPlateau(0.001, 10)
                      -> stopped at epoch 83, CR=0.51 (plateau)
Agent: get_phase_transitions -> [Phase1(0-35), Phase2(36-52), Phase3(53-83)]
```

### Agent as Narrator

An agent can read the full history and narrate the mathematical
journey — what was discovered, in what order, how proofs compose,
where phase transitions occurred. This is valuable for understanding
the emergent structure without interfering with it.

### What an Agent Must NOT Do

- Run expressions through its own evaluation and inject results
- Suggest specific mutations or expressions to add to the population
- Modify reward weights or selection parameters
- Rewrite library entries or proof certificates
- Delete or alter stored data

If an agent attempts any of these, the MCP layer has no tool to
accomplish it. The interface simply doesn't expose the capability.

## Crate: `mathscape-mcp`

The MCP server is a separate crate that depends on `mathscape-core`,
`mathscape-store`, and `mathscape-evolve` (read-only interfaces).

```
mathscape-mcp/
  src/
    main.rs          -- MCP server entry point (stdio transport)
    tools/
      execution.rs   -- step, run, run_until
      population.rs  -- get_population, get_archive
      library.rs     -- get_library, search_library
      metrics.rs     -- get_epoch_metrics, get_metrics_series, get_compression_curve
      proofs.rs      -- get_proof, get_proof_tree, search_proofs
      expressions.rs -- get_expression, get_lineage, get_eval_trace, render_expression
      status.rs      -- get_status, get_weights, get_phase_transitions
```

Built with `rmcp` (Rust MCP SDK) using stdio transport for Claude Code
integration. The server binary is `mathscape-mcp` and is configured as
an MCP server in the user's Claude Code settings.
