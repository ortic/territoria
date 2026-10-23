//! Formulas over a table's columns: calculated columns, filters and label
//! templates share this one language.
//!
//! The syntax is the same in every locale: `[Column Name]` for columns
//! (brackets may be left out for simple names), `"text"`, `1.5`,
//! `+ - * / ^ %` (`%` is the remainder), `&` joins text,
//! `= <> < <= > >=` compare (text ignoring case), `and or not`, `null`.

mod check;
mod eval;
mod syntax;

use std::ops::Range;

use territoria_data::{Column, ColumnData, ColumnKind, Table};

pub use check::{FUNCTIONS, FunctionInfo};
pub use eval::Val;

/// What a formula yields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Number,
    Text,
    Bool,
    Date,
    /// Only `null`: fits anywhere.
    Null,
}

impl Type {
    pub fn label(self) -> &'static str {
        match self {
            Type::Number => "number",
            Type::Text => "text",
            Type::Bool => "yes/no",
            Type::Date => "date",
            Type::Null => "empty",
        }
    }

    fn article(self) -> &'static str {
        match self {
            Type::Number => "a number",
            Type::Text => "text",
            Type::Bool => "yes/no",
            Type::Date => "a date",
            Type::Null => "empty",
        }
    }

    /// The column kind for results of this type.
    pub fn column_kind(self) -> ColumnKind {
        match self {
            Type::Number => ColumnKind::Number,
            Type::Text | Type::Null => ColumnKind::Text,
            Type::Bool => ColumnKind::Bool,
            Type::Date => ColumnKind::Date,
        }
    }
}

/// A mistake in a formula, with the byte range it is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
    pub span: Range<usize>,
}

impl Error {
    fn new(message: impl Into<String>, span: Range<usize>) -> Error {
        Error {
            message: message.into(),
            span,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// A checked formula for one table.
#[derive(Debug, Clone)]
pub struct Formula {
    expr: check::Expr,
}

fn today() -> i32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as i32)
}

impl Formula {
    /// Parses and checks `source` against the columns of `table`.
    pub fn compile(source: &str, table: &Table) -> Result<Formula, Error> {
        let ast = syntax::parse(source)?;
        let expr = check::Checker { table }.check(&ast)?;
        Ok(Formula { expr })
    }

    pub fn ty(&self) -> Type {
        self.expr.ty
    }

    /// Indices of the columns the formula reads.
    pub fn columns(&self) -> Vec<usize> {
        fn walk(e: &check::Expr, out: &mut Vec<usize>) {
            match &e.node {
                check::Node::Column(i) => out.push(*i),
                check::Node::Neg(a) | check::Node::Not(a) => walk(a, out),
                check::Node::Binary(_, a, b) => {
                    walk(a, out);
                    walk(b, out);
                }
                check::Node::Call(_, args) => args.iter().for_each(|a| walk(a, out)),
                check::Node::Const(_) => {}
            }
        }
        let mut out = Vec::new();
        walk(&self.expr, &mut out);
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The result for every row of `table` (the table it was compiled for).
    pub fn evaluate(&self, table: &Table) -> Vec<Val> {
        eval::Evaluator {
            table,
            rows: table.rows(),
            today: today(),
        }
        .eval(&self.expr)
    }

    /// The results as a column that remembers its formula.
    pub fn column(&self, name: &str, source: &str, table: &Table) -> Column {
        let values = self.evaluate(table);
        let kind = self.ty().column_kind();
        let data = match kind {
            ColumnKind::Number => ColumnData::Number(values.iter().map(Val::as_num).collect()),
            ColumnKind::Bool => ColumnData::Bool(
                values
                    .iter()
                    .map(|v| if let Val::Bool(b) = v { Some(*b) } else { None })
                    .collect(),
            ),
            ColumnKind::Date => ColumnData::Date(
                values
                    .iter()
                    .map(|v| if let Val::Date(d) = v { Some(*d) } else { None })
                    .collect(),
            ),
            _ => ColumnData::Text(values.iter().map(Val::text).collect()),
        };
        Column {
            name: name.to_owned(),
            kind,
            data,
            formula: Some(source.to_owned()),
        }
    }
}

/// Text with formulas in braces: `{Name}: {format([Revenue] / 1000, 0)}k`.
/// `{{` and `}}` stand for braces.
#[derive(Debug, Clone)]
pub struct Template {
    parts: Vec<Part>,
}

#[derive(Debug, Clone)]
enum Part {
    Text(String),
    Formula(Formula),
}

impl Template {
    pub fn compile(source: &str, table: &Table) -> Result<Template, Error> {
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut chars = source.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '{' if chars.peek().is_some_and(|(_, d)| *d == '{') => {
                    chars.next();
                    text.push('{');
                }
                '}' if chars.peek().is_some_and(|(_, d)| *d == '}') => {
                    chars.next();
                    text.push('}');
                }
                '}' => {
                    return Err(Error::new(
                        "This } has no matching {; write }} for a brace",
                        i..i + 1,
                    ));
                }
                '{' => {
                    // The formula runs to the next } outside text and brackets.
                    let start = i + 1;
                    let mut quote: Option<char> = None;
                    let mut bracket = false;
                    let mut end = None;
                    for (j, d) in chars.by_ref() {
                        match (quote, d) {
                            (Some(q), d) if d == q => quote = None,
                            (Some(_), _) => {}
                            (None, '"' | '\'') if !bracket => quote = Some(d),
                            (None, '[') => bracket = true,
                            (None, ']') => bracket = false,
                            (None, '}') if !bracket => {
                                end = Some(j);
                                break;
                            }
                            _ => {}
                        }
                    }
                    let end = end.ok_or_else(|| {
                        Error::new("This { is not closed: a } is missing", i..source.len())
                    })?;
                    let formula =
                        Formula::compile(&source[start..end], table).map_err(|e| Error {
                            message: e.message,
                            span: e.span.start + start..e.span.end + start,
                        })?;
                    if !text.is_empty() {
                        parts.push(Part::Text(std::mem::take(&mut text)));
                    }
                    parts.push(Part::Formula(formula));
                }
                c => text.push(c),
            }
        }
        if !text.is_empty() {
            parts.push(Part::Text(text));
        }
        Ok(Template { parts })
    }

    /// The text for every row; `None` where it comes out empty. `format`
    /// writes values (e.g. numbers with thousands separators).
    pub fn render(&self, table: &Table, format: &dyn Fn(&Val) -> String) -> Vec<Option<String>> {
        let mut out = vec![String::new(); table.rows()];
        for part in &self.parts {
            match part {
                Part::Text(t) => out.iter_mut().for_each(|o| o.push_str(t)),
                Part::Formula(f) => {
                    for (o, v) in out.iter_mut().zip(f.evaluate(table)) {
                        if v != Val::Null {
                            o.push_str(&format(&v));
                        }
                    }
                }
            }
        }
        out.into_iter()
            .map(|o| {
                let o = o.trim();
                (!o.is_empty()).then(|| o.to_owned())
            })
            .collect()
    }
}

/// Recomputes the calculated columns of a table in order, e.g. after the
/// columns they use changed. Formulas that no longer work are left as
/// they were; their names are returned.
pub fn recalculate(table: &mut Table) -> Vec<String> {
    let mut broken = Vec::new();
    for i in 0..table.columns.len() {
        let Some(source) = table.columns[i].formula.clone() else {
            continue;
        };
        match Formula::compile(&source, table) {
            Ok(f) => {
                let name = table.columns[i].name.clone();
                table.columns[i] = f.column(&name, &source, table);
            }
            Err(_) => broken.push(table.columns[i].name.clone()),
        }
    }
    broken
}

/// What a stretch of formula text is, for colouring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Highlight {
    Column,
    Text,
    Number,
    Function,
    Keyword,
    Plain,
}

/// Colouring for a formula, also while it is incomplete or wrong.
pub fn highlight(source: &str) -> Vec<(Range<usize>, Highlight)> {
    let mut out = Vec::new();
    let mut chars = source.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        let mut end = start + c.len_utf8();
        let until = |close: char, chars: &mut std::iter::Peekable<std::str::CharIndices>| {
            for (i, d) in chars.by_ref() {
                if d == close {
                    return i + d.len_utf8();
                }
            }
            source.len()
        };
        let kind = match c {
            '[' => {
                end = until(']', &mut chars);
                Highlight::Column
            }
            '"' | '\'' => {
                end = until(c, &mut chars);
                Highlight::Text
            }
            c if c.is_ascii_digit() => {
                while let Some(&(i, d)) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' {
                        end = i + 1;
                        chars.next();
                    } else {
                        break;
                    }
                }
                Highlight::Number
            }
            c if c.is_alphabetic() || c == '_' => {
                while let Some(&(i, d)) = chars.peek() {
                    if d.is_alphanumeric() || d == '_' {
                        end = i + d.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let word = source[start..end].to_lowercase();
                if source[end..].trim_start().starts_with('(') {
                    Highlight::Function
                } else if matches!(
                    word.as_str(),
                    "and" | "or" | "not" | "null" | "true" | "false" | "yes" | "no"
                ) {
                    Highlight::Keyword
                } else {
                    Highlight::Column
                }
            }
            _ => Highlight::Plain,
        };
        out.push((start..end, kind));
    }
    out
}

/// What can be typed at the cursor.
#[derive(Debug, Clone, PartialEq)]
pub struct Completions {
    /// The text the suggestions replace.
    pub range: Range<usize>,
    pub items: Vec<Suggestion>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub label: String,
    pub insert: String,
    pub detail: String,
}

/// Columns after `[`, functions and columns for a word being typed.
pub fn completions(source: &str, cursor: usize, table: &Table) -> Completions {
    let cursor = cursor.min(source.len());
    let before = &source[..cursor];
    let column = |c: &Column| Suggestion {
        label: format!("[{}]", c.name),
        insert: format!("[{}]", c.name.replace(']', "]]")),
        detail: c.kind.label().to_owned(),
    };
    // Inside an open bracket: column names.
    if let Some(open) = before.rfind('[')
        && !before[open..].contains(']')
        && before[..open].matches('"').count().is_multiple_of(2)
    {
        let typed = before[open + 1..].to_lowercase();
        let end = source[cursor..]
            .find(']')
            .map_or(cursor, |i| cursor + i + 1);
        let items = table
            .columns
            .iter()
            .filter(|c| c.name.to_lowercase().contains(typed.trim()))
            .map(column)
            .collect();
        return Completions {
            range: open..end,
            items,
        };
    }
    if before.matches('"').count() % 2 == 1 {
        return Completions {
            range: cursor..cursor,
            items: Vec::new(),
        };
    }
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last()
        .map_or(cursor, |(i, _)| i);
    let word = before[start..].to_lowercase();
    if word.is_empty() || word.starts_with(|c: char| c.is_ascii_digit()) {
        return Completions {
            range: cursor..cursor,
            items: Vec::new(),
        };
    }
    let mut items: Vec<Suggestion> = FUNCTIONS
        .iter()
        .filter(|f| f.name.starts_with(&word))
        .map(|f| Suggestion {
            label: f.signature.to_owned(),
            insert: format!("{}(", f.name),
            detail: f.doc.to_owned(),
        })
        .collect();
    items.extend(
        table
            .columns
            .iter()
            .filter(|c| c.name.to_lowercase().contains(&word))
            .map(column),
    );
    Completions {
        range: start..cursor,
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use territoria_data::date;

    fn table() -> Table {
        Table::new(vec![
            Column::text(
                "Code",
                ColumnKind::Key,
                ["0811", "8001", "3011", "8002"]
                    .map(|s| Some(s.to_owned()))
                    .to_vec(),
            ),
            Column::text(
                "Name",
                ColumnKind::Text,
                vec![
                    Some("Zürich Nord".into()),
                    Some("Bern".into()),
                    None,
                    Some("zürich süd".into()),
                ],
            ),
            Column::number("Revenue", vec![Some(100.0), Some(300.0), Some(600.0), None]),
            Column::number(
                "Customers",
                vec![Some(4.0), Some(0.0), Some(12.0), Some(2.0)],
            ),
            Column::text(
                "Region",
                ColumnKind::Text,
                ["A", "B", "A", "B"].map(|s| Some(s.to_owned())).to_vec(),
            ),
            Column {
                name: "Since".into(),
                kind: ColumnKind::Date,
                data: ColumnData::Date(vec![
                    date::from_ymd(2024, 1, 31),
                    date::from_ymd(2025, 6, 1),
                    None,
                    date::from_ymd(2026, 2, 28),
                ]),
                formula: None,
            },
        ])
        .unwrap()
    }

    fn eval(src: &str) -> Vec<Val> {
        let t = table();
        Formula::compile(src, &t)
            .unwrap_or_else(|e| panic!("{src}: {e}"))
            .evaluate(&t)
    }

    fn nums(src: &str) -> Vec<Option<f64>> {
        eval(src).iter().map(Val::as_num).collect()
    }

    fn texts(src: &str) -> Vec<Option<String>> {
        eval(src).iter().map(Val::text).collect()
    }

    fn error(src: &str) -> Error {
        Formula::compile(src, &table()).map(|_| ()).expect_err(src)
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(
            nums("[Revenue] / [Customers]"),
            vec![Some(25.0), None, Some(50.0), None]
        );
        assert_eq!(nums("1 + 2 * 3 ^ 2")[0], Some(19.0));
        assert_eq!(nums("-2 ^ 2")[0], Some(-4.0));
        assert_eq!(nums("2 ^ 3 ^ 2")[0], Some(512.0));
        assert_eq!(nums("(1 + 2) * 3")[0], Some(9.0));
        assert_eq!(nums("7 % 3")[0], Some(1.0));
        assert_eq!(nums("1.5e3")[0], Some(1500.0));
        assert_eq!(nums("Revenue * 2")[1], Some(600.0), "bare column names");
    }

    #[test]
    fn text_functions() {
        assert_eq!(texts("left([Code], 2)")[0].as_deref(), Some("08"));
        assert_eq!(
            texts("[Code] & \" \" & [Name]")[2].as_deref(),
            Some("3011 ")
        );
        assert_eq!(texts("pad(\"42\", 5)")[0].as_deref(), Some("00042"));
        assert_eq!(texts("upper(mid([Name], 2, 3))")[0].as_deref(), Some("ÜRI"));
        assert_eq!(
            texts("split(\"a; b; c\", \";\", 2)")[0].as_deref(),
            Some("b")
        );
        assert_eq!(
            texts("replace([Name], \"Zürich\", \"ZH\")")[0].as_deref(),
            Some("ZH Nord")
        );
        assert_eq!(nums("find([Name], \"NORD\")")[0], Some(8.0));
        assert_eq!(
            eval("like([Name], \"z*rich*\")"),
            vec![
                Val::Bool(true),
                Val::Bool(false),
                Val::Null,
                Val::Bool(true)
            ]
        );
        assert_eq!(
            eval("[Name] = \"BERN\"")[1],
            Val::Bool(true),
            "text compares ignoring case"
        );
        assert_eq!(
            texts("\"say \"\"hi\"\"\"")[0].as_deref(),
            Some("say \"hi\"")
        );
    }

    #[test]
    fn logic_and_nulls() {
        assert_eq!(
            texts("if([Revenue] > 200, \"big\", \"small\")"),
            vec![
                Some("small".into()),
                Some("big".into()),
                Some("big".into()),
                Some("small".into())
            ]
        );
        assert_eq!(nums("ifnull([Revenue], 0)")[3], Some(0.0));
        assert_eq!(eval("isnull([Name])")[2], Val::Bool(true));
        assert_eq!(
            eval("[Revenue] > 50 and [Customers] > 0"),
            vec![
                Val::Bool(true),
                Val::Bool(false),
                Val::Bool(true),
                Val::Null
            ]
        );
        assert_eq!(
            eval("[Revenue] > 50 or [Customers] > 0")[3],
            Val::Bool(true)
        );
        assert_eq!(
            texts("switch([Region], \"A\", \"North\", \"South\")"),
            vec![
                Some("North".into()),
                Some("South".into()),
                Some("North".into()),
                Some("South".into())
            ]
        );
        assert_eq!(
            nums("max([Revenue], [Customers] * 100)"),
            vec![Some(400.0), Some(300.0), Some(1200.0), Some(200.0)]
        );
        assert_eq!(eval("not [Customers] > 3")[0], Val::Bool(false));
    }

    #[test]
    fn column_functions() {
        assert_eq!(nums("sum([Revenue])")[0], Some(1000.0));
        assert_eq!(
            nums("share([Revenue])"),
            vec![Some(0.1), Some(0.3), Some(0.6), None]
        );
        assert_eq!(
            nums("sum([Revenue], [Region])"),
            vec![Some(700.0), Some(300.0), Some(700.0), Some(300.0)]
        );
        assert_eq!(
            nums("rank([Customers])"),
            vec![Some(2.0), Some(4.0), Some(1.0), Some(3.0)]
        );
        assert_eq!(nums("count([Revenue])")[0], Some(3.0));
        assert_eq!(nums("count()")[0], Some(4.0));
        assert_eq!(nums("median([Customers])")[0], Some(3.0));
        assert_eq!(nums("mean([Revenue], [Region])")[1], Some(300.0));
        assert_eq!(
            nums("percentile([Customers])"),
            vec![Some(2.0 / 3.0), Some(0.0), Some(1.0), Some(1.0 / 3.0)]
        );
        assert_eq!(nums("max([Revenue])")[3], Some(600.0));
        assert_eq!(nums("[Revenue] / sum([Revenue]) * 100")[2], Some(60.0));
    }

    #[test]
    fn dates() {
        assert_eq!(nums("year([Since])")[0], Some(2024.0));
        assert_eq!(
            texts("dateadd([Since], 1, \"month\")")[0].as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(
            nums("datediff([Since], date(2026, 1, 31), \"years\")")[0],
            Some(2.0)
        );
        assert_eq!(
            nums("datediff([Since], date(2026, 1, 30), \"months\")")[0],
            Some(23.0)
        );
        assert_eq!(eval("[Since] >= \"1.1.2025\"")[1], Val::Bool(true));
        assert_eq!(
            nums("weekday(date(\"2026-10-23\"))")[0],
            Some(5.0),
            "a Friday"
        );
        assert_eq!(
            texts("format([Since], \"DD.MM.YYYY\")")[3].as_deref(),
            Some("28.02.2026")
        );
        assert_eq!(nums("[Since] - date(2024, 1, 1)")[0], Some(30.0));
        assert_eq!(texts("[Since] + 1")[0].as_deref(), Some("2024-02-01"));
    }

    #[test]
    fn conversions_and_places() {
        assert_eq!(nums("num([Code]) + 1")[1], Some(8002.0));
        assert_eq!(nums("num(\"1'234.5\")")[0], Some(1234.5));
        assert_eq!(nums("num(\"12,5\")")[0], Some(12.5));
        assert_eq!(texts("format(2 / 3, 2)")[0].as_deref(), Some("0.67"));
        assert_eq!(nums("round(1234.567, -2)")[0], Some(1200.0));
        let d = nums("distance(47.3769, 8.5417, 46.948, 7.4474)")[0].unwrap();
        assert!((d - 95.5).abs() < 1.0, "{d} km Zürich–Bern");
        let lat = nums("dms(\"47°22'30\"\"N\")")[0].unwrap();
        assert!((lat - 47.375).abs() < 1e-9, "{lat}");
        assert_eq!(nums("dms(\"8 30 0 W\")")[0], Some(-8.5));
    }

    #[test]
    fn errors_point_at_the_mistake() {
        let e = error("[Revenu] * 2");
        assert_eq!(e.span, 0..8);
        assert!(
            e.message.contains("Did you mean [Revenue]"),
            "{}",
            e.message
        );
        let e = error("[Code] + 1");
        assert!(e.message.contains("num("), "{}", e.message);
        let e = error("[Name] + \" x\"");
        assert!(e.message.contains("&"), "{}", e.message);
        let e = error("round([Revenue], 2");
        assert!(e.message.contains("not closed"), "{}", e.message);
        let e = error("roudn([Revenue])");
        assert!(e.message.contains("round"), "{}", e.message);
        assert_eq!(e.span, 0..5);
        let e = error("[Revenue] [Customers]");
        assert_eq!(e.span.start, 10);
        let e = error("if([Revenue], 1, 2)");
        assert!(e.message.contains("condition"), "{}", e.message);
        let e = error("[Revenue] * ");
        assert!(e.message.contains("missing"), "{}", e.message);
        let e = error("Umsatz * 2");
        assert!(e.message.contains("[Umsatz]"), "{}", e.message);
        assert!(error("left([Name])").message.contains("left(text, n)"));
        assert!(error("[Since] > \"soon\"").message.contains("not a date"));
    }

    #[test]
    fn columns_and_recalculation() {
        let mut t = table();
        let f = Formula::compile("[Revenue] / [Customers]", &t).unwrap();
        assert_eq!(f.ty(), Type::Number);
        t.push_column(f.column("Per customer", "[Revenue] / [Customers]", &t))
            .unwrap();
        assert_eq!(t.column("Per customer").unwrap().f64(0), Some(25.0));
        if let ColumnData::Number(v) = &mut t.columns[2].data {
            v[0] = Some(200.0);
        }
        assert!(recalculate(&mut t).is_empty());
        assert_eq!(t.column("Per customer").unwrap().f64(0), Some(50.0));
        let c = Formula::compile("[Name] = \"Bern\"", &t)
            .unwrap()
            .column("Is Bern", "", &t);
        assert_eq!(c.kind, ColumnKind::Bool);
    }

    #[test]
    fn templates() {
        let t = table();
        let tpl = Template::compile("{Name}: {[Revenue] / 100} {{x}}", &t).unwrap();
        let out = tpl.render(&t, &|v| v.text().unwrap_or_default());
        assert_eq!(out[0].as_deref(), Some("Zürich Nord: 1 {x}"));
        assert_eq!(
            out[2].as_deref(),
            Some(": 6 {x}"),
            "empty values leave nothing"
        );
        let tpl = Template::compile("{Name}", &t).unwrap();
        assert_eq!(tpl.render(&t, &|v| v.text().unwrap_or_default())[2], None);
        let tpl = Template::compile("{if([Revenue] > 200, \"}\", \"-\")}", &t).unwrap();
        assert_eq!(
            tpl.render(&t, &|v| v.text().unwrap_or_default())[1].as_deref(),
            Some("}")
        );
        let e = Template::compile("Total {[Revenu]}", &t).unwrap_err();
        assert_eq!(e.span, 7..15, "positions point into the template");
        assert!(
            Template::compile("{Name", &t)
                .unwrap_err()
                .message
                .contains("not closed")
        );
        assert!(Template::compile("a } b", &t).is_err());
    }

    #[test]
    fn highlighting() {
        let h = highlight("round([Revenue] / 2, 1) & \"x\" and");
        let kinds: Vec<Highlight> = h
            .iter()
            .map(|(_, k)| *k)
            .filter(|k| *k != Highlight::Plain)
            .collect();
        assert_eq!(
            kinds,
            vec![
                Highlight::Function,
                Highlight::Column,
                Highlight::Number,
                Highlight::Number,
                Highlight::Text,
                Highlight::Keyword
            ]
        );
        assert_eq!(h[2].0, 6..15);
        // Unclosed brackets run to the end.
        assert_eq!(highlight("[Reve").last().unwrap().0, 0..5);
        let t = table();
        let f = Formula::compile("[Revenue] / [Customers] + [Revenue]", &t).unwrap();
        assert_eq!(f.columns(), vec![2, 3]);
    }

    #[test]
    fn autocomplete() {
        let t = table();
        let c = completions("[Rev", 4, &t);
        assert_eq!(c.range, 0..4);
        assert_eq!(c.items[0].insert, "[Revenue]");
        let c = completions("ro", 2, &t);
        assert_eq!(c.items[0].insert, "round(");
        assert_eq!(c.range, 0..2);
        let c = completions("1 + cust", 8, &t);
        assert_eq!(c.items[0].insert, "[Customers]");
        assert!(
            completions("\"ro", 3, &t).items.is_empty(),
            "nothing inside text"
        );
    }
}
