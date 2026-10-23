//! Values per feature: from the layer's own attributes or from a linked
//! table, aggregated when several rows hit one feature. Results are cached
//! by revision.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use territoria_data::{Aggregate, Column, ColumnKind, KeyIndex, Matching, Table, keys};
use territoria_io::GeometryKind;

use crate::{LayerId, LinkId, Project};

/// Where a number per feature comes from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ValueSource {
    /// A column of the layer's attributes.
    Attribute { column: String },
    /// A column of a linked table, aggregated per feature.
    Linked {
        link: LinkId,
        column: String,
        agg: Aggregate,
    },
    /// Rows of a linked table per feature (e.g. customers per area).
    RowCount { link: LinkId },
    /// Points of a point layer inside each area (customers per postcode):
    /// counted, or a column of theirs aggregated.
    Points {
        layer: LayerId,
        column: Option<String>,
        agg: Aggregate,
    },
    /// The feature's area in km².
    Area,
    /// 1 per feature: counts units.
    Units,
}

impl ValueSource {
    pub fn uses_links(&self, links: &[LinkId]) -> bool {
        match self {
            ValueSource::Linked { link, .. } | ValueSource::RowCount { link } => {
                links.contains(link)
            }
            _ => false,
        }
    }

    pub fn label(&self, project: &Project) -> String {
        match self {
            ValueSource::Attribute { column } => column.clone(),
            ValueSource::Linked { link, column, agg } => {
                let table = project
                    .link(*link)
                    .and_then(|l| project.table(l.table))
                    .map_or("?".into(), |t| t.name.clone());
                match agg {
                    Aggregate::Sum => format!("{column} ({table})"),
                    a => format!("{column}, {} ({table})", a.label().to_lowercase()),
                }
            }
            ValueSource::RowCount { link } => {
                let table = project
                    .link(*link)
                    .and_then(|l| project.table(l.table))
                    .map_or("?".into(), |t| t.name.clone());
                format!("Rows of {table}")
            }
            ValueSource::Points { layer, column, agg } => {
                let name = project.layer(*layer).map_or("?".into(), |l| l.name.clone());
                match column {
                    None => format!("Number of {name}"),
                    Some(c) if *agg == Aggregate::Sum => format!("{c} ({name})"),
                    Some(c) => format!("{c}, {} ({name})", agg.label().to_lowercase()),
                }
            }
            ValueSource::Area => "Area (km²)".into(),
            ValueSource::Units => "Number of areas".into(),
        }
    }
}

/// Where a text per feature comes from (categories, labels).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TextSource {
    Attribute {
        column: String,
    },
    /// The first linked row's value.
    Linked {
        link: LinkId,
        column: String,
    },
}

/// Matchings and values computed from the project, keyed by the revisions
/// they were computed from. Lives next to the project, not in it.
#[derive(Default)]
pub struct Cache {
    matchings: HashMap<LinkId, (u64, u64, Arc<Matching>)>,
    /// Per (point layer, area layer): the area each point lies in.
    point_areas: HashMap<(LayerId, LayerId), (u64, u64, PointAreas)>,
    feature_tables: HashMap<LayerId, (u64, Arc<Table>)>,
    templates: HashMap<(LayerId, String), (u64, Texts)>,
}

/// A text per feature.
pub type Texts = Arc<Vec<Option<String>>>;

/// Changes when anything a layer's feature table is made of changes.
fn feature_key(project: &Project, layer: &crate::Layer) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    layer.rev.hash(&mut h);
    for link in project.links_to(layer.id) {
        format!("{link:?}").hash(&mut h);
        if let Some(t) = project.table(link.table) {
            t.rev.hash(&mut h);
        }
    }
    if layer.kind() == GeometryKind::Area {
        for p in project
            .layers
            .iter()
            .filter(|l| l.kind() == GeometryKind::Point)
        {
            (p.id, p.rev).hash(&mut h);
        }
    }
    h.finish()
}

/// Numbers with thousands separators, other values as text.
fn template_value(v: &territoria_expr::Val) -> String {
    match v {
        territoria_expr::Val::Num(n) => territoria_analysis::format::format_value(*n),
        v => v.text().unwrap_or_default(),
    }
}

/// Per point feature, the area it lies in.
pub type PointAreas = Arc<Vec<Option<u32>>>;

impl Cache {
    /// Everything known per feature, for templates: the layer's own
    /// columns, its area, linked tables (numbers summed, other values from
    /// the first row, a row count) and points inside (counted, numbers
    /// summed).
    pub fn feature_table(&mut self, project: &Project, layer: LayerId) -> Option<Arc<Table>> {
        let l = project.layer(layer)?;
        let key = feature_key(project, l);
        if let Some((k, t)) = self.feature_tables.get(&layer)
            && *k == key
        {
            return Some(t.clone());
        }
        let n = l.len();
        let mut t = if l.attributes.rows() == n {
            (*l.attributes).clone()
        } else {
            Table::with_rows(n)
        };
        let push = |t: &mut Table, mut c: Column| {
            c.name = t.unique_name(&c.name);
            let _ = t.push_column(c);
        };
        if l.kind() == GeometryKind::Area {
            push(
                &mut t,
                Column::number(
                    "Area km2",
                    l.geometry.areas.iter().map(|a| Some(*a)).collect(),
                ),
            );
        }
        for link in project.links_to(layer) {
            let (Some(ds), Some(m)) = (project.table(link.table), self.matching(project, link.id))
            else {
                continue;
            };
            let mut rows = vec![0.0; n];
            for f in m.row_target.iter().flatten() {
                rows[*f as usize] += 1.0;
            }
            push(
                &mut t,
                Column::number(
                    format!("{} rows", ds.name),
                    rows.into_iter().map(Some).collect(),
                ),
            );
            for c in ds.table.columns.iter().filter(|c| c.name != link.column) {
                let name = if t.column(&c.name).is_some() {
                    format!("{} ({})", c.name, ds.name)
                } else {
                    c.name.clone()
                };
                if c.kind.is_numeric() {
                    let v = territoria_data::aggregate::group(
                        (0..ds.table.rows()).map(|r| c.f64(r)),
                        m.row_target.iter().copied(),
                        n,
                        Aggregate::Sum,
                    );
                    push(&mut t, Column::number(name, v));
                } else {
                    let mut v: Vec<Option<String>> = vec![None; n];
                    for (r, f) in m.row_target.iter().enumerate() {
                        if let Some(f) = f
                            && v[*f as usize].is_none()
                        {
                            v[*f as usize] = c.get(r).to_text();
                        }
                    }
                    push(&mut t, Column::text(name, ColumnKind::Text, v));
                }
            }
        }
        if l.kind() == GeometryKind::Area {
            let points: Vec<LayerId> = project
                .layers
                .iter()
                .filter(|p| p.kind() == GeometryKind::Point)
                .map(|p| p.id)
                .collect();
            for p in points {
                let (Some(hits), Some(pl)) =
                    (self.point_areas(project, p, layer), project.layer(p))
                else {
                    continue;
                };
                let mut count = vec![0.0; n];
                for f in hits.iter().flatten() {
                    count[*f as usize] += 1.0;
                }
                push(
                    &mut t,
                    Column::number(pl.name.clone(), count.into_iter().map(Some).collect()),
                );
                for c in pl.attributes.columns.iter().filter(|c| c.kind.is_numeric()) {
                    let v = territoria_data::aggregate::group(
                        (0..pl.len()).map(|r| c.f64(r)),
                        hits.iter().copied(),
                        n,
                        Aggregate::Sum,
                    );
                    push(
                        &mut t,
                        Column::number(format!("{} ({})", c.name, pl.name), v),
                    );
                }
            }
        }
        let t = Arc::new(t);
        self.feature_tables.insert(layer, (key, t.clone()));
        Some(t)
    }

    /// A template's text per feature (labels, hover cards); `None` when the
    /// template has a mistake.
    pub fn template_texts(
        &mut self,
        project: &Project,
        layer: LayerId,
        source: &str,
    ) -> Option<Texts> {
        let table = self.feature_table(project, layer)?;
        let key = self.feature_tables.get(&layer)?.0;
        let id = (layer, source.to_owned());
        if let Some((k, texts)) = self.templates.get(&id)
            && *k == key
        {
            return Some(texts.clone());
        }
        let template = territoria_expr::Template::compile(source, &table).ok()?;
        let texts = Arc::new(template.render(&table, &template_value));
        // Keep what was typed lately, not every version ever.
        if self.templates.len() > 64 {
            self.templates.clear();
        }
        self.templates.insert(id, (key, texts.clone()));
        Some(texts)
    }

    /// Rows of the link's table matched to the layer's features.
    pub fn matching(&mut self, project: &Project, link: LinkId) -> Option<Arc<Matching>> {
        let l = project.link(link)?;
        let layer = project.layer(l.layer)?;
        let table = project.table(l.table)?;
        if let Some((lr, tr, m)) = self.matchings.get(&link)
            && *lr == layer.rev
            && *tr == table.rev
            && m.mode == l.mode
        {
            return Some(m.clone());
        }
        let m = Arc::new(compute_matching(project, link)?);
        self.matchings
            .insert(link, (layer.rev, table.rev, m.clone()));
        Some(m)
    }

    /// The area of `areas` each feature of `points` lies in.
    pub fn point_areas(
        &mut self,
        project: &Project,
        points: LayerId,
        areas: LayerId,
    ) -> Option<PointAreas> {
        let (p, a) = (project.layer(points)?, project.layer(areas)?);
        if let Some((pr, ar, hits)) = self.point_areas.get(&(points, areas))
            && *pr == p.rev
            && *ar == a.rev
        {
            return Some(hits.clone());
        }
        use rayon::prelude::*;
        let g = &a.geometry;
        let hits: Vec<Option<u32>> = p
            .geometry
            .points
            .par_iter()
            .map(|mp| {
                let pt = mp.0.first()?;
                g.feature_at(pt.0, 0.0).map(|f| f as u32)
            })
            .collect();
        let hits = Arc::new(hits);
        self.point_areas
            .insert((points, areas), (p.rev, a.rev, hits.clone()));
        Some(hits)
    }

    /// One value per feature of `layer` (`None` = no data for it).
    pub fn values(
        &mut self,
        project: &Project,
        layer: LayerId,
        source: &ValueSource,
    ) -> Vec<Option<f64>> {
        let Some(l) = project.layer(layer) else {
            return Vec::new();
        };
        let n = l.len();
        match source {
            ValueSource::Attribute { column } => match l.attributes.column(column) {
                Some(c) => (0..n).map(|r| c.f64(r)).collect(),
                None => vec![None; n],
            },
            ValueSource::Area => l.geometry.areas.iter().map(|a| Some(*a)).collect(),
            ValueSource::Points {
                layer: points,
                column,
                agg,
            } => {
                let Some(hits) = self.point_areas(project, *points, layer) else {
                    return vec![None; n];
                };
                let Some(pl) = project.layer(*points) else {
                    return vec![None; n];
                };
                match column.as_deref().and_then(|c| pl.attributes.column(c)) {
                    Some(col) => territoria_data::aggregate::group(
                        (0..pl.len()).map(|r| col.f64(r)),
                        hits.iter().copied(),
                        n,
                        *agg,
                    ),
                    None => {
                        let mut counts = vec![None; n];
                        for t in hits.iter().flatten() {
                            *counts[*t as usize].get_or_insert(0.0) += 1.0;
                        }
                        counts
                    }
                }
            }
            ValueSource::Units => vec![Some(1.0); n],
            ValueSource::Linked { link, column, agg } => {
                let (Some(m), Some(lk)) = (self.matching(project, *link), project.link(*link))
                else {
                    return vec![None; n];
                };
                let Some(table) = project.table(lk.table) else {
                    return vec![None; n];
                };
                let Some(col) = table.table.column(column) else {
                    return vec![None; n];
                };
                territoria_data::aggregate::group(
                    (0..table.table.rows()).map(|r| col.f64(r)),
                    m.row_target.iter().copied(),
                    n,
                    *agg,
                )
            }
            ValueSource::RowCount { link } => {
                let Some(m) = self.matching(project, *link) else {
                    return vec![None; n];
                };
                let mut counts = vec![None; n];
                for t in m.row_target.iter().flatten() {
                    *counts[*t as usize].get_or_insert(0.0) += 1.0;
                }
                counts
            }
        }
    }

    /// One text per feature of `layer`.
    pub fn texts(
        &mut self,
        project: &Project,
        layer: LayerId,
        source: &TextSource,
    ) -> Vec<Option<String>> {
        let Some(l) = project.layer(layer) else {
            return Vec::new();
        };
        let n = l.len();
        match source {
            TextSource::Attribute { column } => match l.attributes.column(column) {
                Some(c) => (0..n).map(|r| c.get(r).to_text()).collect(),
                None => vec![None; n],
            },
            TextSource::Linked { link, column } => {
                let (Some(m), Some(lk)) = (self.matching(project, *link), project.link(*link))
                else {
                    return vec![None; n];
                };
                let Some(col) = project.table(lk.table).and_then(|t| t.table.column(column)) else {
                    return vec![None; n];
                };
                let mut out = vec![None; n];
                for (row, t) in m.row_target.iter().enumerate() {
                    if let Some(t) = t
                        && out[*t as usize].is_none()
                    {
                        out[*t as usize] = col.get(row).to_text();
                    }
                }
                out
            }
        }
    }
}

/// The layer's keys as text, one per feature.
pub fn layer_keys(project: &Project, layer: LayerId) -> Vec<Option<String>> {
    let Some(l) = project.layer(layer) else {
        return Vec::new();
    };
    match l.key_index() {
        Some(c) => (0..l.len())
            .map(|r| l.attributes.columns[c].get(r).to_text())
            .collect(),
        None => (1..=l.len()).map(|i| Some(i.to_string())).collect(),
    }
}

/// The table column's keys as text (whole numbers without decimals).
pub fn column_keys(table: &territoria_data::Table, column: &str) -> Vec<Option<String>> {
    let Some(c) = table.column(column) else {
        return vec![None; table.rows()];
    };
    (0..table.rows())
        .map(|r| match c.get(r) {
            territoria_data::Value::Number(n) if n.fract() == 0.0 => Some(format!("{}", n as i64)),
            v => v.to_text(),
        })
        .collect()
}

pub fn compute_matching(project: &Project, link: LinkId) -> Option<Matching> {
    let l = project.link(link)?;
    let table = project.table(l.table)?;
    let targets = layer_keys(project, l.layer);
    let index = KeyIndex::new(targets.iter().map(|t| t.as_deref()), l.mode);
    let source = column_keys(&table.table, &l.column);
    Some(keys::match_keys(
        source.iter().map(|s| s.as_deref()),
        &index,
    ))
}
