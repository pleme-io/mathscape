# Discovery Mining Architecture

## Purpose

The discovery mining system bridges the gap between raw evolutionary
output and human-readable mathematics. It iterates over epoch data,
identifies discovered patterns, maps them to known mathematical concepts,
and produces structured representation data that a React UI can render
for human mathematicians.

## The Problem

Mathscape's evolutionary engine produces:
- Named symbols like `S_042: (op ?a ?b) => (op ?b ?a)`
- Fitness scores, compression ratios, novelty metrics
- Derivation lineage (how each expression was created)
- Proof traces (rewrite steps that verify equivalences)

None of this is directly meaningful to a mathematician. Discovery mining
translates machine-generated artifacts into mathematical language.

## Architecture

```
mathscape-discovery (MCP tools)
    │
    ├── EpochScanner         iterates epochs, extracts new symbols
    ├── PatternMatcher       structural matching against known-math catalog
    ├── ConfidenceScorer     how confident is the identification?
    ├── RepresentationBuilder structured JSON for React UI
    └── ExportPipeline       LaTeX, MathML, JSON API
```

### MCP Tools

Discovery mining is exposed as MCP tools so an agent can drive the
analysis interactively or batch-process a completed run:

| Tool | Description |
|---|---|
| `discover_scan_epochs` | Scan epoch range, return new discoveries |
| `discover_identify` | Map a specific symbol to known mathematics |
| `discover_timeline` | Full discovery timeline with identifications |
| `discover_expression_tree` | Visualization data for an expression |
| `discover_proof_chain` | Rendered proof chain for a symbol |
| `discover_symbol_graph` | Symbol relationship graph data |
| `discover_export_latex` | Export discoveries as LaTeX |
| `discover_export_json` | Export full representation data as JSON |

All tools are **read-only** — they analyze existing data but do not
modify the engine or its databases.

## Known-Math Catalog

Structural pattern matchers that identify mathematical properties from
expression structure, without relying on symbol names:

### Algebraic Properties

| Property | Detection Pattern |
|---|---|
| **Commutativity** | `f(a, b) = f(b, a)` — two-arg function invariant under argument swap |
| **Associativity** | `f(f(a, b), c) = f(a, f(b, c))` — nested binary op regrouping |
| **Identity element** | `f(a, e) = a` or `f(e, a) = a` — one argument is inert |
| **Inverse** | `f(a, g(a)) = e` — composing with some transform yields identity |
| **Distributivity** | `f(a, g(b, c)) = g(f(a, b), f(a, c))` — one op distributes over another |
| **Idempotence** | `f(a, a) = a` — self-application is identity |
| **Absorption** | `f(a, g(a, b)) = a` — one op absorbs the other |
| **Involution** | `f(f(a)) = a` — applying twice is identity |

### Structural Properties

| Property | Detection Pattern |
|---|---|
| **Fixed point** | `f(x) = x` for some x |
| **Periodicity** | `f^n(x) = x` for some n |
| **Monotonicity** | `a ≤ b → f(a) ≤ f(b)` (from evaluation traces) |
| **Linearity** | `f(a + b) = f(a) + f(b)` and `f(ca) = cf(a)` |
| **Homomorphism** | `f(g(a, b)) = h(f(a), f(b))` — structure preservation |

### Known Theorem Matching

Beyond property detection, attempt to match discovered equivalences
against a catalog of known mathematical identities:

- Peano arithmetic laws
- Ring/field axioms
- Group theory axioms
- Basic number theory (divisibility, primes)
- Combinatorial identities
- Lambda calculus reductions

The catalog is extensible — each entry is a pattern template with
a name, domain, and confidence threshold.

## Confidence Scoring

Each identification gets a confidence score (0.0 to 1.0):

- **1.0**: Exact structural match with the known pattern
- **0.8-0.99**: Match modulo variable renaming or argument reordering
- **0.5-0.79**: Partial match (e.g., commutativity holds for specific
  cases but not proven universally)
- **0.1-0.49**: Suggestive similarity (structural resemblance but no
  proof of equivalence)

Confidence factors:
- Proof status (verified > conjectured)
- Number of test cases passing
- Generality score from the engine
- Structural complexity match

## Representation Data

All output is structured JSON designed for React UI consumption.

### Discovery Timeline Entry

```json
{
  "epoch": 1234,
  "symbol": {
    "id": 42,
    "name": "S_042",
    "arity": 2,
    "lhs": "(op ?a ?b)",
    "rhs": "(op ?b ?a)"
  },
  "identification": {
    "property": "commutativity",
    "domain": "binary operation",
    "confidence": 0.95,
    "known_analog": "commutative law of addition",
    "latex": "a + b = b + a"
  },
  "metrics": {
    "compression_ratio_delta": 0.03,
    "novelty_score": 0.8,
    "generality": 0.72
  },
  "proof": {
    "status": "verified",
    "type": "equational",
    "step_count": 7,
    "lean_available": true
  }
}
```

### Expression Tree Visualization

```json
{
  "nodes": [
    {"id": "n1", "type": "apply", "label": "add"},
    {"id": "n2", "type": "symbol", "label": "square"},
    {"id": "n3", "type": "var", "label": "x"}
  ],
  "edges": [
    {"from": "n1", "to": "n2", "label": "arg0"},
    {"from": "n2", "to": "n3", "label": "arg0"}
  ],
  "metadata": {
    "depth": 3,
    "node_count": 3,
    "hash": "abc123..."
  }
}
```

### Symbol Relationship Graph

```json
{
  "nodes": [
    {"id": 1, "name": "add-identity", "epoch": 50, "identified_as": "additive identity"},
    {"id": 2, "name": "add-commute", "epoch": 120, "identified_as": "commutativity"},
    {"id": 3, "name": "add-assoc", "epoch": 340, "identified_as": "associativity"}
  ],
  "edges": [
    {"from": 2, "to": 1, "type": "depends_on"},
    {"from": 3, "to": 1, "type": "depends_on"},
    {"from": 3, "to": 2, "type": "depends_on"}
  ]
}
```

## Export Formats

### JSON API

The service binary exposes discovery data via REST endpoints:

| Endpoint | Returns |
|---|---|
| `GET /api/discoveries` | Paginated discovery timeline |
| `GET /api/discoveries/:id` | Single discovery with full detail |
| `GET /api/discoveries/:id/tree` | Expression tree visualization |
| `GET /api/discoveries/:id/proof` | Proof chain visualization |
| `GET /api/discoveries/graph` | Full symbol relationship graph |

### Static Export

`mathscape-db export --format json --output discoveries.json` produces
a self-contained JSON file with all discoveries, suitable for static
hosting or offline analysis.

### LaTeX

Each discovery can be exported as a LaTeX snippet:

```latex
\begin{theorem}[Commutativity — discovered epoch 120]
For all $a, b$: $a + b = b + a$
\end{theorem}
\begin{proof}
By equational reasoning (7 steps). See trace T-0042.
\end{proof}
```

## React UI Integration

The representation data is designed for a React frontend that:

1. **Timeline view**: Scroll through epochs, see discoveries as they emerge
2. **Library browser**: Browse all discovered symbols with identifications
3. **Expression visualizer**: Interactive tree rendering of expressions
4. **Proof inspector**: Step-through proof traces
5. **Relationship graph**: Force-directed graph of symbol dependencies
6. **Metrics dashboard**: Compression ratio, novelty, library size over time

The React UI is a separate repository/package. This crate produces the
data layer it consumes.
