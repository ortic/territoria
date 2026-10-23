//! Neighbours of points (customers, addresses) for planning with points as
//! units: points whose Voronoi cells touch, with the length of the shared
//! cell edge as their "border".

use std::collections::HashMap;

use geo_types::Coord;
use spade::{DelaunayTriangulation, Point2, Triangulation};

/// Neighbours of every point, with the length of the Voronoi edge between
/// them (in km for longitude/latitude, else in the coordinates' unit).
/// Points at the same position are each other's neighbours; points
/// without coordinates have none.
pub fn point_neighbors(points: &[Option<Coord<f64>>], geographic: bool) -> Vec<Vec<(u32, f64)>> {
    let n = points.len();
    let mut out: Vec<HashMap<u32, f64>> = vec![HashMap::new(); n];
    // Local planar coordinates: km around the mean latitude.
    let present: Vec<Coord<f64>> = points.iter().flatten().copied().collect();
    if present.len() < 2 {
        return vec![Vec::new(); n];
    }
    let lat0 = present.iter().map(|c| c.y).sum::<f64>() / present.len() as f64;
    let (kx, ky) = if geographic {
        (111.32 * lat0.to_radians().cos(), 110.57)
    } else {
        (1.0, 1.0)
    };
    // Group points at the same position.
    let mut groups: Vec<Vec<u32>> = Vec::new();
    let mut positions: Vec<Point2<f64>> = Vec::new();
    let mut index: HashMap<(i64, i64), usize> = HashMap::new();
    for (i, p) in points.iter().enumerate() {
        let Some(p) = p else { continue };
        let (x, y) = (p.x * kx, p.y * ky);
        if !(x.is_finite() && y.is_finite()) {
            continue;
        }
        let key = ((x * 1e6).round() as i64, (y * 1e6).round() as i64);
        match index.get(&key) {
            Some(&g) => groups[g].push(i as u32),
            None => {
                index.insert(key, groups.len());
                groups.push(vec![i as u32]);
                positions.push(Point2::new(x, y));
            }
        }
    }
    // Same position: neighbours with a token border.
    for g in &groups {
        for &a in g {
            for &b in g {
                if a != b {
                    out[a as usize].insert(b, 0.001);
                }
            }
        }
    }
    let mut tri: DelaunayTriangulation<Point2<f64>> = DelaunayTriangulation::new();
    let mut group_of_vertex: HashMap<usize, usize> = HashMap::new();
    for (g, p) in positions.iter().enumerate() {
        if let Ok(h) = tri.insert(*p) {
            group_of_vertex.insert(h.index(), g);
        }
    }
    for edge in tri.directed_edges() {
        let [a, b] = [edge.from(), edge.to()];
        let (Some(&ga), Some(&gb)) = (
            group_of_vertex.get(&a.fix().index()),
            group_of_vertex.get(&b.fix().index()),
        ) else {
            continue;
        };
        if ga >= gb {
            continue;
        }
        // The Voronoi edge between them: from circumcentre to circumcentre;
        // on the hull (one side open) the Delaunay edge's length stands in.
        let v = edge.as_voronoi_edge();
        let len = match (v.from().as_delaunay_face(), v.to().as_delaunay_face()) {
            (Some(f1), Some(f2)) => {
                let (c1, c2) = (circumcentre(f1.positions()), circumcentre(f2.positions()));
                ((c1.x - c2.x).powi(2) + (c1.y - c2.y).powi(2)).sqrt()
            }
            _ => edge.length_2().sqrt(),
        };
        let len = if len.is_finite() { len.max(1e-6) } else { 1e-6 };
        for &pa in &groups[ga] {
            for &pb in &groups[gb] {
                *out[pa as usize].entry(pb).or_default() += len;
                *out[pb as usize].entry(pa).or_default() += len;
            }
        }
    }
    out.into_iter()
        .map(|m| {
            let mut v: Vec<(u32, f64)> = m.into_iter().collect();
            v.sort_by_key(|(n, _)| *n);
            v
        })
        .collect()
}

/// Voronoi cells of the points (as areas), clipped to the points' extent
/// plus a margin. Points at the same position share one cell (the first
/// gets it, the others stay empty). Neighbouring cells share their corners
/// exactly, so they form a clean topology.
pub fn voronoi_cells(
    points: &[Option<Coord<f64>>],
    geographic: bool,
) -> Vec<geo_types::MultiPolygon<f64>> {
    use geo_types::{LineString, MultiPolygon, Polygon};
    let n = points.len();
    let mut out = vec![MultiPolygon(vec![]); n];
    let present: Vec<Coord<f64>> = points.iter().flatten().copied().collect();
    if present.is_empty() {
        return out;
    }
    let lat0 = present.iter().map(|c| c.y).sum::<f64>() / present.len() as f64;
    let (kx, ky) = if geographic {
        (111.32 * lat0.to_radians().cos(), 110.57)
    } else {
        (1.0, 1.0)
    };
    let mut firsts: Vec<usize> = Vec::new();
    let mut seen: HashMap<(i64, i64), ()> = HashMap::new();
    let mut positions: Vec<Point2<f64>> = Vec::new();
    for (i, p) in points.iter().enumerate() {
        let Some(p) = p else { continue };
        let (x, y) = (p.x * kx, p.y * ky);
        if !(x.is_finite() && y.is_finite()) {
            continue;
        }
        let key = ((x * 1e6).round() as i64, (y * 1e6).round() as i64);
        if seen.insert(key, ()).is_none() {
            firsts.push(i);
            positions.push(Point2::new(x, y));
        }
    }
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in &positions {
        (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
    }
    let size = (x1 - x0).max(y1 - y0).max(1e-3);
    let margin = size * 0.04;
    let clip = [x0 - margin, y0 - margin, x1 + margin, y1 + margin];
    let mut tri: DelaunayTriangulation<Point2<f64>> = DelaunayTriangulation::new();
    let mut handles = Vec::with_capacity(positions.len());
    for p in &positions {
        handles.push(tri.insert(*p).ok());
    }
    // Far-away ghosts close the cells of the outermost points.
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    for k in 0..8 {
        let a = k as f64 * std::f64::consts::FRAC_PI_4;
        let _ = tri.insert(Point2::new(
            cx + a.cos() * size * 10.0,
            cy + a.sin() * size * 10.0,
        ));
    }
    let mut centres: HashMap<usize, Point2<f64>> = HashMap::new();
    for (&i, h) in firsts.iter().zip(&handles) {
        let Some(h) = h else { continue };
        let v = tri.vertex(*h);
        let mut ring: Vec<Point2<f64>> = Vec::new();
        let mut closed = true;
        for e in v.out_edges() {
            match e.face().as_inner() {
                Some(f) => {
                    let c = *centres
                        .entry(f.fix().index())
                        .or_insert_with(|| circumcentre(f.positions()));
                    ring.push(c);
                }
                None => closed = false,
            }
        }
        if !closed || ring.len() < 3 {
            continue;
        }
        let ring = clip_rect(&ring, clip);
        if ring.len() < 3 {
            continue;
        }
        let mut coords: Vec<Coord<f64>> = ring
            .iter()
            .map(|p| Coord {
                x: p.x / kx,
                y: p.y / ky,
            })
            .collect();
        coords.push(coords[0]);
        out[i] = MultiPolygon(vec![Polygon::new(LineString(coords), vec![])]);
    }
    out
}

/// Sutherland–Hodgman clipping of a convex polygon to a rectangle
/// `[x0, y0, x1, y1]`.
fn clip_rect(poly: &[Point2<f64>], r: [f64; 4]) -> Vec<Point2<f64>> {
    let mut out = poly.to_vec();
    // (axis, bound, keep below?)
    for (axis, bound, below) in [
        (0, r[0], false),
        (0, r[2], true),
        (1, r[1], false),
        (1, r[3], true),
    ] {
        let input = std::mem::take(&mut out);
        if input.is_empty() {
            break;
        }
        let inside = |p: &Point2<f64>| {
            let v = if axis == 0 { p.x } else { p.y };
            if below { v <= bound } else { v >= bound }
        };
        let cross = |a: &Point2<f64>, b: &Point2<f64>| {
            let (va, vb) = if axis == 0 { (a.x, b.x) } else { (a.y, b.y) };
            let t = (bound - va) / (vb - va);
            Point2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
        };
        for i in 0..input.len() {
            let (a, b) = (&input[i], &input[(i + 1) % input.len()]);
            match (inside(a), inside(b)) {
                (true, true) => out.push(*b),
                (true, false) => out.push(cross(a, b)),
                (false, true) => {
                    out.push(cross(a, b));
                    out.push(*b);
                }
                (false, false) => {}
            }
        }
    }
    out
}

fn circumcentre(p: [Point2<f64>; 3]) -> Point2<f64> {
    let (ax, ay, bx, by, cx, cy) = (p[0].x, p[0].y, p[1].x, p[1].y, p[2].x, p[2].y);
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-18 {
        return Point2::new((ax + bx + cx) / 3.0, (ay + by + cy) / 3.0);
    }
    let a2 = ax * ax + ay * ay;
    let b2 = bx * bx + by * by;
    let c2 = cx * cx + cy * cy;
    Point2::new(
        (a2 * (by - cy) + b2 * (cy - ay) + c2 * (ay - by)) / d,
        (a2 * (cx - bx) + b2 * (ax - cx) + c2 * (bx - ax)) / d,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_points_neighbour_their_row_and_column() {
        // 3×3 grid, 1 apart (planar): the centre has 4 Voronoi neighbours
        // (diagonals touch only in a corner).
        let pts: Vec<Option<Coord<f64>>> = (0..9)
            .map(|i| {
                Some(Coord {
                    x: (i % 3) as f64,
                    y: (i / 3) as f64 + (i % 3) as f64 * 1e-7,
                })
            })
            .collect();
        let n = point_neighbors(&pts, false);
        let centre: Vec<u32> = n[4]
            .iter()
            .filter(|(_, l)| *l > 0.1)
            .map(|(i, _)| *i)
            .collect();
        assert_eq!(centre, vec![1, 3, 5, 7]);
    }

    #[test]
    fn cells_tile_the_extent() {
        use geo::Area;
        let pts: Vec<Option<Coord<f64>>> = (0..25)
            .map(|i| {
                Some(Coord {
                    x: (i % 5) as f64 + (i as f64 * 0.37).sin() * 0.2,
                    y: (i / 5) as f64,
                })
            })
            .chain([
                None,
                Some(Coord {
                    x: 0.0 + (0.0f64).sin() * 0.2,
                    y: 0.0,
                }),
            ])
            .collect();
        let cells = voronoi_cells(&pts, false);
        assert_eq!(cells.len(), 27);
        assert!(cells[25].0.is_empty());
        // The duplicate of point 0 shares its cell.
        assert!(cells[26].0.is_empty());
        let total: f64 = cells.iter().map(|c| c.unsigned_area()).sum();
        // The points' extent (4.4 × 4) plus a 4 % margin all round.
        assert!(total > 19.5 && total < 21.0, "{total}");
        // Cells form a topology with shared borders.
        let topo = crate::Topology::build(&cells);
        let n = topo.neighbors(false);
        assert!(n[12].len() >= 4, "{:?}", n[12]);
    }

    #[test]
    fn duplicates_and_missing() {
        let pts = vec![
            Some(Coord { x: 8.5, y: 47.4 }),
            Some(Coord { x: 8.5, y: 47.4 }),
            None,
            Some(Coord { x: 8.6, y: 47.4 }),
            Some(Coord { x: 8.55, y: 47.5 }),
        ];
        let n = point_neighbors(&pts, true);
        assert!(n[0].iter().any(|(i, _)| *i == 1));
        assert!(n[2].is_empty());
        assert!(n[3].iter().any(|(i, _)| *i == 0));
        assert!(n[3].iter().any(|(i, _)| *i == 1));
    }
}
