# The Bettyfine

**bet·ty·fine** (n.) — the modal attractor basin of mathscape's
discovery moduli space. The dominant fixed point of the discovery
dynamics, where ~89% of seed-driven runs converge under the
combined equivalence of alpha-renaming + operator abstraction.

**bet·ty·fines** (pl.) — multiple distinct modal attractors, if a
configuration change produces more than one dominant basin at
equal-ish support (e.g., deep-bimodal regimes after machinery
additions).

## Why the name

Short, irreverent, pluralizes cleanly, carries zero prior technical
baggage. The formal triple-vocabulary name ("modal attractor basin
of the discovery moduli space") remains available when precision is
needed; "bettyfine" is the working noun.

## Equivalent formal names

Three established fields converge here:

- **Moduli space** (algebraic geometry) — classification space of
  possible discoveries modulo equivalence. Our moduli space has
  ~69 points at the operator-abstract layer at current scale.
- **Modal basin** (statistics) — the peak of the distribution over
  discovery outcomes. The 89% basin.
- **Attractor basin** (dynamical systems) — the region of seeds
  whose trajectories flow to a fixed point.

The bettyfine is the intersection of all three — a fixed point of
the discovery dynamics, the normal form under the equivalence
relation, and the modal (highest-support) point of the moduli
space.

## Empirical measurement (2026-04-18, at current machinery scale)

911 out of 1024 pure-procedural seeds (89.0%) land in the bettyfine
under alpha-equivalence + operator-abstract equivalence. The
bettyfine is a property of the MACHINE (discovery + reinforcement +
eager collapse), not of any particular seed. Different seeds are
different entry points into the moduli space; 89% of trajectories
flow to the same attractor.

## Related established concepts

- **Normal form** (term rewriting, Knuth-Bendix) — the canonical
  representative of an equivalence class under a confluent
  rewrite system. Our canonical library IS a normal form under
  alpha-equivalence.
- **Moduli space** (algebraic geometry) — classification space of
  objects up to isomorphism, with a generic stratum where most
  points land. The dominant basin is our generic stratum.
- **Orbit space** (group theory) — quotient of a set by a group
  action. Our group is fresh-id × operator-relabeling.

The dominant attractor basin sits at the intersection of all three
— a fixed point under the discovery dynamics, a normal form under
the equivalence relation, and the generic stratum of a quotient
space.

## What we measured

At 1024 pure-procedural seeds:

| Equivalence layer | Basin count | Compression |
|---|---|---|
| Nominal (S_NNN names as-is) | ~530 | — |
| Structural (anonymize fresh-ids) | 82 | 85% |
| Operator-abstract (anonymize ops too) | 69 | 87% total |

Modal operator-abstract basin: **911/1024 = 89.0%**.

The generic stratum — the 89% — is the Canonical Discovery Basin.
The remaining 11% scatter across 68 singular/long-tail strata.

## The Basin's contents

```
Rule 1: (unary_op ?x) => Symbol_K(unary_op, ?x)
Rule 2: (binary_op ?x ?y) => Symbol_K(binary_op, ?x, ?y)
```

Two rules. A unary reduction and a binary reduction, each producing
a named symbol that records the operator and args. This is the
Canonical Discovery Basin's complete content, regardless of which
specific operators (add vs mul vs succ) the seed's corpus happened
to sample.

## Optimizations enabled

With the Canonical Discovery Basin identified, every downstream path
can now be optimized against it:

### 1. Short-circuit discovery via canonical library

When a user invokes `/mathscape-traverse` and the input configuration
matches a cached `(budget, depth, generator_config)` → canonical-basin
mapping, serve the canonical library directly. No discovery loop
needs to run.

**Implementation hook**: `canonical_basin_cache: HashMap<ConfigHash,
CanonicalLibrary>` keyed by a hash of (BUDGET, DEPTH, ExtractConfig,
RewardConfig). Cold cache = run the ensemble, store. Warm cache =
instant library.

### 2. Canary detection for regressions

Any run that lands in one of the 68 long-tail strata is a *candidate
regression signal*. The generic stratum is the machine's
reliable behavior; drifting into a singular stratum means something
changed. Feed basin classification into CI: after a commit, run 32
seeds, assert ≥80% land in the generic basin. If not, investigate.

### 3. Short-circuit reinforcement

During a live traversal, if the current library has reached alpha-
equivalence with the canonical library's rules, the reinforcement
pass can immediately advance all Axiomatized candidates to the final
status — skipping the W-window wait. Saves epoch time on the happy
path.

### 4. Bootstrap new capabilities

Phases I/J/K each need a baseline to evaluate against. Starting the
evaluation from the canonical library (rather than re-discovering
from scratch) amortizes the bootstrap cost. Example: when running a
subterm-AU test, pre-load the canonical 2-rule library rather than
running 1000 seeds first.

### 5. Library compression in production

The canonical library (2 rules) is strictly smaller than any
single-seed discovery library (2-7 rules on average). Production
mathscape consumers — any downstream axiom-forge user — should
receive the canonical library as their minimal axiom set.

### 6. Coverage-complement corpus generation (phase M3 hook)

To push the machine's frontier, generate corpora that deliberately
avoid the canonical basin — aim for the long tail. 68 singular
strata are a map of "what the machine CAN discover but usually
doesn't." Each stratum is a potential research direction.

### 7. Seed-space indexing

Given the basin-to-seed map, any seed can be classified in O(1) by
hashing its canonical-library fingerprint. For a user who wants a
specific kind of discovery, seed selection becomes: pick a seed that
lands in the right basin. Map-reverse is cheap.

## Invariants the Basin must preserve

The canonical library should satisfy these empirically:

1. **Deterministic per config** — same (budget, depth, gen, reward)
   always produces the same canonical library modulo eager collapse.
2. **Modal support ≥ 80%** — if dominance falls below, the basin is
   drifting; investigate.
3. **Alpha-closed** — no two rules in the canonical library are
   alpha-equivalent (if they were, eager collapse would merge them).
4. **Operator-preserving for the canonical stratum** — the 2-rule
   shape survives all operator substitutions within the corpus's
   vocabulary.
5. **Cross-corpus earned** — every rule in the canonical library is
   Axiomatized, meaning it earned ≥ half-sweep cross-corpus support
   in every seed that reached the dominant basin.

## Testing discipline

Every optimization against the Canonical Discovery Basin ships with
a lock-in test:

- `canonical_basin_is_stable_under_seed_variation` — modal support
  measured, threshold 0.8
- `canonical_library_alpha_closed` — no internal redundancy
- `canonical_bootstrap_preserves_discoveries` — using the canonical
  as starting library doesn't lose any basin content on subsequent
  traversal

These are the M3 deliverables. Phase M3 is named in
`landmarks.md` as the next milestone.

## Relation to the amplituhedron musing

The user mused "I wonder how much it can relate to an actual
amplituhedron." The honest answer: the amplituhedron is a specific
positive geometry encoding scattering amplitudes in N=4 SYM; our
canonical basin is a specific attractor in a term-rewriting
discovery system. They're both canonical objects in their
respective theories, and both compress complex-looking variation
(many Feynman diagrams / many seeds) into a simpler underlying
structure. Whether there's a formal geometric correspondence is a
research question for when the machinery matures — phase M-deep.

For now: the Canonical Discovery Basin is NAMED, MEASURED,
STRUCTURED, and actionable. Optimizations lean on it starting now.
