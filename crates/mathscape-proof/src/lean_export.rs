//! Lean 4 export — emit theorem statements for library rules.
//!
//! This is a **stub** implementation honest about its limits:
//!
//! - It emits a syntactically valid Lean 4 file with the theorem
//!   statement derived from a `RewriteRule`
//! - The proof body is `sorry` — Lean's admit mark. A real prover
//!   would emit an actual derivation; we do not attempt that here.
//! - The emitted file is still useful: it's a **target** for CI to
//!   run `lean build` against, confirming the statement at least
//!   type-checks even though the proof is admitted.
//!
//! Upgrading to a real exporter is Phase J+ work and requires either
//! (a) translating the e-graph saturation into Lean 4 `calc` steps,
//! or (b) invoking an external prover (Aesop, lean-auto) to close the
//! goal. Both are substantial tasks outside the current scope.
//!
//! What this export *does* deliver:
//!
//! 1. `ProofStatus::Exported` is no longer a pure lifecycle label —
//!    there is a file on disk representing the export
//! 2. The knowability criterion 4 pathway has concrete surface area;
//!    agents and operators can see what the Lean emission looks like
//! 3. CI can validate statement well-formedness independently of
//!    Lean's tactic engine

use mathscape_core::eval::RewriteRule;
use mathscape_core::term::Term;
use mathscape_core::value::Value;
use std::fmt::Write as _;

/// Options controlling emission.
pub struct LeanExportOptions {
    /// Lean namespace to put the theorem in.
    pub namespace: String,
    /// Module name (becomes the `import Mathlib.xxx` context).
    pub import: Option<String>,
}

impl Default for LeanExportOptions {
    fn default() -> Self {
        Self {
            namespace: "Mathscape.Generated".into(),
            import: None,
        }
    }
}

/// Produce a Lean 4 file text containing the theorem statement for
/// the rule's `lhs = rhs` equation. The proof body is `sorry` — the
/// statement is machine-readable and CI can `lean build` it to
/// confirm well-formedness.
#[must_use]
pub fn export_rule_as_lean(rule: &RewriteRule, opts: &LeanExportOptions) -> String {
    let mut s = String::new();
    if let Some(import) = &opts.import {
        let _ = writeln!(s, "import {import}");
    }
    let _ = writeln!(s, "-- Mathscape-emitted theorem (stub proof body)");
    let _ = writeln!(
        s,
        "-- Rule: {} :: {} = {}",
        rule.name, rule.lhs, rule.rhs
    );
    let _ = writeln!(s, "-- Status: Conjectured (proof body: sorry)");
    let _ = writeln!(s);
    let _ = writeln!(s, "namespace {}", opts.namespace);
    let _ = writeln!(s);
    let lean_name = sanitize_name(&rule.name);
    let params = extract_params(&rule.lhs);
    let param_sig = if params.is_empty() {
        String::new()
    } else {
        params
            .iter()
            .map(|p| format!("({p} : Nat)"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let lhs_lean = term_to_lean(&rule.lhs, &params);
    let rhs_lean = term_to_lean(&rule.rhs, &params);
    if params.is_empty() {
        let _ = writeln!(s, "theorem {lean_name} : {lhs_lean} = {rhs_lean} := by sorry");
    } else {
        let _ = writeln!(
            s,
            "theorem {lean_name} {param_sig} : {lhs_lean} = {rhs_lean} := by sorry"
        );
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "end {}", opts.namespace);
    s
}

/// Collect free-variable ids from a term in deterministic order.
fn extract_params(t: &Term) -> Vec<String> {
    use std::collections::BTreeSet;
    fn walk(t: &Term, out: &mut BTreeSet<u32>) {
        match t {
            Term::Var(id) => {
                out.insert(*id);
            }
            Term::Apply(f, args) => {
                walk(f, out);
                for a in args {
                    walk(a, out);
                }
            }
            Term::Fn(_, body) => walk(body, out),
            Term::Symbol(_, args) => {
                for a in args {
                    walk(a, out);
                }
            }
            _ => {}
        }
    }
    let mut ids = BTreeSet::new();
    walk(t, &mut ids);
    ids.into_iter().map(|id| format!("x{id}")).collect()
}

/// Translate a Term to a Lean 4 expression string. Maps mathscape's
/// BUILTIN_ADD (id=2) / BUILTIN_MUL (id=3) / BUILTIN_SUCC (id=1) to
/// Lean Nat operators. Any unknown symbol becomes an opaque name.
fn term_to_lean(t: &Term, params: &[String]) -> String {
    match t {
        Term::Var(id) => {
            if params.contains(&format!("x{id}")) {
                format!("x{id}")
            } else {
                // Builtin as head — handled at Apply level; standalone
                // as-is.
                match *id {
                    0 => "Nat.zero".into(),
                    1 => "Nat.succ".into(),
                    2 => "Nat.add".into(),
                    3 => "Nat.mul".into(),
                    _ => format!("v{id}"),
                }
            }
        }
        Term::Number(Value::Nat(n)) => format!("({n} : Nat)"),
        // R7: Int mapped to Lean's signed Int type. Lean has a
        // distinct Int type from Nat; exporting cross-domain
        // theorems keeps them annotated so Lean picks the right
        // instance.
        Term::Number(Value::Int(n)) => format!("({n} : Int)"),
        // R13: Tensor — Lean's Array type with element annotations.
        // Shape info preserved as a comment since Lean doesn't
        // have a single direct mapping for shape-tagged arrays.
        Term::Number(Value::Tensor { shape, data }) => {
            let elems: Vec<String> =
                data.iter().map(|x| format!("({x} : Int)")).collect();
            format!(
                "(#[{}] : Array Int) /- shape: {:?} -/",
                elems.join(", "),
                shape
            )
        }
        Term::Point(p) => format!("(Point.mk {p})"),
        Term::Apply(f, args) => match f.as_ref() {
            Term::Var(2) if args.len() == 2 => format!(
                "({} + {})",
                term_to_lean(&args[0], params),
                term_to_lean(&args[1], params)
            ),
            Term::Var(3) if args.len() == 2 => format!(
                "({} * {})",
                term_to_lean(&args[0], params),
                term_to_lean(&args[1], params)
            ),
            Term::Var(1) if args.len() == 1 => {
                format!("(Nat.succ {})", term_to_lean(&args[0], params))
            }
            _ => {
                let head = term_to_lean(f, params);
                let args_s = args
                    .iter()
                    .map(|a| term_to_lean(a, params))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("({head} {args_s})")
            }
        },
        Term::Symbol(id, args) => {
            if args.is_empty() {
                format!("S{id}")
            } else {
                let args_s = args
                    .iter()
                    .map(|a| term_to_lean(a, params))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("(S{id} {args_s})")
            }
        }
        Term::Fn(_, _) => "_".into(),
    }
}

/// Sanitize a rule name for use as a Lean identifier.
fn sanitize_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    // Lean identifiers must start with a letter.
    if out.is_empty() || !out.chars().next().unwrap().is_ascii_alphabetic() {
        out = format!("rule_{out}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathscape_core::eval::RewriteRule;
    use mathscape_core::term::Term;
    use mathscape_core::value::Value;

    fn apply(f: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(f), args)
    }
    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }
    fn var(id: u32) -> Term {
        Term::Var(id)
    }

    #[test]
    fn exports_nullary_rule() {
        let rule = RewriteRule {
            name: "const-one".into(),
            lhs: Term::Symbol(1, vec![]),
            rhs: nat(1),
        };
        let lean = export_rule_as_lean(&rule, &LeanExportOptions::default());
        assert!(lean.contains("theorem const_one"));
        assert!(lean.contains("by sorry"));
        assert!(lean.contains("namespace Mathscape.Generated"));
    }

    #[test]
    fn exports_add_identity() {
        // add(?x, 0) = ?x
        let rule = RewriteRule {
            name: "add-identity".into(),
            lhs: apply(var(2), vec![var(100), nat(0)]),
            rhs: var(100),
        };
        let lean = export_rule_as_lean(&rule, &LeanExportOptions::default());
        // Statement: ∀ x100, (x100 + 0) = x100
        assert!(
            lean.contains("(x100 : Nat)"),
            "expected x100 as Nat param, got:\n{lean}"
        );
        assert!(lean.contains("(x100 + (0 : Nat)) = x100"), "got:\n{lean}");
        assert!(lean.contains("by sorry"));
    }

    #[test]
    fn exports_multi_var_rule() {
        // add(?x, ?y) = add(?y, ?x)   — commutativity
        let rule = RewriteRule {
            name: "add-comm".into(),
            lhs: apply(var(2), vec![var(100), var(200)]),
            rhs: apply(var(2), vec![var(200), var(100)]),
        };
        let lean = export_rule_as_lean(&rule, &LeanExportOptions::default());
        assert!(lean.contains("(x100 : Nat)"));
        assert!(lean.contains("(x200 : Nat)"));
        assert!(lean.contains("(x100 + x200) = (x200 + x100)"), "got:\n{lean}");
    }

    #[test]
    fn sanitizes_hyphens_in_names() {
        assert_eq!(sanitize_name("add-identity"), "add_identity");
        assert_eq!(sanitize_name("S_001"), "S_001");
        assert_eq!(sanitize_name("123abc"), "rule_123abc");
    }
}
