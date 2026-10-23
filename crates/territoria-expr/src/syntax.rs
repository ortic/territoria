//! Tokens and the syntax tree of a formula.

use std::ops::Range;

use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Number(f64),
    Text(String),
    /// `[Column Name]`
    Column(String),
    Ident(String),
    Op(&'static str),
    End,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Range<usize>,
}

const OPERATORS: [&str; 16] = [
    "<=", ">=", "<>", "+", "-", "*", "/", "^", "%", "&", "=", "<", ">", "(", ")", ",",
];

pub fn lex(src: &str) -> Result<Vec<Token>, Error> {
    let mut tokens = Vec::new();
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|&(_, c)| c);
    let pos = |i: usize| chars.get(i).map_or(src.len(), |&(p, _)| p);
    let mut i = 0;
    while let Some(c) = at(i) {
        let start = pos(i);
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && at(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let mut j = i;
            while at(j).is_some_and(|d| d.is_ascii_digit()) {
                j += 1;
            }
            if at(j) == Some('.') {
                j += 1;
                while at(j).is_some_and(|d| d.is_ascii_digit()) {
                    j += 1;
                }
            }
            if matches!(at(j), Some('e' | 'E')) {
                let mut k = j + 1;
                if matches!(at(k), Some('+' | '-')) {
                    k += 1;
                }
                if at(k).is_some_and(|d| d.is_ascii_digit()) {
                    while at(k).is_some_and(|d| d.is_ascii_digit()) {
                        k += 1;
                    }
                    j = k;
                }
            }
            let text = &src[start..pos(j)];
            let n = text
                .parse()
                .map_err(|_| Error::new(format!("`{text}` is not a number"), start..pos(j)))?;
            tokens.push(Token {
                tok: Tok::Number(n),
                span: start..pos(j),
            });
            i = j;
            continue;
        }
        if c == '"' || c == '\'' || c == '“' || c == '„' {
            let close = if c == '\'' { '\'' } else { '"' };
            let mut text = String::new();
            let mut j = i + 1;
            loop {
                match at(j) {
                    None => {
                        return Err(Error::new(
                            "This text has no closing quote",
                            start..src.len(),
                        ));
                    }
                    Some(d) if d == close || (close == '"' && d == '”') => {
                        if at(j + 1) == Some(d) {
                            text.push(d);
                            j += 2;
                        } else {
                            j += 1;
                            break;
                        }
                    }
                    Some(d) => {
                        text.push(d);
                        j += 1;
                    }
                }
            }
            tokens.push(Token {
                tok: Tok::Text(text),
                span: start..pos(j),
            });
            i = j;
            continue;
        }
        if c == '[' {
            let mut name = String::new();
            let mut j = i + 1;
            loop {
                match at(j) {
                    None => {
                        return Err(Error::new(
                            "This column name has no closing ]",
                            start..src.len(),
                        ));
                    }
                    Some(']') if at(j + 1) == Some(']') => {
                        name.push(']');
                        j += 2;
                    }
                    Some(']') => {
                        j += 1;
                        break;
                    }
                    Some(d) => {
                        name.push(d);
                        j += 1;
                    }
                }
            }
            tokens.push(Token {
                tok: Tok::Column(name.trim().to_owned()),
                span: start..pos(j),
            });
            i = j;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while at(j).is_some_and(|d| d.is_alphanumeric() || d == '_') {
                j += 1;
            }
            tokens.push(Token {
                tok: Tok::Ident(src[start..pos(j)].to_owned()),
                span: start..pos(j),
            });
            i = j;
            continue;
        }
        // Operators; `!=` and `==` as people from other languages write them.
        let two: String = [Some(c), at(i + 1)].iter().flatten().collect();
        let alias = match two.as_str() {
            "!=" => Some("<>"),
            "==" => Some("="),
            _ => None,
        };
        if let Some(op) = alias.or_else(|| {
            OPERATORS
                .iter()
                .copied()
                .find(|op| op.len() == 2 && two == *op)
        }) {
            tokens.push(Token {
                tok: Tok::Op(op),
                span: start..pos(i + 2),
            });
            i += 2;
            continue;
        }
        let c = match c {
            '×' => '*',
            '÷' => '/',
            '−' | '–' => '-',
            ';' => ',',
            c => c,
        };
        if let Some(op) = OPERATORS
            .iter()
            .copied()
            .find(|op| op.len() == 1 && op.starts_with(c))
        {
            tokens.push(Token {
                tok: Tok::Op(op),
                span: start..pos(i + 1),
            });
            i += 1;
            continue;
        }
        return Err(Error::new(
            format!("`{c}` can't be used here"),
            start..pos(i + 1),
        ));
    }
    tokens.push(Token {
        tok: Tok::End,
        span: src.len()..src.len(),
    });
    Ok(tokens)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Mod,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Pow => "^",
            BinOp::Mod => "%",
            BinOp::Concat => "&",
            BinOp::Eq => "=",
            BinOp::Ne => "<>",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }

    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        )
    }

    /// Left and right binding power.
    fn power(self) -> (u8, u8) {
        match self {
            BinOp::Or => (1, 2),
            BinOp::And => (3, 4),
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => (7, 8),
            BinOp::Concat => (9, 10),
            BinOp::Add | BinOp::Sub => (11, 12),
            BinOp::Mul | BinOp::Div | BinOp::Mod => (13, 14),
            // Right-associative and tighter than a leading minus.
            BinOp::Pow => (17, 16),
        }
    }
}

const NOT_POWER: u8 = 5;
const NEG_POWER: u8 = 15;

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    Number(f64),
    Text(String),
    Bool(bool),
    Null,
    Column(String),
    /// A bare name: a column written without brackets.
    Name(String),
    Neg(Box<Spanned>),
    Not(Box<Spanned>),
    Binary(BinOp, Box<Spanned>, Box<Spanned>),
    Call(String, Vec<Spanned>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned {
    pub ast: Ast,
    pub span: Range<usize>,
}

pub fn parse(src: &str) -> Result<Spanned, Error> {
    let tokens = lex(src)?;
    let mut p = Parser { tokens, at: 0 };
    if p.peek().tok == Tok::End {
        return Err(Error::new(
            "Write a formula, e.g. [Revenue] / [Customers]",
            0..0,
        ));
    }
    let e = p.expr(0)?;
    let t = p.peek();
    match &t.tok {
        Tok::End => Ok(e),
        Tok::Op(")") => Err(Error::new("This ) has no matching (", t.span.clone())),
        Tok::Op(",") => Err(Error::new(
            "A comma separates the values of a function, e.g. round([A], 2)",
            t.span.clone(),
        )),
        _ => Err(Error::new(
            "An operator is missing before this, e.g. + or &",
            t.span.clone(),
        )),
    }
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

fn keyword(t: &Tok) -> Option<String> {
    match t {
        Tok::Ident(s) => Some(s.to_lowercase()),
        _ => None,
    }
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.at.min(self.tokens.len() - 1)]
    }

    fn next(&mut self) -> Token {
        let t = self.peek().clone();
        if self.at < self.tokens.len() - 1 {
            self.at += 1;
        }
        t
    }

    fn binary_op(&self) -> Option<BinOp> {
        Some(match &self.peek().tok {
            Tok::Op(op) => match *op {
                "+" => BinOp::Add,
                "-" => BinOp::Sub,
                "*" => BinOp::Mul,
                "/" => BinOp::Div,
                "^" => BinOp::Pow,
                "%" => BinOp::Mod,
                "&" => BinOp::Concat,
                "=" => BinOp::Eq,
                "<>" => BinOp::Ne,
                "<" => BinOp::Lt,
                "<=" => BinOp::Le,
                ">" => BinOp::Gt,
                ">=" => BinOp::Ge,
                _ => return None,
            },
            t => match keyword(t)?.as_str() {
                "and" => BinOp::And,
                "or" => BinOp::Or,
                _ => return None,
            },
        })
    }

    fn expr(&mut self, min: u8) -> Result<Spanned, Error> {
        let mut lhs = self.prefix()?;
        while let Some(op) = self.binary_op() {
            let (l, r) = op.power();
            if l < min {
                break;
            }
            let t = self.next();
            if self.peek().tok == Tok::End {
                return Err(Error::new(
                    format!("A value is missing after {}", op.symbol()),
                    t.span,
                ));
            }
            let rhs = self.expr(r)?;
            let span = lhs.span.start..rhs.span.end;
            lhs = Spanned {
                ast: Ast::Binary(op, Box::new(lhs), Box::new(rhs)),
                span,
            };
        }
        Ok(lhs)
    }

    fn prefix(&mut self) -> Result<Spanned, Error> {
        let t = self.next();
        let span = t.span.clone();
        let ast = match t.tok {
            Tok::Number(n) => Ast::Number(n),
            Tok::Text(s) => Ast::Text(s),
            Tok::Column(c) => {
                if c.is_empty() {
                    return Err(Error::new("Write a column name between [ and ]", span));
                }
                Ast::Column(c)
            }
            Tok::Op("(") => {
                let inner = self.expr(0)?;
                let close = self.next();
                if close.tok != Tok::Op(")") {
                    return Err(Error::new("This ( is not closed", span));
                }
                return Ok(Spanned {
                    ast: inner.ast,
                    span: span.start..close.span.end,
                });
            }
            Tok::Op("-") => {
                let inner = self.expr(NEG_POWER)?;
                let span = span.start..inner.span.end;
                return Ok(Spanned {
                    ast: Ast::Neg(Box::new(inner)),
                    span,
                });
            }
            Tok::Op("+") => return self.expr(NEG_POWER),
            Tok::Ident(name) => match name.to_lowercase().as_str() {
                "true" | "yes" => Ast::Bool(true),
                "false" | "no" => Ast::Bool(false),
                "null" => Ast::Null,
                "not" => {
                    let inner = self.expr(NOT_POWER)?;
                    let span = span.start..inner.span.end;
                    return Ok(Spanned {
                        ast: Ast::Not(Box::new(inner)),
                        span,
                    });
                }
                _ if self.peek().tok == Tok::Op("(") => {
                    self.next();
                    let mut args = Vec::new();
                    if self.peek().tok == Tok::Op(")") {
                        let close = self.next();
                        return Ok(Spanned {
                            ast: Ast::Call(name, args),
                            span: span.start..close.span.end,
                        });
                    }
                    loop {
                        if matches!(self.peek().tok, Tok::Op(",") | Tok::Op(")")) {
                            return Err(Error::new(
                                "A value is missing here",
                                self.peek().span.clone(),
                            ));
                        }
                        args.push(self.expr(0)?);
                        let sep = self.next();
                        match sep.tok {
                            Tok::Op(",") => continue,
                            Tok::Op(")") => {
                                return Ok(Spanned {
                                    ast: Ast::Call(name, args),
                                    span: span.start..sep.span.end,
                                });
                            }
                            Tok::End => {
                                return Err(Error::new(
                                    format!("{name}( is not closed: a ) is missing"),
                                    span.start..sep.span.end,
                                ));
                            }
                            _ => {
                                return Err(Error::new(
                                    "A comma or ) is missing before this",
                                    sep.span,
                                ));
                            }
                        }
                    }
                }
                _ => Ast::Name(name),
            },
            Tok::End => {
                return Err(Error::new(
                    "The formula ends too early: a value is missing",
                    span,
                ));
            }
            Tok::Op(op) => {
                return Err(Error::new(format!("A value is missing before {op}"), span));
            }
        };
        Ok(Spanned { ast, span })
    }
}
