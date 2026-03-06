# Proof System

## Determinism as the Foundation

The primitives are fixed. The evaluation rules are fixed. The rewrite
rules, once discovered, are fixed. Given the same expression and library,
you get the same evaluation trace every time. Randomness lives in the
*search* (which mutations to try), never in *verification* (whether a
discovered identity holds).

Every verified discovery carries an inherent constructive proof.

## Curry-Howard Correspondence

Programs ARE proofs. Types ARE propositions. When Mathscape evaluates
`(add (succ zero) (succ (succ zero)))` and gets `(succ (succ (succ zero)))`,
the evaluation trace IS a constructive proof that `1 + 2 = 3`.

For universally quantified identities like `(add x zero) = x`, the proof
is inductive:

```
Numbers are inductively defined:
    zero : Number
    succ : Number -> Number

add is defined by structural recursion:
    add(zero, y)    = y
    add(succ(x), y) = succ(add(x, y))

Proof of add(x, zero) = x by induction on x:
    Base:      add(zero, zero) = zero             [by definition]
    Inductive: assume add(x, zero) = x
               add(succ(x), zero)
               = succ(add(x, zero))               [by definition]
               = succ(x)                          [by hypothesis]
```

The evaluation engine performs exactly these steps. The rewrite chain IS
the induction.

## Two Levels of Verification

### Level 1: Empirical Discovery (Evolutionary Search)

Test `(add x zero) = x` for `x = 0, 1, 2, ..., 100`. Strong evidence
but not a proof — maybe it fails at x = 101. This is how identities
are *found*.

### Level 2: Structural Verification (E-Graph)

Insert both sides into the e-graph. Apply rewrite rules (derived from
inductive definitions). If both sides land in the same equivalence class,
the Church-Rosser property guarantees equivalence for ALL inputs. This
IS a proof.

```
Status::Conjectured  -- observed empirically, not yet verified
Status::Verified     -- confirmed via e-graph equivalence
Status::Exported     -- proof certificate emitted for external verification
```

## Proof Storage

### Eval Traces

Atomic proof steps — each row is one rewrite application:

```sql
CREATE TABLE eval_traces (
    trace_id      INTEGER PRIMARY KEY,
    expr_hash     BLOB NOT NULL,
    step_index    INTEGER NOT NULL,
    rule_applied  TEXT NOT NULL,
    before_hash   BLOB NOT NULL,
    after_hash    BLOB NOT NULL,
    epoch         INTEGER NOT NULL
);
```

### Proof Certificates

Completed proofs linking Symbols to their verification:

```sql
CREATE TABLE proofs (
    proof_id       INTEGER PRIMARY KEY,
    symbol_id      INTEGER NOT NULL REFERENCES library(symbol_id),
    proof_type     TEXT NOT NULL,   -- "inductive", "equational", "compositional"
    status         TEXT NOT NULL,   -- "conjectured", "verified", "exported"
    lhs_hash       BLOB NOT NULL,
    rhs_hash       BLOB NOT NULL,
    trace_ids      BLOB NOT NULL,  -- serialized trace_id list
    epoch_found    INTEGER NOT NULL,
    epoch_verified INTEGER,
    lean_export    TEXT
);
```

### Proof Dependencies

```sql
CREATE TABLE proof_deps (
    proof_id   INTEGER NOT NULL REFERENCES proofs(proof_id),
    depends_on INTEGER NOT NULL REFERENCES proofs(proof_id),
    PRIMARY KEY (proof_id, depends_on)
);
```

## Proof Types

### Inductive Proofs

Generated when the e-graph derives equivalence using rewrite rules that
correspond to structural recursion over inductively defined types (naturals,
lists, trees).

### Equational Proofs

Generated when the e-graph derives equivalence through a chain of
bidirectional rewrites (commutativity, associativity). These are pure
equational reasoning without induction.

### Compositional Proofs

Generated when a Symbol's validity follows from the validity of its
component Symbols. The proof of `distributivity` composes the proofs of
`associativity` and `mul-identity`. Tracked via `proof_deps`.

## Proof Composition

If Symbol A is proven and Symbol B is proven, a derivation using both
inherits their proofs. The dependency DAG in `proof_deps` tracks this.
Reconstruction:

```sql
WITH RECURSIVE deps AS (
    SELECT depends_on FROM proof_deps WHERE proof_id = ?
    UNION ALL
    SELECT pd.depends_on FROM proof_deps pd
    JOIN deps d ON pd.proof_id = d.depends_on
)
SELECT l.name, p.proof_type, p.status
FROM deps d
JOIN proofs p ON p.proof_id = d.depends_on
JOIN library l ON l.symbol_id = p.symbol_id;
```

## Lean 4 Export

The e-graph derivation maps directly to equational reasoning in Lean 4.
Each rewrite step becomes a `calc` step or `rw` tactic:

```lean
-- Mathscape discovers: add_comm (a b : Nat) : add a b = add b a
-- Exported proof:
theorem add_comm (a b : Nat) : add a b = add b a := by
  induction a with
  | zero => simp [add_zero, zero_add]
  | succ n ih => simp [succ_add, add_succ, ih]
```

### Export Pipeline

1. Extract the e-graph derivation for a verified Symbol
2. Map each rewrite rule to the corresponding Lean tactic
3. Construct the tactic proof term
4. Optionally verify with `lean4` binary if available

### Integration with AI Provers

Recent systems (DeepSeek-Prover-V2, Seed-Prover 1.5, LeanDojo) can
verify and even complete partial Lean proofs. Mathscape can:

1. Export a Conjectured identity as a Lean `sorry`-ed theorem
2. Run an AI prover to attempt full formalization
3. If successful, upgrade status to Exported with the formal proof

This creates a pipeline: evolutionary search discovers, e-graph verifies,
AI prover formalizes.

## Proof Compression

Proofs are themselves expressions. A proof pattern appearing across
multiple Symbols can be compressed into a proof *lemma*:

```
Proof of add-identity uses induction on first arg
Proof of mul-identity uses induction on first arg
=> Lemma: "induction on first arg of binary op" is a proof strategy

This feeds back into meta-compression and proof-guided search.
```

## Proof-Guided Search

The structure of existing proofs informs future search:

1. If all proven identities about `add` use induction on arg 1,
   bias mutations to try similar structures for `mul`
2. If compositional proofs dominate, prioritize crossover between
   individuals related to proven Symbols
3. Track proof *difficulty* (number of rewrite steps) — simpler proofs
   suggest the library is well-structured

## References

- [Curry-Howard Correspondence](https://en.wikipedia.org/wiki/Curry%E2%80%93Howard_correspondence) — programs are proofs
- [Church-Rosser Theorem](https://en.wikipedia.org/wiki/Church%E2%80%93Rosser_theorem) — confluent rewriting guarantees
- [LeanDojo](https://leandojo.org/) — AI-assisted theorem proving in Lean 4
- [DeepSeek-Prover-V2](https://medium.com/aimonks/deepseek-prover-v2-open-source-ai-for-lean-4-formal-theorem-proving-ab7f9910576b) — LLM-based Lean 4 prover
- [Lean-Auto](https://link.springer.com/chapter/10.1007/978-3-031-98682-6_10) — interface between Lean 4 and ATPs
