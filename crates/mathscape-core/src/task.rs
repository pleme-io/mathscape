//! Phase W.6 (2026-04-19): task-domain abstraction.
//!
//! # The user-framed observation
//!
//!   "right now we are using math as the only training data."
//!
//! # The shape
//!
//! `Task<D>` is a labeled example for a pluggable problem
//! domain `D: TaskDomain`. Each domain defines its own
//! input/output/context types and the solve + match semantics;
//! the rest of the system — EventHub, StreamingPolicyTrainer,
//! BanditProbe, etc. — stays domain-agnostic.
//!
//! The existing `MathProblem` (in `math_problem`) is effectively
//! `Task<MathDomain>`; `MathDomain` lands here as the first
//! implementation. Future domains (code synthesis, NLP
//! completion, tensor regression, image classification) slot in
//! as new `impl TaskDomain` — without changes to the hub, the
//! trainer, or any existing consumer.
//!
//! # Invariant
//!
//! `TaskReport::solved_fraction()` ∈ [0, 1]. The delta of this
//! metric between consecutive benchmark runs is the labeled
//! reward signal consumed by the streaming trainer via
//! `MapEvent::BenchmarkScored`.

use crate::eval::{eval, RewriteRule};
use crate::term::Term;

/// A problem domain — the unit of pluggable training data.
///
/// Each domain defines:
///  - `Input`: what the task presents to the solver
///  - `Output`: what the solver must produce
///  - `Context`: the external state the solver reads
///    (typically a library, a set of parameters, or a trained
///    policy — the thing that's getting better over time)
///
/// And the solve + match behaviors.
pub trait TaskDomain: 'static {
    type Input: Clone;
    type Output: Clone + PartialEq;
    type Context: ?Sized;

    /// Human-readable domain name (for reporting, logging, and
    /// event categorization).
    fn name() -> &'static str;

    /// Run the solver. Returns `Some(output)` when the solve
    /// reached a result within `step_limit`; `None` if the
    /// budget was exceeded or the input was malformed.
    fn solve(
        ctx: &Self::Context,
        input: &Self::Input,
        step_limit: usize,
    ) -> Option<Self::Output>;

    /// Compare expected output to actual. Default uses `PartialEq`;
    /// domains can override for tolerance-based matching (e.g.
    /// float comparisons, structural approximate match, etc.).
    fn matches(expected: &Self::Output, actual: &Self::Output) -> bool {
        expected == actual
    }
}

/// A single labeled task within a domain `D`.
#[derive(Clone, Debug)]
pub struct Task<D: TaskDomain> {
    pub id: String,
    pub description: String,
    pub input: D::Input,
    pub expected: D::Output,
    pub step_limit: usize,
}

/// One task's evaluation result.
#[derive(Clone)]
pub struct TaskResult<D: TaskDomain> {
    pub id: String,
    pub solved: bool,
    pub actual: Option<D::Output>,
}

impl<D: TaskDomain> std::fmt::Debug for TaskResult<D>
where
    D::Output: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskResult")
            .field("id", &self.id)
            .field("solved", &self.solved)
            .field("actual", &self.actual)
            .finish()
    }
}

/// Aggregate report across a task set.
pub struct TaskReport<D: TaskDomain> {
    pub domain: &'static str,
    pub problem_set_size: usize,
    pub solved_count: usize,
    pub results: Vec<TaskResult<D>>,
}

impl<D: TaskDomain> std::fmt::Debug for TaskReport<D>
where
    D::Output: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskReport")
            .field("domain", &self.domain)
            .field("problem_set_size", &self.problem_set_size)
            .field("solved_count", &self.solved_count)
            .field("results", &self.results)
            .finish()
    }
}

impl<D: TaskDomain> TaskReport<D> {
    #[must_use]
    pub fn solved_fraction(&self) -> f64 {
        if self.problem_set_size == 0 {
            0.0
        } else {
            self.solved_count as f64 / self.problem_set_size as f64
        }
    }
}

/// Run the full benchmark for task domain `D`.
pub fn run_benchmark<D: TaskDomain>(
    tasks: &[Task<D>],
    ctx: &D::Context,
) -> TaskReport<D> {
    let mut results = Vec::with_capacity(tasks.len());
    let mut solved_count = 0;
    for t in tasks {
        let actual = D::solve(ctx, &t.input, t.step_limit);
        let solved = actual
            .as_ref()
            .map(|a| D::matches(&t.expected, a))
            .unwrap_or(false);
        if solved {
            solved_count += 1;
        }
        results.push(TaskResult {
            id: t.id.clone(),
            solved,
            actual,
        });
    }
    TaskReport {
        domain: D::name(),
        problem_set_size: tasks.len(),
        solved_count,
        results,
    }
}

// ── MathDomain — the first task domain ────────────────────────

/// The math-problem domain: input is a Term, output is the
/// evaluated Term, context is the library of rewrite rules.
///
/// This is the same shape as the existing `MathProblem`/
/// `BenchmarkConsumer` machinery, now typed as one instance of
/// the pluggable `TaskDomain` abstraction.
pub struct MathDomain;

impl TaskDomain for MathDomain {
    type Input = Term;
    type Output = Term;
    type Context = [RewriteRule];

    fn name() -> &'static str {
        "math"
    }

    fn solve(
        ctx: &Self::Context,
        input: &Self::Input,
        step_limit: usize,
    ) -> Option<Self::Output> {
        eval(input, ctx, step_limit).ok()
    }
}

/// Adapter: convert a legacy `MathProblem` to `Task<MathDomain>`.
/// Preserves the id, description, input, expected, and
/// step_limit. Exists so existing `canonical_problem_set()` /
/// `harder_problem_set()` data is immediately usable through
/// the generic `run_benchmark`.
impl From<&crate::math_problem::MathProblem> for Task<MathDomain> {
    fn from(p: &crate::math_problem::MathProblem) -> Self {
        Task {
            id: p.id.clone(),
            description: p.description.clone(),
            input: p.input.clone(),
            expected: p.expected.clone(),
            step_limit: p.step_limit,
        }
    }
}

/// Convert a slice of MathProblem into a Vec<Task<MathDomain>>.
pub fn as_math_tasks(
    problems: &[crate::math_problem::MathProblem],
) -> Vec<Task<MathDomain>> {
    problems.iter().map(Into::into).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_problem::{canonical_problem_set, harder_problem_set};

    #[test]
    fn math_domain_has_name_and_matches() {
        assert_eq!(MathDomain::name(), "math");
    }

    #[test]
    fn generic_benchmark_matches_legacy_math_benchmark_on_canonical() {
        let legacy = canonical_problem_set();
        let tasks = as_math_tasks(&legacy);
        let legacy_report =
            crate::math_problem::run_benchmark(&legacy, &[]);
        let generic_report: TaskReport<MathDomain> =
            run_benchmark(&tasks, &[]);
        assert_eq!(legacy_report.problem_set_size, generic_report.problem_set_size);
        assert_eq!(legacy_report.solved_count, generic_report.solved_count);
        assert_eq!(
            legacy_report.solved_fraction(),
            generic_report.solved_fraction()
        );
    }

    #[test]
    fn generic_benchmark_on_harder_set_with_empty_library_scores_zero() {
        let tasks = as_math_tasks(&harder_problem_set());
        let report: TaskReport<MathDomain> = run_benchmark(&tasks, &[]);
        // Symbolic-identity probes need discovered rules. Empty
        // library → 0/6.
        assert_eq!(report.solved_count, 0);
        assert_eq!(report.solved_fraction(), 0.0);
        assert_eq!(report.domain, "math");
    }

    #[test]
    fn generic_benchmark_on_harder_set_with_identity_rules_scores_full() {
        use crate::value::Value;
        // Hand-build the identity rules that harder_problem_set()
        // expects the library to contain.
        let add_identity = RewriteRule {
            name: "add-id".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(2)),
                vec![
                    Term::Number(Value::Nat(0)),
                    Term::Var(100),
                ],
            ),
            rhs: Term::Var(100),
        };
        let mul_identity = RewriteRule {
            name: "mul-id".into(),
            lhs: Term::Apply(
                Box::new(Term::Var(3)),
                vec![
                    Term::Number(Value::Nat(1)),
                    Term::Var(100),
                ],
            ),
            rhs: Term::Var(100),
        };
        let library = vec![add_identity, mul_identity];
        let tasks = as_math_tasks(&harder_problem_set());
        let report: TaskReport<MathDomain> =
            run_benchmark(&tasks, &library);
        assert!(
            report.solved_count >= 1,
            "with identity rules, at least one symbolic probe scores"
        );
        assert_eq!(report.domain, "math");
    }

    // ── Custom domain demo: SumDomain ───────────────────────────

    /// A toy non-math domain: the solver's "context" is a bias
    /// `i64`; the task input is a `Vec<i64>`; the task output is
    /// the sum of the inputs + the bias. Demonstrates that the
    /// abstraction supports domains far from term-rewriting.
    pub struct SumDomain;

    impl TaskDomain for SumDomain {
        type Input = Vec<i64>;
        type Output = i64;
        type Context = i64; // bias

        fn name() -> &'static str {
            "sum-plus-bias"
        }

        fn solve(
            ctx: &Self::Context,
            input: &Self::Input,
            _step_limit: usize,
        ) -> Option<Self::Output> {
            Some(input.iter().sum::<i64>() + *ctx)
        }
    }

    #[test]
    fn custom_domain_proves_task_abstraction_generalizes_beyond_math() {
        let tasks = vec![
            Task::<SumDomain> {
                id: "one-plus-two".into(),
                description: "1 + 2 + bias 0 = 3".into(),
                input: vec![1, 2],
                expected: 3,
                step_limit: 0,
            },
            Task::<SumDomain> {
                id: "ten-plus-five".into(),
                description: "10 + 5 + bias 0 = 15".into(),
                input: vec![10, 5],
                expected: 15,
                step_limit: 0,
            },
        ];
        let report: TaskReport<SumDomain> = run_benchmark(&tasks, &0i64);
        assert_eq!(report.solved_count, 2);
        assert_eq!(report.solved_fraction(), 1.0);
        assert_eq!(report.domain, "sum-plus-bias");

        // The same tasks with a nonzero bias become "wrong" — the
        // solver is task-context-dependent.
        let wrong_report: TaskReport<SumDomain> = run_benchmark(&tasks, &100i64);
        assert_eq!(wrong_report.solved_count, 0);
    }
}
