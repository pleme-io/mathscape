//! DTOs that mirror `apis/mathscape-inference/openapi.yaml`
//! field-for-field. Use serde's internal-tag representation
//! (`#[serde(tag = "kind")]`) so the JSON shape matches the
//! OpenAPI discriminator convention.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Core substrate DTOs ───────────────────────────────────────

/// Term DTO — recursive, discriminated by `kind`.
///
/// Mirrors `mathscape_core::term::Term` which has 6 variants:
/// Var, Number, Apply (hot path), plus Point, Fn, Symbol
/// (internal / advanced). All six are represented so the API
/// can losslessly round-trip ANY term the kernel produces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TermDto {
    #[serde(rename = "var")]
    Var { symbol_id: u32 },
    #[serde(rename = "number")]
    Number { value: ValueDto },
    #[serde(rename = "apply")]
    Apply {
        head: Box<TermDto>,
        args: Vec<TermDto>,
    },
    #[serde(rename = "point")]
    Point { id: u64 },
    #[serde(rename = "fn")]
    Fn {
        params: Vec<u32>,
        body: Box<TermDto>,
    },
    #[serde(rename = "symbol")]
    Symbol {
        symbol_id: u32,
        args: Vec<TermDto>,
    },
}

/// Value DTO — discriminated by `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ValueDto {
    #[serde(rename = "nat")]
    Nat { n: u64 },
    #[serde(rename = "int")]
    Int { n: i64 },
    #[serde(rename = "float")]
    Float { f: f64 },
    #[serde(rename = "tensor")]
    Tensor { shape: Vec<usize>, data: Vec<i64> },
    #[serde(rename = "floatTensor")]
    FloatTensor {
        shape: Vec<usize>,
        data: Vec<f64>,
    },
}

// ── Conversions between core types and DTOs ──────────────────

impl From<&mathscape_core::term::Term> for TermDto {
    fn from(term: &mathscape_core::term::Term) -> Self {
        use mathscape_core::term::Term;
        match term {
            Term::Var(id) => TermDto::Var { symbol_id: *id },
            Term::Number(v) => TermDto::Number { value: v.into() },
            Term::Apply(head, args) => TermDto::Apply {
                head: Box::new(TermDto::from(&**head)),
                args: args.iter().map(TermDto::from).collect(),
            },
            Term::Point(id) => TermDto::Point { id: *id },
            Term::Fn(params, body) => TermDto::Fn {
                params: params.clone(),
                body: Box::new(TermDto::from(&**body)),
            },
            Term::Symbol(sid, args) => TermDto::Symbol {
                symbol_id: *sid,
                args: args.iter().map(TermDto::from).collect(),
            },
        }
    }
}

impl From<TermDto> for mathscape_core::term::Term {
    fn from(dto: TermDto) -> Self {
        use mathscape_core::term::Term;
        match dto {
            TermDto::Var { symbol_id } => Term::Var(symbol_id),
            TermDto::Number { value } => Term::Number(value.into()),
            TermDto::Apply { head, args } => Term::Apply(
                Box::new((*head).into()),
                args.into_iter().map(Into::into).collect(),
            ),
            TermDto::Point { id } => Term::Point(id),
            TermDto::Fn { params, body } => {
                Term::Fn(params, Box::new((*body).into()))
            }
            TermDto::Symbol { symbol_id, args } => Term::Symbol(
                symbol_id,
                args.into_iter().map(Into::into).collect(),
            ),
        }
    }
}

impl From<&mathscape_core::value::Value> for ValueDto {
    fn from(v: &mathscape_core::value::Value) -> Self {
        use mathscape_core::value::Value;
        match v {
            Value::Nat(n) => ValueDto::Nat { n: *n },
            Value::Int(n) => ValueDto::Int { n: *n },
            // Core stores floats as bit patterns (u64); unpack
            // to f64 at the API boundary.
            Value::Float(bits) => ValueDto::Float {
                f: f64::from_bits(*bits),
            },
            Value::Tensor { shape, data } => ValueDto::Tensor {
                shape: shape.clone(),
                data: data.clone(),
            },
            // FloatTensor data is Vec<u64> bit patterns — unpack
            // for the API DTO.
            Value::FloatTensor { shape, data } => ValueDto::FloatTensor {
                shape: shape.clone(),
                data: data.iter().map(|b| f64::from_bits(*b)).collect(),
            },
        }
    }
}

impl From<ValueDto> for mathscape_core::value::Value {
    fn from(dto: ValueDto) -> Self {
        use mathscape_core::value::Value;
        match dto {
            ValueDto::Nat { n } => Value::Nat(n),
            ValueDto::Int { n } => Value::Int(n),
            // Round-trip via Value::from_f64 to validate finite;
            // fall back to bit-pack if the DTO brought a NaN/Inf
            // (which shouldn't occur from a conformant client).
            ValueDto::Float { f } => Value::from_f64(f)
                .unwrap_or_else(|| Value::Float(f.to_bits())),
            ValueDto::Tensor { shape, data } => {
                Value::tensor(shape, data).expect("valid tensor")
            }
            ValueDto::FloatTensor { shape, data } => {
                Value::float_tensor(shape, data)
                    .expect("valid float tensor")
            }
        }
    }
}

// ── Rule + Policy DTOs ───────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewriteRuleDto {
    pub name: String,
    pub lhs: TermDto,
    pub rhs: TermDto,
}

impl From<&mathscape_core::eval::RewriteRule> for RewriteRuleDto {
    fn from(r: &mathscape_core::eval::RewriteRule) -> Self {
        RewriteRuleDto {
            name: r.name.clone(),
            lhs: (&r.lhs).into(),
            rhs: (&r.rhs).into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearPolicyDto {
    pub weights: Vec<f64>,
    pub bias: f64,
    pub trained_steps: u64,
    pub generation: u64,
}

impl From<&mathscape_core::policy::LinearPolicy> for LinearPolicyDto {
    fn from(p: &mathscape_core::policy::LinearPolicy) -> Self {
        LinearPolicyDto {
            weights: p.weights.to_vec(),
            bias: p.bias,
            trained_steps: p.trained_steps,
            generation: p.generation,
        }
    }
}

// ── Endpoint request/response DTOs ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferRequest {
    pub term: TermDto,
    pub step_limit: u64,
}

/// EvalResult — tagged union matching openapi EvalOk/EvalErr.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum EvalResult {
    #[serde(rename = "ok")]
    Ok { value: TermDto },
    #[serde(rename = "err")]
    Err { message: String },
}

// ── Benchmark + Curriculum DTOs ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemResultDto {
    pub problem_id: String,
    pub solved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<TermDto>,
}

impl From<&mathscape_core::math_problem::ProblemResult> for ProblemResultDto {
    fn from(r: &mathscape_core::math_problem::ProblemResult) -> Self {
        ProblemResultDto {
            problem_id: r.problem_id.clone(),
            solved: r.solved,
            actual: r.actual.as_ref().map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkReportDto {
    pub problem_set_size: usize,
    pub solved_count: usize,
    pub solved_fraction: f64,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub results: Vec<ProblemResultDto>,
}

impl From<&mathscape_core::math_problem::BenchmarkReport> for BenchmarkReportDto {
    fn from(r: &mathscape_core::math_problem::BenchmarkReport) -> Self {
        BenchmarkReportDto {
            problem_set_size: r.problem_set_size,
            solved_count: r.solved_count,
            solved_fraction: r.solved_fraction(),
            results: r.results.iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurriculumReportDto {
    pub total: BenchmarkReportDto,
    pub per_subdomain: BTreeMap<String, BenchmarkReportDto>,
    pub mastered: Vec<String>,
    pub frontier: Vec<String>,
}

impl From<&mathscape_core::math_problem::CurriculumReport> for CurriculumReportDto {
    fn from(r: &mathscape_core::math_problem::CurriculumReport) -> Self {
        CurriculumReportDto {
            total: (&r.total).into(),
            per_subdomain: r
                .per_subdomain
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.into()))
                .collect(),
            mastered: r.mastered().iter().map(|s| s.to_string()).collect(),
            frontier: r.frontier().iter().map(|s| s.to_string()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn term_dto_serde_uses_kind_discriminator() {
        let t = TermDto::Apply {
            head: Box::new(TermDto::Var { symbol_id: 2 }),
            args: vec![
                TermDto::Number {
                    value: ValueDto::Nat { n: 0 },
                },
                TermDto::Var { symbol_id: 100 },
            ],
        };
        let json = serde_json::to_string(&t).unwrap();
        // The JSON must contain "kind":"apply" to match the
        // OpenAPI spec's discriminator.
        assert!(json.contains("\"kind\":\"apply\""));
        assert!(json.contains("\"kind\":\"var\""));
        assert!(json.contains("\"kind\":\"number\""));
        assert!(json.contains("\"kind\":\"nat\""));
        // Round-trip.
        let back: TermDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn term_dto_round_trips_core_term() {
        use mathscape_core::term::Term;
        use mathscape_core::value::Value;
        let original = Term::Apply(
            Box::new(Term::Var(2)),
            vec![
                Term::Number(Value::Nat(0)),
                Term::Var(100),
            ],
        );
        let dto: TermDto = (&original).into();
        let back: Term = dto.into();
        assert_eq!(back, original);
    }

    #[test]
    fn value_dto_covers_all_five_variants() {
        use mathscape_core::value::Value;
        let cases = [
            Value::Nat(7),
            Value::Int(-3),
            Value::from_f64(1.5).unwrap(),
            Value::tensor(vec![2], vec![1, 2]).unwrap(),
            Value::float_tensor(vec![2], vec![1.0, 2.0]).unwrap(),
        ];
        for original in &cases {
            let dto: ValueDto = original.into();
            let json = serde_json::to_string(&dto).unwrap();
            assert!(json.contains("\"kind\""));
            let back: ValueDto = serde_json::from_str(&json).unwrap();
            let back_val: Value = back.into();
            assert_eq!(back_val, *original);
        }
    }

    #[test]
    fn eval_result_serializes_with_status_discriminator() {
        let ok = EvalResult::Ok {
            value: TermDto::Var { symbol_id: 42 },
        };
        let err = EvalResult::Err {
            message: "step limit exceeded".into(),
        };
        let ok_json = serde_json::to_string(&ok).unwrap();
        let err_json = serde_json::to_string(&err).unwrap();
        assert!(ok_json.contains("\"status\":\"ok\""));
        assert!(err_json.contains("\"status\":\"err\""));
        assert!(err_json.contains("step limit"));
    }
}
