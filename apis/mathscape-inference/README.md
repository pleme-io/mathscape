# mathscape-inference — API surface

Single source of truth for every platform that queries the
**live mathscape model**: REST, gRPC, GraphQL, MCP, and language
SDKs. One OpenAPI spec → one `forge-gen` invocation → every
surface.

## The pipeline (normalized)

```
apis/mathscape-inference/openapi.yaml   ← THIS DIRECTORY
         │
         ▼
    sekkei (OpenAPI 3.0 → typed Rust structs)
         │
         ▼
    takumi (resolved IR + CRUD grouping)
         │
         ├──▶ forge-gen --servers     → Rust axum REST + tonic gRPC + async-graphql
         ├──▶ forge-gen --sdks        → Rust, TypeScript, Python, Go, Ruby clients
         ├──▶ forge-gen --mcp         → MCP server (infer, competency, snapshot tools)
         ├──▶ forge-gen --schemas     → JSON Schema, Protobuf .proto, GraphQL SDL
         ├──▶ forge-gen --docs        → redoc HTML + Markdown reference
         └──▶ forge-gen --completions → fish completions + skim-tab YAML
```

Every downstream artifact is **mechanically regenerated** when
the spec changes. No hand-rolled API code, no drift, no
per-platform maintenance burden.

## Why this shape

- **One declaration, many outputs.** The same user adding a new
  method on `LiveInferenceHandle` → one spec edit → all 6
  surfaces updated by one command.
- **Types flow end to end.** Rust struct → sekkei → typed IR →
  every rendered backend uses the same shape. No translation
  error between Rust and the network types.
- **Lisp-authorable.** The OpenAPI YAML can be generated FROM a
  tatara-lisp form via the `defmathscapeservice` macro — one
  Lisp expression declares the whole API:

  ```lisp
  (defmathscapeservice
    :name  "live-inference"
    :title "Mathscape Live Inference"
    :version "0.1.0"
    :endpoints
      (((method POST :path "/infer"       :op infer             :input InferRequest    :output EvalResult))
       ((method GET  :path "/competency"  :op current-competency :output CurriculumReport))
       ((method GET  :path "/model/policy" :op policy-snapshot   :output LinearPolicy))
       ((method GET  :path "/model/library" :op library-snapshot :output (List RewriteRule)))
       ((method GET  :path "/model/library/size" :op library-size :output LibrarySize))))
  ```

  Output of expanding this Lisp form: this openapi.yaml. The
  Lisp layer owns declarative composition; the Rust layer owns
  types and safety.

## Generation commands (staged)

### Stage 1 — verify + render schemas

```bash
# From repo root.
# Stage 1a: Validate spec.
forge-gen validate \
  --spec apis/mathscape-inference/openapi.yaml

# Stage 1b: Render typed Rust + JSON Schema + Protobuf + GraphQL SDL.
forge-gen schemas \
  --spec apis/mathscape-inference/openapi.yaml \
  --rust-out crates/mathscape-inference-api/src/generated.rs \
  --json-out apis/mathscape-inference/schemas.json \
  --proto-out apis/mathscape-inference/service.proto \
  --graphql-out apis/mathscape-inference/schema.graphql
```

### Stage 2 — render servers

```bash
forge-gen servers \
  --spec apis/mathscape-inference/openapi.yaml \
  --rust-rest-out crates/mathscape-inference-rest \
  --rust-grpc-out crates/mathscape-inference-grpc \
  --rust-graphql-out crates/mathscape-inference-graphql
```

Each generated crate wires the API to a trait
`LiveInferenceService` that the operator implements once to
connect the network surface to the live `LiveInferenceHandle`.

### Stage 3 — render MCP server

```bash
forge-gen mcp \
  --spec apis/mathscape-inference/openapi.yaml \
  --out crates/mathscape-inference-mcp
```

Produces a `rmcp`-based MCP server exposing 5 tools:
`infer`, `current_competency`, `policy_snapshot`,
`library_snapshot`, `library_size`.

### Stage 4 — render client SDKs

```bash
forge-gen sdks \
  --spec apis/mathscape-inference/openapi.yaml \
  --languages rust,typescript,python,go,ruby \
  --out-root sdks/
```

### Stage 5 — docs + completions

```bash
forge-gen docs \
  --spec apis/mathscape-inference/openapi.yaml \
  --redoc-out apis/mathscape-inference/docs/index.html \
  --markdown-out docs/inference-api.md

forge-gen completions \
  --spec apis/mathscape-inference/openapi.yaml \
  --fish-out completions/mathscape-inference.fish \
  --skim-tab-out completions/mathscape-inference.yaml
```

## Efficiency notes

- **Shared schemas are referenced via `$ref`, never duplicated.**
  `Term`, `Value`, `RewriteRule` all resolve through the same
  component schemas; any change propagates across every
  endpoint that uses them.
- **Discriminators mean zero-cost deserialization.** `Term` and
  `Value` use OpenAPI 3.0 `oneOf` + `discriminator`, which
  sekkei renders as Rust tagged enums with serde's
  `#[serde(tag = "kind")]`. No speculative parsing, no
  allocation overhead.
- **The generated code is cheap to rebuild.** `cargo build`
  only touches `mathscape-inference-api` when the spec changes;
  downstream servers/SDKs consume that one crate via `Cargo.toml`
  so their rebuild is incremental.
- **One spec, one commit.** Spec changes flow through a single
  PR that regenerates all artifacts in one `forge-gen` run,
  then `cargo build --workspace` verifies nothing broke.

## When state enters

The current surface is **in-memory only** — the live model sits
in `Rc<RefCell<Vec<RewriteRule>>>` + `Rc<StreamingPolicyTrainer>`
via `LiveInferenceHandle`. When we want durability (checkpoint
the library, policy, benchmark history, map attestations across
process restarts), the path is:

1. Add SeaORM entities under `crates/mathscape-store/src/entity/`
   (matching the existing entity pattern).
2. Write migrations in `crates/mathscape-migration`.
3. Add `/history`, `/checkpoints/{id}` paths to this OpenAPI
   spec.
4. Re-run `forge-gen` — every surface gets CRUD over
   checkpoints automatically.

The SeaORM layer + OpenAPI + forge-gen is already how
`pangea-forge`, `terraform-forge`, `mcp-forge`, and friends
render. Adding mathscape-inference to that fleet is pure
additive wiring.
