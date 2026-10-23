//! Line simplification (Ramer–Douglas–Peucker) with fixed end points, so
//! arcs keep meeting at their junctions.

use geo_types::Coord;

/// Simplifies `points` to within `tolerance`, keeping the first and last
/// point. Closed lines keep at least one more point (the farthest).
pub fn simplify(points: &[Coord<f64>], tolerance: f64, out: &mut Vec<Coord<f64>>) {
    let n = points.len();
    if n <= 2 || tolerance <= 0.0 {
        out.extend_from_slice(points);
        return;
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let tol2 = tolerance * tolerance;
    let mut stack = vec![(0usize, n - 1)];
    if points[0] == points[n - 1] {
        // A closed line: split at the point farthest from the start.
        let far = (1..n - 1)
            .max_by(|&i, &j| {
                segment_distance2(points[i], points[0], points[0])
                    .total_cmp(&segment_distance2(points[j], points[0], points[0]))
            })
            .unwrap_or(0);
        keep[far] = true;
        stack = vec![(0, far), (far, n - 1)];
    }
    while let Some((first, last)) = stack.pop() {
        if last <= first + 1 {
            continue;
        }
        let (a, b) = (points[first], points[last]);
        let mut max = -1.0;
        let mut index = first;
        for (i, &p) in points.iter().enumerate().take(last).skip(first + 1) {
            let d = segment_distance2(p, a, b);
            if d > max {
                max = d;
                index = i;
            }
        }
        if max > tol2 {
            keep[index] = true;
            stack.push((first, index));
            stack.push((index, last));
        }
    }
    out.extend(
        points
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(p, _)| *p),
    );
}

/// Squared distance from `p` to the segment `a`–`b`.
pub fn segment_distance2(p: Coord<f64>, a: Coord<f64>, b: Coord<f64>) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.x + t * dx - p.x, a.y + t * dy - p.y);
    cx * cx + cy * cy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(x: f64, y: f64) -> Coord<f64> {
        Coord { x, y }
    }

    #[test]
    fn removes_collinear_points() {
        let mut out = Vec::new();
        simplify(
            &[c(0.0, 0.0), c(1.0, 0.01), c(2.0, 0.0), c(3.0, 5.0)],
            0.1,
            &mut out,
        );
        assert_eq!(out, vec![c(0.0, 0.0), c(2.0, 0.0), c(3.0, 5.0)]);
    }

    #[test]
    fn closed_line_keeps_farthest_point() {
        let ring = [
            c(0.0, 0.0),
            c(1.0, 0.0),
            c(1.0, 1.0),
            c(0.0, 1.0),
            c(0.0, 0.0),
        ];
        let mut out = Vec::new();
        simplify(&ring, 0.01, &mut out);
        assert_eq!(out.len(), 5);
        out.clear();
        simplify(&ring, 10.0, &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1], c(1.0, 1.0));
    }
}
