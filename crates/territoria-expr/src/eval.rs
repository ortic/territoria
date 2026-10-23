//! Evaluation, a column at a time: every node yields one value per row,
//! so column-wide functions (sum, rank, share) see all rows.

use std::collections::HashMap;

use territoria_data::table::format_number;
use territoria_data::{Table, Value, date};

use crate::check::{Expr, Func, Node};
use crate::syntax::BinOp;

/// A value while evaluating.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Null,
    Num(f64),
    Text(String),
    Bool(bool),
    /// Days since 1970-01-01.
    Date(i32),
}

impl Val {
    fn from_cell(v: Value) -> Val {
        match v {
            Value::Null => Val::Null,
            Value::Text(t) => Val::Text(t.to_owned()),
            Value::Number(n) => Val::Num(n),
            Value::Bool(b) => Val::Bool(b),
            Value::Date(d) => Val::Date(d),
        }
    }

    fn num(n: f64) -> Val {
        if n.is_finite() {
            Val::Num(n)
        } else {
            Val::Null
        }
    }

    pub fn as_num(&self) -> Option<f64> {
        match self {
            Val::Num(n) => Some(*n),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Val::Bool(b) => Some(*b),
            _ => None,
        }
    }

    fn as_date(&self) -> Option<i32> {
        match self {
            Val::Date(d) => Some(*d),
            _ => None,
        }
    }

    /// Text of any value; empty for nothing.
    pub fn text(&self) -> Option<String> {
        match self {
            Val::Null => None,
            Val::Num(n) => Some(format_number(*n)),
            Val::Text(t) => Some(t.clone()),
            Val::Bool(b) => Some(if *b { "yes" } else { "no" }.into()),
            Val::Date(d) => Some(date::format(*d)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Days,
    Weeks,
    Months,
    Years,
}

impl Unit {
    pub fn parse(t: &str) -> Option<Unit> {
        match t.trim().to_lowercase().trim_end_matches('s') {
            "day" | "d" | "tag" | "tage" => Some(Unit::Days),
            "week" | "w" | "woche" | "wochen" => Some(Unit::Weeks),
            "month" | "m" | "monat" | "monate" => Some(Unit::Months),
            "year" | "y" | "jahr" | "jahre" => Some(Unit::Years),
            _ => None,
        }
    }
}

fn add_months(d: i32, months: i64) -> Option<i32> {
    let (y, m, day) = date::to_ymd(d);
    let total = i64::from(y) * 12 + i64::from(m) - 1 + months;
    let y = i32::try_from(total.div_euclid(12)).ok()?;
    let m = (total.rem_euclid(12) + 1) as u32;
    date::from_ymd(y, m, day.min(date::days_in_month(y, m)))
}

fn months_between(a: i32, b: i32) -> i64 {
    let (y1, m1, d1) = date::to_ymd(a);
    let (y2, m2, d2) = date::to_ymd(b);
    let mut months = (i64::from(y2) - i64::from(y1)) * 12 + i64::from(m2) - i64::from(m1);
    if months > 0 && d2 < d1 {
        months -= 1;
    } else if months < 0 && d2 > d1 {
        months += 1;
    }
    months
}

fn like(text: &str, pattern: &str) -> bool {
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    // Classic wildcard matching with backtracking to the last `*`.
    let (mut i, mut j) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while i < t.len() {
        if j < p.len() && (p[j] == '?' || p[j] == t[i]) {
            i += 1;
            j += 1;
        } else if j < p.len() && p[j] == '*' {
            star = Some((j, i));
            j += 1;
        } else if let Some((sj, si)) = star {
            j = sj + 1;
            i = si + 1;
            star = Some((sj, si + 1));
        } else {
            return false;
        }
    }
    p[j..].iter().all(|&c| c == '*')
}

fn parse_bool(t: &str) -> Option<bool> {
    match t.trim().to_lowercase().as_str() {
        "yes" | "true" | "1" | "x" | "y" | "ja" | "j" | "oui" | "si" | "sì" | "wahr" => Some(true),
        "no" | "false" | "0" | "n" | "nein" | "non" | "falsch" | "" => Some(false),
        _ => None,
    }
}

fn parse_num(t: &str) -> Option<f64> {
    use territoria_data::infer::{Decimal, parse_number};
    let t = t.trim();
    // A lone comma with up to two decimals after it is a decimal comma.
    let comma = t.matches(',').count() == 1
        && !t.contains('.')
        && t.rsplit(',').next().is_some_and(|d| d.len() <= 2);
    parse_number(
        t,
        if comma {
            Decimal::Comma
        } else {
            Decimal::Point
        },
    )
}

/// Degrees from `47°22'30"N`, `8 32 45 E`, `-8.5`.
fn dms(t: &str) -> Option<f64> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for c in t.chars() {
        if c.is_ascii_digit() || c == '.' || (c == ',' && !cur.is_empty()) {
            cur.push(if c == ',' { '.' } else { c });
        } else if !cur.is_empty() {
            parts.push(cur.parse::<f64>().ok()?);
            cur.clear();
        }
    }
    if !cur.is_empty() {
        parts.push(cur.parse::<f64>().ok()?);
    }
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let v = parts[0] + parts.get(1).unwrap_or(&0.0) / 60.0 + parts.get(2).unwrap_or(&0.0) / 3600.0;
    let upper = t.to_uppercase();
    let negative = t.trim_start().starts_with('-') || upper.contains('S') || upper.contains('W');
    Some(if negative { -v } else { v })
}

fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * 6_371.008_8 * a.sqrt().asin()
}

fn format_date(d: i32, pattern: &str) -> String {
    let (y, m, day) = date::to_ymd(d);
    let mut out = String::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        let (token, len) = [
            ("YYYY", 4),
            ("YY", 2),
            ("MM", 2),
            ("DD", 2),
            ("M", 1),
            ("D", 1),
        ]
        .into_iter()
        .find(|(t, _)| rest.starts_with(t))
        .map_or(
            (None, rest.chars().next().map_or(1, char::len_utf8)),
            |(t, l)| (Some(t), l),
        );
        match token {
            Some("YYYY") => out.push_str(&format!("{y:04}")),
            Some("YY") => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            Some("MM") => out.push_str(&format!("{m:02}")),
            Some("DD") => out.push_str(&format!("{day:02}")),
            Some("M") => out.push_str(&m.to_string()),
            Some("D") => out.push_str(&day.to_string()),
            _ => out.push_str(&rest[..len]),
        }
        rest = &rest[len..];
    }
    out
}

fn compare(a: &Val, b: &Val) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x.partial_cmp(y),
        (Val::Text(x), Val::Text(y)) => Some(x.to_lowercase().cmp(&y.to_lowercase())),
        (Val::Date(x), Val::Date(y)) => Some(x.cmp(y)),
        (Val::Bool(x), Val::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

/// Row indices by group key (all rows in one group without a key).
fn groups(keys: Option<&[Val]>, rows: usize) -> Vec<Vec<usize>> {
    let Some(keys) = keys else {
        return vec![(0..rows).collect()];
    };
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Vec<usize>> = Vec::new();
    for (r, k) in keys.iter().enumerate() {
        let key = k.text().unwrap_or_default().to_lowercase();
        let g = *index.entry(key).or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        out[g].push(r);
    }
    out
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    })
}

pub struct Evaluator<'a> {
    pub table: &'a Table,
    pub rows: usize,
    pub today: i32,
}

impl Evaluator<'_> {
    pub fn eval(&self, e: &Expr) -> Vec<Val> {
        let rows = self.rows;
        match &e.node {
            Node::Const(v) => vec![v.clone(); rows],
            Node::Column(i) => {
                let c = &self.table.columns[*i];
                (0..rows).map(|r| Val::from_cell(c.get(r))).collect()
            }
            Node::Neg(a) => self
                .eval(a)
                .into_iter()
                .map(|v| v.as_num().map_or(Val::Null, |n| Val::num(-n)))
                .collect(),
            Node::Not(a) => self
                .eval(a)
                .into_iter()
                .map(|v| v.as_bool().map_or(Val::Null, |b| Val::Bool(!b)))
                .collect(),
            Node::Binary(op, a, b) => {
                let (a, b) = (self.eval(a), self.eval(b));
                a.into_iter()
                    .zip(b)
                    .map(|(x, y)| binary(*op, x, y))
                    .collect()
            }
            Node::Call(f, args) => self.call(*f, args),
        }
    }

    fn call(&self, f: Func, args: &[Expr]) -> Vec<Val> {
        let rows = self.rows;
        let vals: Vec<Vec<Val>> = args.iter().map(|a| self.eval(a)).collect();
        let arg = |i: usize, r: usize| vals.get(i).map_or(&Val::Null, |v| &v[r]);
        let text = |i: usize, r: usize| arg(i, r).text();
        let num = |i: usize, r: usize| arg(i, r).as_num();
        let row_wise = |g: &dyn Fn(usize) -> Val| (0..rows).map(g).collect::<Vec<Val>>();
        let num1 =
            |g: &dyn Fn(f64) -> f64| row_wise(&|r| num(0, r).map_or(Val::Null, |x| Val::num(g(x))));
        let text1 = |g: &dyn Fn(String) -> Val| row_wise(&|r| text(0, r).map_or(Val::Null, g));
        let text2 = |g: &dyn Fn(&str, &str) -> Val| {
            row_wise(&|r| match (text(0, r), text(1, r)) {
                (Some(a), Some(b)) => g(&a, &b),
                _ => Val::Null,
            })
        };
        match f {
            Func::If => row_wise(&|r| match arg(0, r) {
                Val::Bool(true) => arg(1, r).clone(),
                _ => arg(2, r).clone(),
            }),
            Func::IfNull | Func::Coalesce => row_wise(&|r| {
                vals.iter()
                    .map(|v| &v[r])
                    .find(|v| **v != Val::Null)
                    .cloned()
                    .unwrap_or(Val::Null)
            }),
            Func::IsNull => row_wise(&|r| Val::Bool(*arg(0, r) == Val::Null)),
            Func::Switch => row_wise(&|r| {
                let x = arg(0, r);
                let mut i = 1;
                while i + 1 < vals.len() {
                    if compare(x, arg(i, r)) == Some(std::cmp::Ordering::Equal) {
                        return arg(i + 1, r).clone();
                    }
                    i += 2;
                }
                if i < vals.len() {
                    arg(i, r).clone()
                } else {
                    Val::Null
                }
            }),
            Func::Abs => num1(&f64::abs),
            Func::Floor => num1(&f64::floor),
            Func::Ceil => num1(&f64::ceil),
            Func::Sqrt => num1(&f64::sqrt),
            Func::Ln => num1(&f64::ln),
            Func::Exp => num1(&f64::exp),
            Func::Round => row_wise(&|r| {
                let digits = if vals.len() > 1 { num(1, r) } else { Some(0.0) };
                match (num(0, r), digits) {
                    (Some(x), Some(d)) => {
                        let k = 10f64.powi(d.round() as i32);
                        Val::num((x * k).round() / k)
                    }
                    _ => Val::Null,
                }
            }),
            Func::Log => row_wise(&|r| {
                let base = if vals.len() > 1 {
                    num(1, r)
                } else {
                    Some(10.0)
                };
                match (num(0, r), base) {
                    (Some(x), Some(b)) => Val::num(x.ln() / b.ln()),
                    _ => Val::Null,
                }
            }),
            Func::Pow => row_wise(&|r| match (num(0, r), num(1, r)) {
                (Some(x), Some(y)) => Val::num(x.powf(y)),
                _ => Val::Null,
            }),
            Func::Clamp => row_wise(&|r| match (num(0, r), num(1, r), num(2, r)) {
                (Some(x), Some(lo), Some(hi)) => Val::num(x.max(lo).min(hi)),
                _ => Val::Null,
            }),
            Func::Min | Func::Max => row_wise(&|r| {
                let present = vals.iter().map(|v| &v[r]).filter(|v| **v != Val::Null);
                let pick = |a: &Val, b: &Val| {
                    compare(a, b)
                        .is_some_and(|o| if f == Func::Min { o.is_le() } else { o.is_ge() })
                };
                present
                    .fold(None::<&Val>, |best, v| match best {
                        Some(b) if pick(b, v) => Some(b),
                        _ => Some(v),
                    })
                    .cloned()
                    .unwrap_or(Val::Null)
            }),
            Func::Left | Func::Right => row_wise(&|r| match (text(0, r), num(1, r)) {
                (Some(t), Some(n)) => {
                    let n = n.max(0.0) as usize;
                    let chars: Vec<char> = t.chars().collect();
                    let n = n.min(chars.len());
                    let part = if f == Func::Left {
                        &chars[..n]
                    } else {
                        &chars[chars.len() - n..]
                    };
                    Val::Text(part.iter().collect())
                }
                _ => Val::Null,
            }),
            Func::Mid => row_wise(&|r| {
                let len = if vals.len() > 2 {
                    num(2, r)
                } else {
                    Some(f64::INFINITY)
                };
                match (text(0, r), num(1, r), len) {
                    (Some(t), Some(start), Some(len)) => {
                        let start = (start.max(1.0) as usize) - 1;
                        let len = if len.is_finite() {
                            len.max(0.0) as usize
                        } else {
                            usize::MAX
                        };
                        Val::Text(t.chars().skip(start).take(len).collect())
                    }
                    _ => Val::Null,
                }
            }),
            Func::Len => text1(&|t| Val::Num(t.chars().count() as f64)),
            Func::Lower => text1(&|t| Val::Text(t.to_lowercase())),
            Func::Upper => text1(&|t| Val::Text(t.to_uppercase())),
            Func::Trim => text1(&|t| Val::Text(t.trim().to_owned())),
            Func::Replace => row_wise(&|r| match (text(0, r), text(1, r), text(2, r)) {
                (Some(t), Some(a), b) if !a.is_empty() => {
                    Val::Text(t.replace(&a, &b.unwrap_or_default()))
                }
                (Some(t), _, _) => Val::Text(t),
                _ => Val::Null,
            }),
            Func::Find => text2(&|t, p| {
                let (t, p) = (t.to_lowercase(), p.to_lowercase());
                t.find(&p)
                    .map_or(Val::Null, |i| Val::Num((t[..i].chars().count() + 1) as f64))
            }),
            Func::Contains => {
                text2(&|t, p| Val::Bool(t.to_lowercase().contains(&p.to_lowercase())))
            }
            Func::StartsWith => {
                text2(&|t, p| Val::Bool(t.to_lowercase().starts_with(&p.to_lowercase())))
            }
            Func::EndsWith => {
                text2(&|t, p| Val::Bool(t.to_lowercase().ends_with(&p.to_lowercase())))
            }
            Func::Like => text2(&|t, p| Val::Bool(like(t, p))),
            Func::Pad => row_wise(&|r| {
                let fill = if vals.len() > 2 {
                    text(2, r).and_then(|f| f.chars().next())
                } else {
                    Some('0')
                };
                match (text(0, r), num(1, r), fill) {
                    (Some(t), Some(n), Some(c)) => {
                        let missing = (n.max(0.0) as usize).saturating_sub(t.chars().count());
                        Val::Text(std::iter::repeat_n(c, missing).chain(t.chars()).collect())
                    }
                    (t, _, _) => t.map_or(Val::Null, Val::Text),
                }
            }),
            Func::Split => row_wise(&|r| match (text(0, r), text(1, r), num(2, r)) {
                (Some(t), Some(sep), Some(n)) if !sep.is_empty() && n >= 1.0 => t
                    .split(sep.as_str())
                    .nth(n as usize - 1)
                    .map_or(Val::Null, |s| Val::Text(s.trim().to_owned())),
                _ => Val::Null,
            }),
            Func::Date => row_wise(&|r| {
                if vals.len() == 3 {
                    match (num(0, r), num(1, r), num(2, r)) {
                        (Some(y), Some(m), Some(d)) => date::from_ymd(y as i32, m as u32, d as u32)
                            .map_or(Val::Null, Val::Date),
                        _ => Val::Null,
                    }
                } else {
                    match arg(0, r) {
                        Val::Date(d) => Val::Date(*d),
                        v => v
                            .text()
                            .and_then(|t| date::parse(&t, date::DayOrder::DayMonth))
                            .map_or(Val::Null, Val::Date),
                    }
                }
            }),
            Func::Year | Func::Month | Func::Day | Func::Weekday => row_wise(&|r| {
                arg(0, r).as_date().map_or(Val::Null, |d| {
                    let (y, m, day) = date::to_ymd(d);
                    Val::Num(match f {
                        Func::Year => f64::from(y),
                        Func::Month => f64::from(m),
                        Func::Day => f64::from(day),
                        _ => f64::from((d + 3).rem_euclid(7) + 1),
                    })
                })
            }),
            Func::Today => vec![Val::Date(self.today); rows],
            Func::DateAdd => row_wise(&|r| {
                let unit = if vals.len() > 2 {
                    text(2, r).and_then(|u| Unit::parse(&u))
                } else {
                    Some(Unit::Days)
                };
                match (arg(0, r).as_date(), num(1, r), unit) {
                    (Some(d), Some(n), Some(u)) => {
                        let n = n.round() as i64;
                        let out = match u {
                            Unit::Days => i32::try_from(i64::from(d) + n).ok(),
                            Unit::Weeks => i32::try_from(i64::from(d) + 7 * n).ok(),
                            Unit::Months => add_months(d, n),
                            Unit::Years => add_months(d, 12 * n),
                        };
                        out.map_or(Val::Null, Val::Date)
                    }
                    _ => Val::Null,
                }
            }),
            Func::DateDiff => row_wise(&|r| {
                let unit = if vals.len() > 2 {
                    text(2, r).and_then(|u| Unit::parse(&u))
                } else {
                    Some(Unit::Days)
                };
                match (arg(0, r).as_date(), arg(1, r).as_date(), unit) {
                    (Some(a), Some(b), Some(u)) => Val::Num(match u {
                        Unit::Days => f64::from(b - a),
                        Unit::Weeks => f64::from((b - a) / 7),
                        Unit::Months => months_between(a, b) as f64,
                        Unit::Years => (months_between(a, b) / 12) as f64,
                    }),
                    _ => Val::Null,
                }
            }),
            Func::Format => row_wise(&|r| match (arg(0, r), arg(1, r)) {
                (Val::Num(x), Val::Num(d)) => {
                    Val::Text(format!("{x:.*}", d.clamp(0.0, 12.0) as usize))
                }
                (Val::Date(d), Val::Text(p)) => Val::Text(format_date(*d, p)),
                (v, _) => v.text().map_or(Val::Null, Val::Text),
            }),
            Func::Num => row_wise(&|r| match arg(0, r) {
                Val::Num(n) => Val::Num(*n),
                Val::Bool(b) => Val::Num(f64::from(u8::from(*b))),
                Val::Text(t) => parse_num(t).map_or(Val::Null, Val::num),
                _ => Val::Null,
            }),
            Func::Text => row_wise(&|r| text(0, r).map_or(Val::Null, Val::Text)),
            Func::Bool => row_wise(&|r| match arg(0, r) {
                Val::Bool(b) => Val::Bool(*b),
                Val::Num(n) => Val::Bool(*n != 0.0),
                Val::Text(t) => parse_bool(t).map_or(Val::Null, Val::Bool),
                _ => Val::Null,
            }),
            Func::Distance => row_wise(&|r| match (num(0, r), num(1, r), num(2, r), num(3, r)) {
                (Some(a), Some(b), Some(c), Some(d)) => Val::num(haversine(a, b, c, d)),
                _ => Val::Null,
            }),
            Func::Dms => row_wise(&|r| match arg(0, r) {
                Val::Num(n) => Val::Num(*n),
                v => v.text().and_then(|t| dms(&t)).map_or(Val::Null, Val::num),
            }),
            Func::Sum | Func::Mean | Func::Median | Func::Count | Func::Share | Func::Rank => {
                let mut out = vec![Val::Null; rows];
                if f == Func::Count && vals.is_empty() {
                    return vec![Val::Num(rows as f64); rows];
                }
                for g in groups(vals.get(1).map(Vec::as_slice), rows) {
                    let present: Vec<(usize, f64)> = g
                        .iter()
                        .filter_map(|&r| match (f, &vals[0][r]) {
                            (_, Val::Num(n)) => Some((r, *n)),
                            (Func::Count, v) if *v != Val::Null => Some((r, 0.0)),
                            _ => None,
                        })
                        .collect();
                    let sum: f64 = present.iter().map(|(_, n)| n).sum();
                    match f {
                        Func::Rank => {
                            let mut sorted: Vec<f64> = present.iter().map(|(_, n)| *n).collect();
                            sorted.sort_by(|a, b| b.total_cmp(a));
                            for &(r, n) in &present {
                                let rank = sorted.partition_point(|&x| x > n) + 1;
                                out[r] = Val::Num(rank as f64);
                            }
                        }
                        Func::Share => {
                            for &(r, n) in &present {
                                out[r] = if sum != 0.0 {
                                    Val::num(n / sum)
                                } else {
                                    Val::Null
                                };
                            }
                        }
                        _ => {
                            let v = match f {
                                Func::Sum => Val::Num(sum),
                                Func::Count => Val::Num(present.len() as f64),
                                Func::Mean if !present.is_empty() => {
                                    Val::num(sum / present.len() as f64)
                                }
                                Func::Median => median(present.iter().map(|(_, n)| *n).collect())
                                    .map_or(Val::Null, Val::num),
                                _ => Val::Null,
                            };
                            for &r in &g {
                                out[r] = v.clone();
                            }
                        }
                    }
                }
                out
            }
            Func::Percentile => {
                let mut sorted: Vec<f64> = vals[0].iter().filter_map(Val::as_num).collect();
                sorted.sort_by(f64::total_cmp);
                let n = sorted.len();
                vals[0]
                    .iter()
                    .map(|v| {
                        v.as_num().map_or(Val::Null, |x| {
                            let below = sorted.partition_point(|&y| y < x);
                            Val::Num(if n > 1 {
                                below as f64 / (n - 1) as f64
                            } else {
                                1.0
                            })
                        })
                    })
                    .collect()
            }
            Func::ColumnMin | Func::ColumnMax => {
                let best =
                    vals[0]
                        .iter()
                        .filter(|v| **v != Val::Null)
                        .fold(None::<&Val>, |best, v| match best {
                            Some(b)
                                if compare(b, v).is_some_and(|o| {
                                    if f == Func::ColumnMin {
                                        o.is_le()
                                    } else {
                                        o.is_ge()
                                    }
                                }) =>
                            {
                                Some(b)
                            }
                            _ => Some(v),
                        });
                vec![best.cloned().unwrap_or(Val::Null); rows]
            }
        }
    }
}

fn binary(op: BinOp, a: Val, b: Val) -> Val {
    use Val::*;
    match op {
        BinOp::And => match (a.as_bool(), b.as_bool()) {
            (Some(false), _) | (_, Some(false)) => Bool(false),
            (Some(true), Some(true)) => Bool(true),
            _ => Null,
        },
        BinOp::Or => match (a.as_bool(), b.as_bool()) {
            (Some(true), _) | (_, Some(true)) => Bool(true),
            (Some(false), Some(false)) => Bool(false),
            _ => Null,
        },
        BinOp::Concat => Text(a.text().unwrap_or_default() + &b.text().unwrap_or_default()),
        op if op.is_comparison() => match compare(&a, &b) {
            None => Null,
            Some(o) => Bool(match op {
                BinOp::Eq => o.is_eq(),
                BinOp::Ne => o.is_ne(),
                BinOp::Lt => o.is_lt(),
                BinOp::Le => o.is_le(),
                BinOp::Gt => o.is_gt(),
                _ => o.is_ge(),
            }),
        },
        _ => match (a, b) {
            (Num(x), Num(y)) => match op {
                BinOp::Add => Val::num(x + y),
                BinOp::Sub => Val::num(x - y),
                BinOp::Mul => Val::num(x * y),
                BinOp::Div if y != 0.0 => Val::num(x / y),
                BinOp::Pow => Val::num(x.powf(y)),
                BinOp::Mod if y != 0.0 => Val::num(x.rem_euclid(y)),
                _ => Null,
            },
            (Date(d), Num(n)) | (Num(n), Date(d)) if op == BinOp::Add => {
                i32::try_from(i64::from(d) + n.round() as i64).map_or(Null, Date)
            }
            (Date(d), Num(n)) if op == BinOp::Sub => {
                i32::try_from(i64::from(d) - n.round() as i64).map_or(Null, Date)
            }
            (Date(x), Date(y)) if op == BinOp::Sub => Num(f64::from(x - y)),
            _ => Null,
        },
    }
}
