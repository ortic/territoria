//! Writing tables (CSV, Excel) and geometry (GeoJSON).

use std::io::Write;
use std::path::Path;

use geo_types::Geometry;
use territoria_data::{ColumnKind, Table, Value};

use crate::ImportError;

/// CSV with a header row; numbers with a decimal point, dates as ISO.
pub fn write_csv(path: &Path, table: &Table, delimiter: u8) -> Result<(), ImportError> {
    let mut w = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_path(path)
        .map_err(|e| ImportError::Format(e.to_string()))?;
    let err = |e: csv::Error| ImportError::Format(e.to_string());
    w.write_record(table.columns.iter().map(|c| c.name.as_str()))
        .map_err(err)?;
    for r in 0..table.rows() {
        w.write_record(table.columns.iter().map(|c| c.display(r)))
            .map_err(err)?;
    }
    w.flush()?;
    Ok(())
}

/// An Excel workbook with one sheet per table, header rows in bold, numbers
/// as numbers and keys as text.
pub fn write_xlsx(path: &Path, sheets: &[(&str, &Table)]) -> Result<(), ImportError> {
    use rust_xlsxwriter::{Format, Workbook};
    let err = |e: rust_xlsxwriter::XlsxError| ImportError::Format(e.to_string());
    let mut book = Workbook::new();
    let bold = Format::new().set_bold();
    let mut used: Vec<String> = Vec::new();
    for (name, table) in sheets {
        let ws = book.add_worksheet();
        // Sheet names: at most 31 characters, no []:*?/\, unique (ignoring case).
        let clean: String = name
            .chars()
            .filter(|c| !"[]:*?/\\".contains(*c))
            .take(31)
            .collect();
        let clean = if clean.trim().is_empty() {
            "Sheet".to_owned()
        } else {
            clean
        };
        let mut unique = clean.clone();
        let mut i = 2;
        while used.iter().any(|u| u.eq_ignore_ascii_case(&unique)) {
            let suffix = format!(" ({i})");
            unique = clean.chars().take(31 - suffix.len()).collect::<String>() + &suffix;
            i += 1;
        }
        ws.set_name(&unique).map_err(err)?;
        used.push(unique);
        for (c, col) in table.columns.iter().enumerate() {
            ws.write_string_with_format(0, c as u16, &col.name, &bold)
                .map_err(err)?;
            for r in 0..table.rows() {
                let row = r as u32 + 1;
                match col.get(r) {
                    Value::Null => {}
                    Value::Number(n) if col.kind != ColumnKind::Key => {
                        ws.write_number(row, c as u16, n).map_err(err)?;
                    }
                    Value::Bool(b) => {
                        ws.write_boolean(row, c as u16, b).map_err(err)?;
                    }
                    _ => {
                        ws.write_string(row, c as u16, col.display(r))
                            .map_err(err)?;
                    }
                }
            }
        }
        ws.set_freeze_panes(1, 0).map_err(err)?;
        ws.autofit();
    }
    book.save(path).map_err(err)?;
    Ok(())
}

/// A GeoJSON FeatureCollection in WGS84: one feature per geometry, with the
/// table row as properties.
pub fn write_geojson(
    path: &Path,
    geometries: &[Geometry<f64>],
    properties: &Table,
) -> Result<(), ImportError> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    write!(out, r#"{{"type":"FeatureCollection","features":["#)?;
    for (i, g) in geometries.iter().enumerate() {
        if i > 0 {
            write!(out, ",")?;
        }
        let geometry = geojson::Geometry::from(g);
        let mut props = serde_json::Map::new();
        for c in &properties.columns {
            let v = match c.get(i) {
                Value::Null => serde_json::Value::Null,
                Value::Number(n) if c.kind != ColumnKind::Key => serde_json::json!(n),
                Value::Bool(b) => serde_json::Value::Bool(b),
                _ => serde_json::Value::String(c.display(i)),
            };
            props.insert(c.name.clone(), v);
        }
        let feature = serde_json::json!({
            "type": "Feature",
            "properties": props,
            "geometry": geometry,
        });
        serde_json::to_writer(&mut out, &feature)
            .map_err(|e| ImportError::Format(e.to_string()))?;
    }
    write!(out, "]}}")?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use territoria_data::Column;

    fn table() -> Table {
        Table::new(vec![
            Column::text(
                "code",
                ColumnKind::Key,
                vec![Some("01067".into()), Some("10115".into())],
            ),
            Column::number("value", vec![Some(1.5), None]),
        ])
        .unwrap()
    }

    #[test]
    fn csv_round_trip() {
        let dir = std::env::temp_dir().join(format!("territoria-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.csv");
        write_csv(&path, &table(), b';').unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "code;value\n01067;1.5\n10115;\n");
        let (back, _) = crate::read_table(&path, &Default::default()).unwrap();
        assert_eq!(back.column("code").unwrap().display(0), "01067");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn xlsx_and_geojson() {
        let dir = std::env::temp_dir().join(format!("territoria-export2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let x = dir.join("t.xlsx");
        write_xlsx(&x, &[("Territories", &table())]).unwrap();
        let (back, info) = crate::read_table(&x, &Default::default()).unwrap();
        assert_eq!(info.sheet.as_deref(), Some("Territories"));
        assert_eq!(back.column("code").unwrap().display(0), "01067");
        assert_eq!(back.column("value").unwrap().f64(0), Some(1.5));

        let g = dir.join("t.geojson");
        let geoms: Vec<Geometry<f64>> = vec![
            geo_types::Point::new(8.5, 47.4).into(),
            geo_types::Point::new(7.4, 46.9).into(),
        ];
        write_geojson(&g, &geoms, &table()).unwrap();
        let layers = crate::read_vector(&g, &Default::default()).unwrap();
        assert_eq!(layers[0].geometries.len(), 2);
        assert_eq!(
            layers[0].attributes.column("code").unwrap().display(1),
            "10115"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
