//! Shared-border topology of an area layer.
//!
//! Polygon rings are cut at junctions (vertices where three or more areas
//! meet) into arcs, and arcs shared by two areas are stored once. That gives:
//!
//! - simplification without gaps or slivers between neighbours (each shared
//!   border is simplified once),
//! - adjacency and shared border lengths (for contiguity and compactness),
//! - territory borders as the arcs whose two sides belong to different
//!   territories, without polygon unions.
//!
//! Vertices are matched after rounding to 1e-9 units (about 0.1 mm in
//! degrees), so borders digitized identically on both sides are found.
//! Borders that don't share vertices aren't recognized as shared.

use std::collections::HashMap;

use geo_types::{Coord, LineString, MultiPolygon, Polygon};
use serde::{Deserialize, Serialize};

/// No feature on that side of an arc (the outside of the layer).
pub const OUTSIDE: u32 = u32::MAX;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arc {
    /// At least two points; a ring without junctions is one closed arc.
    pub points: Vec<Coord<f64>>,
    /// The features on its two sides; `b` is [`OUTSIDE`] for an outer border
    /// and equal to `a` for a border inside one feature.
    pub a: u32,
    pub b: u32,
}

impl Arc {
    /// A border between two different features.
    pub fn is_shared(&self) -> bool {
        self.b != OUTSIDE && self.a != self.b
    }
}

/// An arc used in a ring, in its stored or in reverse direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArcRef {
    pub arc: u32,
    pub reversed: bool,
}

/// A polygon as rings of arcs; the first ring is the exterior.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TopoPolygon {
    pub rings: Vec<Vec<ArcRef>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Topology {
    pub arcs: Vec<Arc>,
    /// Per feature, its polygons (empty for features without geometry).
    pub features: Vec<Vec<TopoPolygon>>,
}

const SCALE: f64 = 1e9;

fn key(c: Coord<f64>) -> (i64, i64) {
    ((c.x * SCALE).round() as i64, (c.y * SCALE).round() as i64)
}

impl Topology {
    pub fn build(features: &[MultiPolygon<f64>]) -> Topology {
        // Intern vertices; shared vertices get identical coordinates.
        let mut ids: HashMap<(i64, i64), u32> = HashMap::new();
        let mut coords: Vec<Coord<f64>> = Vec::new();
        // (feature, polygon, ring index, vertex ids without the closing one)
        let mut rings: Vec<(u32, u32, u32, Vec<u32>)> = Vec::new();
        for (f, mp) in features.iter().enumerate() {
            for (p, poly) in mp.0.iter().enumerate() {
                let all = std::iter::once(poly.exterior()).chain(poly.interiors());
                for (r, ring) in all.enumerate() {
                    let mut vs: Vec<u32> = Vec::with_capacity(ring.0.len());
                    for &c in &ring.0 {
                        if !(c.x.is_finite() && c.y.is_finite()) {
                            continue;
                        }
                        let next = coords.len() as u32;
                        let id = *ids.entry(key(c)).or_insert_with(|| {
                            coords.push(c);
                            next
                        });
                        if vs.last() != Some(&id) {
                            vs.push(id);
                        }
                    }
                    while vs.len() > 1 && vs.first() == vs.last() {
                        vs.pop();
                    }
                    if vs.len() >= 3 {
                        rings.push((f as u32, p as u32, r as u32, vs));
                    }
                }
            }
        }
        drop(ids);

        // A vertex is a junction when rings pass it with different neighbours.
        const UNSEEN: (u32, u32) = (u32::MAX, u32::MAX);
        let mut pairs = vec![UNSEEN; coords.len()];
        let mut junction = vec![false; coords.len()];
        for (_, _, _, vs) in &rings {
            let n = vs.len();
            for i in 0..n {
                let (prev, next) = (vs[(i + n - 1) % n], vs[(i + 1) % n]);
                let pair = (prev.min(next), prev.max(next));
                let v = vs[i] as usize;
                if pairs[v] == UNSEEN {
                    pairs[v] = pair;
                } else if pairs[v] != pair {
                    junction[v] = true;
                }
            }
        }
        drop(pairs);

        let mut topo = Topology {
            arcs: Vec::new(),
            features: vec![Vec::new(); features.len()],
        };
        let mut arc_ids: HashMap<Vec<u32>, u32> = HashMap::new();
        for (f, p, r, vs) in rings {
            let n = vs.len();
            let mut refs = Vec::new();
            match (0..n).find(|&i| junction[vs[i] as usize]) {
                None => {
                    // Closed arc: start at the smallest vertex, go towards the
                    // smaller neighbour, so both sides store it the same way.
                    let start = (0..n).min_by_key(|&i| vs[i]).unwrap();
                    let fwd = vs[(start + 1) % n] < vs[(start + n - 1) % n];
                    let mut seq: Vec<u32> = (0..=n)
                        .map(|k| {
                            let i = if fwd { start + k } else { start + n - k };
                            vs[i % n]
                        })
                        .collect();
                    seq.dedup();
                    refs.push(topo.intern(&mut arc_ids, &coords, seq, !fwd, f));
                }
                Some(first) => {
                    let mut seq = vec![vs[first]];
                    for k in 1..=n {
                        let v = vs[(first + k) % n];
                        seq.push(v);
                        if junction[v as usize] {
                            let mut canon = std::mem::replace(&mut seq, vec![v]);
                            let rev = canon.first() > canon.last()
                                || (canon.first() == canon.last()
                                    && canon.len() > 2
                                    && canon[1] > canon[canon.len() - 2]);
                            if rev {
                                canon.reverse();
                            }
                            refs.push(topo.intern(&mut arc_ids, &coords, canon, rev, f));
                        }
                    }
                }
            }
            let polys = &mut topo.features[f as usize];
            while polys.len() <= p as usize {
                polys.push(TopoPolygon::default());
            }
            let rings = &mut polys[p as usize].rings;
            // An exterior that collapsed leaves its holes without a polygon.
            if r == 0 || !rings.is_empty() {
                rings.push(refs);
            }
        }
        for polys in &mut topo.features {
            polys.retain(|p| !p.rings.is_empty());
        }
        topo
    }

    fn intern(
        &mut self,
        ids: &mut HashMap<Vec<u32>, u32>,
        coords: &[Coord<f64>],
        seq: Vec<u32>,
        reversed: bool,
        feature: u32,
    ) -> ArcRef {
        let arc = match ids.get(&seq) {
            Some(&id) => {
                let a = &mut self.arcs[id as usize];
                if a.b == OUTSIDE {
                    a.b = feature;
                }
                id
            }
            None => {
                let id = self.arcs.len() as u32;
                self.arcs.push(Arc {
                    points: seq.iter().map(|&v| coords[v as usize]).collect(),
                    a: feature,
                    b: OUTSIDE,
                });
                ids.insert(seq, id);
                id
            }
        };
        ArcRef { arc, reversed }
    }

    pub fn len(&self) -> usize {
        self.features.len()
    }

    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Appends a ring's coordinates (closed) built from `arcs`, which may be
    /// a simplified copy of [`Topology::arcs`].
    pub fn assemble_ring<'a>(
        refs: &[ArcRef],
        arc_points: impl Fn(u32) -> &'a [Coord<f64>],
        out: &mut Vec<Coord<f64>>,
    ) {
        let start = out.len();
        for r in refs {
            let pts = arc_points(r.arc);
            let skip = usize::from(out.len() > start);
            if r.reversed {
                out.extend(pts.iter().rev().skip(skip));
            } else {
                out.extend(pts.iter().skip(skip));
            }
        }
        if out.len() > start && out[start] != out[out.len() - 1] {
            out.push(out[start]);
        }
    }

    /// The feature's geometry at full resolution.
    pub fn polygon(&self, feature: usize) -> MultiPolygon<f64> {
        let points = |a: u32| self.arcs[a as usize].points.as_slice();
        MultiPolygon(
            self.features[feature]
                .iter()
                .map(|p| {
                    let mut rings = p.rings.iter().map(|refs| {
                        let mut v = Vec::new();
                        Topology::assemble_ring(refs, points, &mut v);
                        LineString(v)
                    });
                    let exterior = rings.next().unwrap_or_else(|| LineString(vec![]));
                    Polygon::new(exterior, rings.collect())
                })
                .collect(),
        )
    }

    /// The outline of a group of features as one multipolygon: the arcs with
    /// a member on one side only, chained into rings. Inner borders vanish,
    /// enclosed non-members become holes.
    pub fn dissolve(&self, members: &[u32]) -> MultiPolygon<f64> {
        use std::collections::HashSet;
        let set: HashSet<u32> = members.iter().copied().collect();
        // Directed boundary edges, each ring normalized to counter-clockwise
        // exteriors and clockwise holes so neighbours chain consistently.
        let mut edges: Vec<Vec<Coord<f64>>> = Vec::new();
        for &f in members {
            let Some(polys) = self.features.get(f as usize) else {
                continue;
            };
            for p in polys {
                for (ri, refs) in p.rings.iter().enumerate() {
                    let mut ring = Vec::new();
                    Topology::assemble_ring(
                        refs,
                        |a| self.arcs[a as usize].points.as_slice(),
                        &mut ring,
                    );
                    let ccw = signed_area(&ring) > 0.0;
                    let flip = if ri == 0 { !ccw } else { ccw };
                    for r in refs {
                        let arc = &self.arcs[r.arc as usize];
                        let other = if arc.a == f { arc.b } else { arc.a };
                        let internal = arc.a == arc.b
                            || (other != OUTSIDE && set.contains(&other) && arc.b != OUTSIDE);
                        if internal {
                            continue;
                        }
                        let mut pts = arc.points.clone();
                        if r.reversed != flip {
                            pts.reverse();
                        }
                        edges.push(pts);
                    }
                }
            }
        }
        // Chain edges end to start into rings.
        let mut by_start: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, e) in edges.iter().enumerate() {
            by_start.entry(key(e[0])).or_default().push(i);
        }
        let mut used = vec![false; edges.len()];
        let mut rings: Vec<Vec<Coord<f64>>> = Vec::new();
        for start in 0..edges.len() {
            if used[start] {
                continue;
            }
            used[start] = true;
            let mut ring = edges[start].clone();
            let first = key(ring[0]);
            loop {
                let end = key(*ring.last().unwrap());
                if end == first && ring.len() > 2 {
                    break;
                }
                let next = by_start
                    .get(&end)
                    .and_then(|c| c.iter().copied().find(|&i| !used[i]));
                match next {
                    Some(i) => {
                        used[i] = true;
                        ring.extend(edges[i].iter().skip(1));
                    }
                    None => break,
                }
            }
            if ring.len() >= 4 && key(ring[0]) == key(*ring.last().unwrap()) {
                rings.push(ring);
            }
        }
        // Counter-clockwise rings are exteriors, clockwise ones holes; a
        // hole goes into the smallest exterior around it.
        let (mut outers, holes): (Vec<_>, Vec<_>) =
            rings.into_iter().partition(|r| signed_area(r) > 0.0);
        outers.sort_by(|a, b| signed_area(a).total_cmp(&signed_area(b)));
        let mut polys: Vec<Polygon<f64>> = outers
            .into_iter()
            .map(|r| Polygon::new(LineString(r), vec![]))
            .collect();
        for h in holes {
            let probe = geo_types::Point(h[0]);
            let inside = polys.iter().position(|p| {
                use geo::Contains;
                p.exterior().0.len() > 3
                    && Polygon::new(p.exterior().clone(), vec![]).contains(&probe)
                    || p.exterior().0.contains(&h[0])
            });
            match inside {
                Some(i) => polys[i].interiors_push(LineString(h)),
                None => {
                    let mut r = h;
                    r.reverse();
                    polys.push(Polygon::new(LineString(r), vec![]));
                }
            }
        }
        MultiPolygon(polys)
    }

    /// Neighbours of every feature with the length of their shared border:
    /// in metres for coordinates in degrees (`geographic`), else in the
    /// coordinates' unit.
    pub fn neighbors(&self, geographic: bool) -> Vec<Vec<(u32, f64)>> {
        let mut out: Vec<HashMap<u32, f64>> = vec![HashMap::new(); self.features.len()];
        for arc in &self.arcs {
            if arc.is_shared() {
                let len = line_length(&arc.points, geographic);
                *out[arc.a as usize].entry(arc.b).or_default() += len;
                *out[arc.b as usize].entry(arc.a).or_default() += len;
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

    /// Perimeter per feature, in the unit of [`Topology::neighbors`].
    pub fn perimeters(&self, geographic: bool) -> Vec<f64> {
        let mut out = vec![0.0; self.features.len()];
        for arc in &self.arcs {
            let len = line_length(&arc.points, geographic);
            out[arc.a as usize] += len;
            if arc.b != OUTSIDE && arc.b != arc.a {
                out[arc.b as usize] += len;
            }
        }
        out
    }
}

/// Neighbours of polygons whose borders don't share vertices exactly
/// (hand-made or converted files): features with vertices within
/// `tolerance` of each other. The shared length is estimated from the
/// close vertices.
pub fn approximate_neighbors(
    polygons: &[MultiPolygon<f64>],
    tolerance: f64,
    geographic: bool,
) -> Vec<Vec<(u32, f64)>> {
    use rstar::{RTree, primitives::GeomWithData};
    let mut points = Vec::new();
    let mut spacing = vec![0.0; polygons.len()];
    for (f, mp) in polygons.iter().enumerate() {
        let mut n = 0usize;
        let mut len = 0.0;
        for p in &mp.0 {
            for ring in std::iter::once(p.exterior()).chain(p.interiors()) {
                len += line_length(&ring.0, geographic);
                n += ring.0.len();
                points.extend(
                    ring.0
                        .iter()
                        .map(|c| GeomWithData::new([c.x, c.y], f as u32)),
                );
            }
        }
        spacing[f] = if n > 0 { len / n as f64 } else { 0.0 };
    }
    let tree = RTree::bulk_load(points);
    let mut out: Vec<HashMap<u32, f64>> = vec![HashMap::new(); polygons.len()];
    let tol2 = tolerance * tolerance;
    for p in tree.iter() {
        let f = p.data;
        for q in tree.locate_within_distance(*p.geom(), tol2) {
            if q.data != f {
                *out[f as usize].entry(q.data).or_default() += spacing[f as usize];
            }
        }
    }
    out.into_iter()
        .map(|m| {
            let mut v: Vec<(u32, f64)> = m.into_iter().filter(|(_, len)| *len > 0.0).collect();
            v.sort_by_key(|(n, _)| *n);
            v
        })
        .collect()
}

/// Twice the signed area of a closed ring (positive = counter-clockwise).
fn signed_area(ring: &[Coord<f64>]) -> f64 {
    ring.windows(2)
        .map(|w| w[0].x * w[1].y - w[1].x * w[0].y)
        .sum()
}

/// Length of a line: in metres on the sphere for lon/lat coordinates, else
/// planar.
pub fn line_length(points: &[Coord<f64>], geographic: bool) -> f64 {
    points
        .windows(2)
        .map(|w| {
            if geographic {
                haversine(w[0], w[1])
            } else {
                (w[1].x - w[0].x).hypot(w[1].y - w[0].y)
            }
        })
        .sum()
}

/// Great-circle distance in metres between two lon/lat points.
pub fn haversine(a: Coord<f64>, b: Coord<f64>) -> f64 {
    const R: f64 = 6_371_008.8;
    let (la1, la2) = (a.y.to_radians(), b.y.to_radians());
    let dlat = la2 - la1;
    let dlon = (b.x - a.x).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().min(1.0).asin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_types::polygon;

    fn square(x: f64, y: f64) -> MultiPolygon<f64> {
        MultiPolygon(vec![polygon![
            (x: x, y: y),
            (x: x + 1.0, y: y),
            (x: x + 1.0, y: y + 1.0),
            (x: x, y: y + 1.0),
            (x: x, y: y),
        ]])
    }

    #[test]
    fn grid_shares_borders() {
        // 2×2 squares: four inner borders, eight outer ones.
        let f = vec![
            square(0.0, 0.0),
            square(1.0, 0.0),
            square(0.0, 1.0),
            square(1.0, 1.0),
        ];
        let t = Topology::build(&f);
        assert_eq!(t.arcs.iter().filter(|a| a.is_shared()).count(), 4);
        let n = t.neighbors(true);
        assert_eq!(n[0].iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(n[3].iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 2]);
        // Diagonal squares only touch in a point: not neighbours.
        assert!(!n[0].iter().any(|x| x.0 == 3));
        // Rings reassemble to the original squares.
        for (i, square) in f.iter().enumerate() {
            let p = t.polygon(i);
            let ext = &p.0[0].exterior().0;
            assert_eq!(ext.len(), 5);
            assert_eq!(ext.first(), ext.last());
            let mut got: Vec<(i64, i64)> = ext[..4].iter().map(|c| key(*c)).collect();
            let mut want: Vec<(i64, i64)> = square.0[0].exterior().0[..4]
                .iter()
                .map(|c| key(*c))
                .collect();
            got.sort();
            want.sort();
            assert_eq!(got, want);
        }
    }

    #[test]
    fn enclave_shares_its_whole_ring() {
        let outer = MultiPolygon(vec![Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (4.0, 0.0),
                (4.0, 4.0),
                (0.0, 4.0),
                (0.0, 0.0),
            ]),
            vec![LineString::from(vec![
                (1.0, 1.0),
                (1.0, 2.0),
                (2.0, 2.0),
                (2.0, 1.0),
                (1.0, 1.0),
            ])],
        )]);
        let inner = square(1.0, 1.0);
        let t = Topology::build(&[outer, inner]);
        // Outer ring + one shared closed ring.
        assert_eq!(t.arcs.len(), 2);
        assert_eq!(t.arcs.iter().filter(|a| a.is_shared()).count(), 1);
        assert_eq!(t.neighbors(true)[1], vec![(0, t.neighbors(true)[0][0].1)]);
        assert_eq!(t.features[0][0].rings.len(), 2);
    }

    #[test]
    fn three_way_junction() {
        // Two squares below one wide rectangle: junction at (1, 1).
        let wide = MultiPolygon(vec![polygon![
            (x: 0.0, y: 1.0), (x: 1.0, y: 1.0), (x: 2.0, y: 1.0), (x: 2.0, y: 2.0), (x: 0.0, y: 2.0), (x: 0.0, y: 1.0),
        ]]);
        let t = Topology::build(&[square(0.0, 0.0), square(1.0, 0.0), wide]);
        let n = t.neighbors(true);
        assert_eq!(n[2].len(), 2);
        assert_eq!(n[0].iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn approximate_neighbors_bridge_small_gaps() {
        // Two squares 1e-6 apart: no shared vertices, but neighbours.
        let a = square(0.0, 0.0);
        let mut b = square(1.0 + 1e-6, 0.0);
        let far = square(5.0, 5.0);
        b.0[0].exterior_mut(|r| r.0.iter_mut().for_each(|c| c.y += 1e-7));
        assert!(Topology::build(&[a.clone(), b.clone()]).neighbors(true)[0].is_empty());
        let n = approximate_neighbors(&[a, b, far], 1e-5, true);
        assert_eq!(n[0].iter().map(|x| x.0).collect::<Vec<_>>(), vec![1]);
        assert!(n[2].is_empty());
    }

    #[test]
    fn dissolve_merges_and_keeps_holes() {
        use geo::Area;
        // 3×3 grid; dissolve the ring around the centre: one polygon with a hole.
        let mut f = Vec::new();
        for y in 0..3 {
            for x in 0..3 {
                f.push(square(x as f64, y as f64));
            }
        }
        let t = Topology::build(&f);
        let all = t.dissolve(&(0..9).collect::<Vec<_>>());
        assert_eq!(all.0.len(), 1);
        assert_eq!(all.0[0].exterior().0.len() - 1, 12);
        assert!((all.unsigned_area() - 9.0).abs() < 1e-9);
        let ring = t.dissolve(&[0, 1, 2, 3, 5, 6, 7, 8]);
        assert_eq!(ring.0.len(), 1);
        assert_eq!(ring.0[0].interiors().len(), 1);
        assert!((ring.unsigned_area() - 8.0).abs() < 1e-9);
        // Two separate pieces stay two polygons.
        let two = t.dissolve(&[0, 8]);
        assert_eq!(two.0.len(), 2);
        // Clockwise input (as in shapefiles) dissolves the same way.
        let cw: Vec<MultiPolygon<f64>> = f
            .iter()
            .map(|mp| {
                let mut p = mp.clone();
                p.0[0].exterior_mut(|r| r.0.reverse());
                p
            })
            .collect();
        let t = Topology::build(&cw);
        assert_eq!(t.dissolve(&[0, 1, 2]).0.len(), 1);
    }

    #[test]
    fn haversine_one_degree() {
        let d = haversine(Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 });
        assert!((d - 111_195.0).abs() < 5.0, "{d}");
    }
}
