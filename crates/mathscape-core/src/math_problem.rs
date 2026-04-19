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

    #[test]
    fn benchmark_report_bincode_roundtrip() {
        let set = canonical_problem_set();
        let report = run_benchmark(&set, &[]);
        let bytes = bincode::serialize(&report).unwrap();
        let back: BenchmarkReport = bincode::deserialize(&bytes).unwrap();
        assert_eq!(report, back);
    }
}
