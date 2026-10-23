//! Geometry layers: boundaries, lines and points, with what's derived from
//! them (boxes, label points, areas, neighbours, spatial index).

use std::sync::Arc;

use geo::{BoundingRect, Contains};
use geo_types::{Coord, MultiLineString, MultiPoint, MultiPolygon, Point, Rect};
use serde::{Deserialize, Serialize};
use territoria_data::Table;

use territoria_geo::{BoxIndex, Topology};
use territoria_io::{Geometries, GeometryKind};

use crate::LayerId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    #[serde(skip)]
    pub geometry: Arc<LayerGeometry>,
    #[serde(skip)]
    pub attributes: Arc<Table>,
    /// Column identifying features (codes data is matched against).
    pub key_column: Option<String>,
    /// Column with a readable name for labels and lists.
    pub name_column: Option<String>,
    pub source: SourceInfo,
    /// Changes whenever geometry or attributes change.
    pub rev: u64,
}

/// Where data came from, for the user and for reloading.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub path: Option<std::path::PathBuf>,
    /// Coordinate system as found, e.g. "EPSG:2056 · CH1903+ / LV95 (from the .prj file)".
    pub crs_note: String,
    /// Credit the data's licence asks for, shown on maps.
    #[serde(default)]
    pub attribution: Option<String>,
}

impl Layer {
    pub fn kind(&self) -> GeometryKind {
        self.geometry.kind
    }

    pub fn len(&self) -> usize {
        self.geometry.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn key_index(&self) -> Option<usize> {
        self.key_column
            .as_deref()
            .and_then(|c| self.attributes.column_index(c))
    }

    /// The feature's key, or its number when there's no key column.
    pub fn key(&self, feature: usize) -> String {
        match self.key_index() {
            Some(c) => self.attributes.columns[c].display(feature),
            None => (feature + 1).to_string(),
        }
    }

    /// The feature's name, or its key.
    pub fn display_name(&self, feature: usize) -> String {
        let name = self
            .name_column
            .as_deref()
            .and_then(|c| self.attributes.column(c))
            .map(|c| c.display(feature))
            .filter(|n| !n.is_empty());
        match name {
            Some(n) if self.key_column.is_some() => format!("{} {n}", self.key(feature)),
            Some(n) => n,
            None => self.key(feature),
        }
    }
}

/// Geometry in WGS84 longitude/latitude (or unknown planar coordinates when
/// `geographic` is false).
#[derive(Debug, Clone, Default)]
pub struct LayerGeometry {
    pub kind: GeometryKind,
    pub geographic: bool,
    /// Areas as shared-border topology (empty for lines and points).
    pub topology: Topology,
    pub lines: Vec<MultiLineString<f64>>,
    pub points: Vec<MultiPoint<f64>>,
    // Derived:
    pub bboxes: Vec<Option<Rect<f64>>>,
    pub label_points: Vec<Option<Coord<f64>>>,
    /// Area in km² (areas only).
    pub areas: Vec<f64>,
    /// Neighbours with shared border length in km (areas only).
    pub neighbors: Vec<Vec<(u32, f64)>>,
    pub index: BoxIndex,
    pub bbox: Option<Rect<f64>>,
    /// Neighbours were found by proximity because borders don't share
    /// vertices (territory borders then look busier).
    pub approximate_neighbors: bool,
}

impl LayerGeometry {
    /// A new geometry with one feature per group (the groups' features
    /// combined). Shared borders inside a group stop being borders.
    pub fn merged(&self, groups: &[Vec<usize>]) -> LayerGeometry {
        let mut new_of = vec![u32::MAX; self.len()];
        for (g, members) in groups.iter().enumerate() {
            for &m in members {
                new_of[m] = g as u32;
            }
        }
        let mut topology = Topology::default();
        let mut lines = Vec::new();
        let mut points = Vec::new();
        match self.kind {
            GeometryKind::Area => {
                topology.arcs = self
                    .topology
                    .arcs
                    .iter()
                    .map(|a| {
                        let map = |f: u32| {
                            if f == territoria_geo::OUTSIDE {
                                f
                            } else {
                                new_of[f as usize]
                            }
                        };
                        territoria_geo::Arc {
                            points: a.points.clone(),
                            a: map(a.a),
                            b: map(a.b),
                        }
                    })
                    .collect();
                topology.features = groups
                    .iter()
                    .map(|members| {
                        members
                            .iter()
                            .flat_map(|&m| self.topology.features[m].iter().cloned())
                            .collect()
                    })
                    .collect();
            }
            GeometryKind::Line => {
                lines = groups
                    .iter()
                    .map(|m| {
                        MultiLineString(
                            m.iter()
                                .flat_map(|&f| self.lines[f].0.iter().cloned())
                                .collect(),
                        )
                    })
                    .collect();
            }
            GeometryKind::Point => {
                points = groups
                    .iter()
                    .map(|m| {
                        MultiPoint(
                            m.iter()
                                .flat_map(|&f| self.points[f].0.iter().copied())
                                .collect(),
                        )
                    })
                    .collect();
            }
        }
        LayerGeometry::from_parts(self.kind, self.geographic, topology, lines, points)
    }

    pub fn from_imported(geometries: Geometries, geographic: bool) -> LayerGeometry {
        let mut g = LayerGeometry {
            kind: geometries.kind(),
            geographic,
            ..Default::default()
        };
        match geometries {
            Geometries::Areas(areas) => g.topology = Topology::build(&areas),
            Geometries::Lines(lines) => g.lines = lines,
            Geometries::Points(points) => g.points = points,
        }
        g.derive();
        g
    }

    pub fn from_parts(
        kind: GeometryKind,
        geographic: bool,
        topology: Topology,
        lines: Vec<MultiLineString<f64>>,
        points: Vec<MultiPoint<f64>>,
    ) -> LayerGeometry {
        let mut g = LayerGeometry {
            kind,
            geographic,
            topology,
            lines,
            points,
            ..Default::default()
        };
        g.derive();
        g
    }

    pub fn len(&self) -> usize {
        match self.kind {
            GeometryKind::Area => self.topology.len(),
            GeometryKind::Line => self.lines.len(),
            GeometryKind::Point => self.points.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Recomputes boxes, label points, areas, neighbours and the index.
    pub fn derive(&mut self) {
        use rayon::prelude::*;
        let geographic = self.geographic;
        match self.kind {
            GeometryKind::Area => {
                let topo = &self.topology;
                type Derived = (Option<Rect<f64>>, Option<Coord<f64>>, f64);
                let per: Vec<Derived> = (0..topo.len())
                    .into_par_iter()
                    .map(|i| {
                        let mp = topo.polygon(i);
                        let bbox = mp.bounding_rect();
                        let label = territoria_geo::label_point(&mp, geographic);
                        let area = territoria_geo::area(&mp, geographic) / 1e6;
                        (bbox, label, area)
                    })
                    .collect();
                self.bboxes = per.iter().map(|p| p.0).collect();
                self.label_points = per.iter().map(|p| p.1).collect();
                self.areas = per.iter().map(|p| p.2).collect();
                let mut neighbors = topo.neighbors(geographic);
                // Most areas without a neighbour: the borders aren't shared
                // exactly. Find neighbours by proximity instead.
                let lonely = neighbors.iter().filter(|n| n.is_empty()).count();
                self.approximate_neighbors = topo.len() > 2 && lonely * 10 > topo.len() * 3;
                if self.approximate_neighbors {
                    let polygons: Vec<MultiPolygon<f64>> =
                        (0..topo.len()).map(|i| topo.polygon(i)).collect();
                    let extent = territoria_geo::union_rect(self.bboxes.iter().flatten().copied());
                    let tolerance = extent.map_or(1e-6, |b| b.width().max(b.height()) * 2e-5);
                    neighbors = territoria_geo::topology::approximate_neighbors(
                        &polygons, tolerance, geographic,
                    );
                }
                self.neighbors = neighbors
                    .into_iter()
                    .map(|n| {
                        n.into_iter()
                            .map(|(f, len)| (f, if geographic { len / 1000.0 } else { len }))
                            .collect()
                    })
                    .collect();
            }
            GeometryKind::Line => {
                self.bboxes = self.lines.iter().map(|l| l.bounding_rect()).collect();
                self.label_points = self
                    .lines
                    .iter()
                    .map(|l| l.0.first().and_then(|ls| ls.0.get(ls.0.len() / 2)).copied())
                    .collect();
                self.areas = vec![0.0; self.lines.len()];
                self.neighbors = vec![Vec::new(); self.lines.len()];
            }
            GeometryKind::Point => {
                self.bboxes = self.points.iter().map(|p| p.bounding_rect()).collect();
                self.label_points = self
                    .points
                    .iter()
                    .map(|p| p.0.first().map(|p| p.0))
                    .collect();
                // Points touch where their Voronoi cells do; each stands for
                // an equal share of the extent (compactness needs an area).
                self.neighbors =
                    territoria_geo::points::point_neighbors(&self.label_points, geographic);
                let extent = territoria_geo::union_rect(self.bboxes.iter().flatten().copied());
                let km2 = extent.map_or(0.0, |b| {
                    let k = if geographic {
                        111.32 * b.center().y.to_radians().cos() * 110.57
                    } else {
                        1e-6
                    };
                    b.width() * b.height() * k
                });
                let n = self.points.len().max(1) as f64;
                self.areas = vec![km2 / n; self.points.len()];
            }
        }
        self.index = BoxIndex::new(&self.bboxes);
        self.bbox = territoria_geo::union_rect(self.bboxes.iter().flatten().copied());
    }

    pub fn polygon(&self, feature: usize) -> MultiPolygon<f64> {
        self.topology.polygon(feature)
    }

    /// The feature at `p` (layer coordinates): the area containing it, or
    /// the nearest line/point within `tolerance`.
    pub fn feature_at(&self, p: Coord<f64>, tolerance: f64) -> Option<usize> {
        match self.kind {
            GeometryKind::Area => {
                let mut hits: Vec<usize> = self
                    .index
                    .at(p)
                    .map(|i| i as usize)
                    .filter(|&i| self.polygon(i).contains(&Point(p)))
                    .collect();
                // Enclaves: the smallest area wins.
                hits.sort_by(|&a, &b| self.areas[a].total_cmp(&self.areas[b]));
                hits.first().copied()
            }
            GeometryKind::Line | GeometryKind::Point => {
                let r = Rect::new(
                    Coord {
                        x: p.x - tolerance,
                        y: p.y - tolerance,
                    },
                    Coord {
                        x: p.x + tolerance,
                        y: p.y + tolerance,
                    },
                );
                let dist = |i: usize| -> f64 {
                    let pts: Vec<Coord<f64>> = if self.kind == GeometryKind::Point {
                        self.points[i].0.iter().map(|q| q.0).collect()
                    } else {
                        self.lines[i]
                            .0
                            .iter()
                            .flat_map(|l| l.0.iter().copied())
                            .collect()
                    };
                    pts.iter()
                        .map(|q| (q.x - p.x).hypot(q.y - p.y))
                        .fold(f64::INFINITY, f64::min)
                };
                self.index
                    .within(r)
                    .map(|i| (i as usize, dist(i as usize)))
                    .filter(|(_, d)| *d <= tolerance * 1.5)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| i)
            }
        }
    }

    /// Groups of mutually connected features within `members` (contiguous
    /// parts of a territory).
    pub fn components(&self, members: &[u32]) -> usize {
        if members.is_empty() {
            return 0;
        }
        let set: std::collections::HashSet<u32> = members.iter().copied().collect();
        let mut seen = std::collections::HashSet::new();
        let mut parts = 0;
        for &start in members {
            if !seen.insert(start) {
                continue;
            }
            parts += 1;
            let mut stack = vec![start];
            while let Some(f) = stack.pop() {
                for &(n, _) in &self.neighbors[f as usize] {
                    if set.contains(&n) && seen.insert(n) {
                        stack.push(n);
                    }
                }
            }
        }
        parts
    }
}

/// Merges features with the same value in `column` (or the same first
/// `prefix` characters of it). The attributes are the first feature's;
/// with `sum_numbers` numeric columns hold the group's sums, and an
/// "Areas" column counts the merged features.
pub fn merge_by_column(
    geometry: &LayerGeometry,
    attributes: &Table,
    column: usize,
    prefix: Option<usize>,
    sum_numbers: bool,
) -> (LayerGeometry, Table) {
    use territoria_data::{Column, ColumnData, ColumnKind};
    let groups = attributes.groups_by(column, prefix);
    let firsts: Vec<usize> = groups.iter().map(|g| g[0]).collect();
    let mut table = attributes.take_rows(&firsts);
    if let Some(n) = prefix
        && let Some(c) = table.columns.get_mut(column)
        && let ColumnData::Text(v) = &mut c.data
    {
        for k in v.iter_mut().flatten() {
            *k = k.chars().take(n).collect();
        }
    }
    if sum_numbers {
        for (ci, c) in table.columns.iter_mut().enumerate() {
            if ci == column || !c.kind.is_numeric() {
                continue;
            }
            let src = &attributes.columns[ci];
            c.data = ColumnData::Number(
                groups
                    .iter()
                    .map(|g| {
                        let vals: Vec<f64> = g.iter().filter_map(|&r| src.f64(r)).collect();
                        (!vals.is_empty()).then(|| vals.iter().sum())
                    })
                    .collect(),
            );
        }
        let name = table.unique_name("Areas");
        let mut count = Column::number(name, groups.iter().map(|g| Some(g.len() as f64)).collect());
        count.kind = ColumnKind::Integer;
        let _ = table.push_column(count);
    }
    (geometry.merged(&groups), table)
}

/// Points from coordinate columns of a table (in `crs`, WGS84 when
/// `None`). Rows without coordinates get no point. Returns the geometry and
/// how many rows had no usable coordinates.
pub fn points_from_table(
    table: &Table,
    x: usize,
    y: usize,
    crs: Option<&territoria_geo::Crs>,
) -> Result<(LayerGeometry, usize), territoria_geo::CrsError> {
    let transform = match crs {
        Some(c) if !c.is_wgs84() => Some(territoria_geo::Transformer::new(
            c,
            &territoria_geo::Crs::wgs84(),
        )?),
        _ => None,
    };
    let mut missing = 0;
    let points = (0..table.rows())
        .map(|r| {
            let (Some(px), Some(py)) = (table.columns[x].f64(r), table.columns[y].f64(r)) else {
                missing += 1;
                return MultiPoint(vec![]);
            };
            let c = Coord { x: px, y: py };
            let c = match &transform {
                Some(t) => t.convert(c),
                None => Some(c),
            };
            match c.filter(|c| c.x.abs() <= 180.0 && c.y.abs() <= 90.0) {
                Some(c) => MultiPoint(vec![Point(c)]),
                None => {
                    missing += 1;
                    MultiPoint(vec![])
                }
            }
        })
        .collect();
    let geometry = LayerGeometry::from_parts(
        GeometryKind::Point,
        true,
        Topology::default(),
        Vec::new(),
        points,
    );
    Ok((geometry, missing))
}

/// Keys that occur on more than one feature.
pub fn duplicate_keys(attributes: &Table, column: usize) -> usize {
    attributes
        .groups_by(column, None)
        .iter()
        .filter(|g| g.len() > 1)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_types::polygon;

    pub fn grid(n: usize) -> LayerGeometry {
        let areas: Vec<MultiPolygon<f64>> = (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f64 * 0.1, (i / n) as f64 * 0.1);
                MultiPolygon(vec![polygon![
                    (x: x, y: y), (x: x + 0.1, y: y), (x: x + 0.1, y: y + 0.1), (x: x, y: y + 0.1), (x: x, y: y),
                ]])
            })
            .collect();
        LayerGeometry::from_imported(Geometries::Areas(areas), true)
    }

    #[test]
    fn merging_dissolves_inner_borders() {
        let g = grid(3);
        // Columns become features.
        let m = g.merged(&[vec![0, 3, 6], vec![1, 4, 7], vec![2, 5, 8]]);
        assert_eq!(m.len(), 3);
        assert_eq!(m.neighbors[1].len(), 2);
        assert!((m.areas[0] - 3.0 * g.areas[0]).abs() < 1.0);
        assert_eq!(m.components(&[0]), 1);
        // Inner borders of a merged feature have the same feature on both sides.
        assert!(m.topology.arcs.iter().filter(|a| a.a == a.b).count() >= 6);
    }

    #[test]
    fn derived_data() {
        let g = grid(3);
        assert_eq!(g.len(), 9);
        assert_eq!(g.neighbors[4].len(), 4);
        assert!((g.areas[0] - 123.6).abs() < 1.0, "{}", g.areas[0]);
        let lp = g.label_points[0].unwrap();
        assert!((lp.x - 0.05).abs() < 0.01 && (lp.y - 0.05).abs() < 0.01);
        assert_eq!(g.feature_at(Coord { x: 0.15, y: 0.25 }, 0.0), Some(7));
        assert_eq!(g.feature_at(Coord { x: 5.0, y: 5.0 }, 0.0), None);
        assert_eq!(g.components(&[0, 1, 2]), 1);
        assert_eq!(g.components(&[0, 2, 8]), 3);
        assert_eq!(g.components(&[0, 4]), 2);
    }
}
