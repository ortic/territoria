//! Display geometry of a layer: projected into the map's coordinate system
//! and simplified into levels of detail, with polygon fills triangulated.
//!
//! Coordinates are stored as `f32` offsets from the layer's centre, which
//! keeps them precise at any zoom while halving memory.

use std::sync::Arc;

use geo_types::Coord;
use rayon::prelude::*;
use territoria_geo::simplify::simplify;
use territoria_geo::{Crs, Topology, Transformer};
use territoria_io::GeometryKind;
use territoria_model::LayerGeometry;

/// A box in display coordinates relative to the mesh origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl Bounds {
    pub const EMPTY: Bounds = Bounds {
        min: [f32::INFINITY; 2],
        max: [f32::NEG_INFINITY; 2],
    };

    pub fn add(&mut self, p: [f32; 2]) {
        self.min[0] = self.min[0].min(p[0]);
        self.min[1] = self.min[1].min(p[1]);
        self.max[0] = self.max[0].max(p[0]);
        self.max[1] = self.max[1].max(p[1]);
    }

    pub fn of(points: &[[f32; 2]]) -> Bounds {
        let mut b = Bounds::EMPTY;
        for &p in points {
            b.add(p);
        }
        b
    }

    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0]
    }

    pub fn intersects(&self, other: &Bounds) -> bool {
        self.min[0] <= other.max[0]
            && self.max[0] >= other.min[0]
            && self.min[1] <= other.max[1]
            && self.max[1] >= other.min[1]
    }

    pub fn width(&self) -> f32 {
        self.max[0] - self.min[0]
    }

    pub fn height(&self) -> f32 {
        self.max[1] - self.min[1]
    }
}

/// Triangulated fill of one feature.
#[derive(Debug, Clone, Default)]
pub struct Fill {
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Level {
    /// Simplification tolerance in display units (0 = full detail).
    pub tolerance: f64,
    /// Areas: per feature its fill.
    pub fills: Vec<Fill>,
    /// Areas: per topology arc its simplified line. Lines: per feature its
    /// parts flattened (see `line_parts`).
    pub arcs: Vec<Vec<[f32; 2]>>,
    pub arc_bounds: Vec<Bounds>,
    /// Lines: per feature, the range of `arcs` holding its parts.
    pub line_parts: Vec<std::ops::Range<u32>>,
}

/// One layer prepared for drawing in one coordinate system.
pub struct LayerMesh {
    pub kind: GeometryKind,
    /// Proj string of the display CRS this was built for.
    pub crs: String,
    /// Layer revision this was built from.
    pub rev: u64,
    pub origin: [f64; 2],
    pub bounds: Vec<Bounds>,
    pub extent: Bounds,
    pub label_points: Vec<Option<[f32; 2]>>,
    /// Points layers: per feature its points.
    pub points: Vec<Vec<[f32; 2]>>,
    /// Coarse to fine.
    pub levels: Vec<Level>,
    pub geometry: Arc<LayerGeometry>,
}

impl LayerMesh {
    /// A mesh with nothing in it, for layers drawn without geometry
    /// (rings around centres).
    pub fn empty() -> LayerMesh {
        LayerMesh {
            kind: GeometryKind::Point,
            crs: String::new(),
            rev: 0,
            origin: [0.0, 0.0],
            bounds: Vec::new(),
            extent: Bounds::EMPTY,
            label_points: Vec::new(),
            points: Vec::new(),
            levels: vec![Level::default()],
            geometry: Arc::new(LayerGeometry::default()),
        }
    }

    /// Projects and simplifies `geometry` for display in `crs`. Unknown
    /// (non-geographic) coordinates are used as they are.
    pub fn build(geometry: Arc<LayerGeometry>, crs: &Crs, rev: u64) -> LayerMesh {
        let transform = if geometry.geographic {
            Transformer::new(&Crs::wgs84(), crs).ok()
        } else {
            None
        };
        let project = |c: Coord<f64>| -> Coord<f64> {
            match &transform {
                Some(t) => t.convert(c).unwrap_or(Coord {
                    x: f64::NAN,
                    y: f64::NAN,
                }),
                None => c,
            }
        };
        let g = &*geometry;

        // Project everything in f64 first.
        let arcs64: Vec<Vec<Coord<f64>>> = g
            .topology
            .arcs
            .par_iter()
            .map(|a| a.points.iter().map(|&c| project(c)).collect())
            .collect();
        let lines64: Vec<Vec<Vec<Coord<f64>>>> = g
            .lines
            .par_iter()
            .map(|ml| {
                ml.0.iter()
                    .map(|l| l.0.iter().map(|&c| project(c)).collect())
                    .collect()
            })
            .collect();
        let points64: Vec<Vec<Coord<f64>>> = g
            .points
            .iter()
            .map(|mp| mp.0.iter().map(|p| project(p.0)).collect())
            .collect();

        let mut ext64 = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        let mut grow = |c: &Coord<f64>| {
            if c.x.is_finite() && c.y.is_finite() {
                ext64 = (
                    ext64.0.min(c.x),
                    ext64.1.min(c.y),
                    ext64.2.max(c.x),
                    ext64.3.max(c.y),
                );
            }
        };
        arcs64.iter().flatten().for_each(&mut grow);
        lines64.iter().flatten().flatten().for_each(&mut grow);
        points64.iter().flatten().for_each(&mut grow);
        let origin = if ext64.0.is_finite() {
            [(ext64.0 + ext64.2) / 2.0, (ext64.1 + ext64.3) / 2.0]
        } else {
            [0.0, 0.0]
        };
        let size = if ext64.0.is_finite() {
            (ext64.2 - ext64.0).max(ext64.3 - ext64.1).max(1e-9)
        } else {
            1.0
        };
        let rel = |c: &Coord<f64>| [(c.x - origin[0]) as f32, (c.y - origin[1]) as f32];

        // Tolerances: a 2000-pixel view of the whole layer, then ×1/4 per
        // level, then full detail.
        let mut tolerances = Vec::new();
        let mut t = size / 2000.0;
        while t > size / 2_000_000.0 && tolerances.len() < 6 {
            tolerances.push(t);
            t /= 4.0;
        }
        tolerances.push(0.0);

        let levels: Vec<Level> = tolerances
            .par_iter()
            .map(|&tol| match g.kind {
                GeometryKind::Area => area_level(&g.topology, &arcs64, tol, &rel),
                GeometryKind::Line => line_level(&lines64, tol, &rel),
                GeometryKind::Point => Level {
                    tolerance: tol,
                    ..Default::default()
                },
            })
            .collect();

        let points: Vec<Vec<[f32; 2]>> = points64
            .iter()
            .map(|ps| ps.iter().filter(|c| c.x.is_finite()).map(&rel).collect())
            .collect();
        let finest = levels.last();
        let bounds: Vec<Bounds> = match g.kind {
            GeometryKind::Area => g
                .topology
                .features
                .iter()
                .map(|polys| {
                    let mut b = Bounds::EMPTY;
                    for p in polys {
                        // The exterior ring holds the whole polygon.
                        for r in p.rings.iter().take(1).flatten() {
                            let arc = &finest.map(|l| &l.arcs[r.arc as usize]);
                            for &pt in arc.map(|a| a.as_slice()).unwrap_or_default() {
                                b.add(pt);
                            }
                        }
                    }
                    b
                })
                .collect(),
            GeometryKind::Line => finest
                .map(|l| {
                    l.line_parts
                        .iter()
                        .map(|r| {
                            let mut b = Bounds::EMPTY;
                            for a in &l.arcs[r.start as usize..r.end as usize] {
                                a.iter().for_each(|&p| b.add(p));
                            }
                            b
                        })
                        .collect()
                })
                .unwrap_or_default(),
            GeometryKind::Point => points.iter().map(|p| Bounds::of(p)).collect(),
        };
        let mut extent = Bounds::EMPTY;
        for b in bounds.iter().filter(|b| !b.is_empty()) {
            extent.add(b.min);
            extent.add(b.max);
        }
        let label_points = g
            .label_points
            .iter()
            .map(|lp| {
                lp.map(project)
                    .filter(|c| c.x.is_finite() && c.y.is_finite())
                    .map(|c| rel(&c))
            })
            .collect();
        LayerMesh {
            kind: g.kind,
            crs: crs.proj.clone(),
            rev,
            origin,
            bounds,
            extent,
            label_points,
            points,
            levels,
            geometry,
        }
    }

    /// The coarsest level that still looks right at `units_per_pixel`.
    pub fn level_for(&self, units_per_pixel: f64) -> &Level {
        self.levels
            .iter()
            .find(|l| l.tolerance <= units_per_pixel * 0.6)
            .unwrap_or_else(|| self.levels.last().expect("at least the full level"))
    }

    /// Display coordinates of a point relative to the origin.
    pub fn to_display(&self, p: [f32; 2]) -> [f64; 2] {
        [self.origin[0] + p[0] as f64, self.origin[1] + p[1] as f64]
    }

    /// The feature's rings at `level` (for outlines).
    pub fn rings(&self, level: &Level, feature: usize) -> Vec<Vec<[f32; 2]>> {
        let Some(polys) = self.geometry.topology.features.get(feature) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for p in polys {
            for refs in &p.rings {
                let mut ring = Vec::new();
                for r in refs {
                    let pts = &level.arcs[r.arc as usize];
                    let skip = usize::from(!ring.is_empty());
                    if r.reversed {
                        ring.extend(pts.iter().rev().skip(skip));
                    } else {
                        ring.extend(pts.iter().skip(skip));
                    }
                }
                if ring.len() >= 3 {
                    out.push(ring);
                }
            }
        }
        out
    }
}

fn area_level(
    topo: &Topology,
    arcs64: &[Vec<Coord<f64>>],
    tolerance: f64,
    rel: &(impl Fn(&Coord<f64>) -> [f32; 2] + Sync),
) -> Level {
    let arcs: Vec<Vec<[f32; 2]>> = arcs64
        .par_iter()
        .map(|a| {
            let mut s = Vec::new();
            simplify(a, tolerance, &mut s);
            s.iter().filter(|c| c.x.is_finite()).map(rel).collect()
        })
        .collect();
    let arc_bounds = arcs.iter().map(|a| Bounds::of(a)).collect();
    let fills = topo
        .features
        .par_iter()
        .map(|polys| {
            let mut fill = Fill::default();
            for p in polys {
                let mut flat: Vec<f64> = Vec::new();
                let mut holes: Vec<usize> = Vec::new();
                let mut ring_pts: Vec<[f32; 2]> = Vec::new();
                for (ri, refs) in p.rings.iter().enumerate() {
                    let mut ring: Vec<[f32; 2]> = Vec::new();
                    for r in refs {
                        let pts = &arcs[r.arc as usize];
                        let skip = usize::from(!ring.is_empty());
                        if r.reversed {
                            ring.extend(pts.iter().rev().skip(skip));
                        } else {
                            ring.extend(pts.iter().skip(skip));
                        }
                    }
                    // Closed rings repeat the first point; earcut wants it once.
                    if ring.len() > 1 && ring.first() == ring.last() {
                        ring.pop();
                    }
                    if ring.len() < 3 {
                        if ri == 0 {
                            break;
                        }
                        continue;
                    }
                    if ri > 0 {
                        holes.push(ring_pts.len());
                    }
                    for pt in &ring {
                        flat.push(pt[0] as f64);
                        flat.push(pt[1] as f64);
                    }
                    ring_pts.extend(ring);
                }
                if ring_pts.len() < 3 {
                    continue;
                }
                if let Ok(tris) = earcutr::earcut(&flat, &holes, 2) {
                    let base = fill.vertices.len() as u32;
                    fill.vertices.extend(ring_pts);
                    fill.indices
                        .extend(tris.into_iter().map(|i| base + i as u32));
                }
            }
            fill
        })
        .collect();
    Level {
        tolerance,
        fills,
        arcs,
        arc_bounds,
        line_parts: Vec::new(),
    }
}

fn line_level(
    lines64: &[Vec<Vec<Coord<f64>>>],
    tolerance: f64,
    rel: &(impl Fn(&Coord<f64>) -> [f32; 2] + Sync),
) -> Level {
    let mut arcs = Vec::new();
    let mut line_parts = Vec::new();
    for parts in lines64 {
        let start = arcs.len() as u32;
        for part in parts {
            let mut s = Vec::new();
            simplify(part, tolerance, &mut s);
            arcs.push(
                s.iter()
                    .filter(|c| c.x.is_finite())
                    .map(rel)
                    .collect::<Vec<_>>(),
            );
        }
        line_parts.push(start..arcs.len() as u32);
    }
    let arc_bounds = arcs.iter().map(|a: &Vec<[f32; 2]>| Bounds::of(a)).collect();
    Level {
        tolerance,
        fills: Vec::new(),
        arcs,
        arc_bounds,
        line_parts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_types::{MultiPolygon, polygon};
    use territoria_io::Geometries;

    #[test]
    fn builds_levels_with_fills() {
        let areas: Vec<MultiPolygon<f64>> = (0..3)
            .map(|i| {
                let x = i as f64;
                MultiPolygon(vec![polygon![
                    (x: x, y: 45.0), (x: x + 1.0, y: 45.0), (x: x + 1.0, y: 46.0), (x: x + 0.5, y: 46.2), (x: x, y: 46.0), (x: x, y: 45.0),
                ]])
            })
            .collect();
        let g = Arc::new(LayerGeometry::from_imported(Geometries::Areas(areas), true));
        let m = LayerMesh::build(g, &Crs::web_mercator(), 1);
        assert!(m.levels.len() >= 2);
        let full = m.levels.last().unwrap();
        assert_eq!(full.tolerance, 0.0);
        assert_eq!(full.fills.len(), 3);
        // A pentagon: three triangles.
        assert_eq!(full.fills[0].indices.len(), 9);
        assert!(!m.bounds[0].is_empty());
        assert!(m.bounds[0].intersects(&m.bounds[1]));
        assert!(!m.bounds[0].intersects(&m.bounds[2]));
        assert_eq!(m.rings(full, 1).len(), 1);
        // Coarse levels for a zoomed-out view, full detail up close.
        let size = m.extent.width() as f64;
        assert!(m.level_for(size).tolerance > 0.0);
        assert_eq!(m.level_for(1e-9).tolerance, 0.0);
    }
}
