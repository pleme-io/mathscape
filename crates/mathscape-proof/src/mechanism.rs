//! ML4+ML5 — mechanism self-mutation with a growable operator set.
//!
//! Every bottleneck parameter in the discovery pipeline is bundled
//! into a `MechanismConfig` that the machine can mutate when it
//! detects saturation. Zero human intervention for parameter
//! bumps.
//!
//! # The self-feeding topology
//!
//! This module is one of several places the system feeds its own
//! output back as input. Being explicit about where and how the
//! feedback happens — so we can reason about correctness at each
//! self-feeding loop.
//!
//! ```text
//!   Level        What feeds what                       Fitness signal
//!   ─────        ───────────────                       ──────────────
//!   L1: data     ledger RHSs → candidate generator     rule validates
//!   L2: corpus   theorem LHSs → corpus terms           post-reduce residue has structure
//!   L3: apparatus apparatus pool → new apparatuses     apparatus theorem yield
//!   L4: mechanism mechanism pool → new configs         delta-novelty under new config
//!   L5: operator  pool.discovered_operators →          mutation-produces-delta-novelty
//!                 new operator proposals               AT COMPOUND LEVEL
//!   L6+: ...      meta-operator proposals →            (not yet built)
//!                 new meta-operators
//! ```
//!
//! Each level has THE SAME STRUCTURAL PATTERN:
//!   - A *bootstrap set* (fixed-at-this-level inputs)
//!   - A *mutation mechanism* (how to produce variants)
//!   - A *trial* (measure whether variant produces novelty)
//!   - A *promotion* (winner gets added back to the input pool)
//!
//! Every level's bootstrap set is the next level's discovered
//! output. Making L(N+1) unlock L(N)'s static bootstrap set.
//! ML5 here makes the mutation OPERATOR SET (previously static
//! enum variants) into a growable pool. ML6 will make the
//! compound-generation strategy itself growable. And so on.
//!
//! Gödel guarantees there's always an N+1. Each level we build
//! is one more place where we're explicit about the self-feeding.
//! The architectural risk is FORGETTING a self-feeding loop exists
//! — then we hand-code where the machine should discover, and the
//! human-as-compiler pattern reasserts.
//!
//! # How this module's self-feeding works (ML5 specifically)
//!
//! - `MechanismPool::discovered_operators` starts empty.
//! - When `respond_to_saturation` finds a COMPOUND mutation that
//!   produces delta-novelty, the compound is promoted to the
//!   discovered_operators pool.
//! - Subsequent calls to `propose_random_mutations` sample from
//!   (atomic variants) ∪ (pool.discovered_operators) — the space
//!   grows each time a compound wins.
//! - An atomic mutation that wins does NOT get added, because
//!   it's already in the bootstrap set.
//! - An atomic-like compound (e.g., `Compound(vec![single_atom])`)
//!   would be a redundant addition; we skip them.
//!
//! The output of the self-mutation loop (winning compound
//! operators) feeds back as INPUT to the same loop at the next
//! saturation. Level 5 self-reference, closed.
//!
//! # The correctness tightening this enables
//!
//! Before ML4: saturation signal → test terminates → human reads
//! diagnostic → human writes Rust → next run uses new parameters.
//!
//! After ML4: saturation signal → orchestrator calls
//! `respond_to_saturation(current_config, session, trial_runner,
//! escalation_budget)` → machine proposes mutations (random sweep
//! as fallback per user direction) → each mutant runs a short
//! trial → winner (if any) becomes new config → main loop resumes.
//!
//! # Scope of the initial catalog
//!
//! Six mutable axes bundled. All hand-picked parameters from the
//! current pipeline. Diagnostic richness is deliberately minimal
//! (ML4.2 level) — we rely on saturations that actually occur to
//! tell us which diagnostics matter, rather than speculating.

use mathscape_core::eval::RewriteRule;
use std::collections::HashSet;

// ── MechanismConfig ──────────────────────────────────────────────

/// Bundled configuration for every parameter the machine can
/// mutate. Default = current hardcoded values (preserves prior
/// behavior). Mutation operators produce bounded perturbations.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MechanismConfig {
    // ── Candidate generation (semantic.rs::generate_semantic_candidates_with_ledger) ──
    /// Max term size for the enumerator. Size 5 reaches
    /// distributivity RHSs; size 6 reaches associativity; cost
    /// grows ~Catalan per +1.
    pub candidate_max_size: usize,
    /// Operators available in enumeration. Default [1, 2, 3] =
    /// {succ, add, mul}. Adding operators here only helps if the
    /// evaluator has semantics for them (currently only these
    /// three plus zero-as-constant).
    pub candidate_vocab: Vec<u32>,
    /// Max pairs of ledger shapes combined per candidate pass.
    pub composition_cap: usize,
    /// Small naturals proposed as candidate RHSs directly.
    pub candidate_constants: Vec<u64>,

    // ── Corpus generation (common/experiment.rs::adaptive_corpus) ──
    /// Operators in adaptive-corpus terms. Broader vocab →
    /// patterns involving rarer operators. But validator
    /// requires evaluator support, so practical effect is
    /// limited to {succ, add, mul, zero}.
    pub corpus_vocab: Vec<u32>,
    /// Baseline corpus depth before substrate-scaling kicks in.
    pub corpus_base_depth: usize,
    /// Phase L2: seed corpus terms by instantiating validated
    /// theorems' LHSs with recursive subterms. Initial=false
    /// (baseline), saturation-response may flip to true.
    pub corpus_seed_from_theorems: bool,
    /// Range of random naturals for corpus leaves.
    pub corpus_max_value: u64,

    // ── Extract config (mathscape-compress::ExtractConfig) ──
    pub extract_min_shared_size: usize,
    pub extract_min_matches: usize,
    pub extract_max_new_rules: usize,

    // ── Validator ──
    pub validator_samples: usize,
    pub validator_max_value: u64,
    pub validator_step_limit: usize,
}

impl Default for MechanismConfig {
    fn default() -> Self {
        Self {
            candidate_max_size: 5,
            candidate_vocab: vec![1, 2, 3], // succ, add, mul
            composition_cap: 30,
            candidate_constants: vec![0, 1],
            corpus_vocab: vec![2, 3, 4, 5, 7], // add, mul, succ, sub, pred
            corpus_base_depth: 4,
            corpus_seed_from_theorems: false,
            corpus_max_value: 10,
            extract_min_shared_size: 2,
            extract_min_matches: 2,
            extract_max_new_rules: 8,
            validator_samples: 24,
            validator_max_value: 10,
            validator_step_limit: 96,
        }
    }
}

// ── M1: Lisp Sexp round-trip ─────────────────────────────────────
//
// Every mutable parameter is now also expressible as a
// tatara-lisp Sexp form. The Sexp form IS the authoritative
// serialization; Rust struct is the runtime cache. The machine's
// own apparatus-mutation loop can operate on the Sexp directly,
// and from here on any migration to Lisp-evaluated fitness
// functions or mutation operators uses this round-trip as the
// shared boundary.

impl MechanismConfig {
    /// Serialize to a Sexp form:
    /// `(config :candidate-max-size 5 :composition-cap 30 ...)`
    pub fn to_sexp(&self) -> tatara_lisp::ast::Sexp {
        use tatara_lisp::ast::Sexp;
        let mut items: Vec<Sexp> = vec![Sexp::symbol("config")];
        items.push(Sexp::keyword("candidate-max-size"));
        items.push(Sexp::int(self.candidate_max_size as i64));
        items.push(Sexp::keyword("candidate-vocab"));
        items.push(Sexp::List(
            self.candidate_vocab
                .iter()
                .map(|v| Sexp::int(*v as i64))
                .collect(),
        ));
        items.push(Sexp::keyword("composition-cap"));
        items.push(Sexp::int(self.composition_cap as i64));
        items.push(Sexp::keyword("candidate-constants"));
        items.push(Sexp::List(
            self.candidate_constants
                .iter()
                .map(|v| Sexp::int(*v as i64))
                .collect(),
        ));
        items.push(Sexp::keyword("corpus-vocab"));
        items.push(Sexp::List(
            self.corpus_vocab
                .iter()
                .map(|v| Sexp::int(*v as i64))
                .collect(),
        ));
        items.push(Sexp::keyword("corpus-base-depth"));
        items.push(Sexp::int(self.corpus_base_depth as i64));
        items.push(Sexp::keyword("corpus-seed-from-theorems"));
        items.push(Sexp::boolean(self.corpus_seed_from_theorems));
        items.push(Sexp::keyword("corpus-max-value"));
        items.push(Sexp::int(self.corpus_max_value as i64));
        items.push(Sexp::keyword("extract-min-shared-size"));
        items.push(Sexp::int(self.extract_min_shared_size as i64));
        items.push(Sexp::keyword("extract-min-matches"));
        items.push(Sexp::int(self.extract_min_matches as i64));
        items.push(Sexp::keyword("extract-max-new-rules"));
        items.push(Sexp::int(self.extract_max_new_rules as i64));
        items.push(Sexp::keyword("validator-samples"));
        items.push(Sexp::int(self.validator_samples as i64));
        items.push(Sexp::keyword("validator-max-value"));
        items.push(Sexp::int(self.validator_max_value as i64));
        items.push(Sexp::keyword("validator-step-limit"));
        items.push(Sexp::int(self.validator_step_limit as i64));
        Sexp::List(items)
    }

    /// Parse from a Sexp form. Returns `None` if the form is
    /// malformed or missing required fields — the caller must
    /// handle invalid forms (treat as a failed mutation).
    pub fn from_sexp(sexp: &tatara_lisp::ast::Sexp) -> Option<Self> {
        use tatara_lisp::ast::Sexp;
        let items = sexp.as_list()?;
        if items.is_empty() || items[0].as_symbol() != Some("config") {
            return None;
        }
        let mut out = Self::default();
        let mut i = 1;
        while i + 1 < items.len() {
            let key = items[i].as_keyword()?;
            let val = &items[i + 1];
            match key {
                "candidate-max-size" => {
                    out.candidate_max_size = val.as_int()? as usize;
                }
                "candidate-vocab" => {
                    out.candidate_vocab = val
                        .as_list()?
                        .iter()
                        .filter_map(|s| s.as_int().map(|n| n as u32))
                        .collect();
                }
                "composition-cap" => {
                    out.composition_cap = val.as_int()? as usize;
                }
                "candidate-constants" => {
                    out.candidate_constants = val
                        .as_list()?
                        .iter()
                        .filter_map(|s| s.as_int().map(|n| n as u64))
                        .collect();
                }
                "corpus-vocab" => {
                    out.corpus_vocab = val
                        .as_list()?
                        .iter()
                        .filter_map(|s| s.as_int().map(|n| n as u32))
                        .collect();
                }
                "corpus-base-depth" => {
                    out.corpus_base_depth = val.as_int()? as usize;
                }
                "corpus-seed-from-theorems" => {
                    out.corpus_seed_from_theorems = matches!(
                        val,
                        Sexp::Atom(tatara_lisp::ast::Atom::Bool(true))
                    );
                }
                "corpus-max-value" => {
                    out.corpus_max_value = val.as_int()? as u64;
                }
                "extract-min-shared-size" => {
                    out.extract_min_shared_size = val.as_int()? as usize;
                }
                "extract-min-matches" => {
                    out.extract_min_matches = val.as_int()? as usize;
                }
                "extract-max-new-rules" => {
                    out.extract_max_new_rules = val.as_int()? as usize;
                }
                "validator-samples" => {
                    out.validator_samples = val.as_int()? as usize;
                }
                "validator-max-value" => {
                    out.validator_max_value = val.as_int()? as u64;
                }
                "validator-step-limit" => {
                    out.validator_step_limit = val.as_int()? as usize;
                }
                _ => {}
            }
            i += 2;
        }
        out.clamp();
        Some(out)
    }

    /// Bounded: enforce hard limits so pathological mutations
    /// don't corrupt subsequent trials.
    pub fn clamp(&mut self) {
        self.candidate_max_size = self.candidate_max_size.clamp(3, 8);
        self.composition_cap = self.composition_cap.clamp(5, 120);
        self.corpus_base_depth = self.corpus_base_depth.clamp(2, 8);
        self.corpus_max_value = self.corpus_max_value.clamp(4, 64);
        self.extract_min_shared_size = self.extract_min_shared_size.clamp(2, 6);
        self.extract_min_matches = self.extract_min_matches.clamp(2, 6);
        self.extract_max_new_rules = self.extract_max_new_rules.clamp(3, 24);
        self.validator_samples = self.validator_samples.clamp(8, 64);
        self.validator_max_value = self.validator_max_value.clamp(4, 32);
        self.validator_step_limit = self.validator_step_limit.clamp(32, 256);
        // Vocab: keep at least one operator.
        if self.candidate_vocab.is_empty() {
            self.candidate_vocab.push(2);
        }
        if self.corpus_vocab.is_empty() {
            self.corpus_vocab.push(2);
        }
    }

    /// Summary string for logging.
    pub fn brief(&self) -> String {
        format!(
            "cms={}, ccap={}, cbd={}, sft={}, emss={}, vsa={}, emn={}",
            self.candidate_max_size,
            self.composition_cap,
            self.corpus_base_depth,
            self.corpus_seed_from_theorems,
            self.extract_min_shared_size,
            self.validator_samples,
            self.extract_max_new_rules,
        )
    }
}

// ── MechanismMutation ───────────────────────────────────────────

/// One atomic change to a MechanismConfig. Bounded perturbations —
/// each variant moves the config by a small step. The full
/// mutation space is this enum's cartesian with the config's
/// parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MechanismMutation {
    BumpCandidateMaxSize(i32),
    AddCandidateVocabOp(u32),
    RemoveCandidateVocabOp(u32),
    BumpCompositionCap(i32),
    AddCandidateConstant(u64),
    AddCorpusVocabOp(u32),
    RemoveCorpusVocabOp(u32),
    BumpCorpusBaseDepth(i32),
    SetCorpusSeedFromTheorems(bool),
    BumpCorpusMaxValue(i32),
    BumpExtractMinShared(i32),
    BumpExtractMinMatches(i32),
    BumpExtractMaxNewRules(i32),
    BumpValidatorSamples(i32),
    BumpValidatorMaxValue(i32),
    BumpValidatorStepLimit(i32),
    /// ML5: a compound mutation. Applies each child sequentially
    /// to the config. A compound that breaks saturation gets
    /// promoted to `MechanismPool::discovered_operators`, making
    /// it available as a first-class operator in future saturation
    /// responses. The mutation SPACE thus grows as the machine
    /// discovers which combinations of atomic mutations work.
    Compound(Vec<MechanismMutation>),
}

impl MechanismMutation {
    /// Apply this mutation to `config`. Applies `clamp()` after
    /// to enforce bounds. Returns the mutated config.
    pub fn apply(&self, config: &MechanismConfig) -> MechanismConfig {
        let mut new = config.clone();
        match self {
            Self::BumpCandidateMaxSize(d) => {
                new.candidate_max_size =
                    (new.candidate_max_size as i32 + d).max(1) as usize;
            }
            Self::AddCandidateVocabOp(op) => {
                if !new.candidate_vocab.contains(op) {
                    new.candidate_vocab.push(*op);
                }
            }
            Self::RemoveCandidateVocabOp(op) => {
                new.candidate_vocab.retain(|x| x != op);
            }
            Self::BumpCompositionCap(d) => {
                new.composition_cap =
                    (new.composition_cap as i32 + d).max(1) as usize;
            }
            Self::AddCandidateConstant(c) => {
                if !new.candidate_constants.contains(c) {
                    new.candidate_constants.push(*c);
                }
            }
            Self::AddCorpusVocabOp(op) => {
                if !new.corpus_vocab.contains(op) {
                    new.corpus_vocab.push(*op);
                }
            }
            Self::RemoveCorpusVocabOp(op) => {
                new.corpus_vocab.retain(|x| x != op);
            }
            Self::BumpCorpusBaseDepth(d) => {
                new.corpus_base_depth =
                    (new.corpus_base_depth as i32 + d).max(1) as usize;
            }
            Self::SetCorpusSeedFromTheorems(b) => {
                new.corpus_seed_from_theorems = *b;
            }
            Self::BumpCorpusMaxValue(d) => {
                new.corpus_max_value =
                    (new.corpus_max_value as i32 + d).max(1) as u64;
            }
            Self::BumpExtractMinShared(d) => {
                new.extract_min_shared_size =
                    (new.extract_min_shared_size as i32 + d).max(1) as usize;
            }
            Self::BumpExtractMinMatches(d) => {
                new.extract_min_matches =
                    (new.extract_min_matches as i32 + d).max(1) as usize;
            }
            Self::BumpExtractMaxNewRules(d) => {
                new.extract_max_new_rules =
                    (new.extract_max_new_rules as i32 + d).max(1) as usize;
            }
            Self::BumpValidatorSamples(d) => {
                new.validator_samples =
                    (new.validator_samples as i32 + d).max(1) as usize;
            }
            Self::BumpValidatorMaxValue(d) => {
                new.validator_max_value =
                    (new.validator_max_value as i32 + d).max(1) as u64;
            }
            Self::BumpValidatorStepLimit(d) => {
                new.validator_step_limit =
                    (new.validator_step_limit as i32 + d).max(1) as usize;
            }
            Self::Compound(children) => {
                let mut staged = new;
                for child in children {
                    staged = child.apply(&staged);
                }
                return staged;
            }
        }
        new.clamp();
        new
    }

    // ── M2: Lisp Sexp round-trip for mutation operators ──────
    //
    // Each atomic mutation serializes to `(op-name arg)`:
    //   BumpCandidateMaxSize(1)         → (bump-candidate-max-size 1)
    //   AddCorpusVocabOp(5)             → (add-corpus-vocab-op 5)
    //   SetCorpusSeedFromTheorems(true) → (set-corpus-seed-from-theorems true)
    //   Compound([a, b])                → (compound (op-a ...) (op-b ...))
    //
    // Discovered operators (from the ML5 pool) can thus be stored,
    // shared across sessions, and eventually mutated by the
    // machine's own Sexp-level process.

    /// Serialize this mutation operator as a Sexp form.
    pub fn to_sexp(&self) -> tatara_lisp::ast::Sexp {
        use tatara_lisp::ast::Sexp;
        let list = |head: &str, arg: Sexp| Sexp::List(vec![Sexp::symbol(head), arg]);
        match self {
            Self::BumpCandidateMaxSize(d) => {
                list("bump-candidate-max-size", Sexp::int(*d as i64))
            }
            Self::AddCandidateVocabOp(op) => {
                list("add-candidate-vocab-op", Sexp::int(*op as i64))
            }
            Self::RemoveCandidateVocabOp(op) => {
                list("remove-candidate-vocab-op", Sexp::int(*op as i64))
            }
            Self::BumpCompositionCap(d) => {
                list("bump-composition-cap", Sexp::int(*d as i64))
            }
            Self::AddCandidateConstant(c) => {
                list("add-candidate-constant", Sexp::int(*c as i64))
            }
            Self::AddCorpusVocabOp(op) => {
                list("add-corpus-vocab-op", Sexp::int(*op as i64))
            }
            Self::RemoveCorpusVocabOp(op) => {
                list("remove-corpus-vocab-op", Sexp::int(*op as i64))
            }
            Self::BumpCorpusBaseDepth(d) => {
                list("bump-corpus-base-depth", Sexp::int(*d as i64))
            }
            Self::SetCorpusSeedFromTheorems(b) => {
                list("set-corpus-seed-from-theorems", Sexp::boolean(*b))
            }
            Self::BumpCorpusMaxValue(d) => {
                list("bump-corpus-max-value", Sexp::int(*d as i64))
            }
            Self::BumpExtractMinShared(d) => {
                list("bump-extract-min-shared", Sexp::int(*d as i64))
            }
            Self::BumpExtractMinMatches(d) => {
                list("bump-extract-min-matches", Sexp::int(*d as i64))
            }
            Self::BumpExtractMaxNewRules(d) => {
                list("bump-extract-max-new-rules", Sexp::int(*d as i64))
            }
            Self::BumpValidatorSamples(d) => {
                list("bump-validator-samples", Sexp::int(*d as i64))
            }
            Self::BumpValidatorMaxValue(d) => {
                list("bump-validator-max-value", Sexp::int(*d as i64))
            }
            Self::BumpValidatorStepLimit(d) => {
                list("bump-validator-step-limit", Sexp::int(*d as i64))
            }
            Self::Compound(children) => {
                let mut items = Vec::with_capacity(children.len() + 1);
                items.push(Sexp::symbol("compound"));
                for child in children {
                    items.push(child.to_sexp());
                }
                Sexp::List(items)
            }
        }
    }

    /// Parse a mutation operator from a Sexp form. Returns `None`
    /// if the form is malformed or names an unknown operator.
    pub fn from_sexp(sexp: &tatara_lisp::ast::Sexp) -> Option<Self> {
        use tatara_lisp::ast::{Atom, Sexp};
        let items = sexp.as_list()?;
        if items.is_empty() {
            return None;
        }
        let head = items[0].as_symbol()?;
        let get_i32 = |item: &Sexp| -> Option<i32> {
            item.as_int().map(|n| n as i32)
        };
        let get_u32 = |item: &Sexp| -> Option<u32> {
            item.as_int().map(|n| n as u32)
        };
        let get_u64 = |item: &Sexp| -> Option<u64> {
            item.as_int().map(|n| n as u64)
        };
        let get_bool = |item: &Sexp| -> Option<bool> {
            match item {
                Sexp::Atom(Atom::Bool(b)) => Some(*b),
                _ => None,
            }
        };
        match head {
            "bump-candidate-max-size" => {
                Some(Self::BumpCandidateMaxSize(get_i32(&items[1])?))
            }
            "add-candidate-vocab-op" => {
                Some(Self::AddCandidateVocabOp(get_u32(&items[1])?))
            }
            "remove-candidate-vocab-op" => {
                Some(Self::RemoveCandidateVocabOp(get_u32(&items[1])?))
            }
            "bump-composition-cap" => {
                Some(Self::BumpCompositionCap(get_i32(&items[1])?))
            }
            "add-candidate-constant" => {
                Some(Self::AddCandidateConstant(get_u64(&items[1])?))
            }
            "add-corpus-vocab-op" => {
                Some(Self::AddCorpusVocabOp(get_u32(&items[1])?))
            }
            "remove-corpus-vocab-op" => {
                Some(Self::RemoveCorpusVocabOp(get_u32(&items[1])?))
            }
            "bump-corpus-base-depth" => {
                Some(Self::BumpCorpusBaseDepth(get_i32(&items[1])?))
            }
            "set-corpus-seed-from-theorems" => {
                Some(Self::SetCorpusSeedFromTheorems(get_bool(&items[1])?))
            }
            "bump-corpus-max-value" => {
                Some(Self::BumpCorpusMaxValue(get_i32(&items[1])?))
            }
            "bump-extract-min-shared" => {
                Some(Self::BumpExtractMinShared(get_i32(&items[1])?))
            }
            "bump-extract-min-matches" => {
                Some(Self::BumpExtractMinMatches(get_i32(&items[1])?))
            }
            "bump-extract-max-new-rules" => {
                Some(Self::BumpExtractMaxNewRules(get_i32(&items[1])?))
            }
            "bump-validator-samples" => {
                Some(Self::BumpValidatorSamples(get_i32(&items[1])?))
            }
            "bump-validator-max-value" => {
                Some(Self::BumpValidatorMaxValue(get_i32(&items[1])?))
            }
            "bump-validator-step-limit" => {
                Some(Self::BumpValidatorStepLimit(get_i32(&items[1])?))
            }
            "compound" => {
                let mut children = Vec::with_capacity(items.len() - 1);
                for child in &items[1..] {
                    children.push(Self::from_sexp(child)?);
                }
                Some(Self::Compound(children))
            }
            _ => None, // Unknown operator — caller can interpret as
                       // a machine-synthesized operator (M5) or reject.
        }
    }

    /// Is this an atomic (single-parameter) mutation?
    #[must_use]
    pub fn is_atomic(&self) -> bool {
        !matches!(self, Self::Compound(_))
    }

    /// Compound arity — 1 for atoms, N for Compound(N children).
    #[must_use]
    pub fn arity(&self) -> usize {
        match self {
            Self::Compound(children) => children.iter().map(|c| c.arity()).sum(),
            _ => 1,
        }
    }

    /// One-line summary for logging.
    pub fn brief(&self) -> String {
        format!("{self:?}")
    }
}

// ── MechanismPool ────────────────────────────────────────────────

/// Current config + record of tried mutations. Mirrors the
/// apparatus pool but operates on the discovery pipeline's knobs.
#[derive(Clone, Debug, Default)]
pub struct MechanismPool {
    pub current: MechanismConfig,
    /// Mutations that produced no delta-novelty on short trial.
    /// Tracked to avoid re-proposing the same failing mutation
    /// from the same base config.
    pub graveyard: Vec<(MechanismMutation, usize)>,
    /// Mutations that WERE accepted (broke saturation). The
    /// full trajectory of self-mutations.
    pub history: Vec<(usize, MechanismMutation)>,
    /// ML5: compound mutations that won at least once. Added to
    /// the baseline proposal set so future saturation responses
    /// can draw from (atomic variants) ∪ (discovered_operators).
    /// The mutation SPACE grows with the machine's experience.
    pub discovered_operators: Vec<MechanismMutation>,
}

impl MechanismPool {
    #[must_use]
    pub fn new() -> Self {
        Self {
            current: MechanismConfig::default(),
            graveyard: Vec::new(),
            history: Vec::new(),
            discovered_operators: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_config(config: MechanismConfig) -> Self {
        Self {
            current: config,
            graveyard: Vec::new(),
            history: Vec::new(),
            discovered_operators: Vec::new(),
        }
    }

    /// Is this operator already promoted (to avoid duplicate
    /// entries in discovered_operators)?
    fn is_discovered(&self, m: &MechanismMutation) -> bool {
        self.discovered_operators.contains(m)
    }
}

// ── Mutation proposal ────────────────────────────────────────────

/// A pool of candidate mutations sampled uniformly from the
/// mutation enum's variants PLUS the pool's discovered compound
/// operators. When `compound_arity > 1`, compound mutations are
/// generated by chaining `compound_arity` atomic mutations.
///
/// This is the ML5 extension of the ML4 sampler: the mutation
/// space is (atomic variants) ∪ (pool.discovered_operators) ∪
/// (compound mutations of any arity).
///
/// Deterministic given `seed` (xorshift). Multiple calls with
/// different seeds give different mutation sets.
pub fn propose_random_mutations(
    current: &MechanismConfig,
    n_mutations: usize,
    seed: u64,
) -> Vec<(MechanismMutation, MechanismConfig)> {
    propose_mutations_from_pool(current, &[], n_mutations, 1, seed)
}

/// Extended proposal: use the pool's discovered_operators AND
/// allow compounds of specified arity. Compound arity of 1 is
/// pure atomic; arity 2 chains two atomics, etc.
pub fn propose_mutations_from_pool(
    current: &MechanismConfig,
    discovered: &[MechanismMutation],
    n_mutations: usize,
    compound_arity: usize,
    seed: u64,
) -> Vec<(MechanismMutation, MechanismConfig)> {
    let mut rng = seed.max(1);
    let mut out: Vec<(MechanismMutation, MechanismConfig)> = Vec::new();
    let mut seen: HashSet<MechanismConfig> = HashSet::new();
    seen.insert(current.clone());

    let mut attempts = 0;
    while out.len() < n_mutations && attempts < n_mutations * 12 {
        attempts += 1;
        let mutation = if compound_arity <= 1 {
            sample_mutation_with_discovered(&mut rng, current, discovered)
        } else {
            // Build a compound by chaining compound_arity atomics.
            let mut children = Vec::with_capacity(compound_arity);
            for _ in 0..compound_arity {
                children.push(sample_mutation_with_discovered(
                    &mut rng, current, discovered,
                ));
            }
            MechanismMutation::Compound(children)
        };
        let mutant = mutation.apply(current);
        if seen.insert(mutant.clone()) {
            out.push((mutation, mutant));
        }
    }
    out
}

fn sample_mutation_with_discovered(
    rng: &mut u64,
    current: &MechanismConfig,
    discovered: &[MechanismMutation],
) -> MechanismMutation {
    // 70% chance pick from atomic variants, 30% from discovered
    // (if any). Biases toward baseline to maintain exploration
    // diversity, but gives discovered operators real coverage.
    if !discovered.is_empty() && (xorshift(rng) % 10) < 3 {
        discovered[(xorshift(rng) as usize) % discovered.len()].clone()
    } else {
        sample_mutation(rng, current)
    }
}

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn sample_mutation(rng: &mut u64, current: &MechanismConfig) -> MechanismMutation {
    use MechanismMutation::*;
    let variants: Vec<MechanismMutation> = {
        // Variants that don't need a parameter from the config:
        let mut v: Vec<MechanismMutation> = vec![
            BumpCandidateMaxSize(1),
            BumpCandidateMaxSize(-1),
            BumpCompositionCap(10),
            BumpCompositionCap(-10),
            BumpCorpusBaseDepth(1),
            BumpCorpusBaseDepth(-1),
            SetCorpusSeedFromTheorems(!current.corpus_seed_from_theorems),
            BumpCorpusMaxValue(4),
            BumpCorpusMaxValue(-2),
            BumpExtractMinShared(1),
            BumpExtractMinShared(-1),
            BumpExtractMinMatches(1),
            BumpExtractMinMatches(-1),
            BumpExtractMaxNewRules(4),
            BumpExtractMaxNewRules(-2),
            BumpValidatorSamples(8),
            BumpValidatorSamples(-4),
            BumpValidatorMaxValue(4),
            BumpValidatorStepLimit(32),
            // Vocab mutations: add operators the evaluator knows
            // about OR (more interesting) add Var(4)=succ-alias to
            // candidate vocab.
            AddCandidateVocabOp(4), // no-op unless candidate_vocab doesn't already have it
            AddCandidateConstant(2),
            AddCandidateConstant(3),
            AddCorpusVocabOp(6),    // div (no evaluator semantics, but corpus-inert)
            AddCorpusVocabOp(1),    // succ alias
        ];
        // Vocab removal for current operators:
        for &op in &current.candidate_vocab {
            v.push(RemoveCandidateVocabOp(op));
        }
        for &op in &current.corpus_vocab {
            v.push(RemoveCorpusVocabOp(op));
        }
        v
    };
    let idx = (xorshift(rng) as usize) % variants.len();
    variants[idx].clone()
}

// ── Saturation response ──────────────────────────────────────────

/// Outcome of a short-trial run of a mutated config. The
/// `delta_novelty` field counts theorems produced that are NOT
/// in the ledger — this is the fitness signal for mechanism
/// mutation.
///
/// Accessible as keyword fields in the Lisp fitness form:
///   `:delta-novelty` → usize
///   `:total-found`   → usize
#[derive(Clone, Debug)]
pub struct TrialResult {
    pub delta_novelty: usize,
    pub total_theorems_found: usize,
}

// ── M3: Fitness functions as Lisp Sexp forms ─────────────────────
//
// A fitness FORM takes a TrialResult and produces a score. Higher
// is better. The machine picks the mutation with the highest
// score; a score of 0 means "fails the fitness check."
//
// Fitness forms are Lisp Sexps evaluated by a tiny arithmetic
// interpreter that knows about the trial's keyword fields:
//
//   canonical:        (> :delta-novelty 0)
//   strict-threshold: (if (>= :delta-novelty 2) 1 0)
//   proportional:     :delta-novelty
//   total-yield:      :total-found
//   product:          (* :delta-novelty :total-found)
//
// When the machine wants to mutate its own fitness criterion, it
// swaps one Lisp form for another. M3's job is just to make every
// existing hardcoded "delta_novelty > 0" check a Sexp form call.

/// Evaluate a fitness Sexp form against a TrialResult, returning
/// a scalar score. Higher = better. Returns 0.0 on evaluation
/// error (malformed form, unknown symbol, etc.) so a bad form
/// can't let a mutation through.
pub fn evaluate_fitness_form(
    form: &tatara_lisp::ast::Sexp,
    trial: &TrialResult,
) -> f64 {
    evaluate_fitness_form_inner(form, trial).unwrap_or(0.0)
}

fn evaluate_fitness_form_inner(
    form: &tatara_lisp::ast::Sexp,
    trial: &TrialResult,
) -> Option<f64> {
    use tatara_lisp::ast::{Atom, Sexp};
    match form {
        Sexp::Atom(Atom::Int(n)) => Some(*n as f64),
        Sexp::Atom(Atom::Float(f)) => Some(*f),
        Sexp::Atom(Atom::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        Sexp::Atom(Atom::Keyword(k)) => match k.as_str() {
            "delta-novelty" => Some(trial.delta_novelty as f64),
            "total-found" => Some(trial.total_theorems_found as f64),
            _ => None,
        },
        Sexp::List(items) => {
            let (head, args) = items.split_first()?;
            let op = head.as_symbol()?;
            match op {
                "+" => Some(
                    args.iter()
                        .map(|a| evaluate_fitness_form_inner(a, trial).unwrap_or(0.0))
                        .sum(),
                ),
                "-" => match args {
                    [] => None,
                    [only] => Some(-evaluate_fitness_form_inner(only, trial)?),
                    [first, rest @ ..] => {
                        let mut v = evaluate_fitness_form_inner(first, trial)?;
                        for r in rest {
                            v -= evaluate_fitness_form_inner(r, trial)?;
                        }
                        Some(v)
                    }
                },
                "*" => {
                    let mut v = 1.0;
                    for a in args {
                        v *= evaluate_fitness_form_inner(a, trial)?;
                    }
                    Some(v)
                }
                "/" => {
                    let [num, den] = args else {
                        return None;
                    };
                    let d = evaluate_fitness_form_inner(den, trial)?;
                    if d == 0.0 {
                        return None;
                    }
                    Some(evaluate_fitness_form_inner(num, trial)? / d)
                }
                ">" | ">=" | "<" | "<=" => {
                    let [a, b] = args else {
                        return None;
                    };
                    let av = evaluate_fitness_form_inner(a, trial)?;
                    let bv = evaluate_fitness_form_inner(b, trial)?;
                    let truth = match op {
                        ">" => av > bv,
                        ">=" => av >= bv,
                        "<" => av < bv,
                        "<=" => av <= bv,
                        _ => return None,
                    };
                    Some(if truth { 1.0 } else { 0.0 })
                }
                "max" => {
                    let [a, b] = args else {
                        return None;
                    };
                    Some(
                        evaluate_fitness_form_inner(a, trial)?
                            .max(evaluate_fitness_form_inner(b, trial)?),
                    )
                }
                "min" => {
                    let [a, b] = args else {
                        return None;
                    };
                    Some(
                        evaluate_fitness_form_inner(a, trial)?
                            .min(evaluate_fitness_form_inner(b, trial)?),
                    )
                }
                "if" => {
                    let [cond, then, els] = args else {
                        return None;
                    };
                    let cv = evaluate_fitness_form_inner(cond, trial)?;
                    if cv != 0.0 {
                        evaluate_fitness_form_inner(then, trial)
                    } else {
                        evaluate_fitness_form_inner(els, trial)
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// The canonical fitness form — equivalent to the hardcoded
/// `delta_novelty > 0` check used in pre-M3 code. When the machine
/// starts mutating fitness forms, this is the seed.
pub fn canonical_fitness_form() -> tatara_lisp::ast::Sexp {
    use tatara_lisp::ast::Sexp;
    Sexp::List(vec![
        Sexp::symbol(">"),
        Sexp::keyword("delta-novelty"),
        Sexp::int(0),
    ])
}

/// Proportional fitness — reward magnitude of delta-novelty, not
/// just the binary threshold. Available as a mutation target when
/// the canonical form saturates.
pub fn proportional_fitness_form() -> tatara_lisp::ast::Sexp {
    tatara_lisp::ast::Sexp::keyword("delta-novelty")
}

/// Product fitness — reward mutations that produce BOTH novelty
/// AND high total-theorem output. Useful for distinguishing
/// "found 1 new theorem + 10 duplicates" from "found 1 new
/// theorem + 0 duplicates" — the former suggests the mutation is
/// also expanding the structural surface.
pub fn product_fitness_form() -> tatara_lisp::ast::Sexp {
    use tatara_lisp::ast::Sexp;
    Sexp::List(vec![
        Sexp::symbol("*"),
        Sexp::keyword("delta-novelty"),
        Sexp::List(vec![
            Sexp::symbol("+"),
            Sexp::int(1),
            Sexp::List(vec![
                Sexp::symbol("/"),
                Sexp::keyword("total-found"),
                Sexp::int(10),
            ]),
        ]),
    ])
}

/// Saturation-response protocol. When `session.has_stalled()`, the
/// orchestrator calls this with:
///   - `pool.current`: current mechanism config
///   - `session`: the discovery session (needed to check
///     delta-novelty — theorems not already in the ledger)
///   - `trial_fn`: a closure that runs a short trial campaign
///     with a candidate config and returns the theorems it found
///   - `escalation_budget`: how many rounds of escalating
///     mutations to try if the initial random sweep doesn't
///     break saturation
///
/// Returns `Some((mutation, new_config))` when a trial breaks the
/// saturation (produced ≥1 ledger-novel theorem). Returns `None`
/// when mechanism mutation itself has saturated — the signal that
/// we've hit ML5 territory.
pub fn respond_to_saturation<F>(
    pool: &mut MechanismPool,
    existing_ledger: &[RewriteRule],
    trial_fn: F,
    mutations_per_round: usize,
    escalation_budget: usize,
    seed: u64,
) -> Option<(MechanismMutation, MechanismConfig)>
where
    F: FnMut(&MechanismConfig) -> TrialResult,
{
    // Use the canonical fitness form by default — preserves
    // pre-M3 behavior exactly. Callers who want a different
    // fitness pass it via respond_to_saturation_with_fitness.
    let form = canonical_fitness_form();
    respond_to_saturation_with_fitness(
        pool,
        existing_ledger,
        trial_fn,
        &form,
        mutations_per_round,
        escalation_budget,
        seed,
    )
}

/// Saturation response parameterized on a fitness form. The form
/// is evaluated against each candidate's TrialResult to produce
/// a score; the highest score wins (ties broken by mutation
/// order). A score of 0 means the mutation failed the fitness
/// check and is skipped (added to graveyard).
pub fn respond_to_saturation_with_fitness<F>(
    pool: &mut MechanismPool,
    existing_ledger: &[RewriteRule],
    mut trial_fn: F,
    fitness_form: &tatara_lisp::ast::Sexp,
    mutations_per_round: usize,
    escalation_budget: usize,
    seed: u64,
) -> Option<(MechanismMutation, MechanismConfig)>
where
    F: FnMut(&MechanismConfig) -> TrialResult,
{
    let mut rng = seed.max(1);
    let _ = existing_ledger; // referenced by trial_fn's caller, not here.

    // ML5 escalation schedule:
    //   round 0: atomic mutations only (arity=1)
    //   round 1: atomic ∪ discovered, still arity=1
    //   round 2: compound arity=2
    //   round 3: compound arity=3
    //   round 4+: compound arity=3, aggressive sweep
    //
    // Each successful compound mutation is promoted to the pool's
    // discovered_operators, making it available as an atomic-level
    // proposal in future saturation responses.
    for round in 0..=escalation_budget {
        let n = mutations_per_round * (round + 1).min(4);
        let compound_arity = match round {
            0 => 1,
            1 => 1,
            2 => 2,
            _ => 3,
        };
        let use_discovered = round >= 1;
        let mutations = propose_mutations_from_pool(
            &pool.current,
            if use_discovered {
                &pool.discovered_operators
            } else {
                &[]
            },
            n,
            compound_arity,
            xorshift(&mut rng),
        );
        if mutations.is_empty() {
            break;
        }
        let mut best: Option<(MechanismMutation, MechanismConfig, f64, TrialResult)> =
            None;
        for (mutation, mutant) in mutations {
            let graveyard_hit = pool
                .graveyard
                .iter()
                .any(|(m, _)| *m == mutation);
            if graveyard_hit {
                continue;
            }
            let result = trial_fn(&mutant);
            // Evaluate the Lisp fitness form. Score > 0 means the
            // mutation passed — zero means it's graveyarded.
            let score = evaluate_fitness_form(fitness_form, &result);
            let beats = match &best {
                None => true,
                Some((_, _, prev_score, _)) => score > *prev_score,
            };
            if beats {
                best =
                    Some((mutation.clone(), mutant.clone(), score, result.clone()));
            }
            if score <= 0.0 {
                pool.graveyard.push((mutation, 0));
            }
        }
        if let Some((mutation, mutant, score, _result)) = best {
            if score > 0.0 {
                // ML5: promote compound-winners to the discovered
                // operators pool. Atomic winners are already in
                // the baseline — no need to promote them.
                if matches!(mutation, MechanismMutation::Compound(_))
                    && !pool.is_discovered(&mutation)
                {
                    pool.discovered_operators.push(mutation.clone());
                }
                pool.history.push((pool.history.len(), mutation.clone()));
                return Some((mutation, mutant));
            }
        }
        // Escalate: next round uses larger sweep / higher arity.
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sexp_round_trip_preserves_default_config() {
        // M1 gold test: the Lisp Sexp form IS the authoritative
        // representation. Serialize-then-parse must preserve
        // every field at every value the default can take.
        let original = MechanismConfig::default();
        let sexp = original.to_sexp();
        let reparsed = MechanismConfig::from_sexp(&sexp)
            .expect("default config must round-trip");
        assert_eq!(original, reparsed);
    }

    #[test]
    fn sexp_round_trip_preserves_mutated_config() {
        // Same gold test for a mutant — every migration path must
        // work on configs that mutations have already perturbed.
        let mut cfg = MechanismConfig::default();
        cfg = MechanismMutation::BumpCandidateMaxSize(2).apply(&cfg);
        cfg = MechanismMutation::AddCorpusVocabOp(8).apply(&cfg);
        cfg = MechanismMutation::SetCorpusSeedFromTheorems(true).apply(&cfg);
        cfg = MechanismMutation::RemoveCandidateVocabOp(2).apply(&cfg);
        let sexp = cfg.to_sexp();
        let reparsed =
            MechanismConfig::from_sexp(&sexp).expect("mutated config round-trips");
        assert_eq!(cfg, reparsed);
    }

    #[test]
    fn sexp_parse_rejects_malformed_input() {
        use tatara_lisp::ast::Sexp;
        // Wrong head symbol.
        let bad = Sexp::List(vec![Sexp::symbol("not-config")]);
        assert!(MechanismConfig::from_sexp(&bad).is_none());
        // Not a list.
        let bad = Sexp::int(42);
        assert!(MechanismConfig::from_sexp(&bad).is_none());
    }

    #[test]
    fn sexp_serialization_uses_kebab_case_keywords() {
        // Human-readable keyword naming. Fields in the Rust struct
        // use snake_case; the Sexp uses kebab-case keywords per
        // Lisp convention.
        let cfg = MechanismConfig::default();
        let sexp = cfg.to_sexp();
        let items = sexp.as_list().unwrap();
        let mut found_kebab = false;
        for item in items {
            if let Some(k) = item.as_keyword() {
                if k.contains('-') {
                    found_kebab = true;
                    break;
                }
            }
        }
        assert!(
            found_kebab,
            "sexp must use kebab-case keywords (for Lisp convention)"
        );
    }

    #[test]
    fn default_config_is_current_hardcoded_values() {
        let c = MechanismConfig::default();
        assert_eq!(c.candidate_max_size, 5);
        assert_eq!(c.composition_cap, 30);
        assert_eq!(c.validator_samples, 24);
    }

    #[test]
    fn clamp_enforces_bounds() {
        let mut c = MechanismConfig::default();
        c.candidate_max_size = 100;
        c.validator_samples = 1;
        c.clamp();
        assert!(c.candidate_max_size <= 8);
        assert!(c.validator_samples >= 8);
    }

    #[test]
    fn bump_max_size_applies_and_clamps() {
        let c = MechanismConfig::default();
        let mutated = MechanismMutation::BumpCandidateMaxSize(2).apply(&c);
        assert_eq!(mutated.candidate_max_size, 7);
        // Try to exceed the upper bound:
        let overbumped = MechanismMutation::BumpCandidateMaxSize(100).apply(&mutated);
        assert!(overbumped.candidate_max_size <= 8);
    }

    #[test]
    fn toggle_seed_from_theorems() {
        let c = MechanismConfig::default();
        assert!(!c.corpus_seed_from_theorems);
        let mutated = MechanismMutation::SetCorpusSeedFromTheorems(true).apply(&c);
        assert!(mutated.corpus_seed_from_theorems);
    }

    #[test]
    fn random_mutations_are_deduped() {
        let c = MechanismConfig::default();
        let proposals = propose_random_mutations(&c, 8, 42);
        // All proposed configs should be distinct from the parent
        // AND from each other.
        let mut seen = HashSet::new();
        seen.insert(c.clone());
        for (_, config) in &proposals {
            assert!(
                seen.insert(config.clone()),
                "duplicate config in mutation proposal"
            );
        }
    }

    #[test]
    fn saturation_response_returns_winner_when_trial_beats_zero() {
        // Trial function: mutations that INCREASE candidate_max_size
        // produce delta-novelty. Everything else is zero. With
        // enough mutations sampled + escalation, the random sweep
        // will hit BumpCandidateMaxSize(+d > 0).
        let trial_fn = |config: &MechanismConfig| {
            if config.candidate_max_size > 5 {
                TrialResult {
                    delta_novelty: 3,
                    total_theorems_found: 10,
                }
            } else {
                TrialResult {
                    delta_novelty: 0,
                    total_theorems_found: 0,
                }
            }
        };
        // Try multiple seeds — at least one must surface a winning
        // mutation within the escalation budget. This asserts the
        // MECHANISM works; individual seeds may fail for small N.
        let mut ever_succeeded = false;
        for seed in 1..=16u64 {
            let mut pool = MechanismPool::new();
            let result = respond_to_saturation(&mut pool, &[], trial_fn, 32, 2, seed);
            if result.is_some() {
                let (_, new_config) = result.unwrap();
                assert!(new_config.candidate_max_size > 5);
                ever_succeeded = true;
                break;
            }
        }
        assert!(
            ever_succeeded,
            "saturation response must surface a winning mutation across 16 seeds × 32 mutations"
        );
    }

    #[test]
    fn mutation_sexp_round_trip_every_variant() {
        // M2 gold test: every mutation operator variant round-trips
        // through its Sexp representation. If this fails at any
        // variant, the Lisp port is lossy and future mutations via
        // Sexp manipulation would corrupt the mutation pool.
        let cases = vec![
            MechanismMutation::BumpCandidateMaxSize(1),
            MechanismMutation::BumpCandidateMaxSize(-2),
            MechanismMutation::AddCandidateVocabOp(7),
            MechanismMutation::RemoveCandidateVocabOp(3),
            MechanismMutation::BumpCompositionCap(10),
            MechanismMutation::AddCandidateConstant(42),
            MechanismMutation::AddCorpusVocabOp(6),
            MechanismMutation::RemoveCorpusVocabOp(5),
            MechanismMutation::BumpCorpusBaseDepth(1),
            MechanismMutation::SetCorpusSeedFromTheorems(true),
            MechanismMutation::SetCorpusSeedFromTheorems(false),
            MechanismMutation::BumpCorpusMaxValue(-4),
            MechanismMutation::BumpExtractMinShared(1),
            MechanismMutation::BumpExtractMinMatches(-1),
            MechanismMutation::BumpExtractMaxNewRules(4),
            MechanismMutation::BumpValidatorSamples(8),
            MechanismMutation::BumpValidatorMaxValue(-2),
            MechanismMutation::BumpValidatorStepLimit(32),
        ];
        for m in cases {
            let sexp = m.to_sexp();
            let reparsed = MechanismMutation::from_sexp(&sexp)
                .unwrap_or_else(|| panic!("round-trip failed for {m:?}"));
            assert_eq!(m, reparsed, "round-trip not identity for {m:?}");
        }
    }

    #[test]
    fn compound_mutation_sexp_round_trip() {
        let compound = MechanismMutation::Compound(vec![
            MechanismMutation::BumpCandidateMaxSize(1),
            MechanismMutation::BumpCompositionCap(10),
            MechanismMutation::SetCorpusSeedFromTheorems(true),
        ]);
        let sexp = compound.to_sexp();
        let reparsed = MechanismMutation::from_sexp(&sexp)
            .expect("compound round-trips");
        assert_eq!(compound, reparsed);
    }

    #[test]
    fn nested_compound_sexp_round_trip() {
        let nested = MechanismMutation::Compound(vec![
            MechanismMutation::BumpCandidateMaxSize(1),
            MechanismMutation::Compound(vec![
                MechanismMutation::BumpCompositionCap(5),
                MechanismMutation::BumpCorpusBaseDepth(1),
            ]),
        ]);
        let sexp = nested.to_sexp();
        let reparsed = MechanismMutation::from_sexp(&sexp).unwrap();
        assert_eq!(nested, reparsed);
    }

    #[test]
    fn unknown_operator_returns_none() {
        use tatara_lisp::ast::Sexp;
        // A Sexp naming an operator we don't know about — e.g.,
        // one that a future M5 machine-synthesized pass might
        // produce. Returning None lets the caller decide how to
        // handle it.
        let unknown = Sexp::List(vec![
            Sexp::symbol("future-operator"),
            Sexp::int(42),
        ]);
        assert!(MechanismMutation::from_sexp(&unknown).is_none());
    }

    #[test]
    fn canonical_fitness_form_reproduces_prev_behavior() {
        // M3 gold test: the canonical fitness Sexp must reject
        // zero-delta trials and accept positive-delta trials —
        // exactly the pre-M3 `delta_novelty > 0` hardcoded check.
        let form = canonical_fitness_form();
        let zero = TrialResult {
            delta_novelty: 0,
            total_theorems_found: 10,
        };
        assert_eq!(evaluate_fitness_form(&form, &zero), 0.0);
        let positive = TrialResult {
            delta_novelty: 1,
            total_theorems_found: 2,
        };
        assert_eq!(evaluate_fitness_form(&form, &positive), 1.0);
        let big = TrialResult {
            delta_novelty: 5,
            total_theorems_found: 20,
        };
        // canonical form is a boolean threshold — 0 or 1 — so
        // magnitudes beyond threshold don't differentiate.
        assert_eq!(evaluate_fitness_form(&form, &big), 1.0);
    }

    #[test]
    fn proportional_fitness_form_rewards_magnitude() {
        let form = proportional_fitness_form();
        let cases = vec![
            (0, 0.0),
            (1, 1.0),
            (5, 5.0),
            (10, 10.0),
        ];
        for (delta, expected) in cases {
            let t = TrialResult {
                delta_novelty: delta,
                total_theorems_found: 100,
            };
            assert_eq!(evaluate_fitness_form(&form, &t), expected);
        }
    }

    #[test]
    fn product_fitness_form_reflects_both_axes() {
        let form = product_fitness_form();
        // formula: delta-novelty * (1 + total-found/10)
        // delta=1, total=0: 1 * 1 = 1
        // delta=1, total=10: 1 * 2 = 2
        // delta=2, total=10: 2 * 2 = 4
        let t1 = TrialResult {
            delta_novelty: 1,
            total_theorems_found: 0,
        };
        assert_eq!(evaluate_fitness_form(&form, &t1), 1.0);
        let t2 = TrialResult {
            delta_novelty: 1,
            total_theorems_found: 10,
        };
        assert_eq!(evaluate_fitness_form(&form, &t2), 2.0);
        let t3 = TrialResult {
            delta_novelty: 2,
            total_theorems_found: 10,
        };
        assert_eq!(evaluate_fitness_form(&form, &t3), 4.0);
    }

    #[test]
    fn malformed_fitness_form_returns_zero_not_panic() {
        use tatara_lisp::ast::Sexp;
        // A form with an unknown operator must NOT panic — it
        // returns 0.0, which fails the fitness check. Safety
        // property for future machine-synthesized forms that
        // might temporarily be malformed.
        let bad = Sexp::List(vec![
            Sexp::symbol("unknown-op"),
            Sexp::keyword("delta-novelty"),
        ]);
        let t = TrialResult {
            delta_novelty: 5,
            total_theorems_found: 10,
        };
        assert_eq!(evaluate_fitness_form(&bad, &t), 0.0);
    }

    #[test]
    fn respond_with_fitness_matches_canonical_behavior() {
        // Regression: passing the canonical fitness form explicitly
        // should produce IDENTICAL behavior to the non-parameterized
        // respond_to_saturation (which uses canonical_fitness_form
        // internally).
        let trial_fn = |config: &MechanismConfig| {
            if config.candidate_max_size > 5 {
                TrialResult {
                    delta_novelty: 3,
                    total_theorems_found: 10,
                }
            } else {
                TrialResult {
                    delta_novelty: 0,
                    total_theorems_found: 0,
                }
            }
        };
        let mut ever_succeeded = false;
        for seed in 1..=16u64 {
            let mut pool = MechanismPool::new();
            let result = respond_to_saturation_with_fitness(
                &mut pool,
                &[],
                trial_fn,
                &canonical_fitness_form(),
                32,
                2,
                seed,
            );
            if result.is_some() {
                ever_succeeded = true;
                break;
            }
        }
        assert!(
            ever_succeeded,
            "canonical fitness form via explicit path must work"
        );
    }

    #[test]
    fn compound_mutation_applies_children_sequentially() {
        let c = MechanismConfig::default();
        let compound = MechanismMutation::Compound(vec![
            MechanismMutation::BumpCandidateMaxSize(1),
            MechanismMutation::BumpCompositionCap(10),
        ]);
        let mutated = compound.apply(&c);
        assert_eq!(mutated.candidate_max_size, 6);
        assert_eq!(mutated.composition_cap, 40);
    }

    #[test]
    fn compound_arity_counts_nested_atomics() {
        let nested = MechanismMutation::Compound(vec![
            MechanismMutation::BumpCandidateMaxSize(1),
            MechanismMutation::Compound(vec![
                MechanismMutation::BumpCompositionCap(5),
                MechanismMutation::BumpCorpusBaseDepth(1),
            ]),
        ]);
        assert_eq!(nested.arity(), 3);
    }

    #[test]
    fn saturation_response_promotes_compound_winners() {
        // Trial function: only a SPECIFIC compound wins —
        // BumpCandidateMaxSize(+1) ∘ SetCorpusSeedFromTheorems(true).
        // Single atomics fail. This forces escalation to compounds.
        let trial_fn = |config: &MechanismConfig| {
            if config.candidate_max_size > 5 && config.corpus_seed_from_theorems {
                TrialResult {
                    delta_novelty: 5,
                    total_theorems_found: 15,
                }
            } else {
                TrialResult {
                    delta_novelty: 0,
                    total_theorems_found: 0,
                }
            }
        };

        let mut ever_succeeded = false;
        for seed in 1..=32u64 {
            let mut pool = MechanismPool::new();
            let result = respond_to_saturation(&mut pool, &[], trial_fn, 24, 3, seed);
            if let Some((mutation, new_config)) = result {
                assert!(new_config.candidate_max_size > 5);
                assert!(new_config.corpus_seed_from_theorems);
                // If the winner was a Compound, it should be in
                // discovered_operators.
                if matches!(mutation, MechanismMutation::Compound(_)) {
                    assert!(pool.discovered_operators.contains(&mutation));
                }
                ever_succeeded = true;
                break;
            }
        }
        assert!(
            ever_succeeded,
            "compound-requiring winner must be findable via escalation"
        );
    }

    #[test]
    fn saturation_response_returns_none_on_exhausted_space() {
        let mut pool = MechanismPool::new();
        // All mutations produce zero delta — nothing breaks saturation.
        let trial_fn = |_: &MechanismConfig| TrialResult {
            delta_novelty: 0,
            total_theorems_found: 0,
        };
        let result = respond_to_saturation(&mut pool, &[], trial_fn, 5, 1, 42);
        assert!(
            result.is_none(),
            "saturation response must return None when no mutation breaks saturation"
        );
    }
}
