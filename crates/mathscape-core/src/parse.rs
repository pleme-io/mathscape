//! S-expression parser and printer for Terms.
//!
//! Syntax:
//!   (add 1 2)              — Apply(Var("add"), [Number(1), Number(2)])
//!   (fn (?x ?y) (add ?x ?y)) — Fn([x, y], Apply(Var("add"), [Var(x), Var(y)]))
//!   ?x                     — Var(x)
//!   42                     — Number(42)
//!   p3                     — Point(3)
//!   S5                     — Symbol(5, [])
//!   (S5 ?x ?y)             — Symbol(5, [Var(x), Var(y)])

use crate::term::Term;
use crate::value::Value;

#[derive(Debug)]
pub enum ParseError {
    UnexpectedEof,
    UnexpectedChar(char),
    InvalidNumber(String),
    InvalidSyntax(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::UnexpectedEof => write!(f, "unexpected end of input"),
            ParseError::UnexpectedChar(c) => write!(f, "unexpected character: {c}"),
            ParseError::InvalidNumber(s) => write!(f, "invalid number: {s}"),
            ParseError::InvalidSyntax(s) => write!(f, "invalid syntax: {s}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Well-known builtin names mapped to their variable IDs.
///
/// R7 (2026-04-18): Int-domain names added. Resolves to the
/// builtin registry's ids — keep in sync with `crate::builtin`.
fn builtin_var(name: &str) -> Option<u32> {
    match name {
        // Nat domain
        "zero" => Some(0),
        "succ" => Some(1),
        "add" => Some(2),
        "mul" => Some(3),
        // Int domain (R7)
        "int_zero" => Some(10),
        "int_succ" => Some(11),
        "int_add" => Some(12),
        "int_mul" => Some(13),
        "neg" => Some(14),
        _ => None,
    }
}

/// Parse an s-expression string into a Term.
pub fn parse(input: &str) -> Result<Term, ParseError> {
    let tokens = tokenize(input);
    let mut pos = 0;
    let result = parse_expr(&tokens, &mut pos)?;
    Ok(result)
}

#[derive(Debug, Clone)]
enum Token {
    LParen,
    RParen,
    Atom(String),
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                chars.next();
            }
            '(' => {
                tokens.push(Token::LParen);
                chars.next();
            }
            ')' => {
                tokens.push(Token::RParen);
                chars.next();
            }
            _ => {
                let mut atom = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '(' || c == ')' || c.is_whitespace() {
                        break;
                    }
                    atom.push(c);
                    chars.next();
                }
                tokens.push(Token::Atom(atom));
            }
        }
    }

    tokens
}

fn parse_expr(tokens: &[Token], pos: &mut usize) -> Result<Term, ParseError> {
    if *pos >= tokens.len() {
        return Err(ParseError::UnexpectedEof);
    }

    match &tokens[*pos] {
        Token::LParen => {
            *pos += 1; // skip (
            if *pos >= tokens.len() {
                return Err(ParseError::UnexpectedEof);
            }

            // Check for `fn` special form
            if let Token::Atom(head) = &tokens[*pos] {
                if head == "fn" {
                    return parse_fn(tokens, pos);
                }
            }

            // Parse as application: (func arg1 arg2 ...)
            let func = parse_expr(tokens, pos)?;
            let mut args = Vec::new();
            while *pos < tokens.len() && !matches!(tokens[*pos], Token::RParen) {
                args.push(parse_expr(tokens, pos)?);
            }

            if *pos >= tokens.len() {
                return Err(ParseError::UnexpectedEof);
            }
            *pos += 1; // skip )

            // If func is a Symbol, put args on the Symbol itself
            if let Term::Symbol(id, _) = &func {
                if !args.is_empty() {
                    return Ok(Term::Symbol(*id, args));
                }
            }

            Ok(Term::Apply(Box::new(func), args))
        }

        Token::Atom(s) => {
            *pos += 1;
            parse_atom(s)
        }

        Token::RParen => Err(ParseError::UnexpectedChar(')')),
    }
}

fn parse_fn(tokens: &[Token], pos: &mut usize) -> Result<Term, ParseError> {
    *pos += 1; // skip "fn"

    // Expect (params...)
    if *pos >= tokens.len() || !matches!(tokens[*pos], Token::LParen) {
        return Err(ParseError::InvalidSyntax("fn expects (params)".into()));
    }
    *pos += 1; // skip (

    let mut params = Vec::new();
    while *pos < tokens.len() && !matches!(tokens[*pos], Token::RParen) {
        if let Token::Atom(s) = &tokens[*pos] {
            if let Some(id) = parse_var_id(s) {
                params.push(id);
            } else {
                return Err(ParseError::InvalidSyntax(format!("expected ?var, got {s}")));
            }
        } else {
            return Err(ParseError::InvalidSyntax("expected atom in params".into()));
        }
        *pos += 1;
    }
    *pos += 1; // skip )

    // Parse body
    let body = parse_expr(tokens, pos)?;

    // Expect closing )
    if *pos >= tokens.len() || !matches!(tokens[*pos], Token::RParen) {
        return Err(ParseError::InvalidSyntax("fn not closed".into()));
    }
    *pos += 1; // skip )

    Ok(Term::Fn(params, Box::new(body)))
}

fn parse_atom(s: &str) -> Result<Term, ParseError> {
    // Variable: ?N or ?name
    if let Some(id) = parse_var_id(s) {
        return Ok(Term::Var(id));
    }

    // Point: pN
    if let Some(rest) = s.strip_prefix('p') {
        if let Ok(id) = rest.parse::<u64>() {
            return Ok(Term::Point(id));
        }
    }

    // Symbol: SN
    if let Some(rest) = s.strip_prefix('S') {
        if let Ok(id) = rest.parse::<u32>() {
            return Ok(Term::Symbol(id, vec![]));
        }
    }

    // Named builtins
    if let Some(var_id) = builtin_var(s) {
        return Ok(Term::Var(var_id));
    }

    // Number — Nat by default (positive u64 literal).
    if let Ok(n) = s.parse::<u64>() {
        return Ok(Term::Number(Value::Nat(n)));
    }

    // R7: Int literal. Two forms:
    //   -N   → Int(-N)  — leading minus implies Int (Nat has no
    //                    negatives, so this is unambiguous)
    //   iN   → Int(N)   — explicit positive Int (mirror of the
    //                    printer's "Ni" suffix; "i3" instead of
    //                    "3i" to keep the atom self-delimiting
    //                    at the start)
    if s.starts_with('-') || s.starts_with('i') {
        let (sign, rest) = if let Some(rest) = s.strip_prefix('i') {
            (1i64, rest)
        } else {
            // leading '-': parse the rest as positive i64, negate.
            let rest = s.strip_prefix('-').unwrap();
            (-1i64, rest)
        };
        if let Ok(n) = rest.parse::<i64>() {
            // Guard overflow: rest is positive i64, sign is ±1.
            if let Some(v) = sign.checked_mul(n) {
                return Ok(Term::Number(Value::Int(v)));
            }
        }
    }

    Err(ParseError::InvalidSyntax(format!("unknown atom: {s}")))
}

fn parse_var_id(s: &str) -> Option<u32> {
    let rest = s.strip_prefix('?')?;
    rest.parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_number() {
        let t = parse("42").unwrap();
        assert_eq!(t, Term::Number(Value::Nat(42)));
    }

    #[test]
    fn parse_point() {
        let t = parse("p7").unwrap();
        assert_eq!(t, Term::Point(7));
    }

    #[test]
    fn parse_var() {
        let t = parse("?3").unwrap();
        assert_eq!(t, Term::Var(3));
    }

    #[test]
    fn parse_apply() {
        let t = parse("(add 1 2)").unwrap();
        assert_eq!(
            t,
            Term::Apply(
                Box::new(Term::Var(2)), // "add" builtin
                vec![Term::Number(Value::Nat(1)), Term::Number(Value::Nat(2))],
            )
        );
    }

    #[test]
    fn parse_nested() {
        let t = parse("(add (mul 2 3) 4)").unwrap();
        let inner = Term::Apply(
            Box::new(Term::Var(3)), // "mul"
            vec![Term::Number(Value::Nat(2)), Term::Number(Value::Nat(3))],
        );
        assert_eq!(
            t,
            Term::Apply(
                Box::new(Term::Var(2)), // "add"
                vec![inner, Term::Number(Value::Nat(4))],
            )
        );
    }

    #[test]
    fn parse_fn() {
        let t = parse("(fn (?10 ?11) (add ?10 ?11))").unwrap();
        assert_eq!(
            t,
            Term::Fn(
                vec![10, 11],
                Box::new(Term::Apply(
                    Box::new(Term::Var(2)),
                    vec![Term::Var(10), Term::Var(11)],
                )),
            )
        );
    }

    #[test]
    fn parse_symbol() {
        let t = parse("(S5 ?1 ?2)").unwrap();
        assert_eq!(t, Term::Symbol(5, vec![Term::Var(1), Term::Var(2)]));
    }

    // ── R7: Int parsing gold tests ───────────────────────────────

    #[test]
    fn parse_negative_number_as_int() {
        let t = parse("-7").unwrap();
        assert_eq!(t, Term::Number(Value::Int(-7)));
    }

    #[test]
    fn parse_positive_int_with_i_prefix() {
        let t = parse("i42").unwrap();
        assert_eq!(t, Term::Number(Value::Int(42)));
    }

    #[test]
    fn parse_int_zero_distinguishes_from_nat() {
        let int_form = parse("-0").unwrap();
        let nat_form = parse("0").unwrap();
        // -0 parses as Int(0); plain 0 parses as Nat(0). Same
        // numeric content, different domains.
        assert_eq!(int_form, Term::Number(Value::Int(0)));
        assert_eq!(nat_form, Term::Number(Value::Nat(0)));
        assert_ne!(int_form, nat_form);
    }

    #[test]
    fn parse_int_builtins_by_name() {
        let t = parse("(int_add i3 i5)").unwrap();
        assert_eq!(
            t,
            Term::Apply(
                Box::new(Term::Var(12)), // INT_ADD
                vec![
                    Term::Number(Value::Int(3)),
                    Term::Number(Value::Int(5)),
                ],
            )
        );
    }

    #[test]
    fn parse_neg_builtin() {
        let t = parse("(neg -5)").unwrap();
        assert_eq!(
            t,
            Term::Apply(
                Box::new(Term::Var(14)), // NEG
                vec![Term::Number(Value::Int(-5))],
            )
        );
    }

    #[test]
    fn parse_and_canonical_fold_int_expression() {
        // End-to-end through R7's capability: parse an Int expression,
        // canonicalize, get the folded Int result. This test proves
        // the whole kernel pipeline works for the new domain.
        use crate::term::Term as T;
        let t = parse("(int_add (int_mul -3 -5) i7)").unwrap();
        let canon = t.canonical();
        // -3 * -5 = 15, 15 + 7 = 22
        assert_eq!(canon, T::Number(Value::Int(22)));
    }

    #[test]
    fn parse_and_eval_int_successor_chain() {
        // (int_succ (int_succ (int_zero))) → Int(2)
        use crate::eval::eval;
        let t = parse("(int_succ (int_succ (int_zero)))").unwrap();
        let v = eval(&t, &[], 100).unwrap();
        assert_eq!(v, Term::Number(Value::Int(2)));
    }

    #[test]
    fn parse_and_eval_neg_composition() {
        // neg(neg(-5)) = -5 (involution). Evaluated through eval,
        // not just canonical — verifies both paths see R7 uniformly.
        use crate::eval::eval;
        let t = parse("(neg (neg -5))").unwrap();
        let v = eval(&t, &[], 100).unwrap();
        assert_eq!(v, Term::Number(Value::Int(-5)));
    }

    #[test]
    fn roundtrip_display_parse() {
        let terms = vec![
            Term::Number(Value::Nat(42)),
            Term::Point(3),
            Term::Var(7),
            Term::Apply(
                Box::new(Term::Var(2)),
                vec![Term::Number(Value::Nat(1)), Term::Number(Value::Nat(2))],
            ),
        ];
        for t in &terms {
            let s = format!("{t}");
            let parsed = parse(&s).unwrap();
            assert_eq!(t, &parsed, "roundtrip failed for: {s}");
        }
    }
}
