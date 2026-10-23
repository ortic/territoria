//! Column types from text: what CSV files and spreadsheets give us.
//!
//! The guesses lean towards keeping information: digit strings with leading
//! zeros or a code-like column name become keys (text), not numbers.

use crate::date::{self, DayOrder};
use crate::table::{Column, ColumnData, ColumnKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decimal {
    /// `1,234.5`
    Point,
    /// `1.234,5` and `1'234,5`
    Comma,
}

/// Parses a number with the given decimal separator. Thousands separators
/// (`,` `.` `'` `’` space), a leading `+`, currency symbols and a trailing
/// `%` are accepted (the percent sign is ignored: `12%` is 12).
pub fn parse_number(text: &str, decimal: Decimal) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let t = t
        .trim_start_matches(['€', '$', '£', '¥'])
        .trim_end_matches(['€', '$', '£', '¥', '%'])
        .trim();
    let t = t
        .strip_prefix("CHF")
        .or_else(|| t.strip_suffix("CHF"))
        .or_else(|| t.strip_prefix("EUR"))
        .or_else(|| t.strip_suffix("EUR"))
        .unwrap_or(t)
        .trim();
    let (dec, thousands) = match decimal {
        Decimal::Point => ('.', ','),
        Decimal::Comma => (',', '.'),
    };
    let mut out = String::with_capacity(t.len());
    let mut seen_dec = false;
    let mut digits_since_group: Option<usize> = None;
    for (i, ch) in t.chars().enumerate() {
        match ch {
            '0'..='9' => {
                out.push(ch);
                if let Some(n) = digits_since_group.as_mut() {
                    *n += 1;
                }
            }
            '-' | '+' if i == 0 => out.push(ch),
            'e' | 'E' if !out.is_empty() => out.push('e'),
            '-' | '+' if out.ends_with('e') => out.push(ch),
            c if c == dec && !seen_dec => {
                if digits_since_group.is_some_and(|n| n != 3) {
                    return None;
                }
                seen_dec = true;
                digits_since_group = None;
                out.push('.');
            }
            c if (c == thousands || c == '\'' || c == '’' || c == ' ' || c == '\u{a0}')
                && !seen_dec
                && !out.is_empty() =>
            {
                // Groups after a thousands separator have three digits.
                if digits_since_group.is_some_and(|n| n != 3) {
                    return None;
                }
                digits_since_group = Some(0);
            }
            _ => return None,
        }
    }
    if digits_since_group.is_some_and(|n| n != 3) {
        return None;
    }
    out.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Picks the decimal separator that explains a column's values best.
pub fn detect_decimal<'a>(values: impl IntoIterator<Item = &'a str>) -> Decimal {
    let (mut point, mut comma) = (0usize, 0usize);
    for v in values {
        let v = v.trim();
        let (p, c) = (v.rfind('.'), v.rfind(','));
        match (p, c) {
            // Whichever comes last is the decimal separator.
            (Some(p), Some(c)) if p > c => point += 1,
            (Some(_), Some(_)) => comma += 1,
            (Some(p), None) => {
                // `1.234` alone is ambiguous; other digit counts aren't.
                let after = v.len() - p - 1;
                if after != 3
                    || v.matches('.').count() == 1 && v[..p].trim_start_matches('-') == "0"
                {
                    point += 1;
                } else if v.matches('.').count() > 1 {
                    comma += 1;
                }
            }
            (None, Some(c)) => {
                let after = v.len() - c - 1;
                if after != 3 || v[..c].trim_start_matches('-') == "0" {
                    comma += 1;
                } else if v.matches(',').count() > 1 {
                    point += 1;
                }
            }
            (None, None) => {}
        }
    }
    if comma > point {
        Decimal::Comma
    } else {
        Decimal::Point
    }
}

fn detect_day_order<'a>(values: impl IntoIterator<Item = &'a str>) -> DayOrder {
    for v in values {
        let parts: Vec<&str> = v.trim().split('/').collect();
        if parts.len() == 3 {
            let a: u32 = parts[0].parse().unwrap_or(0);
            let b: u32 = parts[1].parse().unwrap_or(0);
            if a > 12 {
                return DayOrder::DayMonth;
            }
            if b > 12 {
                return DayOrder::MonthDay;
            }
        }
    }
    DayOrder::DayMonth
}

fn parse_bool(text: &str) -> Option<bool> {
    match text.trim().to_lowercase().as_str() {
        "true" | "yes" | "y" | "ja" | "wahr" | "oui" | "vrai" | "si" | "sì" | "x" => Some(true),
        "false" | "no" | "n" | "nein" | "falsch" | "non" | "faux" => Some(false),
        _ => None,
    }
}

/// Column names that suggest identifiers rather than quantities.
pub fn name_suggests_key(name: &str) -> bool {
    let n: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    const WORDS: &[&str] = &[
        "id",
        "key",
        "code",
        "nr",
        "no",
        "number",
        "nummer",
        "num",
        "plz",
        "zip",
        "zipcode",
        "postcode",
        "postal",
        "postalcode",
        "postleitzahl",
        "cp",
        "cap",
        "npa",
        "bfs",
        "gkz",
        "ags",
        "gmd",
        "fips",
        "iso",
        "nuts",
        "lau",
        "ref",
        "kundennr",
        "kdnr",
        "gemeindenummer",
    ];
    n.split_whitespace().any(|w| {
        WORDS.contains(&w)
            || w.ends_with("id")
            || w.ends_with("nr")
            || w.ends_with("code")
            || w.ends_with("nummer")
            || w.ends_with("plz")
            || w.starts_with("plz")
            || w.starts_with("zip")
    })
}

/// Settings for [`infer_column`]; `None` means detect.
#[derive(Debug, Clone, Copy, Default)]
pub struct InferOptions {
    pub decimal: Option<Decimal>,
}

/// Guesses a column's kind from its text values and converts them.
pub fn infer_column(name: &str, values: Vec<Option<String>>, options: InferOptions) -> Column {
    let kind = infer_kind(name, &values, options);
    let data = convert_with(&values, kind, options);
    Column {
        name: name.to_owned(),
        kind,
        data,
        formula: None,
    }
}

pub fn infer_kind(name: &str, values: &[Option<String>], options: InferOptions) -> ColumnKind {
    let present: Vec<&str> = values
        .iter()
        .filter_map(|v| v.as_deref())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect();
    if present.is_empty() {
        return ColumnKind::Text;
    }
    let digits_only = present
        .iter()
        .all(|v| v.bytes().all(|b| b.is_ascii_digit()));
    let leading_zero = present.iter().any(|v| v.len() > 1 && v.starts_with('0'));
    let key_name = name_suggests_key(name);
    if digits_only && (leading_zero || key_name) {
        return ColumnKind::Key;
    }
    if present.iter().all(|v| parse_bool(v).is_some()) && !digits_only {
        return ColumnKind::Bool;
    }
    let decimal = options
        .decimal
        .unwrap_or_else(|| detect_decimal(present.iter().copied()));
    if present.iter().all(|v| parse_number(v, decimal).is_some()) {
        let integral = present
            .iter()
            .all(|v| parse_number(v, decimal).is_some_and(|n| n.fract() == 0.0));
        return if integral {
            ColumnKind::Integer
        } else {
            ColumnKind::Number
        };
    }
    let order = detect_day_order(present.iter().copied());
    if present.iter().all(|v| date::parse(v, order).is_some()) {
        return ColumnKind::Date;
    }
    // Short codes without spaces (`SW1A`, `D-80331`, `CH-4051`) in a column
    // named like a key.
    if key_name && present.iter().all(|v| v.len() <= 24 && !v.contains(' ')) {
        return ColumnKind::Key;
    }
    ColumnKind::Text
}

pub fn convert_texts(values: &[Option<String>], kind: ColumnKind) -> ColumnData {
    convert_with(values, kind, InferOptions::default())
}

fn convert_with(values: &[Option<String>], kind: ColumnKind, options: InferOptions) -> ColumnData {
    let present = || {
        values
            .iter()
            .filter_map(|v| v.as_deref())
            .filter(|v| !v.trim().is_empty())
    };
    match kind {
        ColumnKind::Text => ColumnData::Text(
            values
                .iter()
                .map(|v| v.as_ref().filter(|s| !s.is_empty()).cloned())
                .collect(),
        ),
        ColumnKind::Key => ColumnData::Text(
            values
                .iter()
                .map(|v| {
                    v.as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                })
                .collect(),
        ),
        ColumnKind::Number | ColumnKind::Integer => {
            let decimal = options.decimal.unwrap_or_else(|| detect_decimal(present()));
            ColumnData::Number(
                values
                    .iter()
                    .map(|v| v.as_deref().and_then(|s| parse_number(s, decimal)))
                    .collect(),
            )
        }
        ColumnKind::Bool => ColumnData::Bool(
            values
                .iter()
                .map(|v| {
                    v.as_deref().and_then(|s| {
                        parse_bool(s).or(match s.trim() {
                            "1" => Some(true),
                            "0" => Some(false),
                            _ => None,
                        })
                    })
                })
                .collect(),
        ),
        ColumnKind::Date => {
            let order = detect_day_order(present());
            ColumnData::Date(
                values
                    .iter()
                    .map(|v| v.as_deref().and_then(|s| date::parse(s, order)))
                    .collect(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(values: &[&str]) -> Vec<Option<String>> {
        values
            .iter()
            .map(|v| (!v.is_empty()).then(|| v.to_string()))
            .collect()
    }

    #[test]
    fn numbers_in_both_conventions() {
        assert_eq!(parse_number("1,234.5", Decimal::Point), Some(1234.5));
        assert_eq!(parse_number("1.234,5", Decimal::Comma), Some(1234.5));
        assert_eq!(parse_number("1'234'567", Decimal::Point), Some(1_234_567.0));
        assert_eq!(parse_number("-0,25", Decimal::Comma), Some(-0.25));
        assert_eq!(parse_number("CHF 12.50", Decimal::Point), Some(12.5));
        assert_eq!(parse_number("12%", Decimal::Point), Some(12.0));
        assert_eq!(parse_number("1e3", Decimal::Point), Some(1000.0));
        assert_eq!(parse_number("12,34,5", Decimal::Point), None);
        assert_eq!(parse_number("abc", Decimal::Point), None);
        assert_eq!(parse_number("", Decimal::Point), None);
    }

    #[test]
    fn detects_decimal_separator() {
        assert_eq!(detect_decimal(["1,5", "2,25", "3"]), Decimal::Comma);
        assert_eq!(detect_decimal(["1.5", "2.25", "3"]), Decimal::Point);
        assert_eq!(detect_decimal(["1.234,5"]), Decimal::Comma);
        assert_eq!(detect_decimal(["1,234.5"]), Decimal::Point);
        assert_eq!(detect_decimal(["0,123"]), Decimal::Comma);
    }

    #[test]
    fn kinds() {
        let k = |name: &str, v: &[&str]| infer_kind(name, &col(v), InferOptions::default());
        assert_eq!(k("PLZ", &["8001", "3000", ""]), ColumnKind::Key);
        assert_eq!(k("zip", &["01067", "10115"]), ColumnKind::Key);
        assert_eq!(k("anything", &["01067", "10115"]), ColumnKind::Key);
        assert_eq!(k("Sales", &["8001", "3000"]), ColumnKind::Integer);
        assert_eq!(k("Sales", &["1'200,50", "3,1"]), ColumnKind::Number);
        assert_eq!(k("Active", &["yes", "no", ""]), ColumnKind::Bool);
        assert_eq!(k("Since", &["2024-01-31", "31.12.2023"]), ColumnKind::Date);
        assert_eq!(k("Postcode", &["SW1A 1AA", "EC1A"]), ColumnKind::Text);
        assert_eq!(k("Postcode", &["SW1A", "EC1A"]), ColumnKind::Key);
        assert_eq!(k("Name", &["Zürich", "Bern"]), ColumnKind::Text);
    }

    #[test]
    fn infer_converts() {
        let c = infer_column(
            "Umsatz",
            col(&["1.200,5", "", "7"]),
            InferOptions::default(),
        );
        assert_eq!(c.kind, ColumnKind::Number);
        assert_eq!(
            c.data,
            ColumnData::Number(vec![Some(1200.5), None, Some(7.0)])
        );
    }
}
