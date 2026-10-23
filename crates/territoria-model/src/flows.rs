//! Flows: lines from where each row starts to where it goes (customers
//! to their store, commuters from home to work), as wide as their value.
//! Rows with the same start and end add up to one line.

use std::collections::HashMap;

use geo_types::Coord;
use serde::{Deserialize, Serialize};
use territoria_analysis::Rgba;
use territoria_data::Table;

use crate::{LayerId, Project, TableId};

/// Where the rows come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowRows {
    Table(TableId),
    /// A layer's features (their attributes are the rows).
    Layer(LayerId),
}

/// One end of each row's line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FlowEnd {
    /// The row's own place (rows of a layer).
    Own,
    /// The feature of `layer` whose key or name is in `column`.
    Column { column: String, layer: LayerId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowStyle {
    pub color: Rgba,
    /// Width of the largest flow in points.
    pub max_width: f32,
    pub curved: bool,
    pub arrows: bool,
}

impl Default for FlowStyle {
    fn default() -> Self {
        FlowStyle {
            color: Rgba([0x2f, 0x6f, 0xb0, 170]),
            max_width: 8.0,
            curved: true,
            arrows: true,
        }
    }
}

/// A line between two places (longitude/latitude) with its value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowLine {
    pub from: Coord<f64>,
    pub to: Coord<f64>,
    pub value: f64,
}

impl FlowRows {
    pub fn table(self, project: &Project) -> Option<&Table> {
        match self {
            FlowRows::Table(t) => project.table(t).map(|t| &*t.table),
            FlowRows::Layer(l) => project.layer(l).map(|l| &*l.attributes),
        }
    }

    pub fn name(self, project: &Project) -> String {
        match self {
            FlowRows::Table(t) => project.table(t).map(|t| t.name.clone()),
            FlowRows::Layer(l) => project.layer(l).map(|l| l.name.clone()),
        }
        .unwrap_or_default()
    }
}

/// Per row, the feature of `layer` its text in `column` names: by key or
/// by name, whichever matches more rows.
pub fn match_column(
    project: &Project,
    table: &Table,
    column: &str,
    layer: LayerId,
) -> Vec<Option<u32>> {
    let (Some(c), Some(l)) = (table.column(column), project.layer(layer)) else {
        return Vec::new();
    };
    let texts: Vec<Option<String>> = (0..table.rows()).map(|r| c.get(r).to_text()).collect();
    let source: Vec<Option<&str>> = texts.iter().map(|t| t.as_deref()).collect();
    let keys = crate::values::layer_keys(project, layer);
    let names: Vec<Option<String>> = (0..l.len()).map(|f| Some(l.display_name(f))).collect();
    [keys, names]
        .iter()
        .map(|targets| {
            let t: Vec<Option<&str>> = targets.iter().map(|k| k.as_deref()).collect();
            territoria_data::keys::best_match(&source, &t)
        })
        .max_by_key(|m| m.stats.rows_matched)
        .map(|m| m.row_target)
        .unwrap_or_default()
}

/// The place of each row at one end.
fn places(
    project: &Project,
    rows: FlowRows,
    table: &Table,
    end: &FlowEnd,
) -> Vec<Option<Coord<f64>>> {
    let at = |layer: LayerId, f: usize| -> Option<Coord<f64>> {
        let g = &project.layer(layer)?.geometry;
        g.points
            .get(f)
            .and_then(|p| p.0.first().map(|p| p.0))
            .or_else(|| g.label_points.get(f).copied().flatten())
    };
    match (end, rows) {
        (FlowEnd::Own, FlowRows::Layer(l)) => (0..table.rows()).map(|r| at(l, r)).collect(),
        (FlowEnd::Own, FlowRows::Table(_)) => vec![None; table.rows()],
        (FlowEnd::Column { column, layer }, _) => match_column(project, table, column, *layer)
            .into_iter()
            .map(|f| f.and_then(|f| at(*layer, f as usize)))
            .collect(),
    }
}

/// The lines of a flow map, largest last; rows from and to the same
/// places add up (each counts 1 without a value column).
pub fn lines(
    project: &Project,
    rows: FlowRows,
    from: &FlowEnd,
    to: &FlowEnd,
    value: Option<&str>,
) -> Vec<FlowLine> {
    let Some(table) = rows.table(project) else {
        return Vec::new();
    };
    let a = places(project, rows, table, from);
    let b = places(project, rows, table, to);
    let v = value.and_then(|c| table.column(c));
    // Same places: the same bits of both coordinates at both ends.
    let key = |p: Coord<f64>, q: Coord<f64>| [p.x, p.y, q.x, q.y].map(f64::to_bits);
    let mut sums: HashMap<[u64; 4], FlowLine> = HashMap::new();
    for r in 0..table.rows() {
        let (Some(p), Some(q)) = (a[r], b[r]) else {
            continue;
        };
        let x = match v {
            Some(c) => match c.f64(r) {
                Some(x) if x > 0.0 => x,
                _ => continue,
            },
            None => 1.0,
        };
        if p == q {
            continue;
        }
        sums.entry(key(p, q))
            .or_insert(FlowLine {
                from: p,
                to: q,
                value: 0.0,
            })
            .value += x;
    }
    let mut out: Vec<FlowLine> = sums.into_values().collect();
    out.sort_by(|a, b| a.value.total_cmp(&b.value));
    out
}
