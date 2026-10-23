//! A selection carried to another layer: the features of one layer that
//! lie in, touch or have their centre in the selected features of another
//! (or, from points, the areas the points lie in).

use std::collections::HashMap;

use geo::BoundingRect;
use geo_types::{Coord, MultiPolygon, Rect};
use rayon::prelude::*;
use territoria_io::GeometryKind;

use crate::Layer;

/// How a feature relates to the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// Its centre (label point) lies in the selection.
    CentreInside,
    /// It overlaps or shares a border with the selection.
    Touching,
    /// It lies wholly in the selection.
    Contained,
}

impl Relation {
    pub const ALL: [Relation; 3] = [
        Relation::CentreInside,
        Relation::Contained,
        Relation::Touching,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Relation::CentreInside => "with their centre inside",
            Relation::Touching => "touching or overlapping",
            Relation::Contained => "wholly inside",
        }
    }
}

/// A polygon prepared for many point tests: its edges sorted into
/// horizontal bands, so that a test looks only at the edges near the
/// point (large merged regions have tens of thousands).
struct Banded {
    bbox: Rect<f64>,
    band: f64,
    bands: Vec<Vec<[Coord<f64>; 2]>>,
}

impl Banded {
    fn new(poly: &MultiPolygon<f64>) -> Option<Banded> {
        let bbox = poly.bounding_rect()?;
        let edges: Vec<[Coord<f64>; 2]> = poly
            .0
            .iter()
            .flat_map(|p| std::iter::once(p.exterior()).chain(p.interiors()))
            .flat_map(|ring| ring.0.windows(2).map(|w| [w[0], w[1]]))
            .collect();
        let n = ((edges.len() as f64).sqrt().ceil() as usize).clamp(1, 4096);
        let band = (bbox.height() / n as f64).max(1e-12);
        let mut bands = vec![Vec::new(); n];
        for e in edges {
            let (lo, hi) = (e[0].y.min(e[1].y), e[0].y.max(e[1].y));
            let (a, b) = (
                ((lo - bbox.min().y) / band) as usize,
                ((hi - bbox.min().y) / band) as usize,
            );
            for slot in bands.iter_mut().take(b.min(n - 1) + 1).skip(a.min(n - 1)) {
                slot.push(e);
            }
        }
        Some(Banded { bbox, band, bands })
    }

    fn band_of(&self, y: f64) -> usize {
        (((y - self.bbox.min().y) / self.band) as usize).min(self.bands.len() - 1)
    }

    /// Even-odd ray casting over the edges in the point's band.
    fn contains(&self, p: Coord<f64>) -> bool {
        let (min, max) = (self.bbox.min(), self.bbox.max());
        if p.x < min.x || p.x > max.x || p.y < min.y || p.y > max.y {
            return false;
        }
        let mut inside = false;
        for [a, c] in &self.bands[self.band_of(p.y)] {
            if (a.y > p.y) != (c.y > p.y) {
                let x = a.x + (p.y - a.y) * (c.x - a.x) / (c.y - a.y);
                if x > p.x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// Whether `p` lies on an edge, within `eps`.
    fn on_edge(&self, p: Coord<f64>, eps: f64) -> bool {
        let (min, max) = (self.bbox.min(), self.bbox.max());
        if p.x < min.x - eps || p.x > max.x + eps || p.y < min.y - eps || p.y > max.y + eps {
            return false;
        }
        let (a, b) = (self.band_of(p.y - eps), self.band_of(p.y + eps));
        self.bands[a..=b].iter().flatten().any(|[s, e]| {
            let (dx, dy) = (e.x - s.x, e.y - s.y);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 {
                (((p.x - s.x) * dx + (p.y - s.y) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (qx, qy) = (s.x + t * dx - p.x, s.y + t * dy - p.y);
            qx * qx + qy * qy <= eps * eps
        })
    }
}

/// The corners of a feature (areas: outer rings; lines and points: all).
fn vertices(layer: &Layer, f: usize) -> Vec<Coord<f64>> {
    let g = &layer.geometry;
    match g.kind {
        GeometryKind::Area => g
            .polygon(f)
            .0
            .iter()
            .flat_map(|p| p.exterior().0.iter().copied())
            .collect(),
        GeometryKind::Line => g.lines[f]
            .0
            .iter()
            .flat_map(|l| l.0.iter().copied())
            .collect(),
        GeometryKind::Point => g.points[f].0.iter().map(|p| p.0).collect(),
    }
}

/// The features of `to` related to `selected` features of `from`.
pub fn transfer(from: &Layer, selected: &[u32], to: &Layer, relation: Relation) -> Vec<u32> {
    let (fk, tk) = (from.kind(), to.kind());
    if fk != GeometryKind::Area {
        // From points or lines: the areas they lie in.
        if tk != GeometryKind::Area {
            return Vec::new();
        }
        let mut hits: Vec<u32> = selected
            .iter()
            .flat_map(|&s| vertices(from, s as usize))
            .filter_map(|p| to.geometry.feature_at(p, 0.0).map(|f| f as u32))
            .collect();
        hits.sort_unstable();
        hits.dedup();
        return hits;
    }
    let polygons: HashMap<u32, Banded> = selected
        .par_iter()
        .filter_map(|&s| Some((s, Banded::new(&from.geometry.polygon(s as usize))?)))
        .collect();
    let index = &from.geometry.index;
    // Shared borders: the same corners, give or take rounding.
    let eps = if from.geometry.geographic { 1e-7 } else { 1e-3 };
    // Whether `p` lies in (or with `edge`, on the border of) the selection.
    let inside = |p: Coord<f64>, edge: bool| {
        index.at(p).any(|i| {
            polygons
                .get(&i)
                .is_some_and(|b| b.contains(p) || (edge && b.on_edge(p, eps)))
        })
    };
    let n = to.len();
    let centres = &to.geometry.label_points;
    (0..n as u32)
        .into_par_iter()
        .filter(|&f| {
            let fu = f as usize;
            let centre = centres.get(fu).copied().flatten();
            if tk == GeometryKind::Point || relation == Relation::CentreInside {
                return match tk {
                    GeometryKind::Point => vertices(to, fu).into_iter().any(|p| inside(p, false)),
                    _ => centre.is_some_and(|c| inside(c, false)),
                };
            }
            let corners = vertices(to, fu);
            match relation {
                Relation::Touching => {
                    centre.is_some_and(|c| inside(c, false))
                        || corners.iter().any(|p| inside(*p, true))
                }
                _ => {
                    // Wholly inside: its corners lie in the selection or on
                    // its border (a few may stray where outlines differ).
                    let hits = corners.iter().filter(|p| inside(**p, true)).count();
                    centre.is_some_and(|c| inside(c, false)) && hits * 20 >= corners.len() * 19
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_types::{LineString, Polygon};

    #[test]
    fn banded_point_tests() {
        // A square with a square hole.
        let ring = |a: f64, b: f64| LineString::from(vec![(a, a), (b, a), (b, b), (a, b), (a, a)]);
        let poly = MultiPolygon(vec![Polygon::new(ring(0.0, 10.0), vec![ring(4.0, 6.0)])]);
        let b = Banded::new(&poly).unwrap();
        let c = |x, y| Coord { x, y };
        assert!(b.contains(c(1.0, 1.0)));
        assert!(b.contains(c(9.5, 5.0)));
        assert!(!b.contains(c(5.0, 5.0)), "in the hole");
        assert!(!b.contains(c(11.0, 5.0)));
        assert!(b.on_edge(c(10.0, 3.0), 1e-9));
        assert!(b.on_edge(c(4.0, 5.0), 1e-9), "the hole's edge");
        assert!(!b.on_edge(c(9.0, 3.0), 1e-9));
    }
}
