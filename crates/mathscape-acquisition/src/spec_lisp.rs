//! The `(defacquisition …)` tatara-lisp authoring surface.
//!
//! The TYPED-SPEC + INTERPRETER triplet's second artifact: the authored
//! Lisp spec. Same `to_sexp`/`from_sexp` shape the mechanism/proof
//! layers already use (`tatara_lisp::ast::Sexp` + `Sexp::symbol`/
//! `keyword`/`int`/`string`), so a consumer authors an acquisition run
//! as data and both any engine and the Rust interpreter drive the same
//! form — drift is impossible.
//!
//! **Honest limit (same as aldrava's `spec_lisp`):** this is
//! acquisition's own reader over `tatara_lisp::ast::Sexp`, not the
//! shared `#[derive(TataraDomain)]` runtime registration machinery
//! (which is not yet a runtime-consumable parsing library for external
//! crates). Swapping it in once that ships is a named, isolated
//! follow-up — not a claim that the runtime domain registration is used
//! here.
//!
//! Form:
//! ```text
//! (defacquisition
//!   :name "peano-ratchet"
//!   :genesis-ceiling 64
//!   :ops (
//!     (seal :lhs "(add zero ?100)" :rhs "?100" :k 16)
//!     (absorb :lhs "(mul ?100 zero)" :rhs "0" :k 16)
//!     (fold-back :primitive 0)
//!     (advance-frontier :to 128)))
//! ```
//! Rule terms are the `Term` Display syntax that
//! `mathscape_core::parse::parse` round-trips.

use crate::spec::{AcquisitionOp, AcquisitionSpec};
use crate::types::{Complexity, SpecError};
use mathscape_core::eval::RewriteRule;
use tatara_lisp::ast::Sexp;

impl AcquisitionSpec {
    /// Serialize to the `(defacquisition …)` Sexp form.
    #[must_use]
    pub fn to_sexp(&self) -> Sexp {
        let mut items: Vec<Sexp> = vec![
            Sexp::symbol("defacquisition"),
            Sexp::keyword("name"),
            Sexp::string(self.name.clone()),
            Sexp::keyword("genesis-ceiling"),
            Sexp::int(self.genesis_ceiling.0 as i64),
            Sexp::keyword("ops"),
        ];
        let ops: Vec<Sexp> = self.ops.iter().map(op_to_sexp).collect();
        items.push(Sexp::List(ops));
        Sexp::List(items)
    }

    /// Parse a `(defacquisition …)` Sexp form. Returns a typed
    /// `SpecError::Parse` on a malformed form — the parse boundary
    /// rejects invalid input rather than construct a bad spec.
    pub fn from_sexp(sexp: &Sexp) -> Result<AcquisitionSpec, SpecError> {
        let items = sexp.as_list().ok_or(parse_err("top form is not a list"))?;
        if items.first().and_then(Sexp::as_symbol) != Some("defacquisition") {
            return Err(parse_err("top form is not (defacquisition …)"));
        }
        let mut name = String::new();
        let mut genesis_ceiling = Complexity(0);
        let mut ops: Vec<AcquisitionOp> = Vec::new();
        let mut i = 1;
        while i + 1 < items.len() {
            let key = items[i]
                .as_keyword()
                .ok_or(parse_err("expected a keyword"))?;
            let val = &items[i + 1];
            match key {
                "name" => {
                    name = val
                        .as_string()
                        .ok_or(parse_err(":name must be a string"))?
                        .to_string();
                }
                "genesis-ceiling" => {
                    genesis_ceiling =
                        Complexity(val.as_int().ok_or(parse_err(":genesis-ceiling must be an int"))?
                            as u64);
                }
                "ops" => {
                    let list = val.as_list().ok_or(parse_err(":ops must be a list"))?;
                    for op_sexp in list {
                        ops.push(op_from_sexp(op_sexp)?);
                    }
                }
                other => {
                    return Err(SpecError::Parse {
                        detail: format!("unknown key :{other}"),
                    });
                }
            }
            i += 2;
        }
        Ok(AcquisitionSpec {
            name,
            genesis_ceiling,
            ops,
        })
    }
}

fn op_to_sexp(op: &AcquisitionOp) -> Sexp {
    match op {
        AcquisitionOp::SealLayer { rule, k } => Sexp::List(vec![
            Sexp::symbol("seal"),
            Sexp::keyword("rule-name"),
            Sexp::string(rule.name.clone()),
            Sexp::keyword("lhs"),
            Sexp::string(rule.lhs.to_string()),
            Sexp::keyword("rhs"),
            Sexp::string(rule.rhs.to_string()),
            Sexp::keyword("k"),
            Sexp::int(*k as i64),
        ]),
        AcquisitionOp::Absorb { rule, k } => Sexp::List(vec![
            Sexp::symbol("absorb"),
            Sexp::keyword("rule-name"),
            Sexp::string(rule.name.clone()),
            Sexp::keyword("lhs"),
            Sexp::string(rule.lhs.to_string()),
            Sexp::keyword("rhs"),
            Sexp::string(rule.rhs.to_string()),
            Sexp::keyword("k"),
            Sexp::int(*k as i64),
        ]),
        AcquisitionOp::FoldBack { primitive_index } => Sexp::List(vec![
            Sexp::symbol("fold-back"),
            Sexp::keyword("primitive"),
            Sexp::int(*primitive_index as i64),
        ]),
        AcquisitionOp::AdvanceFrontier { to } => Sexp::List(vec![
            Sexp::symbol("advance-frontier"),
            Sexp::keyword("to"),
            Sexp::int(to.0 as i64),
        ]),
    }
}

fn op_from_sexp(sexp: &Sexp) -> Result<AcquisitionOp, SpecError> {
    let items = sexp.as_list().ok_or(parse_err("op is not a list"))?;
    let head = items.first().and_then(Sexp::as_symbol).ok_or(parse_err(
        "op must start with a symbol (seal | absorb | fold-back | advance-frontier)",
    ))?;
    match head {
        "seal" | "absorb" => {
            let rule_name = kw_string(items, "rule-name")?;
            let lhs = kw_string(items, "lhs")?;
            let rhs = kw_string(items, "rhs")?;
            let k = kw_int(items, "k")? as usize;
            let rule = parse_rule(&rule_name, &lhs, &rhs)?;
            Ok(if head == "seal" {
                AcquisitionOp::SealLayer { rule, k }
            } else {
                AcquisitionOp::Absorb { rule, k }
            })
        }
        "fold-back" => {
            let idx = kw_int(items, "primitive")? as usize;
            Ok(AcquisitionOp::FoldBack {
                primitive_index: idx,
            })
        }
        "advance-frontier" => {
            let to = kw_int(items, "to")? as u64;
            Ok(AcquisitionOp::AdvanceFrontier {
                to: Complexity(to),
            })
        }
        other => Err(SpecError::Parse {
            detail: format!("unknown op `{other}`"),
        }),
    }
}

fn parse_rule(name: &str, lhs: &str, rhs: &str) -> Result<RewriteRule, SpecError> {
    let lhs_term = mathscape_core::parse::parse(lhs).map_err(|e| SpecError::Parse {
        detail: format!("lhs parse: {e}"),
    })?;
    let rhs_term = mathscape_core::parse::parse(rhs).map_err(|e| SpecError::Parse {
        detail: format!("rhs parse: {e}"),
    })?;
    Ok(RewriteRule {
        name: name.to_string(),
        lhs: lhs_term,
        rhs: rhs_term,
    })
}

fn kw_string(items: &[Sexp], key: &str) -> Result<String, SpecError> {
    kw_val(items, key)?
        .as_string()
        .map(str::to_string)
        .ok_or(SpecError::Parse {
            detail: format!(":{key} must be a string"),
        })
}

fn kw_int(items: &[Sexp], key: &str) -> Result<i64, SpecError> {
    kw_val(items, key)?.as_int().ok_or(SpecError::Parse {
        detail: format!(":{key} must be an int"),
    })
}

fn kw_val<'a>(items: &'a [Sexp], key: &str) -> Result<&'a Sexp, SpecError> {
    let mut i = 1;
    while i + 1 < items.len() {
        if items[i].as_keyword() == Some(key) {
            return Ok(&items[i + 1]);
        }
        i += 2;
    }
    Err(SpecError::Parse {
        detail: format!("missing :{key}"),
    })
}

fn parse_err(detail: &str) -> SpecError {
    SpecError::Parse {
        detail: detail.to_string(),
    }
}
