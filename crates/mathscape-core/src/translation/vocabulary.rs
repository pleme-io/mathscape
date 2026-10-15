use crate::builtin::BUILTINS;
use crate::term::Term;
use crate::value::Value;
use std::sync::OnceLock;
use tatara_lisp::ast::{Atom, Sexp};

const SOURCE: &str = include_str!("vocabulary.tlisp");

#[derive(Clone, Debug, PartialEq)]
pub enum Render {
    Constant(String),
    Function(String),
    Prefix(String),
    Postfix(String),
    Infix(String, u8),
    Bracket(String, String, String),
    Frac,
}

#[derive(Clone, Debug)]
pub struct OperatorDef {
    pub id: u32,
    pub name: String,
    pub render: Render,
    pub carrier: String,
    pub math_name: String,
    pub axiom: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    Meta(String),
    Const(String),
    Lit(Term),
    App(Box<Pattern>, Vec<Pattern>),
}

#[derive(Clone, Debug)]
pub struct InvariantDef {
    pub id: String,
    pub property: String,
    pub canonical_name: String,
    pub math_domain: String,
    pub latex: String,
    pub semantic_meaning: String,
    pub forms: Vec<(Pattern, Pattern)>,
}

#[derive(Clone, Debug)]
pub enum StructureShape {
    Single(Vec<String>),
    Pair {
        additive: Vec<String>,
        multiplicative: Vec<String>,
        joint: Vec<String>,
    },
}

#[derive(Clone, Debug)]
pub struct StructureDef {
    pub id: String,
    pub name: String,
    pub shape: StructureShape,
}

#[derive(Clone, Debug)]
pub struct ProcedureDef {
    pub id: String,
    pub layer: String,
    pub canonical_name: String,
    pub math_domain: String,
    pub latex: String,
    pub implemented_by: String,
}

#[derive(Clone, Debug, Default)]
pub struct Vocabulary {
    pub operators: Vec<OperatorDef>,
    pub invariants: Vec<InvariantDef>,
    pub structures: Vec<StructureDef>,
    pub procedures: Vec<ProcedureDef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyError(pub String);

impl std::fmt::Display for VocabularyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "translation vocabulary: {}", self.0)
    }
}

impl std::error::Error for VocabularyError {}

pub fn vocabulary() -> &'static Vocabulary {
    static VOCABULARY: OnceLock<Vocabulary> = OnceLock::new();
    VOCABULARY.get_or_init(|| {
        Vocabulary::parse(SOURCE).unwrap_or_else(|e| panic!("{e}"))
    })
}

impl Vocabulary {
    pub fn parse(source: &str) -> Result<Self, VocabularyError> {
        let forms = tatara_lisp::reader::read(source)
            .map_err(|e| VocabularyError(format!("read: {e}")))?;
        let mut vocabulary = Vocabulary::default();
        for form in &forms {
            let items = form
                .as_list()
                .ok_or_else(|| VocabularyError(format!("top-level form is not a list: {form}")))?;
            let head = items
                .first()
                .and_then(Sexp::as_symbol)
                .ok_or_else(|| VocabularyError(format!("form has no head symbol: {form}")))?;
            let fields = Fields::of(&items[1..], form)?;
            match head {
                "defoperator" => vocabulary.operators.push(operator(&fields)?),
                "definvariant" => vocabulary.invariants.push(invariant(&fields)?),
                "defstructure" => vocabulary.structures.push(structure(&fields)?),
                "defprocedure" => vocabulary.procedures.push(procedure(&fields)?),
                other => return Err(VocabularyError(format!("unknown form {other}"))),
            }
        }
        Ok(vocabulary)
    }

    pub fn operator(&self, id: u32) -> Option<&OperatorDef> {
        self.operators.iter().find(|o| o.id == id)
    }

    pub fn invariant(&self, id: &str) -> Option<&InvariantDef> {
        self.invariants.iter().find(|i| i.id == id)
    }
}

struct Fields<'a> {
    form: &'a Sexp,
    pairs: Vec<(&'a str, &'a Sexp)>,
}

impl<'a> Fields<'a> {
    fn of(items: &'a [Sexp], form: &'a Sexp) -> Result<Self, VocabularyError> {
        if items.len() % 2 != 0 {
            return Err(VocabularyError(format!("odd keyword list in {form}")));
        }
        let mut pairs = Vec::with_capacity(items.len() / 2);
        for chunk in items.chunks(2) {
            let key = chunk[0]
                .as_keyword()
                .ok_or_else(|| VocabularyError(format!("expected keyword, got {} in {form}", chunk[0])))?;
            pairs.push((key, &chunk[1]));
        }
        Ok(Self { form, pairs })
    }

    fn get(&self, key: &str) -> Result<&'a Sexp, VocabularyError> {
        self.pairs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
            .ok_or_else(|| VocabularyError(format!("missing :{key} in {}", self.form)))
    }

    fn all(&self, key: &str) -> Vec<&'a Sexp> {
        self.pairs.iter().filter(|(k, _)| *k == key).map(|(_, v)| *v).collect()
    }

    fn string(&self, key: &str) -> Result<String, VocabularyError> {
        self.get(key)?
            .as_string()
            .map(str::to_owned)
            .ok_or_else(|| VocabularyError(format!(":{key} must be a string in {}", self.form)))
    }

    fn int(&self, key: &str) -> Result<i64, VocabularyError> {
        self.get(key)?
            .as_int()
            .ok_or_else(|| VocabularyError(format!(":{key} must be an integer in {}", self.form)))
    }

    fn symbols(&self, key: &str) -> Result<Vec<String>, VocabularyError> {
        let list = self
            .get(key)?
            .as_list()
            .ok_or_else(|| VocabularyError(format!(":{key} must be a list in {}", self.form)))?;
        list.iter()
            .map(|s| {
                s.as_symbol()
                    .map(str::to_owned)
                    .ok_or_else(|| VocabularyError(format!(":{key} holds a non-symbol {s}")))
            })
            .collect()
    }
}

fn operator(fields: &Fields<'_>) -> Result<OperatorDef, VocabularyError> {
    let name = fields.string("name")?;
    let builtin = BUILTINS
        .iter()
        .find(|b| b.name == name)
        .ok_or_else(|| VocabularyError(format!("defoperator {name} names no kernel builtin")))?;
    Ok(OperatorDef {
        id: builtin.id,
        render: render(fields.get("render")?, &name)?,
        carrier: fields.string("carrier")?,
        math_name: fields.string("math-name")?,
        axiom: fields.string("axiom")?,
        name,
    })
}

fn render(sexp: &Sexp, name: &str) -> Result<Render, VocabularyError> {
    let bad = || VocabularyError(format!("bad :render for {name}: {sexp}"));
    let items = sexp.as_list().ok_or_else(bad)?;
    let kind = items.first().and_then(Sexp::as_symbol).ok_or_else(bad)?;
    let text = |i: usize| -> Result<String, VocabularyError> {
        items.get(i).and_then(Sexp::as_string).map(str::to_owned).ok_or_else(bad)
    };
    Ok(match kind {
        "constant" => Render::Constant(text(1)?),
        "function" => Render::Function(text(1)?),
        "prefix" => Render::Prefix(text(1)?),
        "postfix" => Render::Postfix(text(1)?),
        "infix" => {
            let precedence = items.get(2).and_then(Sexp::as_int).ok_or_else(bad)?;
            Render::Infix(text(1)?, u8::try_from(precedence).map_err(|_| bad())?)
        }
        "bracket" => Render::Bracket(text(1)?, text(2)?, text(3)?),
        "frac" => Render::Frac,
        _ => return Err(bad()),
    })
}

fn invariant(fields: &Fields<'_>) -> Result<InvariantDef, VocabularyError> {
    let id = fields.string("id")?;
    let forms = fields
        .all("internal-repr")
        .into_iter()
        .map(|repr| rule_pattern(repr, &id))
        .collect::<Result<Vec<_>, _>>()?;
    if forms.is_empty() {
        return Err(VocabularyError(format!("definvariant {id} has no :internal-repr")));
    }
    Ok(InvariantDef {
        property: fields.string("property")?,
        canonical_name: fields.string("canonical-name")?,
        math_domain: fields.string("math-domain")?,
        latex: fields.string("latex")?,
        semantic_meaning: fields.string("semantic-meaning")?,
        forms,
        id,
    })
}

fn rule_pattern(sexp: &Sexp, id: &str) -> Result<(Pattern, Pattern), VocabularyError> {
    let bad = || VocabularyError(format!("{id}: :internal-repr must be (=> LHS RHS), got {sexp}"));
    let items = sexp.as_list().ok_or_else(bad)?;
    match items {
        [arrow, lhs, rhs] if arrow.as_symbol() == Some("=>") => {
            Ok((pattern(lhs, id)?, pattern(rhs, id)?))
        }
        _ => Err(bad()),
    }
}

fn pattern(sexp: &Sexp, id: &str) -> Result<Pattern, VocabularyError> {
    let bad = |what: &str| VocabularyError(format!("{id}: {what}: {sexp}"));
    match sexp {
        Sexp::Atom(Atom::Symbol(s)) => {
            if let Some(name) = s.strip_prefix('?') {
                return Ok(Pattern::Meta(name.to_owned()));
            }
            if let Some(n) = s.strip_prefix('i').and_then(|r| r.parse::<i64>().ok()) {
                return Ok(Pattern::Lit(Term::Number(Value::Int(n))));
            }
            BUILTINS
                .iter()
                .find(|b| b.name == s.as_str())
                .map(|b| Pattern::Lit(Term::Var(b.id)))
                .ok_or_else(|| bad("unknown symbol"))
        }
        Sexp::Atom(Atom::Int(n)) if *n >= 0 => {
            Ok(Pattern::Lit(Term::Number(Value::Nat(*n as u64))))
        }
        Sexp::Atom(Atom::Int(n)) => Ok(Pattern::Lit(Term::Number(Value::Int(*n)))),
        Sexp::Atom(Atom::Float(f)) => Value::from_f64(*f)
            .map(|v| Pattern::Lit(Term::Number(v)))
            .ok_or_else(|| bad("non-finite float")),
        Sexp::List(items) => match items.as_slice() {
            [head, meta] if head.as_symbol() == Some("const") => match meta.as_symbol() {
                Some(m) if m.starts_with('?') => Ok(Pattern::Const(m[1..].to_owned())),
                _ => Err(bad("(const ?x) needs a metavariable")),
            },
            [head, args @ ..] => Ok(Pattern::App(
                Box::new(pattern(head, id)?),
                args.iter().map(|a| pattern(a, id)).collect::<Result<_, _>>()?,
            )),
            [] => Err(bad("empty application")),
        },
        _ => Err(bad("unsupported pattern atom")),
    }
}

fn structure(fields: &Fields<'_>) -> Result<StructureDef, VocabularyError> {
    let id = fields.string("id")?;
    let shape = match fields.int("arity")? {
        1 => StructureShape::Single(fields.symbols("requires")?),
        2 => StructureShape::Pair {
            additive: fields.symbols("additive")?,
            multiplicative: fields.symbols("multiplicative")?,
            joint: fields.symbols("joint")?,
        },
        n => return Err(VocabularyError(format!("defstructure {id}: arity {n} unsupported"))),
    };
    Ok(StructureDef {
        name: fields.string("name")?,
        shape,
        id,
    })
}

fn procedure(fields: &Fields<'_>) -> Result<ProcedureDef, VocabularyError> {
    Ok(ProcedureDef {
        id: fields.string("id")?,
        layer: fields.string("layer")?,
        canonical_name: fields.string("canonical-name")?,
        math_domain: fields.string("math-domain")?,
        latex: fields.string("latex")?,
        implemented_by: fields.string("implemented-by")?,
    })
}
