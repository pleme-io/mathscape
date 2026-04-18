use serde::{Deserialize, Serialize};
use std::fmt;

/// A numeric value in the expression system.
///
/// R7 (2026-04-18): added `Int(i64)` as the second numeric domain.
/// Before R7, Value was single-variant (Nat only) — the enum layer
/// was nominally extensible but the extension had never been made,
/// and every `match` on Value was exhaustive on one variant. Adding
/// Int proves the enum really is extensible AND opens a second
/// numeric domain for the machine to discover theorems in.
///
/// Domains are INDEPENDENT: a Nat builtin (id 0..=3) rejects Int
/// args (returns `None` from eval), and vice versa for Int
/// builtins (id 10..=14). No implicit promotion Nat→Int. The
/// machine would have to discover a promotion operator itself if
/// it wanted one — that's precisely the kind of structural
/// discovery R7 is supposed to make tractable.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum Value {
    /// Natural number. Successor-only semantics: zero is the
    /// identity, succ is `n -> n+1`. No negation.
    Nat(u64),
    /// Signed integer. Supports negation and a two-sided successor
    /// semantics. Distinct from Nat at the value level — promoting
    /// `Nat(3)` to `Int(3)` requires an explicit operator, it's not
    /// automatic.
    Int(i64),
    /// R13: a shape-tagged integer tensor — the compute-layer
    /// primitive. `shape` is the dimensions (e.g. `[2, 3]` for a
    /// 2×3 matrix). `data` is row-major flat storage. The
    /// invariant `data.len() == shape.iter().product()` is
    /// maintained by constructor helpers in `impl Value` below;
    /// direct field construction can break it (avoid).
    ///
    /// Integer-only for now: avoids floating-point NaN/ordering
    /// complexity. Future extension to Float(f64) data requires a
    /// new variant since Value derives Eq/Ord/Hash and f64
    /// doesn't satisfy those.
    Tensor { shape: Vec<usize>, data: Vec<i64> },
}

impl Value {
    /// Nat zero — the additive identity for the Nat domain.
    pub fn zero_nat() -> Self {
        Value::Nat(0)
    }

    /// Int zero — the additive identity for the Int domain.
    pub fn zero_int() -> Self {
        Value::Int(0)
    }

    /// Deprecated: prefer `zero_nat` / `zero_int` for explicit
    /// domain. Kept for backward compatibility.
    #[deprecated(note = "use zero_nat() or zero_int() — domain must be explicit post-R7")]
    pub fn zero() -> Self {
        Value::Nat(0)
    }

    /// Successor — defined for both Nat and Int. Maps each value
    /// to its +1 in its own domain. Undefined for Tensor (caller
    /// gets identity — no increment semantics on a multi-element
    /// container).
    pub fn succ(&self) -> Self {
        match self {
            Value::Nat(n) => Value::Nat(n + 1),
            Value::Int(n) => Value::Int(n + 1),
            Value::Tensor { .. } => self.clone(),
        }
    }

    /// Negation — defined for Int and Tensor (element-wise). Nat has
    /// no negatives; caller gets `None` for Nat input.
    pub fn neg(&self) -> Option<Self> {
        match self {
            Value::Nat(_) => None,
            Value::Int(n) => Some(Value::Int(-n)),
            Value::Tensor { shape, data } => Some(Value::Tensor {
                shape: shape.clone(),
                data: data.iter().map(|x| -x).collect(),
            }),
        }
    }

    /// View as Nat; `None` if this value lives in another domain.
    pub fn as_nat(&self) -> Option<u64> {
        match self {
            Value::Nat(n) => Some(*n),
            _ => None,
        }
    }

    /// View as Int; `None` if this value lives in another domain.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// View as Tensor; `None` if this is a scalar.
    pub fn as_tensor(&self) -> Option<(&[usize], &[i64])> {
        match self {
            Value::Tensor { shape, data } => Some((shape, data)),
            _ => None,
        }
    }

    /// True if both values live in the same numeric domain. Used
    /// by eval rules that reject cross-domain args.
    pub fn same_domain(&self, other: &Value) -> bool {
        matches!(
            (self, other),
            (Value::Nat(_), Value::Nat(_))
                | (Value::Int(_), Value::Int(_))
                | (Value::Tensor { .. }, Value::Tensor { .. })
        )
    }

    // ── R13 tensor constructors ──────────────────────────────────

    /// Construct a tensor from shape + data. Validates that
    /// `data.len()` equals the product of `shape`; returns `None`
    /// on mismatch.
    pub fn tensor(shape: Vec<usize>, data: Vec<i64>) -> Option<Self> {
        let expected: usize = shape.iter().product();
        if data.len() != expected {
            return None;
        }
        Some(Value::Tensor { shape, data })
    }

    /// Tensor of all zeros with the given shape.
    pub fn tensor_zeros(shape: Vec<usize>) -> Self {
        let n: usize = shape.iter().product();
        Value::Tensor {
            shape,
            data: vec![0; n],
        }
    }

    /// Tensor of all ones with the given shape.
    pub fn tensor_ones(shape: Vec<usize>) -> Self {
        let n: usize = shape.iter().product();
        Value::Tensor {
            shape,
            data: vec![1; n],
        }
    }

    /// Number of elements in a tensor. Undefined for scalars — use
    /// `as_tensor` first.
    pub fn tensor_numel(&self) -> Option<usize> {
        self.as_tensor().map(|(_, d)| d.len())
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nat(n) => write!(f, "{n}"),
            // Int rendering disambiguates from Nat with a leading
            // sign when negative; positive Ints print as plain
            // digits to keep tests/golden files readable. A reader
            // can't tell Nat(3) from Int(3) in display alone —
            // that's by design; Display is for humans, Debug is
            // for the machine.
            Value::Int(n) => write!(f, "{n}"),
            // R13: Tensor renders shape + data.
            //   scalar-shape:  <> means 0-rank (rare)
            //   1D: [1 2 3]
            //   2D: [[1 2] [3 4]]  (just flat for now; multi-dim
            //       pretty-printing is future work)
            Value::Tensor { shape, data } => {
                let shape_str = shape
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join("x");
                let data_str = data
                    .iter()
                    .map(|d| d.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                write!(f, "T<{shape_str}>[{data_str}]")
            }
        }
    }
}

impl From<u64> for Value {
    fn from(n: u64) -> Self {
        Value::Nat(n)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Int(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nat_and_int_are_distinct_values() {
        // Same numeric content, different domains — structurally
        // distinct. The kernel "no equal terms" invariant applies
        // within a domain; across domains the values are genuinely
        // different.
        let a = Value::Nat(3);
        let b = Value::Int(3);
        assert_ne!(a, b);
    }

    #[test]
    fn succ_works_in_both_domains() {
        assert_eq!(Value::Nat(5).succ(), Value::Nat(6));
        assert_eq!(Value::Int(-1).succ(), Value::Int(0));
        assert_eq!(Value::Int(5).succ(), Value::Int(6));
    }

    #[test]
    fn neg_defined_only_for_int() {
        assert_eq!(Value::Int(7).neg(), Some(Value::Int(-7)));
        // Nat has no negatives — neg must refuse.
        assert_eq!(Value::Nat(7).neg(), None);
        // Involution: neg(neg(x)) = x for Int.
        let once = Value::Int(42).neg().unwrap();
        let twice = once.neg().unwrap();
        assert_eq!(twice, Value::Int(42));
    }

    #[test]
    fn as_nat_and_as_int_are_domain_sensitive() {
        assert_eq!(Value::Nat(5).as_nat(), Some(5));
        assert_eq!(Value::Nat(5).as_int(), None);
        assert_eq!(Value::Int(5).as_int(), Some(5));
        assert_eq!(Value::Int(5).as_nat(), None);
    }

    #[test]
    fn same_domain_predicate() {
        assert!(Value::Nat(3).same_domain(&Value::Nat(8)));
        assert!(Value::Int(3).same_domain(&Value::Int(8)));
        assert!(!Value::Nat(3).same_domain(&Value::Int(3)));
        assert!(!Value::Int(3).same_domain(&Value::Nat(3)));
    }

    #[test]
    fn ord_puts_nat_before_int() {
        // Derived Ord orders variants by declaration order; Nat
        // declared first, so Nat(anything) < Int(anything). This
        // affects canonical sort: in an AC args list with mixed
        // domains, Nats cluster before Ints. (Though in practice
        // the kernel rejects mixed-domain eval, so canonical form
        // wouldn't need to mix them.)
        assert!(Value::Nat(1_000_000) < Value::Int(-5));
    }

    #[test]
    fn zero_nat_and_zero_int_are_different() {
        assert_eq!(Value::zero_nat(), Value::Nat(0));
        assert_eq!(Value::zero_int(), Value::Int(0));
        assert_ne!(Value::zero_nat(), Value::zero_int());
    }

    #[test]
    fn from_u64_yields_nat_from_i64_yields_int() {
        let a: Value = 42u64.into();
        let b: Value = 42i64.into();
        assert_eq!(a, Value::Nat(42));
        assert_eq!(b, Value::Int(42));
        assert_ne!(a, b);
    }

    #[test]
    fn display_renders_both_domains() {
        assert_eq!(format!("{}", Value::Nat(7)), "7");
        assert_eq!(format!("{}", Value::Int(7)), "7");
        assert_eq!(format!("{}", Value::Int(-3)), "-3");
    }

    // ── R13: Tensor value tests ──────────────────────────────────

    #[test]
    fn tensor_constructor_validates_shape() {
        // Good: shape product matches data length.
        assert!(Value::tensor(vec![2, 3], vec![1, 2, 3, 4, 5, 6]).is_some());
        // Bad: shape says 6 elements, data has 5.
        assert!(Value::tensor(vec![2, 3], vec![1, 2, 3, 4, 5]).is_none());
        // Bad: shape says 0 elements, data has 1.
        assert!(Value::tensor(vec![0], vec![1]).is_none());
    }

    #[test]
    fn tensor_zeros_and_ones_have_correct_size() {
        let z = Value::tensor_zeros(vec![3, 4]);
        let (shape, data) = z.as_tensor().unwrap();
        assert_eq!(shape, &[3, 4]);
        assert_eq!(data.len(), 12);
        assert!(data.iter().all(|d| *d == 0));

        let o = Value::tensor_ones(vec![2, 2]);
        let (shape, data) = o.as_tensor().unwrap();
        assert_eq!(shape, &[2, 2]);
        assert_eq!(data, &[1, 1, 1, 1]);
    }

    #[test]
    fn tensor_neg_flips_all_elements() {
        let t = Value::tensor(vec![3], vec![1, -2, 3]).unwrap();
        let n = t.neg().unwrap();
        assert_eq!(
            n,
            Value::tensor(vec![3], vec![-1, 2, -3]).unwrap()
        );
    }

    #[test]
    fn tensor_same_domain_only_matches_other_tensors() {
        let t1 = Value::tensor(vec![2], vec![1, 2]).unwrap();
        let t2 = Value::tensor(vec![5], vec![0, 0, 0, 0, 0]).unwrap();
        assert!(t1.same_domain(&t2));
        // Tensor and Int are different domains.
        assert!(!t1.same_domain(&Value::Int(5)));
        assert!(!t1.same_domain(&Value::Nat(5)));
    }

    #[test]
    fn tensor_display_shows_shape_and_data() {
        let t = Value::tensor(vec![2, 3], vec![1, 2, 3, 4, 5, 6]).unwrap();
        assert_eq!(format!("{t}"), "T<2x3>[1 2 3 4 5 6]");
    }

    #[test]
    fn tensor_is_distinct_from_int_with_same_content() {
        // A 1D tensor of length 1 containing 5 is NOT equal to
        // the scalar Int 5. Domain distinction is preserved.
        let t = Value::tensor(vec![1], vec![5]).unwrap();
        assert_ne!(t, Value::Int(5));
    }

    #[test]
    fn tensor_numel_counts_elements() {
        let t = Value::tensor(vec![3, 4], vec![0; 12]).unwrap();
        assert_eq!(t.tensor_numel(), Some(12));
        assert_eq!(Value::Int(5).tensor_numel(), None);
    }

    #[test]
    fn tensor_roundtrips_via_bincode() {
        let t = Value::tensor(vec![2, 2], vec![10, 20, 30, 40]).unwrap();
        let bytes = bincode::serialize(&t).unwrap();
        let back: Value = bincode::deserialize(&bytes).unwrap();
        assert_eq!(t, back);
    }
}
