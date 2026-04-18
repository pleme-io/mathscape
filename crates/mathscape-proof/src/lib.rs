//! Proof construction, verification, composition, and Lean 4 export.
//!
//! Phase J (semantic validation) lives in `semantic` — empirical
//! projection discovery that turns structural patterns into
//! SemanticallyValidated equations the machine can compose and
//! Rustify with confidence.

pub mod lean_export;
pub mod semantic;

pub use lean_export::{export_rule_as_lean, LeanExportOptions};
pub use semantic::{
    discover_semantic_projections, discover_semantic_projections_with_ledger,
    generate_semantic_candidates, generate_semantic_candidates_with_ledger,
    validate_semantically, CandidateKind, SemanticCandidate, SemanticVerdict,
    ValidationConfig,
};

use mathscape_core::eval::RewriteRule;
use mathscape_core::term::Term;
use serde::{Deserialize, Serialize};

/// A proof step: one application of a rewrite rule.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofStep {
    /// Which rule was applied.
    pub rule_name: String,
    /// Expression before this step.
    pub before: Term,
    /// Expression after this step.
    pub after: Term,
}

/// Proof status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofStatus {
    /// Conjectured from evaluation traces, not yet verified.
    Conjectured,
    /// Verified by replaying all steps.
    Verified,
    /// Exported to Lean 4.
    Exported,
}

/// Proof type — how the proof was constructed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofType {
    /// Both sides reduce to the same normal form.
    Equational,
    /// Proof by induction over structure.
    Inductive,
    /// Composed from existing proofs.
    Compositional,
}

/// A proof certificate for a rewrite rule.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofCertificate {
    /// The rule being proven.
    pub rule: RewriteRule,
    /// The proof steps.
    pub steps: Vec<ProofStep>,
    /// Current status.
    pub status: ProofStatus,
    /// How the proof was constructed.
    pub proof_type: ProofType,
    /// Lean 4 export (if available).
    pub lean_export: Option<String>,
}

/// Attempt to construct an equational proof: both sides reduce to the
/// same normal form under the given library.
pub fn prove_equational(
    rule: &RewriteRule,
    library: &[RewriteRule],
    step_limit: usize,
) -> Option<ProofCertificate> {
    let mut steps = Vec::new();

    // Reduce LHS to normal form
    let lhs_nf = mathscape_core::eval::eval(&rule.lhs, library, step_limit).ok()?;
    if lhs_nf != rule.lhs {
        steps.push(ProofStep {
            rule_name: "reduce-lhs".into(),
            before: rule.lhs.clone(),
            after: lhs_nf.clone(),
        });
    }

    // Reduce RHS to normal form
    let rhs_nf = mathscape_core::eval::eval(&rule.rhs, library, step_limit).ok()?;
    if rhs_nf != rule.rhs {
        steps.push(ProofStep {
            rule_name: "reduce-rhs".into(),
            before: rule.rhs.clone(),
            after: rhs_nf.clone(),
        });
    }

    // If both sides reached the same normal form, the proof is valid
    if lhs_nf == rhs_nf {
        Some(ProofCertificate {
            rule: rule.clone(),
            steps,
            status: ProofStatus::Verified,
            proof_type: ProofType::Equational,
            lean_export: None,
        })
    } else {
        None
    }
}

/// Verify a proof certificate by replaying all steps.
pub fn verify(cert: &ProofCertificate, library: &[RewriteRule]) -> bool {
    for step in &cert.steps {
        let result = mathscape_core::eval::eval(&step.before, library, 100);
        match result {
            Ok(reduced) => {
                if reduced != step.after {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// Generate a Lean 4 proof sketch for a verified certificate.
///
/// Translates proof steps into Lean tactic comments with `native_decide`
/// or `norm_num` for concrete arithmetic, and `rw` hints for rewrite steps.
pub fn export_lean4(cert: &ProofCertificate) -> String {
    let rule = &cert.rule;
    let mut lean = String::new();

    lean.push_str(&format!("-- Proof of {}\n", rule.name));
    lean.push_str(&format!("-- LHS: {}\n", rule.lhs));
    lean.push_str(&format!("-- RHS: {}\n", rule.rhs));
    lean.push_str(&format!(
        "-- Status: {:?}, Type: {:?}\n",
        cert.status, cert.proof_type
    ));
    lean.push_str(&format!("-- Steps: {}\n\n", cert.steps.len()));

    // Generate theorem statement
    let lhs_lean = term_to_lean(&rule.lhs);
    let rhs_lean = term_to_lean(&rule.rhs);
    lean.push_str(&format!(
        "theorem {} : {lhs_lean} = {rhs_lean} := by\n",
        sanitize_lean_name(&rule.name)
    ));

    if cert.steps.is_empty() {
        lean.push_str("  rfl\n");
    } else {
        for (i, step) in cert.steps.iter().enumerate() {
            lean.push_str(&format!(
                "  -- Step {}: {} ({} → {})\n",
                i + 1,
                step.rule_name,
                step.before,
                step.after
            ));
        }
        // For equational proofs of concrete terms, native_decide or norm_num works
        match cert.proof_type {
            ProofType::Equational => {
                if is_concrete(&rule.lhs) && is_concrete(&rule.rhs) {
                    lean.push_str("  norm_num\n");
                } else {
                    lean.push_str("  simp [Nat.add_comm, Nat.add_assoc, Nat.mul_comm]\n");
                }
            }
            ProofType::Inductive => {
                lean.push_str("  induction n with\n");
                lean.push_str("  | zero => simp\n");
                lean.push_str("  | succ n ih => simp [ih]\n");
            }
            ProofType::Compositional => {
                for step in &cert.steps {
                    lean.push_str(&format!(
                        "  rw [{name}]\n",
                        name = sanitize_lean_name(&step.rule_name)
                    ));
                }
            }
        }
    }

    lean
}

/// Convert a Term to Lean 4 expression syntax.
fn term_to_lean(term: &Term) -> String {
    match term {
        Term::Number(v) => format!("{v}"),
        Term::Point(id) => format!("p{id}"),
        Term::Var(2) => "Nat.add".into(),
        Term::Var(3) => "Nat.mul".into(),
        Term::Var(1) => "Nat.succ".into(),
        Term::Var(0) => "0".into(),
        Term::Var(v) => format!("x{v}"),
        Term::Apply(func, args) => {
            let func_str = term_to_lean(func);
            let args_str: Vec<String> = args.iter().map(|a| {
                let s = term_to_lean(a);
                if needs_parens(a) { format!("({s})") } else { s }
            }).collect();
            format!("{func_str} {}", args_str.join(" "))
        }
        Term::Fn(params, body) => {
            let ps: Vec<String> = params.iter().map(|p| format!("x{p}")).collect();
            format!("fun {} => {}", ps.join(" "), term_to_lean(body))
        }
        Term::Symbol(id, args) => {
            if args.is_empty() {
                format!("S{id}")
            } else {
                let args_str: Vec<String> = args.iter().map(|a| term_to_lean(a)).collect();
                format!("S{id} {}", args_str.join(" "))
            }
        }
    }
}

fn needs_parens(term: &Term) -> bool {
    matches!(term, Term::Apply(_, _) | Term::Fn(_, _))
}

fn sanitize_lean_name(name: &str) -> String {
    name.replace('-', "_")
}

fn is_concrete(term: &Term) -> bool {
    match term {
        Term::Number(_) | Term::Point(_) => true,
        Term::Var(_) => false,
        Term::Apply(func, args) => is_concrete(func) && args.iter().all(is_concrete),
        Term::Fn(_, body) => is_concrete(body),
        Term::Symbol(_, args) => args.iter().all(is_concrete),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::test_helpers::{apply, nat, var};

    #[test]
    fn equational_proof_of_concrete_add() {
        // Prove: add(2, 3) = 5
        let rule = RewriteRule {
            name: "add-2-3".into(),
            lhs: apply(var(2), vec![nat(2), nat(3)]),
            rhs: nat(5),
        };

        let cert = prove_equational(&rule, &[], 100);
        assert!(cert.is_some());
        let cert = cert.unwrap();
        assert_eq!(cert.status, ProofStatus::Verified);
    }

    #[test]
    fn verify_valid_proof_returns_true() {
        // Build a valid proof: add(2, 3) = 5
        let rule = RewriteRule {
            name: "add-2-3".into(),
            lhs: apply(var(2), vec![nat(2), nat(3)]),
            rhs: nat(5),
        };
        let cert = prove_equational(&rule, &[], 100).unwrap();
        assert!(verify(&cert, &[]));
    }

    #[test]
    fn verify_tampered_proof_returns_false() {
        // Build a valid proof, then tamper with a step's expected output
        let rule = RewriteRule {
            name: "add-2-3".into(),
            lhs: apply(var(2), vec![nat(2), nat(3)]),
            rhs: nat(5),
        };
        let mut cert = prove_equational(&rule, &[], 100).unwrap();
        // Tamper: change the expected "after" of the first step to a wrong value
        if !cert.steps.is_empty() {
            cert.steps[0].after = nat(999);
        } else {
            // If no reduction steps, add a bogus step to force a mismatch
            cert.steps.push(ProofStep {
                rule_name: "bogus".into(),
                before: nat(1),
                after: nat(999),
            });
        }
        assert!(!verify(&cert, &[]));
    }

    #[test]
    fn export_lean4_contains_rule_name_and_theorem() {
        let rule = RewriteRule {
            name: "my-rule".into(),
            lhs: apply(var(2), vec![nat(1), nat(2)]),
            rhs: nat(3),
        };
        let cert = prove_equational(&rule, &[], 100).unwrap();
        let lean = export_lean4(&cert);
        assert!(lean.contains("my-rule"), "should contain the rule name");
        assert!(
            lean.contains("theorem"),
            "should contain the theorem keyword"
        );
    }

    #[test]
    fn prove_equational_failure_different_normal_forms() {
        // lhs reduces to 5, rhs reduces to 7 — they don't match
        let rule = RewriteRule {
            name: "bad-rule".into(),
            lhs: apply(var(2), vec![nat(2), nat(3)]), // reduces to 5
            rhs: nat(7),                              // already 7
        };
        let cert = prove_equational(&rule, &[], 100);
        assert!(
            cert.is_none(),
            "should return None when normal forms differ"
        );
    }

    #[test]
    fn empty_steps_proof_lhs_equals_rhs() {
        // When lhs == rhs already, no reduction steps are needed
        let rule = RewriteRule {
            name: "identity".into(),
            lhs: nat(42),
            rhs: nat(42),
        };
        let cert = prove_equational(&rule, &[], 100);
        assert!(cert.is_some());
        let cert = cert.unwrap();
        assert!(cert.steps.is_empty(), "no steps needed when lhs == rhs");
        assert_eq!(cert.status, ProofStatus::Verified);
        assert!(verify(&cert, &[]), "empty-step proof should verify");
    }
}
