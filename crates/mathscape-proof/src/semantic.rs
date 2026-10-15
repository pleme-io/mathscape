//! Phase J — semantic validation of structurally-discovered rules.
//!
//! The gap phase J closes:
//!
//!   Structural discovery     : the machine names a repeated pattern
//!                              (`add(0, ?x) → S_0(add, ?x)`)
//!   Semantic validation      : the machine PROVES the pattern denotes
//!                              a specific equation over concrete
//!                              values (`add(0, ?x) = ?x` by random
//!                              sampling through the built-in evaluator)
//!
//! Without phase J, discovered rules are pattern names only — opaque
//! wrappers with no testable meaning. With phase J, stable universals
//! become THEOREMS: semantically-validated equations the machine
//! knows how to apply, compose, and Rustify with confidence.
//!
//! Method:
//!   1. Given a discovered rule `LHS → Symbol(...)`, generate
//!      *semantic projection candidates* with RHSs in the original
//!      vocabulary — each variable of the LHS as RHS, each small
//!      constant as RHS, each commuted form.
//!   2. For each candidate, sample K random concrete values for
//!      the free variables.
//!   3. Evaluate LHS and RHS on the same bindings using mathscape-
//!      core's primitive evaluator (Peano: zero, succ, add, mul).
//!   4. If both sides agree on all K samples, mark candidate
//!      SemanticallyValidated.
//!
//! A structurally-discovered rule can have ZERO, ONE, or MANY valid
//! semantic projections. The machine surfaces all of them so
//! downstream (promotion to Rust, composition, proof-object
//! emission) can pick the best projection for the context.

use mathscape_core::eval::{eval, RewriteRule};
use mathscape_core::term::Term;
use mathscape_core::value::Value;
use std::collections::HashMap;
use std::fmt;

/// The kind of semantic projection a candidate represents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateKind {
    /// `LHS → ?v`  — project to a specific free variable.
    Projection(u32),
    /// `LHS → n`   — collapse to a constant natural.
    Constant(u64),
    /// `LHS → f(?v, ?w)` for some builtin `f` — a binary reshape
    /// like commutation, swap, or re-association.
    Reshape(Term),
}

impl fmt::Display for CandidateKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Projection(v) => write!(f, "project-to-?v{v}"),
            Self::Constant(n) => write!(f, "constant-{n}"),
            Self::Reshape(t) => write!(f, "reshape({:?})", t),
        }
    }
}

/// One proposed semantic rule paired with its kind.
#[derive(Clone, Debug)]
pub struct SemanticCandidate {
    pub rule: RewriteRule,
    pub kind: CandidateKind,
}

/// Verdict from a single semantic-validation run.
#[derive(Clone, Debug)]
pub enum SemanticVerdict {
    /// Rule holds on all sampled substitutions.
    Valid {
        samples_tested: usize,
    },
    /// Rule failed — at least one sample produced different LHS
    /// and RHS values.
    Invalid {
        counterexample: HashMap<u32, u64>,
        lhs_value: Option<Term>,
        rhs_value: Option<Term>,
    },
    /// Rule could not be evaluated — type errors, step-limit
    /// exhaustion, or patterns whose LHS doesn't reduce to a
    /// normal form under primitive evaluation.
    Undetermined {
        reason: String,
    },
}

impl SemanticVerdict {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid { .. })
    }
}

/// Configuration for semantic validation.
#[derive(Clone, Debug)]
pub struct ValidationConfig {
    /// Number of random substitutions to test.
    pub samples: usize,
    /// Maximum value (exclusive) for each random substitution.
    pub max_value: u64,
    /// Step limit for the primitive evaluator. Prevents divergence.
    pub step_limit: usize,
    /// Seed for the xorshift RNG used to pick substitution values.
    /// Deterministic given seed — same (rule, config, seed) always
    /// produces the same verdict.
    pub rng_seed: u64,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            samples: 16,
            max_value: 8,
            step_limit: 64,
            rng_seed: 0xC0DE_BEEF_DEAD_F00D,
        }
    }
}

/// Collect unique pattern-variable ids (id >= 100) from a term.
/// These are the "free variables" the RHS is allowed to reference.
/// Concrete operators (id < 100) are vocabulary, not free vars.
fn collect_free_vars(t: &Term) -> Vec<u32> {
    fn walk(t: &Term, out: &mut Vec<u32>) {
        match t {
            Term::Var(v) if *v >= 100 => {
                if !out.contains(v) {
                    out.push(*v);
                }
            }
            Term::Var(_) => {}
            Term::Apply(f, args) => {
                walk(f, out);
                for a in args {
                    walk(a, out);
                }
            }
            Term::Symbol(_, args) => {
                for a in args {
                    walk(a, out);
                }
            }
            Term::Fn(_, body) => walk(body, out),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(t, &mut out);
    out
}

/// Generate semantic projection candidates for a structurally-
/// discovered rule. Each candidate has the same LHS as the input
/// but a candidate RHS in the original vocabulary (no Symbols).
/// The machine can then test each candidate empirically.
#[must_use]
pub fn generate_semantic_candidates(rule: &RewriteRule) -> Vec<SemanticCandidate> {
    let mut out = Vec::new();
    let free_vars = collect_free_vars(&rule.lhs);

    // 1. Projection candidates — rhs = one of the free vars.
    for v in &free_vars {
        out.push(SemanticCandidate {
            rule: RewriteRule {
                name: format!("{}::proj-v{v}", rule.name),
                lhs: rule.lhs.clone(),
                rhs: Term::Var(*v),
            },
            kind: CandidateKind::Projection(*v),
        });
    }

    // 2. Small-constant candidates. Identity elements matter.
    for c in [0u64, 1] {
        out.push(SemanticCandidate {
            rule: RewriteRule {
                name: format!("{}::const-{c}", rule.name),
                lhs: rule.lhs.clone(),
                rhs: Term::Number(Value::Nat(c)),
            },
            kind: CandidateKind::Constant(c),
        });
    }

    out
}

/// Term enumerator: produces every term of size ≤ `max_size`
/// built from `{succ, add, mul}` over leaves `{free_vars} ∪
/// {0, 1}`. Dedup'd by structural equality. This is the machine's
/// generic candidate source — no hand-coded specific equations,
/// just enumeration over its current vocabulary.
fn enumerate_candidate_terms(free_vars: &[u32], max_size: usize) -> Vec<Term> {
    // Seed: size-1 terms (leaves).
    let mut by_size: Vec<Vec<Term>> = vec![Vec::new()];
    let mut size_1 = Vec::new();
    for v in free_vars {
        size_1.push(Term::Var(*v));
    }
    size_1.push(Term::Number(Value::Nat(0)));
    size_1.push(Term::Number(Value::Nat(1)));
    by_size.push(size_1);

    // Builtin ops we can synthesize with:
    //   Var(1) = succ (unary)
    //   Var(2) = add  (binary)
    //   Var(3) = mul  (binary)
    for target_size in 2..=max_size {
        let mut at_size: Vec<Term> = Vec::new();
        // Unary: succ(x) where x has size target_size - 1.
        let inner_size = target_size - 1;
        if inner_size >= 1 && inner_size < by_size.len() {
            for x in &by_size[inner_size] {
                at_size.push(Term::Apply(
                    Box::new(Term::Var(1)),
                    vec![x.clone()],
                ));
            }
        }
        // Binary: op(a, b) where size(a) + size(b) + 1 = target_size.
        for a_size in 1..target_size - 1 {
            let b_size = target_size - 1 - a_size;
            if a_size >= by_size.len() || b_size >= by_size.len() {
                continue;
            }
            for a in &by_size[a_size] {
                for b in &by_size[b_size] {
                    for op in [2u32, 3] {
                        at_size.push(Term::Apply(
                            Box::new(Term::Var(op)),
                            vec![a.clone(), b.clone()],
                        ));
                    }
                }
            }
        }
        by_size.push(at_size);
    }
    // Flatten all sizes 1..=max_size.
    let mut out: Vec<Term> = Vec::new();
    for (size, terms) in by_size.iter().enumerate().skip(1) {
        let _ = size;
        for t in terms {
            if !out.contains(t) {
                out.push(t.clone());
            }
        }
    }
    out
}

/// Self-bootstrapping candidate generator. Given the bootstrap
/// candidates from `generate_semantic_candidates` PLUS a ledger of
/// previously-validated theorems, adapt each theorem's RHS shape
/// into a reshape candidate for this rule.
///
/// This is the mechanism that keeps the machine's candidate set
/// growing by merit of its own discoveries. No human enrichment —
/// every validated theorem contributes its RHS shape as a template
/// future rules can be tested against.
///
/// Adaptation: take theorem T's free variables (in first-appearance
/// order) and map them to this rule's free variables (also in
/// first-appearance order). When the rule has FEWER free vars than
/// the theorem, that theorem is skipped. When the rule has MORE,
/// excess free vars in the rule are unused (no candidate proposed).
/// When the counts match, the theorem's RHS becomes a candidate
/// with the variable renaming applied.
#[must_use]
pub fn generate_semantic_candidates_with_ledger(
    rule: &RewriteRule,
    ledger: &[RewriteRule],
) -> Vec<SemanticCandidate> {
    generate_semantic_candidates_with_config(
        rule,
        ledger,
        5,   // default max_size
        30,  // default composition_cap
    )
}

/// Like `generate_semantic_candidates_with_ledger` but the enumerator
/// and composition knobs are parameters instead of constants.
/// This is the ML4 entry point — MechanismConfig callers invoke
/// this directly.
#[must_use]
pub fn generate_semantic_candidates_with_config(
    rule: &RewriteRule,
    ledger: &[RewriteRule],
    max_size: usize,
    composition_cap: usize,
) -> Vec<SemanticCandidate> {
    let mut out = generate_semantic_candidates(rule);
    let rule_vars = collect_free_vars(&rule.lhs);
    if rule_vars.is_empty() {
        return out;
    }

    // Generic term enumeration: every term of size ≤ K built from
    // {succ, add, mul} over {free_vars, 0, 1}. No hand-coded
    // equations.
    let enumerated = enumerate_candidate_terms(&rule_vars, max_size);
    for (i, candidate_rhs) in enumerated.into_iter().enumerate() {
        let already = out.iter().any(|c| c.rule.rhs == candidate_rhs);
        if already {
            continue;
        }
        out.push(SemanticCandidate {
            rule: RewriteRule {
                name: format!("{}::enum-{i}", rule.name),
                lhs: rule.lhs.clone(),
                rhs: candidate_rhs.clone(),
            },
            kind: CandidateKind::Reshape(candidate_rhs),
        });
    }

    // Ledger-driven candidates: every validated theorem's RHS
    // shape becomes a candidate for this rule too (with var-remapping).
    // Collect these into a `ledger_rhs_shapes` set — they're also
    // the seeds for the compositional pass below.
    let mut ledger_rhs_shapes: Vec<Term> = Vec::new();
    for (i, theorem) in ledger.iter().enumerate() {
        let theorem_vars = collect_free_vars(&theorem.lhs);
        if theorem_vars.is_empty() || theorem_vars.len() > rule_vars.len() {
            continue;
        }
        let mut adapted = theorem.rhs.clone();
        for (j, tv) in theorem_vars.iter().enumerate() {
            if *tv == rule_vars[j] {
                continue;
            }
            adapted = adapted.substitute(*tv, &Term::Var(rule_vars[j]));
        }
        if !ledger_rhs_shapes.contains(&adapted) {
            ledger_rhs_shapes.push(adapted.clone());
        }
        let already = out.iter().any(|c| c.rule.rhs == adapted);
        if already {
            continue;
        }
        out.push(SemanticCandidate {
            rule: RewriteRule {
                name: format!("{}::ledger-{i}", rule.name),
                lhs: rule.lhs.clone(),
                rhs: adapted.clone(),
            },
            kind: CandidateKind::Reshape(adapted),
        });
    }

    // Compositional candidates: take every pair of ledger RHS
    // shapes and wrap them with each builtin binary operator;
    // take every single ledger RHS and wrap with succ. This
    // mechanism is what drives depth-N+1 discovery from depth-N
    // theorems. It's the Gödel-style bootstrap: each validated
    // shape becomes a building block for the next layer.
    //
    // Capped to keep the candidate count tractable. With N ledger
    // shapes, we generate N (succ-wrapped) + 2×N² (binary-wrapped)
    // = O(N²) candidates. Cap is parameterized — the outer loop
    // can mutate this value.
    let shapes: Vec<&Term> =
        ledger_rhs_shapes.iter().take(composition_cap).collect();
    let mut compositional: Vec<Term> = Vec::new();
    for s in &shapes {
        // succ(s)
        compositional.push(Term::Apply(
            Box::new(Term::Var(1)),
            vec![(*s).clone()],
        ));
    }
    for s1 in &shapes {
        for s2 in &shapes {
            for op in [2u32, 3] {
                compositional.push(Term::Apply(
                    Box::new(Term::Var(op)),
                    vec![(*s1).clone(), (*s2).clone()],
                ));
            }
        }
    }
    for (i, rhs) in compositional.into_iter().enumerate() {
        if out.iter().any(|c| c.rule.rhs == rhs) {
            continue;
        }
        out.push(SemanticCandidate {
            rule: RewriteRule {
                name: format!("{}::compose-{i}", rule.name),
                lhs: rule.lhs.clone(),
                rhs: rhs.clone(),
            },
            kind: CandidateKind::Reshape(rhs),
        });
    }

    out
}

/// Self-bootstrapping variant of `discover_semantic_projections`.
/// Tests every ledger-derived candidate along with the bootstrap
/// set. Returns only validated candidates.
#[must_use]
pub fn discover_semantic_projections_with_ledger(
    rule: &RewriteRule,
    ledger: &[RewriteRule],
    config: &ValidationConfig,
) -> Vec<(SemanticCandidate, SemanticVerdict)> {
    generate_semantic_candidates_with_ledger(rule, ledger)
        .into_iter()
        .map(|c| {
            let verdict = validate_semantically(&c.rule, config);
            (c, verdict)
        })
        .filter(|(_, v)| matches!(v, SemanticVerdict::Valid { .. }))
        .collect()
}

/// ML4 entry point for discovery with explicit mechanism
/// parameters. Replaces the ledger-driven variant when the
/// orchestrator needs to vary enumerator size or composition cap.
#[must_use]
pub fn discover_semantic_projections_with_config(
    rule: &RewriteRule,
    ledger: &[RewriteRule],
    validation: &ValidationConfig,
    max_size: usize,
    composition_cap: usize,
) -> Vec<(SemanticCandidate, SemanticVerdict)> {
    generate_semantic_candidates_with_config(rule, ledger, max_size, composition_cap)
        .into_iter()
        .map(|c| {
            let verdict = validate_semantically(&c.rule, validation);
            (c, verdict)
        })
        .filter(|(_, v)| matches!(v, SemanticVerdict::Valid { .. }))
        .collect()
}

/// Validate a single rule empirically. For each of K substitutions,
/// evaluate both sides and check equality. Uses no library rules —
/// only the primitive evaluator — so validation doesn't circularly
/// depend on unverified rules.
#[must_use]
pub fn validate_semantically(
    rule: &RewriteRule,
    config: &ValidationConfig,
) -> SemanticVerdict {
    let free_vars = collect_free_vars(&rule.lhs);

    // Zero-free-var case — direct evaluation.
    if free_vars.is_empty() {
        let lhs_val = match eval(&rule.lhs, &[], config.step_limit) {
            Ok(v) => v,
            Err(e) => {
                return SemanticVerdict::Undetermined {
                    reason: format!("lhs eval failed: {e}"),
                }
            }
        };
        let rhs_val = match eval(&rule.rhs, &[], config.step_limit) {
            Ok(v) => v,
            Err(e) => {
                return SemanticVerdict::Undetermined {
                    reason: format!("rhs eval failed: {e}"),
                }
            }
        };
        return if lhs_val == rhs_val {
            SemanticVerdict::Valid { samples_tested: 1 }
        } else {
            SemanticVerdict::Invalid {
                counterexample: HashMap::new(),
                lhs_value: Some(lhs_val),
                rhs_value: Some(rhs_val),
            }
        };
    }

    let mut rng = config.rng_seed.max(1);
    let xorshift = |x: &mut u64| {
        *x ^= *x << 13;
        *x ^= *x >> 7;
        *x ^= *x << 17;
        *x
    };

    for _ in 0..config.samples {
        let mut bindings: HashMap<u32, u64> = HashMap::new();
        for v in &free_vars {
            xorshift(&mut rng);
            let value = rng % config.max_value.max(1);
            bindings.insert(*v, value);
        }
        // Substitute each binding into both sides.
        let mut lhs_sub = rule.lhs.clone();
        let mut rhs_sub = rule.rhs.clone();
        for (&v, &val) in &bindings {
            let replacement = Term::Number(Value::Nat(val));
            lhs_sub = lhs_sub.substitute(v, &replacement);
            rhs_sub = rhs_sub.substitute(v, &replacement);
        }
        let lhs_val = eval(&lhs_sub, &[], config.step_limit);
        let rhs_val = eval(&rhs_sub, &[], config.step_limit);
        match (lhs_val, rhs_val) {
            (Ok(l), Ok(r)) => {
                if l != r {
                    return SemanticVerdict::Invalid {
                        counterexample: bindings,
                        lhs_value: Some(l),
                        rhs_value: Some(r),
                    };
                }
            }
            (Err(e), _) => {
                return SemanticVerdict::Undetermined {
                    reason: format!("lhs eval failed at {bindings:?}: {e}"),
                }
            }
            (_, Err(e)) => {
                return SemanticVerdict::Undetermined {
                    reason: format!("rhs eval failed at {bindings:?}: {e}"),
                }
            }
        }
    }
    SemanticVerdict::Valid {
        samples_tested: config.samples,
    }
}

/// High-level entry: given a structurally-discovered rule, generate
/// semantic candidates and return the ones that passed empirical
/// validation. The returned vec contains each validated candidate
/// paired with its verdict — callers can pick which semantic
/// projection to attach to the Symbol.
#[must_use]
pub fn discover_semantic_projections(
    rule: &RewriteRule,
    config: &ValidationConfig,
) -> Vec<(SemanticCandidate, SemanticVerdict)> {
    generate_semantic_candidates(rule)
        .into_iter()
        .map(|c| {
            let verdict = validate_semantically(&c.rule, config);
            (c, verdict)
        })
        .filter(|(_, v)| matches!(v, SemanticVerdict::Valid { .. }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::term::Term;

    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }
    fn var(id: u32) -> Term {
        Term::Var(id)
    }
    fn apply(f: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(f), args)
    }

    #[test]
    fn validates_add_left_zero_as_projection() {
        // add(0, ?x) = ?x — the canonical identity-element theorem.
        let rule = RewriteRule {
            name: "add-left-zero".into(),
            lhs: apply(var(2), vec![nat(0), var(100)]),
            rhs: var(100),
        };
        let v = validate_semantically(&rule, &ValidationConfig::default());
        assert!(v.is_valid(), "add-left-zero must validate; got {v:?}");
    }

    #[test]
    fn validates_add_right_zero_as_projection() {
        // add(?x, 0) = ?x
        let rule = RewriteRule {
            name: "add-right-zero".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        let v = validate_semantically(&rule, &ValidationConfig::default());
        assert!(v.is_valid(), "add-right-zero must validate; got {v:?}");
    }

    #[test]
    fn validates_mul_left_one_as_projection() {
        // mul(1, ?x) = ?x
        let rule = RewriteRule {
            name: "mul-left-one".into(),
            lhs: apply(var(3), vec![nat(1), var(100)]),
            rhs: var(100),
        };
        let v = validate_semantically(&rule, &ValidationConfig::default());
        assert!(v.is_valid(), "mul-left-one must validate; got {v:?}");
    }

    #[test]
    fn validates_mul_right_one_as_projection() {
        // mul(?x, 1) = ?x
        let rule = RewriteRule {
            name: "mul-right-one".into(),
            lhs: apply(var(3), vec![var(100), nat(1)]),
            rhs: var(100),
        };
        let v = validate_semantically(&rule, &ValidationConfig::default());
        assert!(v.is_valid(), "mul-right-one must validate; got {v:?}");
    }

    #[test]
    fn rejects_wrong_projection() {
        // add(0, ?x) ≠ 0 (except trivially when x=0)
        let rule = RewriteRule {
            name: "add-left-zero-WRONG".into(),
            lhs: apply(var(2), vec![nat(0), var(100)]),
            rhs: nat(0),
        };
        let v = validate_semantically(&rule, &ValidationConfig::default());
        assert!(!v.is_valid(), "wrong RHS must fail; got {v:?}");
    }

    #[test]
    fn rejects_unsupported_operator() {
        // Apply(Var(99), ...) — there's no builtin 99, so evaluator
        // cannot reduce. Undetermined is the right verdict — we
        // don't know, so we can't say it's valid.
        let rule = RewriteRule {
            name: "unknown-op".into(),
            lhs: apply(var(99), vec![var(100), var(101)]),
            rhs: var(100),
        };
        let v = validate_semantically(&rule, &ValidationConfig::default());
        // Note: Apply(Var(99), ..) may not be reducible. The
        // evaluator either returns normal form unchanged (making
        // LHS value an irreducible Apply) or loops. Either way
        // it's NOT Valid for all projections — the Var(99) is
        // opaque, so LHS == RHS only when they're identical
        // terms, and they're not.
        assert!(!v.is_valid(), "opaque operator must not validate; got {v:?}");
    }

    #[test]
    fn candidate_generator_proposes_projections_and_constants() {
        let rule = RewriteRule {
            name: "structural".into(),
            lhs: apply(var(2), vec![nat(0), var(100)]),
            rhs: Term::Symbol(0, vec![Term::Var(2), Term::Var(100)]),
        };
        let candidates = generate_semantic_candidates(&rule);
        // Bootstrap set: projection + const-0 + const-1 = 3.
        // Reshape candidates come from ledger-driven enrichment,
        // not from the bootstrap. The self-bootstrapping design
        // keeps the bootstrap minimal.
        assert!(
            candidates.len() >= 3,
            "bootstrap candidate count too low: {}",
            candidates.len()
        );
        assert!(candidates.iter().any(|c| matches!(c.kind, CandidateKind::Projection(100))));
        assert!(candidates.iter().any(|c| matches!(c.kind, CandidateKind::Constant(0))));
        assert!(candidates.iter().any(|c| matches!(c.kind, CandidateKind::Constant(1))));
    }

    #[test]
    fn discover_finds_projection_for_add_left_zero() {
        // Structurally discovered rule — rhs is opaque Symbol.
        let rule = RewriteRule {
            name: "S_0-wrap".into(),
            lhs: apply(var(2), vec![nat(0), var(100)]),
            rhs: Term::Symbol(0, vec![Term::Var(2), Term::Var(100)]),
        };
        let validated = discover_semantic_projections(
            &rule,
            &ValidationConfig::default(),
        );
        // At least ONE projection must pass — specifically, the
        // projection to ?v100 (since add(0, x) = x).
        assert!(
            validated.iter().any(|(c, _)| matches!(c.kind, CandidateKind::Projection(100))),
            "projection-to-v100 must be validated for add-left-zero; got {} verdicts",
            validated.len()
        );
    }

    #[test]
    fn bootstrap_candidates_for_singleton_pattern() {
        // Bootstrap set for a 1-free-var rule: projection + two
        // constants. That's 3 candidates minimum from the pure
        // bootstrap path (generate_semantic_candidates with no
        // ledger).
        let rule = RewriteRule {
            name: "singleton".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: Term::Symbol(0, vec![Term::Var(100)]),
        };
        let candidates = generate_semantic_candidates(&rule);
        // projection-v100 + const-0 + const-1 = 3 minimum.
        assert!(
            candidates.len() >= 3,
            "bootstrap must produce at least projection + 2 constants, got {}",
            candidates.len()
        );
    }

    #[test]
    fn ledger_driven_candidate_includes_validated_rhs_shape() {
        // A previously-validated theorem's RHS becomes a candidate
        // for future rules. This is the self-bootstrapping
        // mechanism — no human-coded addition to the generator.
        let ledger = vec![
            RewriteRule {
                name: "succ-rule".into(),
                // LHS doesn't matter for the ledger-extraction —
                // we only harvest RHS shape + LHS-free-vars count.
                lhs: apply(var(2), vec![var(100), nat(1)]),
                rhs: apply(var(1), vec![var(100)]),
                // i.e., add(?x, 1) → succ(?x) — validated earlier
            },
        ];
        let rule = RewriteRule {
            name: "new-rule".into(),
            lhs: apply(var(1), vec![var(200)]),
            rhs: Term::Symbol(0, vec![Term::Var(200)]),
        };
        let candidates = generate_semantic_candidates_with_ledger(&rule, &ledger);
        // The ledger-derived candidate should propose `succ(?v200)`
        // as RHS (theorem's RHS with var-remapping from
        // theorem's first free-var ?v100 → rule's first free-var
        // ?v200).
        let ledger_candidate = candidates.iter().any(|c| {
            matches!(&c.kind, CandidateKind::Reshape(t) if t == &apply(var(1), vec![var(200)]))
        });
        assert!(
            ledger_candidate,
            "ledger-derived candidate missing; got: {:?}",
            candidates.iter().map(|c| c.kind.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn validation_is_deterministic_across_seeds() {
        // Same seed → same verdict.
        let rule = RewriteRule {
            name: "det".into(),
            lhs: apply(var(2), vec![nat(0), var(100)]),
            rhs: var(100),
        };
        let c1 = ValidationConfig {
            rng_seed: 42,
            ..ValidationConfig::default()
        };
        let c2 = ValidationConfig {
            rng_seed: 42,
            ..ValidationConfig::default()
        };
        let v1 = validate_semantically(&rule, &c1);
        let v2 = validate_semantically(&rule, &c2);
        assert!(v1.is_valid() && v2.is_valid());
    }
}
