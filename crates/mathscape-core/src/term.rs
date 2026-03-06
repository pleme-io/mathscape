use crate::hash::TermRef;
use crate::value::Value;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Symbol identifier — index into the library.
pub type SymbolId = u32;

/// The expression tree — the universal representation for mathematical
/// objects in Mathscape. This is the in-memory form used during evaluation
/// and mutation. Children are inline (not hash refs) for fast traversal.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
}
