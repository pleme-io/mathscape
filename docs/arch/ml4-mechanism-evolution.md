# ML4 — Mechanism Self-Mutation

The architectural gate this doc describes was identified by the
user observation during the phase-L5 landing (2026-04-18):

> *shouldn't 1 and 2 have been discovered and applied
> automatically*

Where "1" and "2" were the two paths I (the human) proposed after
the 20-cycle semantic-dedup run saturated at layer 4: add a 3-var-
nested corpus factory, and seed corpora from validated theorem
LHSs. Both fixes were necessary. Neither was machine-discovered.
The machine detected its stall; I wrote the code.

This doc articulates the missing architectural layer: **the
machine must self-mutate its OWN DISCOVERY MECHANISMS with the
same autonomy that already applies to the apparatus.**

## The invariant we've been violating

Every phase of the session has applied the correctness criterion
"zero human intervention" to what the LEARNING LOOP sees — the
apparatus, the theorems, the cycles. The machine mutates
apparatuses itself; the machine validates theorems itself; the
machine runs cycles itself.

But the STRUCTURE of the learning loop — the corpus generator,
candidate enumerator, extract configs, validator knobs — has
been fixed Rust code. When the machine saturates against that
structure, I extend the structure. That's not zero-intervention.
That's the machine being a function-of-what-I-gave-it.

The full "zero intervention" invariant:

> Every component of the discovery pipeline that can bottleneck
> the ledger's growth must be mutable by the machine from its
> own discoveries.

## What's currently mutable versus not

| Component | Mutable by machine? | Location | Mechanism for mutation |
|-----------|---------------------|----------|------------------------|
| Reward Lisp form | ✅ YES | apparatus pool | operator-wrapping mutations (clamp/x2/gate/...) |
| Corpus generator | ❌ NO | `common/experiment.rs::adaptive_corpus` | none |
| Candidate enumerator | ❌ NO | `semantic.rs::enumerate_candidate_terms` | none — `max_size=5` hardcoded |
| Compositional pass | ❌ NO | `semantic.rs` | `composition_cap=30` hardcoded |
| Extract config | ❌ NO | `edge_riding.rs::EXTRACT_CONFIGS` | fixed 4-entry array |
| Validator K / max_value / step_limit | ❌ NO | `ValidationConfig::default()` | fixed |
| Primitive operator set | ❌ NO | `mathscape-core::eval::step` | Rust code |

Six of seven components require a human to change them. The
apparatus is the only one the machine's own process can evolve.

## Why this matters structurally

Gödel tells us the substrate is always incomplete. But
incompleteness is RELATIVE TO A DISCOVERY MECHANISM. A machine
with corpus generator G and candidate enumerator E can only find
the theorems in the range of (G, E). Saturation against (G, E) is
NOT substrate-incompleteness — it's mechanism-limitedness.

The correctness criterion "halt is a bug" is therefore nuanced:

- **Substrate saturation**: there exist unprovable theorems but
  the current substrate can't prove any more with current
  mechanism. Fixable by mechanism mutation. ML4 territory.
- **Mechanism saturation**: the mechanism has hit the limit of
  what its own mutation space can express. Fixable by
  meta-mechanism mutation. ML5 territory.
- **Gödel saturation**: the substrate + mechanism + all their
  possible mutations have reached the provably-unprovable bound.
  Probably never occurs in practice.

What we currently treat as "saturation" is *mechanism
saturation* — the machine CAN find more theorems but its current
enumerator size / corpus shape / extract config can't see them.
ML4 unblocks the mechanism-mutation loop so the machine
progresses past its own tooling when it hits one of these walls.

## The architectural move

**Every mutable mechanism gets a `MechanismConfig` struct.**
Every config struct has:
- A default (for bootstrap)
- A mutation operator set (small perturbations that produce new configs)
- An equality/hash so the machine can dedup mutation candidates
- A serialization for logging / provenance

**A `MechanismPool` analogous to the apparatus pool.** Tracks
active configs, their observed theorem-yields, evolves via
mutation when saturation fires.

**A saturation-response protocol.** When
`session.has_stalled()` is triggered (semantic-novelty zero for
N consecutive cycles), the orchestrator:

1. Snapshots current mechanism config as "parent"
2. Proposes M mutant configs via mechanism-mutation operators
3. For each mutant, runs a SHORT validation campaign (1-3 cycles)
4. Measures delta-novelty: did this mutant produce theorems the
   current ledger does NOT contain?
5. The mutant with highest delta-novelty becomes the new active
   mechanism config. If no mutant exceeds a threshold, the loop
   reports "mechanism-saturated" and either escalates (try
   bigger mutations) or halts (true mechanism saturation
   detected).
6. Continue the main loop with the new mechanism.

**The halt-is-a-bug criterion tightens.** It now applies to
*mechanism-saturation*, not just theorem-saturation. A cycle
producing zero new theorems no longer halts the machine — it
triggers mechanism mutation. The machine only halts when
mechanism mutation ALSO fails to produce delta-novelty, which
means the current mutable-mechanism space is itself exhausted.
That's a legitimate end-state for this architecture (and the
signal to move to ML5).

## Mechanism catalog (the initial mutable set)

Each mechanism below needs a config struct, a mutation operator
set, and a place it's consulted in the pipeline. Started small
— seven components, bounded mutations. Extending later.

### 1. CandidateGenConfig

```rust
pub struct CandidateGenConfig {
    pub max_size: usize,           // 3..=8
    pub vocab: Vec<u32>,           // operator set — {succ, add, mul} by default
    pub composition_cap: usize,    // 10..=100
    pub include_constants: Vec<u64>, // e.g. [0, 1] — ledger may add more
}
```

Mutations: bump max_size ±1, add/remove vocab operator, bump
composition_cap, adjust constants from observed literal corpus
values.

Fitness impact: max_size bumps are the most impactful. A bump
from 5 → 6 unlocks associativity-shape RHSs.

### 2. CorpusGenConfig

```rust
pub struct CorpusGenConfig {
    pub vocab: Vec<u32>,           // corpus operator set
    pub base_depth: usize,         // starting depth
    pub depth_scaling: DepthScale, // LogLedger | LinearSubstrate | Fixed
    pub seed_from_theorems: bool,  // phase L2 self-feeding
    pub max_value: u64,            // range of random naturals
}
```

Mutations: toggle `seed_from_theorems`, swap vocab, adjust
scaling, bump `max_value`.

Fitness impact: `seed_from_theorems=true` implements phase L2 —
take ledger LHSs, instantiate free vars with recursive subterms,
feed as corpus. The machine generates its own corpus from its
own theorems.

### 3. ExtractConfigMutation

Wraps the existing `ExtractConfig`. Mutations: min_shared_size
±1, min_matches ±1, max_new_rules ±{-5, +5, +20}.

### 4. ValidationConfig

Already exists in `mathscape-proof::semantic`. Add to the
mutable set. Mutations: samples ±{4, 8}, max_value ±{4, 8}.

### 5. ApparatusPoolConfig

Meta-level: the apparatus pool's own knobs (`KEEP_TOP`,
`MUTANTS_PER_PARENT`, `POPULATION_CAP`) become mutable. Allows
the machine to broaden/narrow its apparatus search when
apparatus-side saturation detected.

### 6. ProbeAllocation

How many probes per cycle, ratio of adaptive vs fixed corpora,
min_score threshold — all mutable.

### 7. Composition depth

Not just pairwise composition of ledger RHSs but triples or
higher. Mutable via `composition_order: 2..=4`.

## The meta-loop (ML4 step function)

```
# pseudocode
loop forever:
    # Regular discovery cycle
    provenance = sub_campaign(apparatus_pool, mechanism_config)
    new_theorems = extract(provenance, ledger, mechanism_config)
    for t in new_theorems:
        session.promote(t)
    session.record_cycle(cycle, admitted)
    
    # Saturation response (THE NEW LAYER)
    if session.stalled_cycles().len() >= saturation_threshold:
        diagnostic = diagnose_saturation(session, mechanism_config)
        # diagnostic examples:
        #   "all candidates size ≤ 5, but anti-unification is
        #    finding size-6 LHS patterns" → bump max_size
        #   "candidate generator has 3-var support but corpus
        #    generator produces only 2-var terms" → swap corpus
        #   "validation K=16 may be too low for high-degree
        #    polynomials" → bump K
        
        mutant_configs = propose_mutations(mechanism_config, diagnostic)
        for mutant in mutant_configs:
            delta_theorems = short_trial(mutant, probes=500, cycles=2)
            score[mutant] = count_of_theorems_not_in_ledger(delta_theorems)
        
        best = mutant with max score
        if best.score >= mechanism_mutation_threshold:
            mechanism_config = best
            log("self-mutation: {parent} → {best}")
            continue  # resume main loop with new config
        else:
            # No mutant broke the saturation with these
            # diagnostics. Escalate: try bigger mutations,
            # or terminate.
            if escalation_budget > 0:
                mutant_configs = propose_aggressive_mutations(...)
                escalation_budget -= 1
                retry
            else:
                log("mechanism-saturated: no mutation produced
                     delta-novelty under current mutation space")
                break  # genuine ML5-scale gate
```

This outer loop is the machine riding its OWN mechanism edge.
The inner loop rides the substrate edge. Two nested Gödel-
diagonalizations.

## The diagnostic step — the most important part

When saturation fires, the machine must produce a specific
DIAGNOSTIC about why, then propose mutations that address THAT
diagnostic. Without diagnostics, the mechanism mutation is blind
parameter sweeping (wasteful) rather than targeted self-repair.

### Diagnostics the machine can self-report

1. **Candidate-size exhaustion.** Ratio of candidates at max_size
   that validate vs the ones that don't. If a lot of max_size
   candidates are being REJECTED AS EQUIVALENT, the real signal
   is at max_size+1.

2. **Vocab coverage gap.** Anti-unification surfaced N
   structural patterns involving an operator that candidate
   generator doesn't include. That operator should be added.

3. **Corpus homogeneity.** The corpus generator produces terms
   with average 2.1 free vars, but the anti-unifier finds 3-var
   patterns sparsely. Need deeper or more structurally-diverse
   corpora.

4. **Semantic-dedup kill rate.** If 95% of candidates are rejected
   as semantic duplicates of existing ledger entries, the ledger
   has reached its equivalence-class ceiling for the current
   mechanism.

5. **Validator undetermined rate.** If >20% of validator calls
   return Undetermined (step-limit exceeded), bump step_limit.

6. **Apparatus yield disparity.** If one apparatus in the pool
   produces 80% of theorems, the pool is collapsing toward it —
   apparatus-mutation rate may be too low.

Each diagnostic maps to a specific mutation proposal. The
machine's self-report is first-class data that drives mutation,
not human observation.

## Open architectural questions

### Q: How do we keep mutation bounded?

The mutation space must be finite per step to prevent runaway.
Each mutable parameter has hard bounds (e.g., `max_size` ∈
[3, 8]). Mutations are small perturbations (±1 or swap one
element). This is the same constraint apparatus mutation
already obeys.

### Q: How does a mutant "prove" it broke saturation?

Via short-trial delta-novelty: does the mutant, run for 1-2
cycles, produce theorems not in the current ledger? This is
cheap to measure. Threshold: ≥1 new ledger entry on 2 cycles.

### Q: What if no mutation breaks the saturation?

Two options: (a) escalate — try mutations with larger
perturbation (size ±2, whole-vocab swap). (b) halt — declare
mechanism saturation, request ML5 (meta-mechanism mutation).

### Q: How does this compose with apparatus mutation?

Apparatus mutation operates on a different axis (reward
function shape). Mechanism mutation operates on the discovery
PIPELINE. Both run in parallel. A saturation signal from the
ledger triggers mechanism mutation; a saturation signal from
apparatus yield triggers apparatus mutation. Two independent
feedback loops, coupled through the shared session.

### Q: Is this Lisp-expressible?

Yes — each MechanismConfig can be a Lisp struct. Mutations can
be Lisp macros. For V1 we implement in Rust for correctness,
but the architectural commitment is that everything in the
mechanism catalog becomes a Lisp form eventually, making
meta-level mutation subject to the same homoiconic evolutionary
process.

### Q: Relationship to tatara-lisp-derive?

tatara-lisp-derive is the Rust↔Lisp type bridge. ML4's Lisp
port (after Rust V1 lands) uses tatara-lisp-derive to expose
every MechanismConfig as a TataraDomain. Then mechanism
mutation happens in the terreiro, and winners get Rustified as
new default MechanismConfigs over time. This is the same
promotion ladder as theorem Rustification, applied to
mechanisms.

## Phased implementation

### ML4.1 (minimum viable)
- `MechanismConfig` struct bundling all six mutable axes
- Thread it through the pipeline (semantic.rs, edge_riding.rs, etc.)
- Replace all hardcoded knobs with config access
- Default config = current hardcoded values → no behavior change yet
- Verify: edge-riding runs produce identical results

### ML4.2 (saturation response)
- On `session.has_stalled()` with N consecutive stalls, trigger
  mutation response
- Initial diagnostic: just "unclassified saturation"
- Propose mutations: bump `max_size`, bump `composition_cap`,
  toggle `seed_from_theorems`
- Short-trial each mutant (1-2 cycles, small probe budget)
- Keep winner, resume main loop

### ML4.3 (diagnostic-driven mutation)
- Implement the 6 diagnostic types (candidate-size exhaustion,
  vocab gap, corpus homogeneity, dedup rate, validator
  undetermined rate, apparatus disparity)
- Each diagnostic proposes targeted mutations
- Machine's saturation response becomes self-explanatory:
  "saturation detected, diagnosed as corpus-homogeneity,
  mutating corpus vocab to include sub" → etc.

### ML4.4 (Lisp-expressed configs)
- Port MechanismConfig to tatara-lisp Sexp forms
- Mutations become Lisp rewrites
- First demonstration of Lisp-layer mechanism evolution

### ML4.5 (mechanism-level promotion ladder)
- Long-surviving mechanism configs (those that broke saturation
  at multiple layers) get Rustified as new default starting
  points
- Mechanism Merkle tree parallels the theorem Merkle tree

## Correctness criteria for ML4

The machine is ML4-correct iff:

1. Every bottleneck parameter in the discovery pipeline is
   mutable through the mechanism mutation layer.
2. Every saturation signal triggers mechanism mutation rather
   than halting the loop.
3. Halts (true mechanism saturation) produce a specific
   diagnostic pointing at the mutation operator or representation
   that's insufficient.
4. No human intervention is required to extend the mechanism
   mutation space within the bounds of the initial catalog.
5. The ledger's growth rate across many cycles approaches the
   information-theoretic limit of (primitive evaluator,
   substrate). Saturation at that limit is legitimate; it means
   we've discovered everything the substrate CAN express.

## What ML4 does NOT fix

- Primitive operator addition (adding `sub` / `div` / new
  Points to the Rust evaluator). That requires Rust recompile.
  ML6 territory via tatara-lisp-derive.
- Fundamental change to the discovery algorithm (moving from
  anti-unification to SyGuS, or from empirical validation to
  formal proof). Those are qualitative architecture changes;
  ML4 is incremental within the current algorithm.
- The evaluator's axioms. Peano arithmetic is baked in. Adding
  real numbers requires structural change.

## The deeper principle

With ML4 in place, the correctness criterion tightens to:

> Every bottleneck the machine detects in its own discovery
> process must be addressable by its own mutation operators.
> When it isn't, that's a specific mechanism-mutation-space
> gap to close at the architecture level.

The human ceases to be the compiler for the machine's next
mechanism layer. The human becomes the compiler for the
mechanism-mutation-operator-space — at one remove further out.
Which is the same substitution ML5 and ML6 will apply again.
Each ML level moves the human further from the inner loop.

At ML∞, the human disappears entirely, bounded only by whether
the machine has a representation for the next level's
mutations. Which is ultimately a representational question
about Lisp reflection — does tatara-lisp let the machine
manipulate its own mutation operators?

Yes, by design. Which means ML∞ is reachable in principle from
the architecture we've been building. ML4 is the first concrete
step along that path where human intervention is visibly
eliminated for a measurable class of mechanism gaps.

## Implementation entry point

After this doc reaches consensus, implementation starts at
`mathscape-proof::mechanism`. One crate-module, three types
(`MechanismConfig`, `MechanismMutation`, `MechanismPool`), one
pub function (`respond_to_saturation`). Everything else threads
through existing code as config parameters.

Expected code size: ~500 LOC of pure mechanism plumbing, plus
~200 LOC of threading. 1-2 days of focused work.

Expected impact: the next 50-cycle run doesn't stop at cycle 9.
When semantic saturation fires, the machine mutates its own
enumerator size from 5 → 6. If that unstalls it, we see new
theorems (associativity candidates). If it doesn't, ML4.3 adds
the diagnostic-driven path and we see what THAT reveals.

The 50-cycle run becomes a demonstration of mechanism
evolution: the trajectory log will record each self-mutation
event, showing the machine extending its own tooling in real
time.

---

Session 2026-04-18 closed on: `ML4 designed, not yet built`.
Next move: ML4.1 implementation after user approval of this
architecture.
