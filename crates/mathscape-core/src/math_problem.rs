//! Phase V.benchmark (2026-04-18): static math problems with
//! known answers; the motor's report card.
//!
//! # The user-framed problem
//!
//!   "In order to truly improve it needs to know if it's getting
//!    better. The way we are going to make it know or not if it's
//!    getting better is make it solve math that we store quite
//!    statically. Also, for it to translate that data into
//!    something it understands it will need to do something so
//!    we will have to figure that out. We feed it math problems
//!    and see how it does and depending on how it does we push
//!    it further along the mathscape after optimizing it more
//!    and training it more."
//!
//! # The translation
//!
//! A math problem is a pair `(input: Term, expected: Term)`.
//! The motor's library is a set of rewrite rules. "Solving" the
//! problem = `eval(input, library, step_limit)` producing a
//! normal form equal to `expected`.
//!
//! The model doesn't "solve" problems directly — the LIBRARY
//! does. The model's job is to produce a library capable of
//! solving more problems. So the benchmark is a measurement of
//! the library's (equivalently: the motor's discovery trajectory
//! so far) competence.
//!
//! The metric is simple: fraction of the problem set solved. The
//! DELTA of that metric between consecutive benchmarks is the
//! reward signal for the streaming trainer. "Am I getting
//! better?" → "Did my solved_fraction increase since last
//! benchmark?"
//!
//! # Why this is the last piece
//!
//! Without benchmarks, the motor's reward signal is local (new
//! rule found, staleness crossed) — it doesn't know whether
//! those rules are USEFUL for external goals. With a benchmark,
//! the motor has a GROUNDED measure: solve the canonical
//! problem set better, or your discoveries don't count.
//!
//! The motor then becomes a true closed loop with an external
//! grading signal.

use crate::eval::{eval, RewriteRule};
use crate::term::Term;
use crate::value::Value;
use serde::{Deserialize, Serialize};

/// A single math problem: reduce `input` to `expected` using
/// the library and kernel. `step_limit` caps the eval work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MathProblem {
    /// Stable identifier — e.g. "add-nat-2-3" or "tensor-id-left".
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// The term to evaluate.
    pub input: Term,
    /// The expected normal form after evaluation under the library.
    pub expected: Term,
    /// Eval-step budget for this problem.
    pub step_limit: usize,
}

/// Outcome of attempting a single problem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProblemResult {
    pub problem_id: String,
    pub solved: bool,
    /// `Some(term)` when eval produced a result (even if it
    /// doesn't match expected); `None` on eval error.
    pub actual: Option<Term>,
}

impl ProblemResult {
    /// Did eval produce SOME result (even if not the expected one)?
    /// Useful for diagnosing "eval error" vs "wrong answer."
    #[must_use]
    pub fn eval_completed(&self) -> bool {
        self.actual.is_some()
    }
}

/// Aggregate report from benchmarking a problem set against a
/// library. The `solved_fraction` is what the motor uses as
/// its competence signal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub problem_set_size: usize,
    pub solved_count: usize,
    pub results: Vec<ProblemResult>,
}

impl BenchmarkReport {
    /// Fraction of problems solved, in [0.0, 1.0]. This is the
    /// competence metric.
    #[must_use]
    pub fn solved_fraction(&self) -> f64 {
        if self.problem_set_size == 0 {
            return 0.0;
        }
        self.solved_count as f64 / self.problem_set_size as f64
    }

    /// Count of problems where eval completed (even if wrong) —
    /// distinguishes "eval failed" from "wrong answer."
    #[must_use]
    pub fn eval_completed_count(&self) -> usize {
        self.results.iter().filter(|r| r.eval_completed()).count()
    }
}

/// Solve a single problem. `eval(input, library, step_limit)` is
/// compared against `expected`.
#[must_use]
pub fn solve_problem(
    problem: &MathProblem,
    library: &[RewriteRule],
) -> ProblemResult {
    match eval(&problem.input, library, problem.step_limit) {
        Ok(actual) => ProblemResult {
            problem_id: problem.id.clone(),
            solved: actual == problem.expected,
            actual: Some(actual),
        },
        Err(_) => ProblemResult {
            problem_id: problem.id.clone(),
            solved: false,
            actual: None,
        },
    }
}

/// Benchmark an entire problem set against a library. Returns a
/// report aggregating every problem's result.
#[must_use]
pub fn run_benchmark(
    problems: &[MathProblem],
    library: &[RewriteRule],
) -> BenchmarkReport {
    let results: Vec<ProblemResult> = problems
        .iter()
        .map(|p| solve_problem(p, library))
        .collect();
    let solved_count = results.iter().filter(|r| r.solved).count();
    BenchmarkReport {
        problem_set_size: problems.len(),
        solved_count,
        results,
    }
}

/// Canonical problem set — a curated list of math problems
/// covering the basic domains the machine's kernel supports.
/// Hand-crafted. Stable. Serves as the motor's report card.
#[must_use]
pub fn canonical_problem_set() -> Vec<MathProblem> {
    use crate::builtin::{ADD, FT_ADD, FT_MUL, MUL, TENSOR_ADD, TENSOR_MUL};
    let apply = |h: u32, args: Vec<Term>| -> Term {
        Term::Apply(Box::new(Term::Var(h)), args)
    };
    let nat = |n: u64| Term::Number(Value::Nat(n));
    let int = |n: i64| Term::Number(Value::Int(n));
    let tensor = |shape: Vec<usize>, data: Vec<i64>| {
        Term::Number(Value::tensor(shape, data).unwrap())
    };
    let float_tensor = |shape: Vec<usize>, data: Vec<f64>| {
        Term::Number(Value::float_tensor(shape, data).unwrap())
    };

    vec![
        // ── Concrete Nat arithmetic ──────────────────────────────
        MathProblem {
            id: "nat-add-2-3".into(),
            description: "add(2, 3) = 5".into(),
            input: apply(ADD, vec![nat(2), nat(3)]),
            expected: nat(5),
            step_limit: 100,
        },
        MathProblem {
            id: "nat-mul-4-5".into(),
            description: "mul(4, 5) = 20".into(),
            input: apply(MUL, vec![nat(4), nat(5)]),
            expected: nat(20),
            step_limit: 100,
        },
        MathProblem {
            id: "nat-add-identity-left".into(),
            description: "add(0, 7) = 7".into(),
            input: apply(ADD, vec![nat(0), nat(7)]),
            expected: nat(7),
            step_limit: 100,
        },
        MathProblem {
            id: "nat-mul-identity-left".into(),
            description: "mul(1, 9) = 9".into(),
            input: apply(MUL, vec![nat(1), nat(9)]),
            expected: nat(9),
            step_limit: 100,
        },
        MathProblem {
            id: "nat-nested-add".into(),
            description: "add(add(2, 3), 4) = 9".into(),
            input: apply(ADD, vec![apply(ADD, vec![nat(2), nat(3)]), nat(4)]),
            expected: nat(9),
            step_limit: 100,
        },
        // ── Concrete Int arithmetic ──────────────────────────────
        MathProblem {
            id: "int-add-neg-3-5".into(),
            description: "int_add(-3, 5) = 2".into(),
            input: apply(12, vec![int(-3), int(5)]), // INT_ADD = 12
            expected: int(2),
            step_limit: 100,
        },
        MathProblem {
            id: "int-mul-neg".into(),
            description: "int_mul(-2, 3) = -6".into(),
            input: apply(13, vec![int(-2), int(3)]), // INT_MUL = 13
            expected: int(-6),
            step_limit: 100,
        },
        // ── Tensor operations ────────────────────────────────────
        MathProblem {
            id: "tensor-add-zeros-right".into(),
            description: "tensor_add(zeros, [3, 4]) = [3, 4]".into(),
            input: apply(
                TENSOR_ADD,
                vec![tensor(vec![2], vec![0, 0]), tensor(vec![2], vec![3, 4])],
            ),
            expected: tensor(vec![2], vec![3, 4]),
            step_limit: 100,
        },
        MathProblem {
            id: "tensor-mul-ones-right".into(),
            description: "tensor_mul(ones, [5, 7]) = [5, 7]".into(),
            input: apply(
                TENSOR_MUL,
                vec![tensor(vec![2], vec![1, 1]), tensor(vec![2], vec![5, 7])],
            ),
            expected: tensor(vec![2], vec![5, 7]),
            step_limit: 100,
        },
        MathProblem {
            id: "tensor-add-concrete".into(),
            description: "tensor_add([1, 2], [3, 4]) = [4, 6]".into(),
            input: apply(
                TENSOR_ADD,
                vec![tensor(vec![2], vec![1, 2]), tensor(vec![2], vec![3, 4])],
            ),
            expected: tensor(vec![2], vec![4, 6]),
            step_limit: 100,
        },
        // ── Float-tensor ─────────────────────────────────────────
        MathProblem {
            id: "ft-add-zeros".into(),
            description: "ft_add(zeros, [1.5, 2.5]) = [1.5, 2.5]".into(),
            input: apply(
                FT_ADD,
                vec![
                    float_tensor(vec![2], vec![0.0, 0.0]),
                    float_tensor(vec![2], vec![1.5, 2.5]),
                ],
            ),
            expected: float_tensor(vec![2], vec![1.5, 2.5]),
            step_limit: 100,
        },
        MathProblem {
            id: "ft-mul-ones".into(),
            description: "ft_mul(ones, [0.5, 3.0]) = [0.5, 3.0]".into(),
            input: apply(
                FT_MUL,
                vec![
                    float_tensor(vec![2], vec![1.0, 1.0]),
                    float_tensor(vec![2], vec![0.5, 3.0]),
                ],
            ),
            expected: float_tensor(vec![2], vec![0.5, 3.0]),
            step_limit: 100,
        },
    ]
}

/// Harder problems that REQUIRE discovered rules — the kernel
/// alone can't fold these because they mix concrete and symbolic
/// terms (Var with id ≥ 100), which the kernel's builtin folders
/// don't apply to. A library with identity/nested-identity rules
/// solves them; a library without those rules does not.
///
/// This set is the DIAGNOSTIC signal: its solved_fraction
/// directly measures whether the machine has discovered the
/// identity-class laws. It starts at 0% with empty library and
/// climbs as the motor discovers rules.
#[must_use]
pub fn harder_problem_set() -> Vec<MathProblem> {
    use crate::builtin::{ADD, MUL, TENSOR_ADD, TENSOR_MUL};
    let apply = |h: u32, args: Vec<Term>| -> Term {
        Term::Apply(Box::new(Term::Var(h)), args)
    };
    let nat = |n: u64| Term::Number(Value::Nat(n));
    let tensor = |shape: Vec<usize>, data: Vec<i64>| {
        Term::Number(Value::tensor(shape, data).unwrap())
    };
    let pv = |id: u32| Term::Var(id);

    vec![
        // ── Requires `add(0, ?x) = ?x` ───────────────────────────
        MathProblem {
            id: "hard-add-id-left-symbolic".into(),
            description: "add(0, ?x) must reduce to ?x (needs add-identity rule)".into(),
            input: apply(ADD, vec![nat(0), pv(100)]),
            expected: pv(100),
            step_limit: 20,
        },
        // ── Requires `mul(1, ?x) = ?x` ───────────────────────────
        MathProblem {
            id: "hard-mul-id-left-symbolic".into(),
            description: "mul(1, ?x) must reduce to ?x (needs mul-identity rule)".into(),
            input: apply(MUL, vec![nat(1), pv(100)]),
            expected: pv(100),
            step_limit: 20,
        },
        // ── Requires nested `add(0, add(0, ?x)) = ?x` ────────────
        MathProblem {
            id: "hard-nested-add-id".into(),
            description: "add(0, add(0, ?x)) must reduce to ?x via nested identity".into(),
            input: apply(ADD, vec![nat(0), apply(ADD, vec![nat(0), pv(100)])]),
            expected: pv(100),
            step_limit: 20,
        },
        // ── Requires `tensor_add(zeros, ?x) = ?x` ────────────────
        MathProblem {
            id: "hard-tensor-add-id-symbolic".into(),
            description: "tensor_add(zeros, ?x) must reduce to ?x".into(),
            input: apply(
                TENSOR_ADD,
                vec![tensor(vec![2], vec![0, 0]), pv(100)],
            ),
            expected: pv(100),
            step_limit: 20,
        },
        // ── Requires `tensor_mul(ones, ?x) = ?x` ─────────────────
        MathProblem {
            id: "hard-tensor-mul-id-symbolic".into(),
            description: "tensor_mul(ones, ?x) must reduce to ?x".into(),
            input: apply(
                TENSOR_MUL,
                vec![tensor(vec![2], vec![1, 1]), pv(100)],
            ),
            expected: pv(100),
            step_limit: 20,
        },
        // ── Nested tensor — requires both tensor-identities ──────
        MathProblem {
            id: "hard-nested-tensor-add-id".into(),
            description:
                "tensor_add(zeros, tensor_add(zeros, ?x)) must reduce to ?x"
                    .into(),
            input: apply(
                TENSOR_ADD,
                vec![
                    tensor(vec![2], vec![0, 0]),
                    apply(
                        TENSOR_ADD,
                        vec![tensor(vec![2], vec![0, 0]), pv(100)],
                    ),
                ],
            ),
            expected: pv(100),
            step_limit: 20,
        },
    ]
}

// ════════════════════════════════════════════════════════════════
// Phase X — The Mathematician's Curriculum (2026-04-19)
//
// An excellent mathematician does not memorize answers; they
// master *subdomains* and compose across them. This curriculum
// is a tiered ladder that the machine can climb from the kernel
// upward. Each problem is tagged with a subdomain so we can
// measure per-subdomain mastery, not just an overall number.
//
// Subdomains:
//   1. arithmetic-nat   — concrete additions, multiplications,
//                         nesting over Peano naturals
//   2. arithmetic-int   — negation, subtraction, signed combo
//   3. symbolic-nat     — the identity laws on Nat operators
//                         (needs add-id / mul-id discovered)
//   4. tensor-algebra   — zeros/ones identities on tensor ops
//   5. compound         — multi-step reductions mixing operators
//   6. generalization   — the same law applied at multiple
//                         concrete instantiations; all-or-nothing
//
// Each of the 6 subdomains carries 5+ problems for a total of
// 30+. Scoring produces a per-subdomain breakdown so the machine
// can see WHERE it's strong and WHERE it's weak.
// ════════════════════════════════════════════════════════════════

/// One problem tagged with its subdomain.
#[derive(Debug, Clone)]
pub struct CurriculumProblem {
    pub subdomain: &'static str,
    pub problem: MathProblem,
}

/// Full curriculum report with per-subdomain breakdown.
#[derive(Debug, Clone)]
pub struct CurriculumReport {
    pub total: BenchmarkReport,
    pub per_subdomain: std::collections::BTreeMap<&'static str, BenchmarkReport>,
}

impl CurriculumReport {
    /// Human-readable per-subdomain summary.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "curriculum: {}/{} solved ({:.0}%)",
            self.total.solved_count,
            self.total.problem_set_size,
            self.total.solved_fraction() * 100.0
        );
        for (subdomain, report) in &self.per_subdomain {
            s.push_str(&format!(
                "\n  {}: {}/{} ({:.0}%)",
                subdomain,
                report.solved_count,
                report.problem_set_size,
                report.solved_fraction() * 100.0
            ));
        }
        s
    }

    /// Subdomains at which the model has fully mastered (100%).
    pub fn mastered(&self) -> Vec<&'static str> {
        self.per_subdomain
            .iter()
            .filter(|(_, r)| r.solved_fraction() >= 0.9999)
            .map(|(k, _)| *k)
            .collect()
    }

    /// Subdomains where the model scores zero (the frontier — where
    /// the next most valuable learning can happen).
    pub fn frontier(&self) -> Vec<&'static str> {
        self.per_subdomain
            .iter()
            .filter(|(_, r)| r.solved_fraction() < 0.01)
            .map(|(k, _)| *k)
            .collect()
    }
}

/// Run the full curriculum and produce a per-subdomain report.
#[must_use]
pub fn run_curriculum(
    curriculum: &[CurriculumProblem],
    library: &[RewriteRule],
) -> CurriculumReport {
    use std::collections::BTreeMap;
    let mut all_results: Vec<ProblemResult> = Vec::with_capacity(curriculum.len());
    let mut bucket: BTreeMap<&'static str, Vec<ProblemResult>> = BTreeMap::new();
    for cp in curriculum {
        let r = solve_problem(&cp.problem, library);
        bucket.entry(cp.subdomain).or_default().push(r.clone());
        all_results.push(r);
    }
    let total_solved = all_results.iter().filter(|r| r.solved).count();
    let total = BenchmarkReport {
        problem_set_size: all_results.len(),
        solved_count: total_solved,
        results: all_results,
    };
    let per_subdomain = bucket
        .into_iter()
        .map(|(k, v)| {
            let solved = v.iter().filter(|r| r.solved).count();
            (
                k,
                BenchmarkReport {
                    problem_set_size: v.len(),
                    solved_count: solved,
                    results: v,
                },
            )
        })
        .collect();
    CurriculumReport {
        total,
        per_subdomain,
    }
}

/// The complete mathematician's curriculum — 32 problems across
/// 6 subdomains. The machine's goal: score 100% on every
/// subdomain. That is what "excellent mathematician" means for
/// this substrate.
#[must_use]
pub fn mathematician_curriculum() -> Vec<CurriculumProblem> {
    use crate::builtin::{ADD, MUL, TENSOR_ADD, TENSOR_MUL};
    const INT_ADD: u32 = 12;
    const INT_MUL: u32 = 13;
    const INT_NEG: u32 = 14;
    const INT_SUB: u32 = 15;

    let apply = |h: u32, args: Vec<Term>| -> Term {
        Term::Apply(Box::new(Term::Var(h)), args)
    };
    let nat = |n: u64| Term::Number(Value::Nat(n));
    let int = |n: i64| Term::Number(Value::Int(n));
    let tensor = |shape: Vec<usize>, data: Vec<i64>| {
        Term::Number(Value::tensor(shape, data).unwrap())
    };
    let pv = |id: u32| Term::Var(id);

    let mut curriculum = Vec::new();

    // ── Subdomain 1: arithmetic-nat (5 problems) ────────────────
    let sd_nat = "arithmetic-nat";
    for (id, input, expected) in [
        ("nat-0", apply(ADD, vec![nat(0), nat(0)]), nat(0)),
        ("nat-simple-add", apply(ADD, vec![nat(3), nat(4)]), nat(7)),
        ("nat-simple-mul", apply(MUL, vec![nat(6), nat(7)]), nat(42)),
        (
            "nat-deep-nested",
            apply(
                ADD,
                vec![
                    apply(ADD, vec![nat(1), nat(2)]),
                    apply(ADD, vec![nat(3), nat(4)]),
                ],
            ),
            nat(10),
        ),
        (
            "nat-mul-chain",
            apply(MUL, vec![apply(MUL, vec![nat(2), nat(3)]), nat(4)]),
            nat(24),
        ),
    ] {
        curriculum.push(CurriculumProblem {
            subdomain: sd_nat,
            problem: MathProblem {
                id: id.into(),
                description: id.into(),
                input,
                expected,
                step_limit: 100,
            },
        });
    }

    // ── Subdomain 2: arithmetic-int (5 problems) ────────────────
    let sd_int = "arithmetic-int";
    for (id, input, expected) in [
        ("int-add-neg", apply(INT_ADD, vec![int(-5), int(3)]), int(-2)),
        ("int-mul-neg-neg", apply(INT_MUL, vec![int(-2), int(-4)]), int(8)),
        ("int-neg-neg", apply(INT_NEG, vec![int(-7)]), int(7)),
        ("int-sub", apply(INT_SUB, vec![int(10), int(3)]), int(7)),
        (
            "int-deep",
            apply(
                INT_ADD,
                vec![
                    apply(INT_MUL, vec![int(2), int(-3)]),
                    apply(INT_NEG, vec![int(-4)]),
                ],
            ),
            int(-2),
        ),
    ] {
        curriculum.push(CurriculumProblem {
            subdomain: sd_int,
            problem: MathProblem {
                id: id.into(),
                description: id.into(),
                input,
                expected,
                step_limit: 100,
            },
        });
    }

    // ── Subdomain 3: symbolic-nat (6 problems) ──────────────────
    // These need identity rules in the library to solve.
    let sd_symbolic = "symbolic-nat";
    let symbolic_cases: Vec<(&str, Term, Term)> = vec![
        (
            "sym-add-id-left",
            apply(ADD, vec![nat(0), pv(100)]),
            pv(100),
        ),
        (
            "sym-mul-id-left",
            apply(MUL, vec![nat(1), pv(100)]),
            pv(100),
        ),
        (
            "sym-add-id-nested",
            apply(ADD, vec![nat(0), apply(ADD, vec![nat(0), pv(100)])]),
            pv(100),
        ),
        (
            "sym-mul-id-nested",
            apply(MUL, vec![nat(1), apply(MUL, vec![nat(1), pv(100)])]),
            pv(100),
        ),
        (
            "sym-add-id-different-var",
            apply(ADD, vec![nat(0), pv(200)]),
            pv(200),
        ),
        (
            "sym-mul-id-different-var",
            apply(MUL, vec![nat(1), pv(300)]),
            pv(300),
        ),
    ];
    for (id, input, expected) in symbolic_cases {
        curriculum.push(CurriculumProblem {
            subdomain: sd_symbolic,
            problem: MathProblem {
                id: id.into(),
                description: id.into(),
                input,
                expected,
                step_limit: 20,
            },
        });
    }

    // ── Subdomain 4: tensor-algebra (6 problems) ────────────────
    let sd_tensor = "tensor-algebra";
    let tensor_cases: Vec<(&str, Term, Term)> = vec![
        (
            "tensor-add-zeros-concrete",
            apply(
                TENSOR_ADD,
                vec![tensor(vec![2], vec![0, 0]), tensor(vec![2], vec![3, 4])],
            ),
            tensor(vec![2], vec![3, 4]),
        ),
        (
            "tensor-mul-ones-concrete",
            apply(
                TENSOR_MUL,
                vec![tensor(vec![2], vec![1, 1]), tensor(vec![2], vec![5, 7])],
            ),
            tensor(vec![2], vec![5, 7]),
        ),
        (
            "tensor-add-zeros-sym",
            apply(
                TENSOR_ADD,
                vec![tensor(vec![2], vec![0, 0]), pv(100)],
            ),
            pv(100),
        ),
        (
            "tensor-mul-ones-sym",
            apply(
                TENSOR_MUL,
                vec![tensor(vec![2], vec![1, 1]), pv(100)],
            ),
            pv(100),
        ),
        (
            "tensor-add-nested-zeros",
            apply(
                TENSOR_ADD,
                vec![
                    tensor(vec![2], vec![0, 0]),
                    apply(
                        TENSOR_ADD,
                        vec![tensor(vec![2], vec![0, 0]), pv(100)],
                    ),
                ],
            ),
            pv(100),
        ),
        (
            "tensor-3d-zeros",
            apply(
                TENSOR_ADD,
                vec![tensor(vec![3], vec![0, 0, 0]), pv(100)],
            ),
            pv(100),
        ),
    ];
    for (id, input, expected) in tensor_cases {
        curriculum.push(CurriculumProblem {
            subdomain: sd_tensor,
            problem: MathProblem {
                id: id.into(),
                description: id.into(),
                input,
                expected,
                step_limit: 20,
            },
        });
    }

    // ── Subdomain 5: compound (5 problems) ──────────────────────
    // Multi-step problems that exercise MULTIPLE rules in sequence.
    let sd_compound = "compound";
    let compound_cases: Vec<(&str, Term, Term)> = vec![
        // add(0, mul(1, ?x)) = ?x — needs BOTH add-id AND mul-id
        (
            "compound-add-mul-ids",
            apply(ADD, vec![nat(0), apply(MUL, vec![nat(1), pv(100)])]),
            pv(100),
        ),
        // mul(1, add(0, ?x)) = ?x
        (
            "compound-mul-add-ids",
            apply(MUL, vec![nat(1), apply(ADD, vec![nat(0), pv(100)])]),
            pv(100),
        ),
        // add(0, add(2, 3)) = 5 — identity on top of concrete
        (
            "compound-id-over-concrete",
            apply(ADD, vec![nat(0), apply(ADD, vec![nat(2), nat(3)])]),
            nat(5),
        ),
        // mul(1, mul(2, 3)) = 6
        (
            "compound-mul-id-over-concrete",
            apply(MUL, vec![nat(1), apply(MUL, vec![nat(2), nat(3)])]),
            nat(6),
        ),
        // tensor_add(zeros, tensor_mul(ones, ?x)) = ?x
        (
            "compound-tensor-both-ids",
            apply(
                TENSOR_ADD,
                vec![
                    tensor(vec![2], vec![0, 0]),
                    apply(
                        TENSOR_MUL,
                        vec![tensor(vec![2], vec![1, 1]), pv(100)],
                    ),
                ],
            ),
            pv(100),
        ),
    ];
    for (id, input, expected) in compound_cases {
        curriculum.push(CurriculumProblem {
            subdomain: sd_compound,
            problem: MathProblem {
                id: id.into(),
                description: id.into(),
                input,
                expected,
                step_limit: 30,
            },
        });
    }

    // ── Subdomain 6: generalization (5 problems) ────────────────
    // Same law instantiated with distinct concrete values — the
    // rule generalizes if and only if ALL pass.
    let sd_gen = "generalization";
    for n in [7u64, 42, 123, 999, 5555] {
        curriculum.push(CurriculumProblem {
            subdomain: sd_gen,
            problem: MathProblem {
                id: format!("gen-add-id-at-{n}"),
                description: format!(
                    "add(0, {n}) = {n} — generalization probe"
                ),
                input: apply(ADD, vec![nat(0), nat(n)]),
                expected: nat(n),
                step_limit: 100,
            },
        });
    }

    curriculum
}

/// The ingress point: a consumer that periodically benchmarks
/// the current library against a fixed problem set and emits
/// `MapEvent::BenchmarkScored` with delta-from-prior. This is
/// the labeled-data hook — the stream gains a supervised signal
/// the streaming trainer can reward.
///
/// Usage: hold a reference to the current library (e.g. via a
/// RefCell the motor updates), call `benchmark_now(library,
/// downstream)` after notable mutations. Or invoke periodically
/// from the motor's loop.
#[derive(Debug)]
pub struct BenchmarkConsumer {
    /// The canonical problem set this consumer evaluates against.
    pub problems: Vec<MathProblem>,
    /// Last observed solved_fraction. Used to compute delta.
    /// `None` before the first benchmark (→ NaN delta).
    last_score: std::cell::Cell<Option<f64>>,
    /// Count of benchmarks run.
    runs: std::cell::Cell<u64>,
}

impl BenchmarkConsumer {
    #[must_use]
    pub fn new(problems: Vec<MathProblem>) -> Self {
        Self {
            problems,
            last_score: std::cell::Cell::new(None),
            runs: std::cell::Cell::new(0),
        }
    }

    /// Create a BenchmarkConsumer with the canonical problem set.
    #[must_use]
    pub fn with_canonical_set() -> Self {
        Self::new(canonical_problem_set())
    }

    /// Run one benchmark against the given library and emit the
    /// `BenchmarkScored` event to `downstream`. Returns the report
    /// for observation.
    pub fn benchmark_now<C: crate::mathscape_map::MapEventConsumer>(
        &self,
        library: &[RewriteRule],
        downstream: &C,
    ) -> BenchmarkReport {
        let report = run_benchmark(&self.problems, library);
        let fraction = report.solved_fraction();
        let delta = match self.last_score.get() {
            Some(prev) => fraction - prev,
            None => f64::NAN,
        };
        self.last_score.set(Some(fraction));
        self.runs.set(self.runs.get() + 1);
        downstream.on_event(&crate::mathscape_map::MapEvent::BenchmarkScored {
            solved_count: report.solved_count,
            total: report.problem_set_size,
            solved_fraction: fraction,
            delta_from_prior: delta,
        });
        report
    }

    pub fn runs(&self) -> u64 {
        self.runs.get()
    }

    pub fn last_score(&self) -> Option<f64> {
        self.last_score.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_problem_set_is_non_empty() {
        let set = canonical_problem_set();
        assert!(set.len() >= 10, "canonical set has ≥10 problems");
    }

    #[test]
    fn canonical_set_solves_most_with_empty_library() {
        // With no library (pure kernel reduction), most concrete
        // problems should solve — the kernel handles constant
        // folding on Nats / Ints / Tensors.
        let set = canonical_problem_set();
        let report = run_benchmark(&set, &[]);
        println!(
            "  canonical-set bench (empty library): {}/{} solved ({:.0}%)",
            report.solved_count,
            report.problem_set_size,
            report.solved_fraction() * 100.0,
        );
        for r in &report.results {
            let mark = if r.solved { "✓" } else { "✗" };
            println!("    {mark} {}", r.problem_id);
        }
        // Require at least half solve from the kernel alone.
        assert!(
            report.solved_fraction() >= 0.5,
            "kernel alone must solve ≥50% of canonical set, got {:.0}%",
            report.solved_fraction() * 100.0
        );
    }

    #[test]
    fn benchmark_is_deterministic() {
        let set = canonical_problem_set();
        let a = run_benchmark(&set, &[]);
        let b = run_benchmark(&set, &[]);
        assert_eq!(a, b);
    }

    #[test]
    fn empty_problem_set_scores_zero() {
        let report = run_benchmark(&[], &[]);
        assert_eq!(report.problem_set_size, 0);
        assert_eq!(report.solved_fraction(), 0.0);
    }

    #[test]
    fn problem_result_distinguishes_eval_error_from_wrong_answer() {
        let nat = |n: u64| Term::Number(Value::Nat(n));
        // A deliberately wrong expected answer — eval completes
        // but the actual doesn't match.
        let wrong_problem = MathProblem {
            id: "wrong".into(),
            description: "add(2, 3) deliberately wrong expected".into(),
            input: Term::Apply(
                Box::new(Term::Var(crate::builtin::ADD)),
                vec![nat(2), nat(3)],
            ),
            expected: nat(999),
            step_limit: 100,
        };
        let result = solve_problem(&wrong_problem, &[]);
        assert!(!result.solved);
        assert!(result.eval_completed(), "eval DID complete with actual=5");
        assert_eq!(result.actual, Some(nat(5)));
    }

    // ── BenchmarkConsumer (ingress) tests ────────────────────────

    #[test]
    fn benchmark_consumer_dispatches_event() {
        use crate::mathscape_map::{BufferedConsumer, MapEvent};
        let bench = BenchmarkConsumer::with_canonical_set();
        let downstream = BufferedConsumer::new();
        let report = bench.benchmark_now(&[], &downstream);
        // Event was emitted.
        let events = downstream.drain();
        assert_eq!(events.len(), 1);
        match &events[0] {
            MapEvent::BenchmarkScored {
                solved_count,
                total,
                solved_fraction,
                delta_from_prior,
            } => {
                assert_eq!(*solved_count, report.solved_count);
                assert_eq!(*total, report.problem_set_size);
                assert!((solved_fraction - report.solved_fraction()).abs() < 1e-9);
                // First benchmark → delta is NaN.
                assert!(delta_from_prior.is_nan());
            }
            _ => panic!("expected BenchmarkScored"),
        }
    }

    #[test]
    fn second_benchmark_carries_real_delta() {
        use crate::mathscape_map::{BufferedConsumer, MapEvent};
        let bench = BenchmarkConsumer::with_canonical_set();
        let downstream = BufferedConsumer::new();
        bench.benchmark_now(&[], &downstream);
        downstream.drain();
        bench.benchmark_now(&[], &downstream);
        let events = downstream.drain();
        assert_eq!(events.len(), 1);
        if let MapEvent::BenchmarkScored {
            delta_from_prior, ..
        } = &events[0]
        {
            // Same library → same score → delta = 0.
            assert!(!delta_from_prior.is_nan());
            assert!(delta_from_prior.abs() < 1e-9);
        } else {
            panic!("expected BenchmarkScored");
        }
    }

    #[test]
    fn streaming_trainer_rewards_benchmark_improvement() {
        use crate::mathscape_map::{MapEvent, MapEventConsumer};
        use crate::streaming_policy::StreamingPolicyTrainer;
        // Simulate a benchmark event with improvement.
        let trainer = StreamingPolicyTrainer::new(0.1);
        let improvement_event = MapEvent::BenchmarkScored {
            solved_count: 10,
            total: 12,
            solved_fraction: 10.0 / 12.0,
            delta_from_prior: 0.2, // improved by 20pp
        };
        trainer.on_event(&improvement_event);
        let snap = trainer.snapshot();
        // Absolute contribution: 2.0 × 0.833 = 1.667
        // Delta contribution  : 3.0 × 0.2 = 0.6
        // Total reward ≈ 2.27; bias should move significantly
        // positive.
        assert!(
            snap.bias > 0.1,
            "benchmark improvement must produce strong positive bias, got {}",
            snap.bias
        );
    }

    #[test]
    fn streaming_trainer_penalizes_benchmark_regression() {
        use crate::mathscape_map::{MapEvent, MapEventConsumer};
        use crate::streaming_policy::StreamingPolicyTrainer;
        let trainer = StreamingPolicyTrainer::new(0.1);
        let regression_event = MapEvent::BenchmarkScored {
            solved_count: 5,
            total: 12,
            solved_fraction: 5.0 / 12.0,
            delta_from_prior: -0.3, // regressed 30pp
        };
        trainer.on_event(&regression_event);
        let snap = trainer.snapshot();
        // Absolute:  2.0 × 0.417 = 0.833
        // Delta   : -0.3 × 5.0 = -1.5 (regression asymmetric penalty)
        // Total  ≈ -0.667; bias goes negative.
        assert!(
            snap.bias < 0.0,
            "benchmark regression must produce negative bias, got {}",
            snap.bias
        );
    }

    #[test]
    fn benchmark_consumer_chained_with_streaming_trainer() {
        // End-to-end labeled training loop:
        //   bench.benchmark_now(library, trainer)
        // The trainer's bias moves based on the benchmark result.
        use crate::streaming_policy::StreamingPolicyTrainer;
        let bench = BenchmarkConsumer::with_canonical_set();
        let trainer = StreamingPolicyTrainer::new(0.1);
        let pre_bias = trainer.snapshot().bias;
        bench.benchmark_now(&[], &trainer);
        let post_bias = trainer.snapshot().bias;
        // The canonical set scores 100% on empty lib → strong
        // positive absolute signal; delta is NaN on first call
        // so zero contribution. Bias rises.
        assert!(
            post_bias > pre_bias,
            "trainer bias must rise on first benchmark (100% on empty lib)"
        );
    }

    // ── Harder problem set tests ────────────────────────────────

    #[test]
    fn harder_set_baseline_with_empty_library() {
        // Kernel alone cannot fold symbolic identities — it needs
        // the identity rules. Empty-library score should be LOW.
        let set = harder_problem_set();
        let report = run_benchmark(&set, &[]);
        println!(
            "\n  harder-set bench (empty library): {}/{} solved ({:.0}%)",
            report.solved_count,
            report.problem_set_size,
            report.solved_fraction() * 100.0,
        );
        for r in &report.results {
            let mark = if r.solved { "✓" } else { "✗" };
            println!("    {mark} {}", r.problem_id);
        }
        // Empty library should solve at MOST half — the point of
        // this set is to leave room for the library to help.
        assert!(
            report.solved_fraction() <= 0.5,
            "harder set should NOT be mostly solved by kernel alone; got {:.0}%",
            report.solved_fraction() * 100.0
        );
    }

    #[test]
    fn harder_set_improves_with_identity_rules() {
        use crate::builtin::{ADD, MUL, TENSOR_ADD, TENSOR_MUL};
        let apply = |h: u32, args: Vec<Term>| {
            Term::Apply(Box::new(Term::Var(h)), args)
        };
        let nat = |n: u64| Term::Number(Value::Nat(n));
        let tensor = |shape: Vec<usize>, data: Vec<i64>| {
            Term::Number(Value::tensor(shape, data).unwrap())
        };
        let pv = |id: u32| Term::Var(id);

        // Pre-load the library with the identity rules the
        // machine would discover via Phase I + J.
        let lib = vec![
            RewriteRule {
                name: "add-id-left".into(),
                lhs: apply(ADD, vec![nat(0), pv(200)]),
                rhs: pv(200),
            },
            RewriteRule {
                name: "mul-id-left".into(),
                lhs: apply(MUL, vec![nat(1), pv(200)]),
                rhs: pv(200),
            },
            RewriteRule {
                name: "tensor-add-id".into(),
                lhs: apply(
                    TENSOR_ADD,
                    vec![tensor(vec![2], vec![0, 0]), pv(200)],
                ),
                rhs: pv(200),
            },
            RewriteRule {
                name: "tensor-mul-id".into(),
                lhs: apply(
                    TENSOR_MUL,
                    vec![tensor(vec![2], vec![1, 1]), pv(200)],
                ),
                rhs: pv(200),
            },
        ];
        let set = harder_problem_set();
        let empty_report = run_benchmark(&set, &[]);
        let full_report = run_benchmark(&set, &lib);
        println!(
            "\n  harder-set with identity rules: {}/{} (was {}/{} empty)",
            full_report.solved_count,
            full_report.problem_set_size,
            empty_report.solved_count,
            empty_report.problem_set_size,
        );
        // Library should strictly improve score on harder set.
        assert!(
            full_report.solved_count > empty_report.solved_count,
            "identity-rule library must solve MORE than empty library"
        );
    }

    #[test]
    fn benchmark_report_bincode_roundtrip() {
        let set = canonical_problem_set();
        let report = run_benchmark(&set, &[]);
        let bytes = bincode::serialize(&report).unwrap();
        let back: BenchmarkReport = bincode::deserialize(&bytes).unwrap();
        assert_eq!(report, back);
    }
}
