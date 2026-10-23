//! New columns from elsewhere: the area a point lies in, the points in an
//! area, rows of another table by key, the nearest point of another
//! layer. The columns are values at the time (add them again after
//! changes).

use territoria_data::{Aggregate, Column, ColumnKind, KeyIndex, Table};
use territoria_geo::topology::haversine;

use crate::values::Cache;
use crate::{LayerId, Project, TableId};

/// Columns of the area each point lies in.
pub fn from_areas(
    project: &Project,
    cache: &mut Cache,
    points: LayerId,
    areas: LayerId,
    columns: &[String],
) -> Vec<Column> {
    let (Some(a), Some(hits)) = (
        project.layer(areas),
        cache.point_areas(project, points, areas),
    ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // The area's key (or name) always comes first.
    let key_name = a.key_column.clone().unwrap_or_else(|| a.name.clone());
    if !columns.contains(&key_name) {
        out.push(Column::text(
            format!("{} ({})", key_name, a.name),
            ColumnKind::Key,
            hits.iter().map(|h| h.map(|u| a.key(u as usize))).collect(),
        ));
    }
    for name in columns {
        let Some(c) = a.attributes.column(name) else {
            continue;
        };
        let kind = c.kind;
        let col = if kind.is_numeric() {
            let mut n = Column::number(
                format!("{name} ({})", a.name),
                hits.iter()
                    .map(|h| h.and_then(|u| c.f64(u as usize)))
                    .collect(),
            );
            n.kind = kind;
            n
        } else {
            Column::text(
                format!("{name} ({})", a.name),
                if kind == ColumnKind::Key {
                    ColumnKind::Key
                } else {
                    ColumnKind::Text
                },
                hits.iter()
                    .map(|h| h.and_then(|u| c.get(u as usize).to_text()))
                    .collect(),
            )
        };
        out.push(col);
    }
    out
}

/// Per area: how many points lie in it, and their numbers summed.
pub fn from_points(
    project: &Project,
    cache: &mut Cache,
    areas: LayerId,
    points: LayerId,
) -> Vec<Column> {
    let (Some(a), Some(p), Some(hits)) = (
        project.layer(areas),
        project.layer(points),
        cache.point_areas(project, points, areas),
    ) else {
        return Vec::new();
    };
    let n = a.len();
    let mut count = vec![0.0; n];
    for u in hits.iter().flatten() {
        count[*u as usize] += 1.0;
    }
    let mut out = vec![Column::number(
        format!("Number of {}", p.name),
        count.into_iter().map(Some).collect(),
    )];
    for c in p.attributes.columns.iter().filter(|c| c.kind.is_numeric()) {
        out.push(Column::number(
            format!("{} ({})", c.name, p.name),
            territoria_data::aggregate::group(
                (0..p.len()).map(|r| c.f64(r)),
                hits.iter().copied(),
                n,
                Aggregate::Sum,
            ),
        ));
    }
    out
}

/// Columns of another table, by key: rows whose key matches this row's;
/// numbers aggregated when several match, texts from the first.
pub fn lookup(
    target: &Table,
    target_key: &str,
    source: &Table,
    source_key: &str,
    columns: &[String],
    agg: Aggregate,
) -> Vec<Column> {
    let (Some(tk), Some(sk)) = (target.column(target_key), source.column(source_key)) else {
        return Vec::new();
    };
    let keys: Vec<Option<String>> = (0..target.rows()).map(|r| tk.get(r).to_text()).collect();
    let source_keys: Vec<Option<String>> =
        (0..source.rows()).map(|r| sk.get(r).to_text()).collect();
    // The way of matching that finds the most (leading zeros, case, …).
    let mode = territoria_data::keys::best_match(
        &source_keys.iter().map(|k| k.as_deref()).collect::<Vec<_>>(),
        &keys.iter().map(|k| k.as_deref()).collect::<Vec<_>>(),
    )
    .mode;
    let index = KeyIndex::new(keys.iter().map(|k| k.as_deref()), mode);
    // Per source row, the target rows with its key (keys may repeat).
    let mut by_key: std::collections::HashMap<u32, Vec<usize>> = Default::default();
    for (r, k) in keys.iter().enumerate() {
        if let Some(i) = k.as_deref().and_then(|k| index.find(k)) {
            by_key.entry(i).or_default().push(r);
        }
    }
    let first_of: Vec<Option<u32>> = source_keys
        .iter()
        .map(|k| k.as_deref().and_then(|k| index.find(k)))
        .collect();
    let n = target.rows();
    let mut out = Vec::new();
    for name in columns {
        let Some(c) = source.column(name) else {
            continue;
        };
        if c.kind.is_numeric() {
            // Aggregate per key, then hand to each target row with it.
            let per_key = territoria_data::aggregate::group(
                (0..source.rows()).map(|r| c.f64(r)),
                first_of.iter().copied(),
                keys.len(),
                agg,
            );
            let mut v = vec![None; n];
            for (k, rows) in &by_key {
                for &r in rows {
                    v[r] = per_key.get(*k as usize).copied().flatten();
                }
            }
            out.push(Column::number(name.clone(), v));
        } else {
            let mut v: Vec<Option<String>> = vec![None; n];
            for (sr, k) in first_of.iter().enumerate() {
                if let Some(rows) = k.and_then(|k| by_key.get(&k)) {
                    for &r in rows {
                        if v[r].is_none() {
                            v[r] = c.get(sr).to_text();
                        }
                    }
                }
            }
            out.push(Column::text(name.clone(), ColumnKind::Text, v));
        }
    }
    out
}

/// Per point: the nearest point of another layer, its name and the
/// straight-line distance in km.
pub fn nearest(project: &Project, points: LayerId, others: LayerId) -> Vec<Column> {
    let (Some(p), Some(o)) = (project.layer(points), project.layer(others)) else {
        return Vec::new();
    };
    let targets: Vec<(usize, geo_types::Coord<f64>)> = o
        .geometry
        .points
        .iter()
        .enumerate()
        .filter_map(|(i, mp)| mp.0.first().map(|q| (i, q.0)))
        .collect();
    let found: Vec<Option<(usize, f64)>> = p
        .geometry
        .points
        .iter()
        .map(|mp| {
            let at = mp.0.first()?.0;
            targets
                .iter()
                .filter(|(i, _)| {
                    points != others
                        || mp.0.first().map(|q| q.0) != Some(o.geometry.points[*i].0[0].0)
                })
                .map(|(i, q)| (*i, haversine(at, *q) / 1000.0))
                .min_by(|a, b| a.1.total_cmp(&b.1))
        })
        .collect();
    vec![
        Column::text(
            format!("Nearest {}", o.name),
            ColumnKind::Text,
            found
                .iter()
                .map(|f| f.map(|(i, _)| o.display_name(i)))
                .collect(),
        ),
        Column::number(
            format!("km to the nearest {}", o.name),
            found.iter().map(|f| f.map(|(_, d)| d)).collect(),
        ),
    ]
}

/// Columns of a table (by id), for lookups.
pub fn table_columns(project: &Project, table: TableId) -> Vec<String> {
    project
        .table(table)
        .map(|t| t.table.columns.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_by_key() {
        let target = Table::new(vec![Column::text(
            "Code",
            ColumnKind::Key,
            ["01", "02", "03"].map(|s| Some(s.to_owned())).to_vec(),
        )])
        .unwrap();
        let source = Table::new(vec![
            Column::text(
                "Area",
                ColumnKind::Key,
                ["1", "01", "02"].map(|s| Some(s.to_owned())).to_vec(),
            ),
            Column::number("Sales", vec![Some(10.0), Some(5.0), Some(7.0)]),
            Column::text(
                "Rep",
                ColumnKind::Text,
                ["Ann", "Bob", "Cy"].map(|s| Some(s.to_owned())).to_vec(),
            ),
        ])
        .unwrap();
        let cols = lookup(
            &target,
            "Code",
            &source,
            "Area",
            &["Sales".into(), "Rep".into()],
            Aggregate::Sum,
        );
        // "1" and "01" are the same key (leading zeros).
        assert_eq!(
            (0..3).map(|r| cols[0].f64(r)).collect::<Vec<_>>(),
            vec![Some(15.0), Some(7.0), None]
        );
        assert_eq!(cols[1].display(0), "Ann");
    }
}
