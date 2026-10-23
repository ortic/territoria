//! Guesses about tables that save the user a step: which column identifies
//! rows, which one names them, which one to match against a layer.

use std::collections::HashSet;

use crate::infer::name_suggests_key;
use crate::keys::{self, Matching};
use crate::table::{ColumnKind, Table};

/// The column that best identifies rows: unique, rarely empty, a key or
/// whole number, preferably named like one.
pub fn key_column(table: &Table) -> Option<usize> {
    let rows = table.rows();
    if rows == 0 {
        return None;
    }
    table
        .columns
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            if !matches!(
                c.kind,
                ColumnKind::Key | ColumnKind::Integer | ColumnKind::Text
            ) {
                return None;
            }
            let values: Vec<String> = (0..rows).filter_map(|r| c.get(r).to_text()).collect();
            let filled = values.len() as f64 / rows as f64;
            let distinct = values.iter().collect::<HashSet<_>>().len() as f64 / rows as f64;
            if filled < 0.95 || distinct < 0.5 {
                return None;
            }
            let avg_len =
                values.iter().map(String::len).sum::<usize>() as f64 / values.len().max(1) as f64;
            let mut score = distinct * 10.0;
            score += match c.kind {
                ColumnKind::Key => 4.0,
                ColumnKind::Integer => 1.0,
                _ => 0.0,
            };
            if name_suggests_key(&c.name) {
                score += 3.0;
            }
            // Long texts (names, UUIDs) are poor keys for people.
            if avg_len > 16.0 {
                score -= 6.0;
            }
            Some((i, score))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// A text column with readable, mostly distinct values.
pub fn name_column(table: &Table) -> Option<usize> {
    let rows = table.rows();
    if rows == 0 {
        return None;
    }
    const NAMES: &[&str] = &[
        "name",
        "names",
        "bezeichnung",
        "label",
        "ort",
        "city",
        "town",
        "gemeinde",
        "gemeindename",
        "municipality",
        "commune",
        "nom",
        "nome",
        "title",
        "region",
        "locality",
        "place",
    ];
    table
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.kind == ColumnKind::Text)
        .filter_map(|(i, c)| {
            let values: Vec<String> = (0..rows).filter_map(|r| c.get(r).to_text()).collect();
            if values.is_empty() {
                return None;
            }
            let distinct = values.iter().collect::<HashSet<_>>().len() as f64 / rows as f64;
            let letters = values
                .iter()
                .filter(|v| v.chars().any(char::is_alphabetic))
                .count() as f64
                / values.len() as f64;
            let avg_len = values.iter().map(|v| v.chars().count()).sum::<usize>() as f64
                / values.len() as f64;
            // Names tell areas apart: mostly distinct values.
            if letters < 0.8 || avg_len > 40.0 || distinct < 0.3 {
                return None;
            }
            let lower = c.name.to_lowercase();
            let named = NAMES
                .iter()
                .any(|n| lower == *n || lower.starts_with(n) || lower.ends_with(n));
            // Codes like UUIDs have no spaces and are long; names are shortish.
            let uuid_like = values
                .iter()
                .take(20)
                .all(|v| v.len() >= 30 && !v.contains(' '));
            if uuid_like {
                return None;
            }
            Some((i, distinct * 5.0 + if named { 5.0 } else { 0.0 }))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Columns holding point coordinates: (x/longitude, y/latitude, whether
/// they look like degrees). Found by name (`lon`, `lat`, `x`, `y`,
/// `easting`, `Breite`, …) among numeric columns.
pub fn coordinate_columns(table: &Table) -> Option<(usize, usize, bool)> {
    const X: &[&str] = &[
        "lon",
        "lng",
        "long",
        "longitude",
        "laenge",
        "länge",
        "lange",
        "x",
        "easting",
        "east",
        "e",
        "ost",
        "rechtswert",
        "xcoord",
        "x_coord",
        "coordx",
        "geo_lon",
        "geolon",
    ];
    const Y: &[&str] = &[
        "lat", "latitude", "breite", "y", "northing", "north", "n", "nord", "hochwert", "ycoord",
        "y_coord", "coordy", "geo_lat", "geolat",
    ];
    let norm = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect()
    };
    let find = |names: &[&str]| {
        table.columns.iter().position(|c| {
            let n = norm(&c.name);
            c.kind.is_numeric()
                && (names.contains(&n.as_str())
                    || names.iter().any(|w| n.ends_with(&format!("_{w}"))))
        })
    };
    let (x, y) = (find(X)?, find(Y)?);
    let range = |c: usize| {
        let col = &table.columns[c];
        (0..table.rows())
            .filter_map(|r| col.f64(r))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v), b.max(v))
            })
    };
    let ((x0, x1), (y0, y1)) = (range(x), range(y));
    let degrees = x0 >= -180.0 && x1 <= 180.0 && y0 >= -90.0 && y1 <= 90.0;
    Some((x, y, degrees))
}

/// The column of `table` whose values best match `targets` (a layer's
/// keys), with its matching. Candidates are compared on the first rows,
/// the winner is matched in full.
pub fn match_column(table: &Table, targets: &[Option<&str>]) -> Option<(usize, Matching)> {
    const SAMPLE: usize = 2000;
    let texts = |i: usize, rows: usize| -> Vec<Option<String>> {
        let c = &table.columns[i];
        (0..rows)
            .map(|r| match c.get(r) {
                crate::Value::Number(n) if n.fract() == 0.0 => Some(format!("{}", n as i64)),
                crate::Value::Number(_) => None,
                v => v.to_text(),
            })
            .collect()
    };
    let sample_rows = table.rows().min(SAMPLE);
    let best = table
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.kind != ColumnKind::Bool && c.kind != ColumnKind::Date)
        .filter_map(|(i, _)| {
            let t = texts(i, sample_rows);
            let refs: Vec<Option<&str>> = t.iter().map(|t| t.as_deref()).collect();
            let m = keys::best_match(&refs, targets);
            (m.stats.rows_matched > 0).then_some((i, m.stats.rows_matched))
        })
        .max_by_key(|(_, n)| *n)?;
    let t = texts(best.0, table.rows());
    let refs: Vec<Option<&str>> = t.iter().map(|t| t.as_deref()).collect();
    Some((best.0, keys::best_match(&refs, targets)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Column;

    fn texts(v: &[&str]) -> Vec<Option<String>> {
        v.iter().map(|s| Some(s.to_string())).collect()
    }

    #[test]
    fn finds_key_and_name() {
        let t = Table::new(vec![
            Column::text(
                "uuid",
                ColumnKind::Text,
                texts(&[
                    "8743F6EE-56D0-4A68-8616-791AE83F5F6C",
                    "6C5568FB-C3D7-48B3-A784-3D0552A7D103",
                ]),
            ),
            Column::text("Gemeinde", ColumnKind::Text, texts(&["Zürich", "Bern"])),
            Column::text("BFS_NR", ColumnKind::Key, texts(&["261", "351"])),
            Column::number("pop", vec![Some(1.0), Some(2.0)]),
        ])
        .unwrap();
        assert_eq!(key_column(&t), Some(2));
        assert_eq!(name_column(&t), Some(1));
    }

    #[test]
    fn status_columns_are_no_names() {
        let t = Table::new(vec![
            Column::text(
                "STATUS",
                ColumnKind::Text,
                texts(&["REAL", "REAL", "REAL", "REAL"]),
            ),
            Column::text("ID", ColumnKind::Key, texts(&["1", "2", "3", "4"])),
        ])
        .unwrap();
        assert_eq!(name_column(&t), None);
    }

    #[test]
    fn finds_coordinates() {
        let t = Table::new(vec![
            Column::text("Name", ColumnKind::Text, texts(&["a", "b"])),
            Column::number("Latitude", vec![Some(47.37), Some(46.95)]),
            Column::number("Longitude", vec![Some(8.54), Some(7.44)]),
        ])
        .unwrap();
        assert_eq!(coordinate_columns(&t), Some((2, 1, true)));
        let t = Table::new(vec![
            Column::number("E", vec![Some(2_600_000.0)]),
            Column::number("N", vec![Some(1_200_000.0)]),
        ])
        .unwrap();
        assert_eq!(coordinate_columns(&t), Some((0, 1, false)));
        let t = Table::new(vec![Column::number("Sales", vec![Some(1.0)])]).unwrap();
        assert_eq!(coordinate_columns(&t), None);
    }

    #[test]
    fn finds_matching_column() {
        let t = Table::new(vec![
            Column::text("Name", ColumnKind::Text, texts(&["a", "b", "c"])),
            Column::number("PLZ", vec![Some(1067.0), Some(10115.0), Some(80331.0)]),
        ])
        .unwrap();
        let targets = [Some("01067"), Some("10115"), Some("80331"), Some("20095")];
        let (col, m) = match_column(&t, &targets).unwrap();
        assert_eq!(col, 1);
        assert_eq!(m.stats.rows_matched, 3);
        assert_eq!(m.stats.targets_matched, 3);
    }
}
