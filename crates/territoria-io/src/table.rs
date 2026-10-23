//! Reading tables: CSV/TSV/TXT (encoding, delimiter, header detected) and
//! spreadsheets (xlsx, xls, xlsb, ods).

use std::path::Path;

use calamine::{Data, Reader};
use territoria_data::infer::{Decimal, InferOptions, infer_column};
use territoria_data::{Table, date};

use crate::ImportError;

pub const TABLE_EXTENSIONS: &[&str] = &["csv", "tsv", "txt", "xlsx", "xlsm", "xlsb", "xls", "ods"];

#[derive(Debug, Clone, Default)]
pub struct TableOptions {
    /// Field separator; detected when `None`.
    pub delimiter: Option<u8>,
    /// Text encoding label (`utf-8`, `windows-1252`…); detected when `None`.
    pub encoding: Option<String>,
    /// First row holds column names; detected when `None`.
    pub header: Option<bool>,
    pub decimal: Option<Decimal>,
    /// Spreadsheet sheet; the first when `None`.
    pub sheet: Option<String>,
}

/// What reading decided, shown in the import dialog so it can be changed.
#[derive(Debug, Clone, PartialEq)]
pub struct TableInfo {
    pub delimiter: Option<u8>,
    pub encoding: Option<String>,
    pub header: bool,
    pub sheets: Vec<String>,
    pub sheet: Option<String>,
}

pub fn is_spreadsheet(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .as_deref(),
        Some("xlsx" | "xlsm" | "xlsb" | "xls" | "ods")
    )
}

pub fn read_table(path: &Path, options: &TableOptions) -> Result<(Table, TableInfo), ImportError> {
    if is_spreadsheet(path) {
        read_spreadsheet(path, options)
    } else {
        let bytes = std::fs::read(path)?;
        read_text(&bytes, options)
    }
}

/// Decodes text: a byte-order mark wins, then valid UTF-8, then
/// Windows-1252 (what spreadsheets save as "CSV" on Windows).
pub fn decode(bytes: &[u8], label: Option<&str>) -> (String, &'static str) {
    if let Some(enc) = label.and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes())) {
        let (text, _, _) = enc.decode(bytes);
        return (text.into_owned(), enc.name());
    }
    if let Some((enc, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        let (text, _) = enc.decode_without_bom_handling(&bytes[bom..]);
        return (text.into_owned(), enc.name());
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_owned(), "UTF-8"),
        Err(_) => {
            let (text, _) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes);
            (text.into_owned(), "windows-1252")
        }
    }
}

/// The separator that splits the first lines into the most consistent
/// number of fields (more than one).
pub fn detect_delimiter(text: &str) -> u8 {
    let sample: String = text.lines().take(50).collect::<Vec<_>>().join("\n");
    let mut best = (b',', 0usize, 0usize);
    for d in [b',', b';', b'\t', b'|'] {
        let mut rdr = csv::ReaderBuilder::new()
            .delimiter(d)
            .has_headers(false)
            .flexible(true)
            .from_reader(sample.as_bytes());
        let counts: Vec<usize> = rdr
            .records()
            .filter_map(Result::ok)
            .map(|r| r.len())
            .collect();
        if counts.is_empty() {
            continue;
        }
        // The most common field count and how many lines have it.
        let mut freq = std::collections::HashMap::new();
        for c in &counts {
            *freq.entry(*c).or_insert(0usize) += 1;
        }
        let (fields, lines) = freq
            .into_iter()
            .max_by_key(|&(f, n)| (n, f))
            .unwrap_or((1, 0));
        if fields > 1 && (lines, fields) > (best.2, best.1) {
            best = (d, fields, lines);
        }
    }
    best.0
}

fn read_text(bytes: &[u8], options: &TableOptions) -> Result<(Table, TableInfo), ImportError> {
    let (text, encoding) = decode(bytes, options.encoding.as_deref());
    let delimiter = options.delimiter.unwrap_or_else(|| detect_delimiter(&text));
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut rows: Vec<Vec<String>> = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| ImportError::Format(format!("line {}: {e}", rows.len() + 1)))?;
        if rec.iter().all(|f| f.trim().is_empty()) {
            continue;
        }
        rows.push(rec.iter().map(str::to_owned).collect());
    }
    let header = options.header.unwrap_or_else(|| looks_like_header(&rows));
    let table = build(rows, header, options.decimal);
    Ok((
        table,
        TableInfo {
            delimiter: Some(delimiter),
            encoding: Some(encoding.to_owned()),
            header,
            sheets: Vec::new(),
            sheet: None,
        },
    ))
}

/// A first row that has no numbers while the second has some is a header.
fn looks_like_header(rows: &[Vec<String>]) -> bool {
    let Some(first) = rows.first() else {
        return true;
    };
    let numeric = |r: &Vec<String>| {
        r.iter()
            .filter(|c| {
                territoria_data::infer::parse_number(c, Decimal::Point).is_some()
                    || territoria_data::infer::parse_number(c, Decimal::Comma).is_some()
            })
            .count()
    };
    let empty = first.iter().filter(|c| c.trim().is_empty()).count();
    match rows.get(1) {
        Some(second) => numeric(first) < numeric(second) || (numeric(first) == 0 && empty == 0),
        None => true,
    }
}

fn build(mut rows: Vec<Vec<String>>, header: bool, decimal: Option<Decimal>) -> Table {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let names: Vec<String> = if header && !rows.is_empty() {
        let h = rows.remove(0);
        (0..width)
            .map(|i| {
                h.get(i)
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("Column {}", i + 1))
            })
            .collect()
    } else {
        (0..width).map(|i| format!("Column {}", i + 1)).collect()
    };
    let mut table = Table::with_rows(rows.len());
    for (i, name) in names.iter().enumerate() {
        let values: Vec<Option<String>> = rows
            .iter()
            .map(|r| {
                r.get(i)
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            })
            .collect();
        let name = table.unique_name(name);
        let col = infer_column(&name, values, InferOptions { decimal });
        table.push_column(col).expect("one value per row");
    }
    table
}

pub fn sheet_names(path: &Path) -> Result<Vec<String>, ImportError> {
    let wb = calamine::open_workbook_auto(path).map_err(|e| ImportError::Format(e.to_string()))?;
    Ok(wb.sheet_names())
}

fn read_spreadsheet(
    path: &Path,
    options: &TableOptions,
) -> Result<(Table, TableInfo), ImportError> {
    let mut wb =
        calamine::open_workbook_auto(path).map_err(|e| ImportError::Format(e.to_string()))?;
    let sheets = wb.sheet_names();
    let sheet = options
        .sheet
        .clone()
        .or_else(|| sheets.first().cloned())
        .ok_or_else(|| ImportError::Format("the workbook has no sheets".into()))?;
    let range = wb
        .worksheet_range(&sheet)
        .map_err(|e| ImportError::Format(format!("sheet “{sheet}”: {e}")))?;
    let mut rows: Vec<Vec<String>> = range
        .rows()
        .map(|r| r.iter().map(|c| cell_text(c).unwrap_or_default()).collect())
        .collect();
    // Skip title rows above the table: start at the first row that is at
    // least half filled.
    let width = rows
        .iter()
        .map(|r| r.iter().filter(|c| !c.is_empty()).count())
        .max()
        .unwrap_or(0);
    let start = rows
        .iter()
        .position(|r| r.iter().filter(|c| !c.is_empty()).count() * 2 >= width)
        .unwrap_or(0);
    rows.drain(..start);
    rows.retain(|r| r.iter().any(|c| !c.is_empty()));
    let header = options.header.unwrap_or_else(|| looks_like_header(&rows));
    let table = build(rows, header, Some(Decimal::Point));
    Ok((
        table,
        TableInfo {
            delimiter: None,
            encoding: None,
            header,
            sheets,
            sheet: Some(sheet),
        },
    ))
}

fn cell_text(c: &Data) -> Option<String> {
    match c {
        Data::Empty | Data::Error(_) => None,
        Data::Int(i) => Some(i.to_string()),
        Data::Float(f) => Some(territoria_data::table::format_number(*f)),
        Data::String(s) => Some(s.clone()),
        Data::Bool(b) => Some(b.to_string()),
        Data::DateTime(dt) => Some(date::format(date::from_excel(dt.as_f64()))),
        Data::DateTimeIso(s) | Data::DurationIso(s) => Some(s.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use territoria_data::ColumnKind;

    #[test]
    fn semicolon_csv_with_decimal_commas() {
        let csv = "PLZ;Ort;Umsatz\n01067;Dresden;1.234,50\n10115;Berlin;99,5\n";
        let (t, info) = read_text(csv.as_bytes(), &TableOptions::default()).unwrap();
        assert_eq!(info.delimiter, Some(b';'));
        assert!(info.header);
        assert_eq!(t.rows(), 2);
        assert_eq!(t.column("PLZ").unwrap().kind, ColumnKind::Key);
        assert_eq!(t.column("PLZ").unwrap().display(0), "01067");
        assert_eq!(t.column("Umsatz").unwrap().f64(0), Some(1234.5));
    }

    #[test]
    fn windows_1252_and_tabs() {
        let mut bytes = b"Name\tValue\n".to_vec();
        bytes.extend_from_slice(b"Z\xfcrich\t1\n");
        let (t, info) = read_text(&bytes, &TableOptions::default()).unwrap();
        assert_eq!(info.encoding.as_deref(), Some("windows-1252"));
        assert_eq!(t.column("Name").unwrap().display(0), "Zürich");
    }

    #[test]
    fn headerless_and_quoted() {
        let csv = "1,\"a, b\",2.5\n2,c,3\n";
        let (t, info) = read_text(csv.as_bytes(), &TableOptions::default()).unwrap();
        assert!(!info.header);
        assert_eq!(t.rows(), 2);
        assert_eq!(t.columns[1].display(0), "a, b");
        assert_eq!(t.columns[0].name, "Column 1");
    }
}
