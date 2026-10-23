//! Resolving names and checking types, so mistakes show up while typing
//! with a message that points at the place.

use std::ops::Range;

use territoria_data::{ColumnKind, Table};

use crate::eval::Val;
use crate::syntax::{Ast, BinOp, Spanned};
use crate::{Error, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    If,
    IfNull,
    Coalesce,
    IsNull,
    Switch,
    Abs,
    Round,
    Floor,
    Ceil,
    Sqrt,
    Pow,
    Ln,
    Log,
    Exp,
    Clamp,
    Min,
    Max,
    Left,
    Right,
    Mid,
    Len,
    Lower,
    Upper,
    Trim,
    Replace,
    Find,
    Contains,
    StartsWith,
    EndsWith,
    Like,
    Pad,
    Split,
    Date,
    Year,
    Month,
    Day,
    Weekday,
    Today,
    DateAdd,
    DateDiff,
    Format,
    Num,
    Text,
    Bool,
    Sum,
    Mean,
    Median,
    Count,
    ColumnMin,
    ColumnMax,
    Rank,
    Percentile,
    Share,
    Distance,
    Dms,
}

/// A function for the reference and autocomplete.
#[derive(Debug, Clone, Copy)]
pub struct FunctionInfo {
    pub name: &'static str,
    pub signature: &'static str,
    pub doc: &'static str,
    pub group: &'static str,
}

macro_rules! f {
    ($group:literal, $name:literal, $sig:literal, $doc:literal) => {
        FunctionInfo {
            name: $name,
            signature: $sig,
            doc: $doc,
            group: $group,
        }
    };
}

pub const FUNCTIONS: &[FunctionInfo] = &[
    f!(
        "Logic",
        "if",
        "if(condition, then, else)",
        "`then` where the condition holds, otherwise `else` (empty when left out)."
    ),
    f!(
        "Logic",
        "ifnull",
        "ifnull(value, fallback)",
        "The value, or the fallback where it is empty."
    ),
    f!(
        "Logic",
        "coalesce",
        "coalesce(a, b, …)",
        "The first value that is not empty."
    ),
    f!(
        "Logic",
        "isnull",
        "isnull(value)",
        "Yes where the value is empty."
    ),
    f!(
        "Logic",
        "switch",
        "switch(value, case1, result1, …, default)",
        "The result of the first case equal to the value."
    ),
    f!("Math", "abs", "abs(number)", "The number without its sign."),
    f!(
        "Math",
        "round",
        "round(number, digits)",
        "Rounded to `digits` decimals (0 when left out; negative rounds to tens, hundreds…)."
    ),
    f!("Math", "floor", "floor(number)", "Rounded down."),
    f!("Math", "ceil", "ceil(number)", "Rounded up."),
    f!("Math", "sqrt", "sqrt(number)", "Square root."),
    f!(
        "Math",
        "pow",
        "pow(number, exponent)",
        "The number to a power, like number ^ exponent."
    ),
    f!("Math", "ln", "ln(number)", "Natural logarithm."),
    f!(
        "Math",
        "log",
        "log(number, base)",
        "Logarithm (base 10 when left out)."
    ),
    f!(
        "Math",
        "exp",
        "exp(number)",
        "e to the power of the number."
    ),
    f!(
        "Math",
        "clamp",
        "clamp(number, low, high)",
        "The number kept between low and high."
    ),
    f!(
        "Math",
        "min",
        "min(a, b, …) or min([column])",
        "The smallest of the values in a row; with one column, the smallest in the column."
    ),
    f!(
        "Math",
        "max",
        "max(a, b, …) or max([column])",
        "The largest of the values in a row; with one column, the largest in the column."
    ),
    f!("Text", "left", "left(text, n)", "The first n characters."),
    f!("Text", "right", "right(text, n)", "The last n characters."),
    f!(
        "Text",
        "mid",
        "mid(text, start, n)",
        "n characters from position start (1 = first); to the end when n is left out."
    ),
    f!("Text", "len", "len(text)", "Number of characters."),
    f!("Text", "lower", "lower(text)", "In lower case."),
    f!("Text", "upper", "upper(text)", "In upper case."),
    f!(
        "Text",
        "trim",
        "trim(text)",
        "Without spaces at the start and end."
    ),
    f!(
        "Text",
        "replace",
        "replace(text, find, with)",
        "Every `find` replaced by `with`."
    ),
    f!(
        "Text",
        "find",
        "find(text, part)",
        "Position of `part` (1 = first), empty if not found. Ignores case."
    ),
    f!(
        "Text",
        "contains",
        "contains(text, part)",
        "Yes where the text contains `part`. Ignores case."
    ),
    f!(
        "Text",
        "startswith",
        "startswith(text, part)",
        "Yes where the text starts with `part`. Ignores case."
    ),
    f!(
        "Text",
        "endswith",
        "endswith(text, part)",
        "Yes where the text ends with `part`. Ignores case."
    ),
    f!(
        "Text",
        "like",
        "like(text, pattern)",
        "Yes where the text matches the pattern: * any characters, ? one character. Ignores case."
    ),
    f!(
        "Text",
        "pad",
        "pad(text, length, fill)",
        "Filled at the start to the length, e.g. pad([Code], 5, \"0\") turns 123 into 00123."
    ),
    f!(
        "Text",
        "split",
        "split(text, separator, n)",
        "The n-th part (1 = first) of the text split at the separator."
    ),
    f!(
        "Dates",
        "date",
        "date(year, month, day) or date(text)",
        "A date from its parts or from text like 2026-10-23 or 23.10.2026."
    ),
    f!("Dates", "year", "year(date)", "The year."),
    f!("Dates", "month", "month(date)", "The month, 1–12."),
    f!("Dates", "day", "day(date)", "The day of the month."),
    f!(
        "Dates",
        "weekday",
        "weekday(date)",
        "The day of the week, 1 = Monday … 7 = Sunday."
    ),
    f!("Dates", "today", "today()", "Today's date."),
    f!(
        "Dates",
        "dateadd",
        "dateadd(date, n, unit)",
        "The date n days later; unit \"weeks\", \"months\" or \"years\" for those."
    ),
    f!(
        "Dates",
        "datediff",
        "datediff(from, to, unit)",
        "Days from one date to another; unit \"weeks\", \"months\" or \"years\" for whole ones of those."
    ),
    f!(
        "Conversion",
        "format",
        "format(value, pattern)",
        "Text of a number with that many decimals, or of a date like \"DD.MM.YYYY\"."
    ),
    f!(
        "Conversion",
        "num",
        "num(value)",
        "A number from text (1234.5, 1'234.50, 1.234,5) or yes/no (1/0)."
    ),
    f!("Conversion", "text", "text(value)", "The value as text."),
    f!(
        "Conversion",
        "bool",
        "bool(value)",
        "Yes/no from text (yes, true, 1, x) or a number (not 0)."
    ),
    f!(
        "Column",
        "sum",
        "sum([column], [group])",
        "The column's total, or the total of the row's group."
    ),
    f!(
        "Column",
        "mean",
        "mean([column], [group])",
        "The column's average (of its group's rows when a group is given)."
    ),
    f!(
        "Column",
        "median",
        "median([column], [group])",
        "The column's middle value (of its group's rows when a group is given)."
    ),
    f!(
        "Column",
        "count",
        "count([column], [group])",
        "How many rows have a value (of the row's group when given); count() is all rows."
    ),
    f!(
        "Column",
        "rank",
        "rank([column], [group])",
        "Rank by size, 1 = largest (within the row's group when given)."
    ),
    f!(
        "Column",
        "percentile",
        "percentile([column])",
        "Share of rows with a smaller value, 0 to 1."
    ),
    f!(
        "Column",
        "share",
        "share([column], [group])",
        "The value as a share of the column's total (or its group's), 0 to 1."
    ),
    f!(
        "Places",
        "distance",
        "distance(lat1, lon1, lat2, lon2)",
        "Straight-line distance in km between two coordinates in degrees."
    ),
    f!(
        "Places",
        "dms",
        "dms(text)",
        "Degrees from text like 47°22'30\"N or 8 32 45 E."
    ),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Const(Val),
    Column(usize),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub node: Node,
    pub ty: Type,
    pub span: Range<usize>,
}

fn column_type(kind: ColumnKind) -> Type {
    match kind {
        ColumnKind::Text | ColumnKind::Key => Type::Text,
        ColumnKind::Number | ColumnKind::Integer => Type::Number,
        ColumnKind::Bool => Type::Bool,
        ColumnKind::Date => Type::Date,
    }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// The column name closest to `name`, for "did you mean".
fn similar(table: &Table, name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    table
        .columns
        .iter()
        .map(|c| {
            let l = c.name.to_lowercase();
            let d = if l.contains(&lower) || lower.contains(&l) {
                1
            } else {
                levenshtein(&l, &lower)
            };
            (d, &c.name)
        })
        .filter(|(d, _)| *d <= 2.max(lower.chars().count() / 3))
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| n.clone())
}

pub struct Checker<'a> {
    pub table: &'a Table,
}

fn unify(a: Type, b: Type) -> Option<Type> {
    match (a, b) {
        (Type::Null, t) | (t, Type::Null) => Some(t),
        (a, b) if a == b => Some(a),
        _ => None,
    }
}

impl Checker<'_> {
    pub fn check(&self, s: &Spanned) -> Result<Expr, Error> {
        let span = s.span.clone();
        let (node, ty) = match &s.ast {
            Ast::Number(n) => (Node::Const(Val::Num(*n)), Type::Number),
            Ast::Text(t) => (Node::Const(Val::Text(t.clone())), Type::Text),
            Ast::Bool(b) => (Node::Const(Val::Bool(*b)), Type::Bool),
            Ast::Null => (Node::Const(Val::Null), Type::Null),
            Ast::Column(name) => self.column(name, &span, true)?,
            Ast::Name(name) => self.column(name, &span, false)?,
            Ast::Neg(inner) => {
                let e = self.check(inner)?;
                self.expect(&e, &[Type::Number], "a number to negate")?;
                (Node::Neg(Box::new(e)), Type::Number)
            }
            Ast::Not(inner) => {
                let e = self.check(inner)?;
                self.expect(&e, &[Type::Bool], "yes/no after `not`")?;
                (Node::Not(Box::new(e)), Type::Bool)
            }
            Ast::Binary(op, a, b) => {
                let a = self.check(a)?;
                let b = self.check(b)?;
                return self.binary(*op, a, b, span);
            }
            Ast::Call(name, args) => {
                let args = args
                    .iter()
                    .map(|a| self.check(a))
                    .collect::<Result<Vec<_>, _>>()?;
                return self.call(name, args, span);
            }
        };
        Ok(Expr { node, ty, span })
    }

    fn column(
        &self,
        name: &str,
        span: &Range<usize>,
        bracketed: bool,
    ) -> Result<(Node, Type), Error> {
        let found = self.table.column_index(name).or_else(|| {
            self.table
                .columns
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(name))
        });
        if let Some(i) = found {
            return Ok((Node::Column(i), column_type(self.table.columns[i].kind)));
        }
        let hint = similar(self.table, name)
            .map(|n| format!(" Did you mean [{n}]?"))
            .unwrap_or_default();
        Err(Error::new(
            if bracketed {
                format!("There is no column [{name}].{hint}")
            } else if FUNCTIONS.iter().any(|f| f.name.eq_ignore_ascii_case(name)) {
                format!("`{name}` is a function: write {}(…)", name.to_lowercase())
            } else if hint.is_empty() {
                format!(
                    "Unknown name `{name}`. Write column names in brackets, e.g. [{name}], and text in quotes, e.g. \"{name}\"."
                )
            } else {
                format!("Unknown name `{name}`.{hint}")
            },
            span.clone(),
        ))
    }

    fn expect(&self, e: &Expr, types: &[Type], what: &str) -> Result<(), Error> {
        if e.ty == Type::Null || types.contains(&e.ty) {
            return Ok(());
        }
        let fix = match (e.ty, types.first()) {
            (Type::Text, Some(Type::Number)) => " Use num(…) to read numbers from text.",
            (Type::Text, Some(Type::Date)) => " Use date(…) to read dates from text.",
            (Type::Number, Some(Type::Text)) => " Use text(…) to turn it into text.",
            _ => "",
        };
        Err(Error::new(
            format!("Expected {what}, but this is {}.{fix}", e.ty.article()),
            e.span.clone(),
        ))
    }

    fn binary(
        &self,
        op: BinOp,
        mut a: Expr,
        mut b: Expr,
        span: Range<usize>,
    ) -> Result<Expr, Error> {
        use Type::*;
        let ty = match op {
            BinOp::Add | BinOp::Sub => match (a.ty, b.ty) {
                (Date, Number | Null) => Date,
                (Number | Null, Date) if op == BinOp::Add => Date,
                (Date, Date) if op == BinOp::Sub => Number,
                (Text, _) | (_, Text) if op == BinOp::Add => {
                    return Err(Error::new(
                        "Use & to join text, e.g. [First] & \" \" & [Last]. For numbers in text columns use num(…).",
                        span,
                    ));
                }
                _ => {
                    self.expect(&a, &[Number], &format!("a number before {}", op.symbol()))?;
                    self.expect(&b, &[Number], &format!("a number after {}", op.symbol()))?;
                    Number
                }
            },
            BinOp::Mul | BinOp::Div | BinOp::Pow | BinOp::Mod => {
                self.expect(&a, &[Number], &format!("a number before {}", op.symbol()))?;
                self.expect(&b, &[Number], &format!("a number after {}", op.symbol()))?;
                Number
            }
            BinOp::Concat => Text,
            BinOp::And | BinOp::Or => {
                self.expect(&a, &[Bool], &format!("yes/no before `{}`", op.symbol()))?;
                self.expect(&b, &[Bool], &format!("yes/no after `{}`", op.symbol()))?;
                Bool
            }
            _ => {
                // Dates compare with text written as a date.
                date_literal(&mut a, &b)?;
                date_literal(&mut b, &a)?;
                if unify(a.ty, b.ty).is_none() {
                    return Err(Error::new(
                        format!("Can't compare {} with {}", a.ty.article(), b.ty.article()),
                        span,
                    ));
                }
                Bool
            }
        };
        Ok(Expr {
            node: Node::Binary(op, Box::new(a), Box::new(b)),
            ty,
            span,
        })
    }

    fn call(&self, name: &str, args: Vec<Expr>, span: Range<usize>) -> Result<Expr, Error> {
        use Type::*;
        let lower = name.to_lowercase();
        let n = args.len();
        let count = |min: usize, max: usize| -> Result<(), Error> {
            if n < min || n > max {
                let sig = FUNCTIONS
                    .iter()
                    .find(|f| f.name == lower)
                    .map_or("", |f| f.signature);
                let wanted = if min == max {
                    format!("{min}")
                } else if max == usize::MAX {
                    format!("at least {min}")
                } else {
                    format!("{min} to {max}")
                };
                return Err(Error::new(
                    format!("{lower} takes {wanted} values, not {n}: {sig}"),
                    span.clone(),
                ));
            }
            Ok(())
        };
        let num = |i: usize| {
            self.expect(
                &args[i],
                &[Number],
                &format!("a number as value {} of {lower}", i + 1),
            )
        };
        let date = |i: usize| {
            self.expect(
                &args[i],
                &[Date],
                &format!("a date as value {} of {lower}", i + 1),
            )
        };
        let unit = |i: usize| -> Result<(), Error> {
            match &args.get(i).map(|a| &a.node) {
                None => Ok(()),
                Some(Node::Const(Val::Text(t))) if crate::eval::Unit::parse(t).is_some() => Ok(()),
                _ => Err(Error::new(
                    "The unit is one of \"days\", \"weeks\", \"months\", \"years\"",
                    args[i].span.clone(),
                )),
            }
        };
        let (func, ty) = match lower.as_str() {
            "if" | "iif" => {
                count(2, 3)?;
                self.expect(
                    &args[0],
                    &[Bool],
                    "a condition (yes/no), e.g. [Revenue] > 1000",
                )?;
                let other = args.get(2).map_or(Null, |a| a.ty);
                let ty = unify(args[1].ty, other).ok_or_else(|| {
                    Error::new(
                        format!(
                            "Both results should be of one kind: here {} and {}",
                            args[1].ty.article(),
                            other.article()
                        ),
                        span.clone(),
                    )
                })?;
                (Func::If, ty)
            }
            "ifnull" => {
                count(2, 2)?;
                (Func::IfNull, self.same(&args, &span)?)
            }
            "coalesce" => {
                count(1, usize::MAX)?;
                (Func::Coalesce, self.same(&args, &span)?)
            }
            "isnull" | "isempty" => {
                count(1, 1)?;
                (Func::IsNull, Bool)
            }
            "switch" | "case" => {
                count(3, usize::MAX)?;
                let mut ty = Null;
                for pair in args[1..].chunks(2) {
                    if let [case, result] = pair {
                        if unify(case.ty, args[0].ty).is_none() {
                            return Err(Error::new(
                                format!(
                                    "This case is {}, the value {}",
                                    case.ty.article(),
                                    args[0].ty.article()
                                ),
                                case.span.clone(),
                            ));
                        }
                        ty = unify(ty, result.ty).ok_or_else(|| {
                            Error::new("All results should be of one kind", result.span.clone())
                        })?;
                    } else {
                        ty = unify(ty, pair[0].ty).ok_or_else(|| {
                            Error::new(
                                "The default should be of the results' kind",
                                pair[0].span.clone(),
                            )
                        })?;
                    }
                }
                (Func::Switch, ty)
            }
            "abs" | "floor" | "ceil" | "sqrt" | "ln" | "exp" => {
                count(1, 1)?;
                num(0)?;
                let f = match lower.as_str() {
                    "abs" => Func::Abs,
                    "floor" => Func::Floor,
                    "ceil" => Func::Ceil,
                    "sqrt" => Func::Sqrt,
                    "ln" => Func::Ln,
                    _ => Func::Exp,
                };
                (f, Number)
            }
            "round" | "log" => {
                count(1, 2)?;
                for i in 0..n {
                    num(i)?;
                }
                (
                    if lower == "round" {
                        Func::Round
                    } else {
                        Func::Log
                    },
                    Number,
                )
            }
            "pow" | "power" => {
                count(2, 2)?;
                num(0)?;
                num(1)?;
                (Func::Pow, Number)
            }
            "clamp" => {
                count(3, 3)?;
                for i in 0..3 {
                    num(i)?;
                }
                (Func::Clamp, Number)
            }
            "min" | "max" if n == 1 => {
                self.expect(
                    &args[0],
                    &[Number, Date],
                    &format!("a number or date column for {lower}"),
                )?;
                (
                    if lower == "min" {
                        Func::ColumnMin
                    } else {
                        Func::ColumnMax
                    },
                    args[0].ty,
                )
            }
            "min" | "max" => {
                count(1, usize::MAX)?;
                let ty = self.same(&args, &span)?;
                if !matches!(ty, Number | Date | Null) {
                    return Err(Error::new(
                        format!("{lower} compares numbers or dates"),
                        span,
                    ));
                }
                (if lower == "min" { Func::Min } else { Func::Max }, ty)
            }
            "left" | "right" => {
                count(2, 2)?;
                num(1)?;
                (
                    if lower == "left" {
                        Func::Left
                    } else {
                        Func::Right
                    },
                    Text,
                )
            }
            "mid" | "substr" | "substring" => {
                count(2, 3)?;
                for i in 1..n {
                    num(i)?;
                }
                (Func::Mid, Text)
            }
            "len" | "length" => {
                count(1, 1)?;
                (Func::Len, Number)
            }
            "lower" | "upper" | "trim" => {
                count(1, 1)?;
                let f = match lower.as_str() {
                    "lower" => Func::Lower,
                    "upper" => Func::Upper,
                    _ => Func::Trim,
                };
                (f, Text)
            }
            "replace" => {
                count(3, 3)?;
                (Func::Replace, Text)
            }
            "find" => {
                count(2, 2)?;
                (Func::Find, Number)
            }
            "contains" | "startswith" | "endswith" | "like" => {
                count(2, 2)?;
                let f = match lower.as_str() {
                    "contains" => Func::Contains,
                    "startswith" => Func::StartsWith,
                    "endswith" => Func::EndsWith,
                    _ => Func::Like,
                };
                (f, Bool)
            }
            "pad" => {
                count(2, 3)?;
                num(1)?;
                (Func::Pad, Text)
            }
            "split" => {
                count(3, 3)?;
                num(2)?;
                (Func::Split, Text)
            }
            "date" => {
                match n {
                    1 => self.expect(&args[0], &[Text, Date], "text like 2026-10-23")?,
                    3 => {
                        for i in 0..3 {
                            num(i)?;
                        }
                    }
                    _ => {
                        return Err(Error::new(
                            "date takes a text, or year, month and day: date(2026, 10, 23)",
                            span,
                        ));
                    }
                }
                (Func::Date, Date)
            }
            "year" | "month" | "day" | "weekday" => {
                count(1, 1)?;
                date(0)?;
                let f = match lower.as_str() {
                    "year" => Func::Year,
                    "month" => Func::Month,
                    "day" => Func::Day,
                    _ => Func::Weekday,
                };
                (f, Number)
            }
            "today" => {
                count(0, 0)?;
                (Func::Today, Date)
            }
            "dateadd" => {
                count(2, 3)?;
                date(0)?;
                num(1)?;
                unit(2)?;
                (Func::DateAdd, Date)
            }
            "datediff" => {
                count(2, 3)?;
                date(0)?;
                date(1)?;
                unit(2)?;
                (Func::DateDiff, Number)
            }
            "format" => {
                count(1, 2)?;
                if n == 2 {
                    match args[0].ty {
                        Number => num(1)?,
                        Date => self.expect(&args[1], &[Text], "a pattern like \"DD.MM.YYYY\"")?,
                        _ => {}
                    }
                }
                (Func::Format, Text)
            }
            "num" | "number" | "value" => {
                count(1, 1)?;
                (Func::Num, Number)
            }
            "text" | "str" => {
                count(1, 1)?;
                (Func::Text, Text)
            }
            "bool" => {
                count(1, 1)?;
                (Func::Bool, Bool)
            }
            "sum" | "mean" | "avg" | "average" | "median" | "share" | "rank" => {
                count(1, 2)?;
                num(0)?;
                let f = match lower.as_str() {
                    "sum" => Func::Sum,
                    "median" => Func::Median,
                    "share" => Func::Share,
                    "rank" => Func::Rank,
                    _ => Func::Mean,
                };
                (f, Number)
            }
            "count" => {
                count(0, 2)?;
                (Func::Count, Number)
            }
            "percentile" | "percentrank" => {
                count(1, 1)?;
                num(0)?;
                (Func::Percentile, Number)
            }
            "distance" => {
                count(4, 4)?;
                for i in 0..4 {
                    num(i)?;
                }
                (Func::Distance, Number)
            }
            "dms" => {
                count(1, 1)?;
                (Func::Dms, Number)
            }
            _ => {
                let close = FUNCTIONS
                    .iter()
                    .map(|f| (levenshtein(f.name, &lower), f.name))
                    .filter(|(d, _)| *d <= 2)
                    .min_by_key(|(d, _)| *d)
                    .map(|(_, n)| format!(" Did you mean {n}(…)?"))
                    .unwrap_or_default();
                return Err(Error::new(
                    format!("There is no function {name}.{close}"),
                    span.start..span.start + name.len(),
                ));
            }
        };
        Ok(Expr {
            node: Node::Call(func, args),
            ty,
            span,
        })
    }

    /// The one type all `args` share.
    fn same(&self, args: &[Expr], span: &Range<usize>) -> Result<Type, Error> {
        let mut ty = Type::Null;
        for a in args {
            ty = unify(ty, a.ty).ok_or_else(|| {
                Error::new(
                    format!(
                        "The values should be of one kind: {} and {}",
                        ty.article(),
                        a.ty.article()
                    ),
                    span.clone(),
                )
            })?;
        }
        Ok(ty)
    }
}

/// Text constants compared with dates are read as dates.
fn date_literal(e: &mut Expr, other: &Expr) -> Result<(), Error> {
    if other.ty == Type::Date
        && let Node::Const(Val::Text(t)) = &e.node
    {
        let d = territoria_data::date::parse(t, territoria_data::date::DayOrder::DayMonth)
            .ok_or_else(|| {
                Error::new(
                    format!("\"{t}\" is not a date; write it like 2026-10-23"),
                    e.span.clone(),
                )
            })?;
        e.node = Node::Const(Val::Date(d));
        e.ty = Type::Date;
    }
    Ok(())
}
