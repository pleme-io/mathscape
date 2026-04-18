//! ML4 — mechanism self-mutation.
//!
//! Every bottleneck parameter in the discovery pipeline is bundled
//! into a `MechanismConfig` that the machine can mutate when it
//! detects saturation. No more human-as-compiler for parameter
//! bumps.
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

impl MechanismConfig {
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
        }
        new.clamp();
        new
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
}

impl MechanismPool {
    #[must_use]
    pub fn new() -> Self {
        Self {
            current: MechanismConfig::default(),
            graveyard: Vec::new(),
            history: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_config(config: MechanismConfig) -> Self {
        Self {
            current: config,
            graveyard: Vec::new(),
            history: Vec::new(),
        }
    }
}

// ── Mutation proposal ────────────────────────────────────────────

/// A pool of candidate mutations sampled uniformly from the
/// mutation enum's variants. This is the "C escalation" path
/// per the ML4 design: random sweep rather than targeted
/// diagnosis, at least for V1.
///
/// Deterministic given `seed` (xorshift). Multiple calls with
/// different seeds give different mutation sets, which the
/// saturation-response loop uses as trial variants.
pub fn propose_random_mutations(
    current: &MechanismConfig,
    n_mutations: usize,
    seed: u64,
) -> Vec<(MechanismMutation, MechanismConfig)> {
    let mut rng = seed.max(1);
    let mut out: Vec<(MechanismMutation, MechanismConfig)> = Vec::new();
    let mut seen: HashSet<MechanismConfig> = HashSet::new();
    seen.insert(current.clone());

    let mut attempts = 0;
    while out.len() < n_mutations && attempts < n_mutations * 8 {
        attempts += 1;
        let mutation = sample_mutation(&mut rng, current);
        let mutant = mutation.apply(current);
        if seen.insert(mutant.clone()) {
            out.push((mutation, mutant));
        }
    }
    out
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
#[derive(Clone, Debug)]
pub struct TrialResult {
    pub delta_novelty: usize,
    pub total_theorems_found: usize,
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
    mut trial_fn: F,
    mutations_per_round: usize,
    escalation_budget: usize,
    seed: u64,
) -> Option<(MechanismMutation, MechanismConfig)>
where
    F: FnMut(&MechanismConfig) -> TrialResult,
{
    let mut rng = seed.max(1);
    let _ = existing_ledger; // referenced by trial_fn's caller, not here.
    for round in 0..=escalation_budget {
        let n = mutations_per_round * (round + 1).min(4);
        let mutations = propose_random_mutations(&pool.current, n, {
            xorshift(&mut rng)
        });
        if mutations.is_empty() {
            break;
        }
        let mut best: Option<(MechanismMutation, MechanismConfig, TrialResult)> = None;
        for (mutation, mutant) in mutations {
            // Skip if we've already tried this exact mutation from
            // this config (graveyard check).
            let graveyard_hit = pool
                .graveyard
                .iter()
                .any(|(m, _)| *m == mutation);
            if graveyard_hit {
                continue;
            }
            let result = trial_fn(&mutant);
            let beats = match &best {
                None => true,
                Some((_, _, prev)) => result.delta_novelty > prev.delta_novelty,
            };
            if beats {
                best = Some((mutation.clone(), mutant.clone(), result.clone()));
            }
            // Graveyard zero-delta variants so we skip them next
            // round.
            if result.delta_novelty == 0 {
                pool.graveyard.push((mutation, 0));
            }
        }
        if let Some((mutation, mutant, result)) = best {
            if result.delta_novelty > 0 {
                pool.history.push((pool.history.len(), mutation.clone()));
                return Some((mutation, mutant));
            }
        }
        // Escalate: try bigger random sweep next round.
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
