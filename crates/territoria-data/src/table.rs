//! Typed, column-oriented tables.

use serde::{Deserialize, Serialize};

use crate::date;

/// What a column means; decides formatting, sorting and what it can be
/// used for. Storage follows from it (see [`ColumnData`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColumnKind {
    Text,
    /// An identifier stored as text: postcodes, area codes, customer
    /// numbers. Leading zeros survive; never summed.
    Key,
    Number,
    Integer,
    Bool,
    Date,
}

impl ColumnKind {
    pub const ALL: [ColumnKind; 6] = [
        ColumnKind::Text,
        ColumnKind::Key,
        ColumnKind::Number,
        ColumnKind::Integer,
        ColumnKind::Bool,
        ColumnKind::Date,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ColumnKind::Text => "Text",
            ColumnKind::Key => "Key / code",
            ColumnKind::Number => "Number",
            ColumnKind::Integer => "Whole number",
            ColumnKind::Bool => "Yes/no",
            ColumnKind::Date => "Date",
        }
    }

    pub fn is_numeric(self) -> bool {
        matches!(self, ColumnKind::Number | ColumnKind::Integer)
    }

    pub fn is_textual(self) -> bool {
        matches!(self, ColumnKind::Text | ColumnKind::Key)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ColumnData {
    Text(Vec<Option<String>>),
    Number(Vec<Option<f64>>),
    Bool(Vec<Option<bool>>),
    /// Days since 1970-01-01.
    Date(Vec<Option<i32>>),
}

impl ColumnData {
    pub fn len(&self) -> usize {
        match self {
            ColumnData::Text(v) => v.len(),
            ColumnData::Number(v) => v.len(),
            ColumnData::Bool(v) => v.len(),
            ColumnData::Date(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn empty_for(kind: ColumnKind, len: usize) -> ColumnData {
        match kind {
            ColumnKind::Text | ColumnKind::Key => ColumnData::Text(vec![None; len]),
            ColumnKind::Number | ColumnKind::Integer => ColumnData::Number(vec![None; len]),
            ColumnKind::Bool => ColumnData::Bool(vec![None; len]),
            ColumnKind::Date => ColumnData::Date(vec![None; len]),
        }
    }
}

/// A single cell, borrowed from its table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    Null,
    Text(&'a str),
    Number(f64),
    Bool(bool),
    Date(i32),
}

impl Value<'_> {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
            Value::Date(d) => Some(f64::from(*d)),
            Value::Text(t) => crate::infer::parse_number(t, crate::infer::Decimal::Point),
            Value::Null => None,
        }
    }

    /// Text for keys and display, without any locale formatting.
    pub fn to_text(&self) -> Option<String> {
        match self {
            Value::Null => None,
            Value::Text(t) => Some((*t).to_owned()),
            Value::Number(n) => Some(format_number(*n)),
            Value::Bool(b) => Some(if *b { "true" } else { "false" }.into()),
            Value::Date(d) => Some(date::format(*d)),
        }
    }
}

/// Plain decimal without trailing zeros (`5`, `2.25`).
pub fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let s = format!("{n}");
        if s.contains('e') {
            format!("{n:.6}")
        } else {
            s
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub kind: ColumnKind,
    pub data: ColumnData,
    /// The formula of a calculated column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
}

impl Column {
    pub fn new(name: impl Into<String>, kind: ColumnKind, len: usize) -> Column {
        Column {
            name: name.into(),
            kind,
            data: ColumnData::empty_for(kind, len),
            formula: None,
        }
    }

    pub fn text(name: impl Into<String>, kind: ColumnKind, values: Vec<Option<String>>) -> Column {
        Column {
            name: name.into(),
            kind,
            data: ColumnData::Text(values),
            formula: None,
        }
    }

    pub fn number(name: impl Into<String>, values: Vec<Option<f64>>) -> Column {
        Column {
            name: name.into(),
            kind: ColumnKind::Number,
            data: ColumnData::Number(values),
            formula: None,
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn get(&self, row: usize) -> Value<'_> {
        match &self.data {
            ColumnData::Text(v) => v[row].as_deref().map_or(Value::Null, Value::Text),
            ColumnData::Number(v) => v[row].map_or(Value::Null, Value::Number),
            ColumnData::Bool(v) => v[row].map_or(Value::Null, Value::Bool),
            ColumnData::Date(v) => v[row].map_or(Value::Null, Value::Date),
        }
    }

    pub fn f64(&self, row: usize) -> Option<f64> {
        match &self.data {
            ColumnData::Number(v) => v[row],
            _ => self.get(row).as_f64(),
        }
    }

    /// The cell as text for display.
    pub fn display(&self, row: usize) -> String {
        match self.get(row) {
            Value::Number(n) if self.kind == ColumnKind::Integer => format!("{}", n.round() as i64),
            v => v.to_text().unwrap_or_default(),
        }
    }

    /// Sets a cell from text as it would be read for the column's kind;
    /// empty text empties it. `false` when the text doesn't fit the kind.
    pub fn set_text(&mut self, row: usize, text: &str) -> bool {
        if row >= self.len() {
            return false;
        }
        let t = text.trim();
        let value = (!t.is_empty()).then(|| t.to_owned());
        let converted = crate::infer::convert_texts(std::slice::from_ref(&value), self.kind);
        let ok = value.is_none() || converted.null_count_data() == 0;
        if !ok {
            return false;
        }
        match (&mut self.data, converted) {
            (ColumnData::Text(v), ColumnData::Text(c)) => v[row] = c[0].clone(),
            (ColumnData::Number(v), ColumnData::Number(c)) => v[row] = c[0],
            (ColumnData::Bool(v), ColumnData::Bool(c)) => v[row] = c[0],
            (ColumnData::Date(v), ColumnData::Date(c)) => v[row] = c[0],
            _ => return false,
        }
        true
    }

    fn push_empty(&mut self) {
        match &mut self.data {
            ColumnData::Text(v) => v.push(None),
            ColumnData::Number(v) => v.push(None),
            ColumnData::Bool(v) => v.push(None),
            ColumnData::Date(v) => v.push(None),
        }
    }

    pub fn null_count(&self) -> usize {
        (0..self.len()).filter(|&r| self.get(r).is_null()).count()
    }

    /// Converts to another kind; values that don't convert become empty.
    /// Returns how many non-empty values were lost.
    pub fn convert(&mut self, kind: ColumnKind) -> usize {
        if kind == self.kind {
            return 0;
        }
        let texts: Vec<Option<String>> = (0..self.len())
            .map(|r| match self.get(r) {
                Value::Number(n) if self.kind == ColumnKind::Integer => {
                    Some(format!("{}", n.round() as i64))
                }
                v => v.to_text(),
            })
            .collect();
        let before = texts.iter().filter(|t| t.is_some()).count();
        let converted = crate::infer::convert_texts(&texts, kind);
        let after = converted.len() - converted.null_count_data();
        self.kind = kind;
        self.data = converted;
        before.saturating_sub(after)
    }
}

impl ColumnData {
    fn null_count_data(&self) -> usize {
        match self {
            ColumnData::Text(v) => v.iter().filter(|x| x.is_none()).count(),
            ColumnData::Number(v) => v.iter().filter(|x| x.is_none()).count(),
            ColumnData::Bool(v) => v.iter().filter(|x| x.is_none()).count(),
            ColumnData::Date(v) => v.iter().filter(|x| x.is_none()).count(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub columns: Vec<Column>,
    rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LengthMismatch {
    pub column: String,
    pub len: usize,
    pub expected: usize,
}

impl std::fmt::Display for LengthMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "column `{}` has {} rows, the table {}",
            self.column, self.len, self.expected
        )
    }
}

impl std::error::Error for LengthMismatch {}

impl Table {
    pub fn new(columns: Vec<Column>) -> Result<Table, LengthMismatch> {
        let rows = columns.first().map_or(0, Column::len);
        if let Some(c) = columns.iter().find(|c| c.len() != rows) {
            return Err(LengthMismatch {
                column: c.name.clone(),
                len: c.len(),
                expected: rows,
            });
        }
        Ok(Table { columns, rows })
    }

    /// A table with rows but no columns yet.
    pub fn with_rows(rows: usize) -> Table {
        Table {
            columns: Vec::new(),
            rows,
        }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    pub fn push_column(&mut self, column: Column) -> Result<(), LengthMismatch> {
        if self.columns.is_empty() && self.rows == 0 {
            self.rows = column.len();
        }
        if column.len() != self.rows {
            return Err(LengthMismatch {
                len: column.len(),
                column: column.name,
                expected: self.rows,
            });
        }
        self.columns.push(column);
        Ok(())
    }

    /// Adds an empty row at the end; returns its index.
    pub fn push_row(&mut self) -> usize {
        for c in &mut self.columns {
            c.push_empty();
        }
        self.rows += 1;
        self.rows - 1
    }

    /// Removes rows (any order, duplicates ignored).
    pub fn remove_rows(&mut self, rows: &[usize]) {
        let drop: std::collections::HashSet<usize> = rows.iter().copied().collect();
        let keep: Vec<usize> = (0..self.rows).filter(|r| !drop.contains(r)).collect();
        let formulas: Vec<Option<String>> =
            self.columns.iter().map(|c| c.formula.clone()).collect();
        *self = self.take_rows(&keep);
        for (c, f) in self.columns.iter_mut().zip(formulas) {
            c.formula = f;
        }
    }

    /// A table of the given rows, in that order.
    pub fn take_rows(&self, rows: &[usize]) -> Table {
        let columns = self
            .columns
            .iter()
            .map(|c| Column {
                name: c.name.clone(),
                kind: c.kind,
                data: match &c.data {
                    ColumnData::Text(v) => {
                        ColumnData::Text(rows.iter().map(|&r| v[r].clone()).collect())
                    }
                    ColumnData::Number(v) => {
                        ColumnData::Number(rows.iter().map(|&r| v[r]).collect())
                    }
                    ColumnData::Bool(v) => ColumnData::Bool(rows.iter().map(|&r| v[r]).collect()),
                    ColumnData::Date(v) => ColumnData::Date(rows.iter().map(|&r| v[r]).collect()),
                },
                formula: None,
            })
            .collect();
        Table {
            columns,
            rows: rows.len(),
        }
    }

    /// Row groups with the same text in `column` (first-seen order); empty
    /// cells stay rows of their own.
    pub fn groups_by(&self, column: usize, prefix: Option<usize>) -> Vec<Vec<usize>> {
        let c = &self.columns[column];
        let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for r in 0..self.rows {
            let key = match c.get(r) {
                Value::Number(n) if c.kind == ColumnKind::Integer => {
                    Some(format!("{}", n.round() as i64))
                }
                v => v.to_text(),
            }
            .map(|k| match prefix {
                Some(n) => k.chars().take(n).collect(),
                None => k,
            });
            match key {
                Some(k) => match index.get(&k) {
                    Some(&g) => groups[g].push(r),
                    None => {
                        index.insert(k, groups.len());
                        groups.push(vec![r]);
                    }
                },
                None => groups.push(vec![r]),
            }
        }
        groups
    }

    /// A column name not used yet, based on `base`.
    pub fn unique_name(&self, base: &str) -> String {
        if self.column(base).is_none() {
            return base.to_owned();
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|n| self.column(n).is_none())
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_checks_lengths() {
        let t = Table::new(vec![
            Column::text("k", ColumnKind::Key, vec![Some("01".into()), None]),
            Column::number("v", vec![Some(1.0), Some(2.5)]),
        ])
        .unwrap();
        assert_eq!(t.rows(), 2);
        assert_eq!(t.column("k").unwrap().display(0), "01");
        assert_eq!(t.column("v").unwrap().display(1), "2.5");
        assert!(
            Table::new(vec![
                Column::number("a", vec![None]),
                Column::number("b", vec![])
            ])
            .is_err()
        );
        assert_eq!(t.unique_name("v"), "v 2");
    }

    #[test]
    fn groups_and_takes_rows() {
        let t = Table::new(vec![
            Column::text(
                "k",
                ColumnKind::Key,
                vec![
                    Some("8001".into()),
                    Some("8002".into()),
                    Some("8001".into()),
                    None,
                ],
            ),
            Column::number("v", vec![Some(1.0), Some(2.0), Some(3.0), Some(4.0)]),
        ])
        .unwrap();
        assert_eq!(t.groups_by(0, None), vec![vec![0, 2], vec![1], vec![3]]);
        assert_eq!(t.groups_by(0, Some(2)), vec![vec![0, 1, 2], vec![3]]);
        let s = t.take_rows(&[2, 1]);
        assert_eq!(s.rows(), 2);
        assert_eq!(s.column("v").unwrap().f64(0), Some(3.0));
    }

    #[test]
    fn convert_numbers_to_keys_and_back() {
        let mut c = Column::number("plz", vec![Some(8001.0), None, Some(3000.0)]);
        c.kind = ColumnKind::Integer;
        assert_eq!(c.convert(ColumnKind::Key), 0);
        assert_eq!(c.get(0), Value::Text("8001"));
        let mut t = Column::text(
            "x",
            ColumnKind::Text,
            vec![Some("1.5".into()), Some("abc".into())],
        );
        assert_eq!(t.convert(ColumnKind::Number), 1);
        assert_eq!(t.get(0), Value::Number(1.5));
        assert_eq!(t.get(1), Value::Null);
    }

    #[test]
    fn edit_cells_and_rows() {
        let mut t = Table::new(vec![
            Column::text(
                "Code",
                ColumnKind::Key,
                vec![Some("01".into()), Some("02".into())],
            ),
            Column::number("Sales", vec![Some(1.0), Some(2.0)]),
        ])
        .unwrap();
        assert!(t.columns[1].set_text(0, "1'500.5"));
        assert_eq!(t.columns[1].f64(0), Some(1500.5));
        assert!(!t.columns[1].set_text(0, "lots"), "not a number");
        assert!(t.columns[0].set_text(1, "007"));
        assert_eq!(t.columns[0].display(1), "007", "keys stay text");
        let r = t.push_row();
        assert_eq!((r, t.rows()), (2, 3));
        assert!(t.columns[1].set_text(2, ""));
        t.remove_rows(&[0]);
        assert_eq!(t.rows(), 2);
        assert_eq!(t.columns[0].display(0), "007");
    }
}
