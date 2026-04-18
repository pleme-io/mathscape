use crate::hash::TermRef;
use crate::value::Value;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Symbol identifier — index into the library.
pub type SymbolId = u32;

/// The expression tree — the universal representation for mathematical
/// objects in Mathscape. This is the in-memory form used during evaluation
/// and mutation. Children are inline (not hash refs) for fast traversal.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum Term {
    /// Irreducible identity — a distinguishable atom.
    Point(u64),
    /// Irreducible quantity — a numeric value.
    Number(Value),
    /// Irreducible transformation — params are variable IDs, body is the
    /// expression computed when applied.
    Fn(Vec<u32>, Box<Term>),
    /// Function application — func applied to args.
    Apply(Box<Term>, Vec<Term>),
    /// A variable reference (bound by an enclosing Fn).
    Var(u32),
    /// A discovered compression — a named rewrite pattern from the library.
    /// The args are the matched subexpressions.
    Symbol(SymbolId, Vec<Term>),
}

/// The stored form — children are hash references, not inline trees.
/// This is what lives in redb. Reconstitute a full Term by recursively
/// resolving TermRefs from the store.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StoredTerm {
    Point(u64),
    Number(Value),
    Fn(Vec<u32>, TermRef),
    Apply(TermRef, Vec<TermRef>),
    Var(u32),
    Symbol(SymbolId, Vec<TermRef>),
}

impl Term {
    /// Count nodes in the expression tree.
    pub fn size(&self) -> usize {
        match self {
            Term::Point(_) | Term::Number(_) | Term::Var(_) => 1,
            Term::Fn(_, body) => 1 + body.size(),
            Term::Apply(func, args) => {
                1 + func.size() + args.iter().map(Term::size).sum::<usize>()
            }
            Term::Symbol(_, args) => 1 + args.iter().map(Term::size).sum::<usize>(),
        }
    }

    /// Maximum depth of the expression tree.
    pub fn depth(&self) -> usize {
        match self {
            Term::Point(_) | Term::Number(_) | Term::Var(_) => 1,
            Term::Fn(_, body) => 1 + body.depth(),
            Term::Apply(func, args) => {
                let max_arg = args.iter().map(Term::depth).max().unwrap_or(0);
                1 + func.depth().max(max_arg)
            }
            Term::Symbol(_, args) => {
                let max_arg = args.iter().map(Term::depth).max().unwrap_or(0);
                1 + max_arg
            }
        }
    }

    /// Count distinct operator types used in the tree.
    pub fn distinct_ops(&self) -> usize {
        use std::collections::HashSet;
        let mut ops = HashSet::new();
        self.collect_ops(&mut ops);
        ops.len()
    }

    fn collect_ops(&self, ops: &mut std::collections::HashSet<String>) {
        match self {
            Term::Point(_) => {
                ops.insert("Point".into());
            }
            Term::Number(_) => {
                ops.insert("Number".into());
            }
            Term::Var(_) => {
                ops.insert("Var".into());
            }
            Term::Fn(_, body) => {
                ops.insert("Fn".into());
                body.collect_ops(ops);
            }
            Term::Apply(func, args) => {
                ops.insert("Apply".into());
                func.collect_ops(ops);
                for a in args {
                    a.collect_ops(ops);
                }
            }
            Term::Symbol(id, args) => {
                ops.insert(format!("Symbol({id})"));
                for a in args {
                    a.collect_ops(ops);
                }
            }
        }
    }

    /// Substitute variable `var` with `replacement` throughout the term.
    pub fn substitute(&self, var: u32, replacement: &Term) -> Term {
        match self {
            Term::Var(v) if *v == var => replacement.clone(),
            Term::Var(_) | Term::Point(_) | Term::Number(_) => self.clone(),
            Term::Fn(params, body) => {
                if params.contains(&var) {
                    // var is shadowed by this binding
                    self.clone()
                } else {
                    Term::Fn(params.clone(), Box::new(body.substitute(var, replacement)))
                }
            }
            Term::Apply(func, args) => Term::Apply(
                Box::new(func.substitute(var, replacement)),
                args.iter().map(|a| a.substitute(var, replacement)).collect(),
            ),
            Term::Symbol(id, args) => Term::Symbol(
                *id,
                args.iter().map(|a| a.substitute(var, replacement)).collect(),
            ),
        }
    }

    /// Compute the blake3 content hash of this term.
    /// Used for hash-consing: structurally identical terms get the same hash.
    pub fn content_hash(&self) -> TermRef {
        let bytes = bincode::serialize(self).expect("term serialization cannot fail");
        TermRef::from_bytes(&bytes)
    }

    /// Check if this term is a pattern variable (Var).
    pub fn is_var(&self) -> bool {
        matches!(self, Term::Var(_))
    }

    /// Check if this term is a leaf (no children).
    pub fn is_leaf(&self) -> bool {
        matches!(self, Term::Point(_) | Term::Number(_) | Term::Var(_))
    }

    /// Collect all free variables in the term.
    pub fn free_vars(&self) -> std::collections::HashSet<u32> {
        let mut vars = std::collections::HashSet::new();
        self.collect_free_vars(&mut std::collections::HashSet::new(), &mut vars);
        vars
    }

    fn collect_free_vars(
        &self,
        bound: &mut std::collections::HashSet<u32>,
        free: &mut std::collections::HashSet<u32>,
    ) {
        match self {
            Term::Var(v) => {
                if !bound.contains(v) {
                    free.insert(*v);
                }
            }
            Term::Point(_) | Term::Number(_) => {}
            Term::Fn(params, body) => {
                for p in params {
                    bound.insert(*p);
                }
                body.collect_free_vars(bound, free);
                for p in params {
                    bound.remove(p);
                }
            }
            Term::Apply(func, args) => {
                func.collect_free_vars(bound, free);
                for a in args {
                    a.collect_free_vars(bound, free);
                }
            }
            Term::Symbol(_, args) => {
                for a in args {
                    a.collect_free_vars(bound, free);
                }
            }
        }
    }
}

// ── R3: AC canonicalization ──────────────────────────────────────
//
// The "no equal terms" kernel invariant. Two terms that are
// semantically equal must be structurally equal. For commutative
// operators (add, mul), we enforce this by sorting argument lists
// at canonicalization time. For associative operators, we would
// additionally flatten nested same-operator trees — but
// association is deferred to R4 because it changes Apply arity
// (currently binary) which would require the evaluator to handle
// variadic add/mul.
//
// R3 for now: commutativity only. `add(3, 5)` and `add(5, 3)`
// canonicalize to the same term. The evaluator still sees binary
// add, so no downstream changes required.

// R5: canonicalization's notion of "commutative operator" comes
// from the central registry in `crate::builtin`, not from a
// duplicate list of magic constants. One source of truth.
use crate::builtin::is_commutative as is_commutative_op;

use crate::builtin::is_associative as is_associative_op;

impl Term {
    /// Produce a canonical form — structurally equal representations
    /// of semantically-equal terms become literally identical.
    ///
    /// R3 + R4 + R6 (2026-04-18): commutativity, associativity, AND
    /// constant folding. For every `Apply(head, args)`:
    ///   1. Recursively canonicalize args (bottom-up).
    ///   2. R6a — if head is a registered builtin AND every arg is
    ///      a resolved leaf (Number/Point, not Var/Apply/Fn),
    ///      invoke the builtin's eval rule. If it returns Some,
    ///      that IS the canonical form (single Number / single
    ///      term). Short-circuit.
    ///   3. R4 — if head is associative, flatten nested same-head
    ///      Applys into one operand list.
    ///   4. R3 — if head is commutative, sort operands by derived
    ///      Ord. AC-sort puts Numbers/Points ahead of Vars.
    ///   5. R6b — for AC binary builtins, fold the leading run of
    ///      Numbers (which cluster at the front after R3's sort)
    ///      into a single Number by repeatedly applying the
    ///      builtin's binary eval.
    ///   6. Collapse degenerate result: if R6b reduced to one
    ///      operand, return that operand directly (no Apply
    ///      wrapper, no arity-1 Add).
    ///   7. Rebuild in binary LEFT-ASSOCIATED form so the evaluator
    ///      (which expects arity=2 add/mul) still sees a valid
    ///      binary tree: `[a, b, c]` → `add(add(a, b), c)`.
    ///
    /// Result: every semantically-equal expression over the builtin
    /// registry maps to ONE canonical term. `Apply(add, [3, 5])`,
    /// `Apply(add, [5, 3])`, and `Number(8)` all canonicalize to
    /// `Number(8)`. `Apply(succ, [Apply(succ, [Apply(zero, [])])])`
    /// canonicalizes to `Number(2)`. The structural-equality →
    /// semantic-equality invariant holds for reducible subterms.
    #[must_use]
    pub fn canonical(&self) -> Term {
        match self {
            Term::Point(_) | Term::Number(_) | Term::Var(_) => self.clone(),
            Term::Fn(params, body) => {
                Term::Fn(params.clone(), Box::new(body.canonical()))
            }
            Term::Apply(head, args) => {
                let head_c = head.canonical();
                let args_c: Vec<Term> = args.iter().map(Term::canonical).collect();

                // R6a: whole-application fold. If head is a registered
                // builtin and every arg is fully resolved, compute
                // the result directly. Covers nullary (zero), unary
                // (succ), binary (add/mul) — registry-driven.
                if let Some(folded) = try_builtin_fold(&head_c, &args_c) {
                    return folded;
                }

                // R4: if associative, flatten nested same-head
                // applications into a single operand list.
                let flat_args: Vec<Term> = if is_associative_op(&head_c) {
                    flatten_associative(&head_c, args_c)
                } else {
                    args_c
                };

                // R3: if commutative, sort the flat operand list.
                // Because Number < Apply < Var in derived Ord,
                // Numbers cluster at the front.
                let sorted_args = if is_commutative_op(&head_c) {
                    let mut v = flat_args;
                    v.sort();
                    v
                } else {
                    flat_args
                };

                // R6b: AC binary-builtin constant folding. Fold the
                // leading Number-run (guaranteed contiguous after R3)
                // into a single Number using the builtin's eval.
                let folded_args = fold_ac_constants(&head_c, sorted_args);

                // R6 collapse: if folding crushed everything to one
                // operand, that IS the canonical form. No more Apply.
                if folded_args.len() == 1 {
                    return folded_args.into_iter().next().unwrap();
                }

                // R4 rebuild: binary left-associated form preserves
                // arity=2 so the existing evaluator works unchanged.
                // [a, b, c, d] → add(add(add(a, b), c), d)
                if is_associative_op(&head_c) && folded_args.len() > 2 {
                    rebuild_left_associated(head_c, folded_args)
                } else {
                    Term::Apply(Box::new(head_c), folded_args)
                }
            }
            Term::Symbol(id, args) => {
                Term::Symbol(*id, args.iter().map(Term::canonical).collect())
            }
        }
    }

    /// Smart constructor for an Apply that's already canonical.
    /// Prefer this over `Term::Apply(...)` when constructing terms
    /// intended for storage/comparison/validation.
    #[must_use]
    pub fn apply_canonical(head: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(head), args).canonical()
    }
}

/// R6a: whole-Apply constant fold. If the head is a registered
/// builtin and every arg is already a resolved leaf
/// (`Term::Number` or `Term::Point`), invoke the builtin's `eval`
/// rule. `None` if any precondition fails — the caller falls
/// through to the AC path.
///
/// Registry-driven: adding a new builtin instantly gives its eval
/// rule constant-folding power. The kernel never needs to know
/// about specific operators.
fn try_builtin_fold(head: &Term, args: &[Term]) -> Option<Term> {
    let Term::Var(id) = head else { return None; };
    let builtin = crate::builtin::lookup(*id)?;
    // Only fold when every arg is a resolved leaf. Vars, Applys,
    // Fns mean "not yet reduced" — the builtin eval would return
    // None anyway, but we check up front for clarity.
    if !args
        .iter()
        .all(|a| matches!(a, Term::Number(_) | Term::Point(_)))
    {
        return None;
    }
    (builtin.eval)(args)
}

/// R6b: fold the leading Number-run of an AC-binary-builtin
/// application into a single Number. Because R3's sort puts
/// Numbers at the front, the leading run is the contiguous
/// Number prefix.
///
/// `[Number(1), Number(2), Number(3)]` → `[Number(6)]`
/// `[Number(1), Number(2), Var(x)]`   → `[Number(3), Var(x)]`
/// `[Number(5), Var(x)]`              → `[Number(5), Var(x)]` (prefix < 2)
/// `[Var(x), Var(y)]`                 → `[Var(x), Var(y)]`
///
/// No-op for non-binary, non-associative, or unknown heads.
fn fold_ac_constants(head: &Term, args: Vec<Term>) -> Vec<Term> {
    if args.len() < 2 {
        return args;
    }
    let Term::Var(id) = head else { return args; };
    let Some(builtin) = crate::builtin::lookup(*id) else {
        return args;
    };
    if !builtin.associative || builtin.arity != 2 {
        return args;
    }

    // Count the leading Number prefix.
    let n_leading = args
        .iter()
        .take_while(|a| matches!(a, Term::Number(_)))
        .count();
    if n_leading < 2 {
        return args;
    }

    // Fold the first n_leading args into one by iterated binary eval.
    let mut iter = args.into_iter();
    let mut acc = iter.next().expect("n_leading >= 2");
    for _ in 1..n_leading {
        let next = iter.next().expect("n_leading counted");
        match (builtin.eval)(&[acc.clone(), next.clone()]) {
            Some(folded) => acc = folded,
            None => {
                // Evaluator rejected — unexpected for same-shape
                // Numbers, but stay safe and keep them as-is.
                let mut v = vec![acc, next];
                v.extend(iter);
                return v;
            }
        }
    }
    let mut result = vec![acc];
    result.extend(iter);
    result
}

/// Flatten nested same-head Apply trees into a single operand list.
/// Only called when `head` is associative. Each arg that's itself
/// `Apply(head, inner_args)` with the same head contributes its
/// inner args; other args pass through.
fn flatten_associative(head: &Term, args: Vec<Term>) -> Vec<Term> {
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        match &a {
            Term::Apply(inner_head, inner_args)
                if inner_head.as_ref() == head =>
            {
                // Recursive flatten in case nesting is deeper.
                let flat = flatten_associative(head, inner_args.clone());
                out.extend(flat);
            }
            _ => out.push(a),
        }
    }
    out
}

/// Rebuild a variadic operand list as a binary left-associated
/// tree. Preserves the evaluator's arity=2 expectation.
///   [a]          → a (shouldn't happen in practice)
///   [a, b]       → Apply(head, [a, b])
///   [a, b, c]    → Apply(head, [Apply(head, [a, b]), c])
///   [a, b, c, d] → Apply(head, [Apply(head, [Apply(head, [a, b]), c]), d])
fn rebuild_left_associated(head: Term, args: Vec<Term>) -> Term {
    if args.is_empty() {
        return head;
    }
    if args.len() == 1 {
        return args.into_iter().next().unwrap();
    }
    let mut iter = args.into_iter();
    let first = iter.next().unwrap();
    let second = iter.next().unwrap();
    let mut acc = Term::Apply(Box::new(head.clone()), vec![first, second]);
    for rest in iter {
        acc = Term::Apply(Box::new(head.clone()), vec![acc, rest]);
    }
    acc
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Term::Point(id) => write!(f, "p{id}"),
            Term::Number(v) => write!(f, "{v}"),
            Term::Var(v) => write!(f, "?{v}"),
            Term::Fn(params, body) => {
                let ps: Vec<String> = params.iter().map(|p| format!("?{p}")).collect();
                write!(f, "(fn ({}) {body})", ps.join(" "))
            }
            Term::Apply(func, args) => {
                let arg_strs: Vec<String> = args.iter().map(|a| format!("{a}")).collect();
                write!(f, "({func} {})", arg_strs.join(" "))
            }
            Term::Symbol(id, args) => {
                if args.is_empty() {
                    write!(f, "S{id}")
                } else {
                    let arg_strs: Vec<String> = args.iter().map(|a| format!("{a}")).collect();
                    write!(f, "(S{id} {})", arg_strs.join(" "))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nat(n: u64) -> Term {
        Term::Number(Value::Nat(n))
    }
    fn app(head: Term, args: Vec<Term>) -> Term {
        Term::Apply(Box::new(head), args)
    }

    #[test]
    fn canonical_commutative_add_sorts_args() {
        // R3: add(3, 5) and add(5, 3) canonicalize to the same term.
        let t1 = app(Term::Var(2), vec![nat(3), nat(5)]);
        let t2 = app(Term::Var(2), vec![nat(5), nat(3)]);
        assert_ne!(t1, t2, "pre-canonical: structurally distinct");
        assert_eq!(
            t1.canonical(),
            t2.canonical(),
            "canonical form of commutative args: identical"
        );
    }

    #[test]
    fn canonical_commutative_mul_sorts_args() {
        let t1 = app(Term::Var(3), vec![nat(7), nat(2)]);
        let t2 = app(Term::Var(3), vec![nat(2), nat(7)]);
        assert_eq!(t1.canonical(), t2.canonical());
    }

    #[test]
    fn canonical_noncommutative_preserves_arg_order() {
        // succ is unary — no sort applies. For hypothetical
        // non-AC binary ops (not in the builtin set), arg order
        // must NOT change.
        let t1 = app(Term::Var(99), vec![nat(3), nat(5)]);
        let t2 = app(Term::Var(99), vec![nat(5), nat(3)]);
        assert_ne!(
            t1.canonical(),
            t2.canonical(),
            "non-AC operator preserves arg order"
        );
    }

    #[test]
    fn canonical_is_recursive() {
        // add(mul(?a, ?b), mul(?b, ?a)) — outer is AC (add), inner
        // muls are AC. Inner muls sort their args identically;
        // outer sees two identical subterms. Vars are used (not
        // Numbers) so R6 constant-folding doesn't collapse the
        // whole expression — we're testing the recursion, not the
        // folding.
        let left = app(Term::Var(3), vec![Term::Var(100), Term::Var(101)]);
        let right = app(Term::Var(3), vec![Term::Var(101), Term::Var(100)]);
        let outer = app(Term::Var(2), vec![left, right]);
        let canon = outer.canonical();
        // Both inner muls → mul(?100, ?101). Outer args both the same.
        if let Term::Apply(_, args) = &canon {
            assert_eq!(args.len(), 2);
            assert_eq!(args[0], args[1], "identical subterms after canonicalization");
        } else {
            panic!("expected Apply, got {canon:?}");
        }
    }

    #[test]
    fn canonical_is_idempotent() {
        // canonical(canonical(t)) == canonical(t) — key algebraic
        // property. Without this, recanonicalization could drift.
        let t = app(
            Term::Var(2),
            vec![
                app(Term::Var(3), vec![nat(5), nat(2)]),
                nat(7),
                app(Term::Var(3), vec![nat(2), nat(5)]),
            ],
        );
        let once = t.canonical();
        let twice = once.canonical();
        assert_eq!(once, twice, "canonical must be idempotent");
    }

    #[test]
    fn canonical_preserves_semantics() {
        // add(3, 5) and add(5, 3) both evaluate to 8. Canonical
        // form of either — wherever the evaluator sees it — must
        // still evaluate to 8. Guards the invariant: sorting args
        // for AC operators doesn't change what they compute.
        use crate::eval::eval;
        let t1 = app(Term::Var(2), vec![nat(3), nat(5)]).canonical();
        let t2 = app(Term::Var(2), vec![nat(5), nat(3)]).canonical();
        let v1 = eval(&t1, &[], 100).unwrap();
        let v2 = eval(&t2, &[], 100).unwrap();
        assert_eq!(v1, nat(8));
        assert_eq!(v2, nat(8));
    }

    #[test]
    fn apply_canonical_smart_constructor() {
        // The smart-constructor form builds already-canonical terms.
        let built = Term::apply_canonical(Term::Var(2), vec![nat(7), nat(2)]);
        let raw = app(Term::Var(2), vec![nat(7), nat(2)]);
        assert_eq!(built, raw.canonical());
    }

    #[test]
    fn term_size() {
        // (add 1 2) = Apply(Var("add"), [Number(1), Number(2)])
        let t = Term::Apply(
            Box::new(Term::Var(0)),
            vec![Term::Number(Value::Nat(1)), Term::Number(Value::Nat(2))],
        );
        assert_eq!(t.size(), 4); // Apply + Var + Number + Number
    }

    #[test]
    fn term_depth() {
        let t = Term::Apply(
            Box::new(Term::Var(0)),
            vec![Term::Number(Value::Nat(1))],
        );
        assert_eq!(t.depth(), 2);
    }

    #[test]
    fn substitute_replaces_var() {
        let t = Term::Apply(
            Box::new(Term::Var(0)),
            vec![Term::Var(1), Term::Var(2)],
        );
        let replaced = t.substitute(1, &Term::Number(Value::Nat(42)));
        assert_eq!(
            replaced,
            Term::Apply(
                Box::new(Term::Var(0)),
                vec![Term::Number(Value::Nat(42)), Term::Var(2)],
            )
        );
    }

    #[test]
    fn substitute_respects_shadowing() {
        let t = Term::Fn(vec![1], Box::new(Term::Var(1)));
        let replaced = t.substitute(1, &Term::Number(Value::Nat(99)));
        // Var(1) is bound by the Fn, so it should NOT be replaced
        assert_eq!(t, replaced);
    }

    #[test]
    fn content_hash_deterministic() {
        let t1 = Term::Number(Value::Nat(42));
        let t2 = Term::Number(Value::Nat(42));
        assert_eq!(t1.content_hash(), t2.content_hash());
    }

    #[test]
    fn content_hash_different() {
        let t1 = Term::Number(Value::Nat(1));
        let t2 = Term::Number(Value::Nat(2));
        assert_ne!(t1.content_hash(), t2.content_hash());
    }

    #[test]
    fn free_vars_collected() {
        // (fn (?0) (apply ?0 ?1)) — ?0 is bound, ?1 is free
        let t = Term::Fn(
            vec![0],
            Box::new(Term::Apply(
                Box::new(Term::Var(0)),
                vec![Term::Var(1)],
            )),
        );
        let fv = t.free_vars();
        assert!(!fv.contains(&0));
        assert!(fv.contains(&1));
    }

    #[test]
    fn display_sexpr() {
        let t = Term::Apply(
            Box::new(Term::Symbol(1, vec![])),
            vec![Term::Number(Value::Nat(3)), Term::Number(Value::Nat(4))],
        );
        assert_eq!(format!("{t}"), "(S1 3 4)");
    }

    // ── R4: associativity canonicalization gold tests ──────────────

    #[test]
    fn canonical_associative_flattens_left_vs_right_nesting() {
        // The core R4 claim. add is associative, so these two
        // structurally-distinct trees denote the same arithmetic value.
        // After canonicalization they must be LITERALLY identical.
        let left_nested = app(
            Term::Var(2),
            vec![app(Term::Var(2), vec![nat(1), nat(2)]), nat(3)],
        );
        let right_nested = app(
            Term::Var(2),
            vec![nat(1), app(Term::Var(2), vec![nat(2), nat(3)])],
        );
        assert_ne!(
            left_nested, right_nested,
            "pre-canonical: distinct groupings"
        );
        assert_eq!(
            left_nested.canonical(),
            right_nested.canonical(),
            "canonical form of associative nesting: identical"
        );
    }

    #[test]
    fn canonical_ac_unifies_all_permutations_and_groupings() {
        // Six trees that all denote 1 + 2 + 3. With commutativity
        // AND associativity, canonical form must collapse all of
        // them to ONE structural representative.
        let trees = vec![
            app(
                Term::Var(2),
                vec![app(Term::Var(2), vec![nat(1), nat(2)]), nat(3)],
            ),
            app(
                Term::Var(2),
                vec![app(Term::Var(2), vec![nat(2), nat(1)]), nat(3)],
            ),
            app(
                Term::Var(2),
                vec![nat(1), app(Term::Var(2), vec![nat(2), nat(3)])],
            ),
            app(
                Term::Var(2),
                vec![nat(3), app(Term::Var(2), vec![nat(1), nat(2)])],
            ),
            app(
                Term::Var(2),
                vec![app(Term::Var(2), vec![nat(3), nat(2)]), nat(1)],
            ),
            app(
                Term::Var(2),
                vec![nat(2), app(Term::Var(2), vec![nat(3), nat(1)])],
            ),
        ];
        let canons: Vec<Term> = trees.iter().map(Term::canonical).collect();
        let first = &canons[0];
        for (i, c) in canons.iter().enumerate() {
            assert_eq!(
                c, first,
                "permutation {i} must canonicalize to the same term"
            );
        }
    }

    #[test]
    fn canonical_rebuilt_shape_is_binary_left_associated() {
        // The evaluator expects arity=2 add/mul. R4 reshapes any
        // canonical AC expression into binary left-associated form:
        //   [a, b, c] → add(add(a, b), c)
        //   [a, b, c, d] → add(add(add(a, b), c), d)
        // Uses Vars (not Numbers) to avoid R6 constant folding —
        // this test checks the rebuild SHAPE, not the folding.
        // Vars sort by id: ?10 < ?11 < ?12.
        let a = Term::Var(10);
        let b = Term::Var(11);
        let c = Term::Var(12);
        let t = app(
            Term::Var(2),
            vec![app(Term::Var(2), vec![a.clone(), b.clone()]), c.clone()],
        );
        let canon = t.canonical();
        match canon {
            Term::Apply(ref outer_head, ref outer_args) => {
                assert_eq!(**outer_head, Term::Var(2));
                assert_eq!(outer_args.len(), 2, "binary at top");
                // second arg is the largest Var (?12)
                assert_eq!(outer_args[1], c);
                // first arg is the nested add(?10, ?11)
                match &outer_args[0] {
                    Term::Apply(inner_head, inner_args) => {
                        assert_eq!(**inner_head, Term::Var(2));
                        assert_eq!(inner_args.len(), 2);
                        assert_eq!(inner_args[0], a);
                        assert_eq!(inner_args[1], b);
                    }
                    other => panic!("expected inner Apply, got {other:?}"),
                }
            }
            other => panic!("expected outer Apply, got {other:?}"),
        }
    }

    #[test]
    fn canonical_idempotent_with_deep_ac_nesting() {
        // Idempotence is the algebraic bedrock. canonical(canonical(t))
        // must equal canonical(t) even when associativity has
        // reshuffled the tree.
        let t = app(
            Term::Var(2),
            vec![
                app(
                    Term::Var(2),
                    vec![
                        nat(5),
                        app(Term::Var(3), vec![nat(7), nat(2)]),
                    ],
                ),
                app(Term::Var(2), vec![nat(1), nat(3)]),
            ],
        );
        let once = t.canonical();
        let twice = once.canonical();
        assert_eq!(once, twice);
    }

    #[test]
    fn canonical_preserves_semantics_across_associativity() {
        // R4 reshapes the tree but must NOT change what it computes.
        // Three groupings of 1 + 2 + 3, all must evaluate to 6.
        use crate::eval::eval;
        let groupings = [
            app(
                Term::Var(2),
                vec![app(Term::Var(2), vec![nat(1), nat(2)]), nat(3)],
            ),
            app(
                Term::Var(2),
                vec![nat(1), app(Term::Var(2), vec![nat(2), nat(3)])],
            ),
            app(
                Term::Var(2),
                vec![app(Term::Var(2), vec![nat(3), nat(1)]), nat(2)],
            ),
        ];
        for g in &groupings {
            let canon = g.canonical();
            let v = eval(&canon, &[], 200).unwrap();
            assert_eq!(v, nat(6), "canonical form must still evaluate to 6");
        }
    }

    #[test]
    fn canonical_non_associative_does_not_flatten() {
        // A made-up non-AC binary op (Var(77)): nested calls are
        // NOT flattened, arg order is NOT sorted. The kernel must
        // only transform what its registry says is safe to transform.
        let inner = app(Term::Var(77), vec![nat(1), nat(2)]);
        let outer = app(Term::Var(77), vec![inner.clone(), nat(3)]);
        let canon = outer.canonical();
        // Expect the exact same binary shape we fed in.
        match canon {
            Term::Apply(ref head, ref args) => {
                assert_eq!(**head, Term::Var(77));
                assert_eq!(args.len(), 2, "not flattened");
                assert_eq!(args[0], inner, "nested arg preserved verbatim");
                assert_eq!(args[1], nat(3), "trailing arg preserved verbatim");
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn canonical_deeply_nested_ac_collapses_to_one_tree() {
        // An adversarial shape case: add(add(add(?d,?a),add(?c,?b)),?e)
        // — 5 operands arriving through 4 levels of nesting. After
        // canonicalization the operands must be sorted by Var id
        // and rebuilt as a binary left-associated spine. Uses Vars
        // so R6 constant-folding doesn't collapse to one Number;
        // this test checks the REBUILD SHAPE, not the folding.
        let a = Term::Var(20);
        let b = Term::Var(21);
        let c = Term::Var(22);
        let d = Term::Var(23);
        let e = Term::Var(24);
        let t = app(
            Term::Var(2),
            vec![
                app(
                    Term::Var(2),
                    vec![
                        app(Term::Var(2), vec![d.clone(), a.clone()]),
                        app(Term::Var(2), vec![c.clone(), b.clone()]),
                    ],
                ),
                e.clone(),
            ],
        );
        let canon = t.canonical();
        // Expected: add(add(add(add(?a, ?b), ?c), ?d), ?e)
        let expected = app(
            Term::Var(2),
            vec![
                app(
                    Term::Var(2),
                    vec![
                        app(
                            Term::Var(2),
                            vec![
                                app(Term::Var(2), vec![a, b]),
                                c,
                            ],
                        ),
                        d,
                    ],
                ),
                e,
            ],
        );
        assert_eq!(canon, expected);
    }

    // ── R6: constant-folding gold tests ──────────────────────────

    #[test]
    fn canonical_folds_binary_add_to_number() {
        // The core R6 claim: Apply(add, [3, 5]) and Number(8) are
        // semantically equal, therefore canonically equal.
        let apply_form = app(Term::Var(2), vec![nat(3), nat(5)]);
        let number_form = nat(8);
        assert_ne!(apply_form, number_form, "structurally distinct before canonical");
        assert_eq!(apply_form.canonical(), number_form);
    }

    #[test]
    fn canonical_folds_binary_mul_to_number() {
        let apply_form = app(Term::Var(3), vec![nat(7), nat(9)]);
        assert_eq!(apply_form.canonical(), nat(63));
    }

    #[test]
    fn canonical_folds_nullary_zero_to_number() {
        // Apply(zero, []) IS Number(0) after canonical.
        let zero_apply = app(Term::Var(0), vec![]);
        assert_eq!(zero_apply.canonical(), nat(0));
    }

    #[test]
    fn canonical_folds_unary_succ_to_number() {
        // succ is non-AC, unary. R6a (whole-fold) handles it.
        let succ_apply = app(Term::Var(1), vec![nat(41)]);
        assert_eq!(succ_apply.canonical(), nat(42));
    }

    #[test]
    fn canonical_folds_nested_succ_chain() {
        // succ(succ(succ(zero))) → 3. Bottom-up folding means every
        // layer reduces to a Number, so the outer sees Number input
        // and can fold too.
        let zero_apply = app(Term::Var(0), vec![]);
        let s1 = app(Term::Var(1), vec![zero_apply]);
        let s2 = app(Term::Var(1), vec![s1]);
        let s3 = app(Term::Var(1), vec![s2]);
        assert_eq!(s3.canonical(), nat(3));
    }

    #[test]
    fn canonical_folds_leading_numbers_in_ac_multi_add() {
        // add(1, 2, ?x) after sort is [1, 2, ?x]. R6b folds the
        // leading Number-run [1, 2] → 3, leaving [3, ?x]. Result:
        // add(3, ?x).
        let t = app(
            Term::Var(2),
            vec![
                nat(1),
                nat(2),
                Term::Var(99),
            ],
        );
        let canon = t.canonical();
        let expected = app(Term::Var(2), vec![nat(3), Term::Var(99)]);
        assert_eq!(canon, expected);
    }

    #[test]
    fn canonical_leaves_var_plus_constant_intact() {
        // add(?x, 0) after sort is [0, ?x]. Only ONE Number in the
        // leading run → R6b does nothing. Result: add(0, ?x).
        let t = app(Term::Var(2), vec![Term::Var(7), nat(0)]);
        let canon = t.canonical();
        let expected = app(Term::Var(2), vec![nat(0), Term::Var(7)]);
        assert_eq!(canon, expected);
    }

    #[test]
    fn canonical_folds_mixed_add_mul_bottom_up() {
        // add(mul(3, 5), mul(2, 4)) — inner muls fold to 15 and 8,
        // outer add folds 15+8 = 23.
        let t = app(
            Term::Var(2),
            vec![
                app(Term::Var(3), vec![nat(3), nat(5)]),
                app(Term::Var(3), vec![nat(2), nat(4)]),
            ],
        );
        assert_eq!(t.canonical(), nat(23));
    }

    #[test]
    fn canonical_fold_is_idempotent_on_numbers() {
        // canonical(Number(n)) = Number(n). No-op on already-folded.
        let n = nat(42);
        assert_eq!(n.canonical(), n);
        assert_eq!(n.canonical().canonical(), n);
    }

    #[test]
    fn canonical_ac_folding_unifies_all_number_groupings() {
        // Every all-Number grouping of 1+2+3+4 canonicalizes to
        // Number(10). Regardless of nesting, permutation, or depth.
        let groupings = [
            app(Term::Var(2), vec![
                app(Term::Var(2), vec![
                    app(Term::Var(2), vec![nat(1), nat(2)]),
                    nat(3),
                ]),
                nat(4),
            ]),
            app(Term::Var(2), vec![
                nat(4),
                app(Term::Var(2), vec![
                    nat(3),
                    app(Term::Var(2), vec![nat(2), nat(1)]),
                ]),
            ]),
            app(Term::Var(2), vec![
                app(Term::Var(2), vec![nat(2), nat(3)]),
                app(Term::Var(2), vec![nat(1), nat(4)]),
            ]),
        ];
        for g in &groupings {
            assert_eq!(g.canonical(), nat(10), "grouping {g:?} must fold to 10");
        }
    }

    #[test]
    fn canonical_does_not_fold_unknown_head() {
        // Var(999) is not in the builtin registry. Canonical must
        // NOT fold; returns the Apply structurally sorted-if-known
        // (here not sorted since unknown → not commutative, not
        // associative).
        let t = app(Term::Var(999), vec![nat(3), nat(5)]);
        let canon = t.canonical();
        match canon {
            Term::Apply(h, args) => {
                assert_eq!(*h, Term::Var(999));
                assert_eq!(args, vec![nat(3), nat(5)]);
            }
            other => panic!("unknown head must not fold, got {other:?}"),
        }
    }

    #[test]
    fn canonical_does_not_fold_symbol() {
        // Symbols are library discoveries; the kernel doesn't try
        // to evaluate them here — that's the library's job. Args
        // are recursively canonicalized but the Symbol itself
        // stays.
        let t = Term::Symbol(
            42,
            vec![app(Term::Var(2), vec![nat(3), nat(5)])],
        );
        let canon = t.canonical();
        match canon {
            Term::Symbol(id, args) => {
                assert_eq!(id, 42);
                // inner Apply(add, [3, 5]) canonicalized under R6
                // folds to Number(8); Symbol itself preserved.
                assert_eq!(args, vec![nat(8)]);
            }
            other => panic!("expected Symbol, got {other:?}"),
        }
    }
}
