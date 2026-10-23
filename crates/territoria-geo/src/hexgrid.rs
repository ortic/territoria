//! Hexagon grids of equal-area cells, in a local Lambert azimuthal
//! equal-area projection centred on the data. Cells are pointy-top with
//! axial coordinates (q, r). Corners come from an integer lattice, so
//! neighbours share bit-identical corners and the topology sees their
//! common borders.

use geo_types::{Coord, LineString, Polygon, Rect};

use crate::{Crs, CrsError, Transformer};

const SQRT3: f64 = 1.732_050_807_568_877_2;

pub type Cell = (i32, i32);

pub struct HexGrid {
    to_local: Transformer,
    to_wgs: Transformer,
    /// Circumradius in metres.
    size: f64,
}

impl HexGrid {
    /// A grid of hexagons `width_m` across (flat side to flat side),
    /// centred on `centre` (longitude, latitude).
    pub fn new(centre: Coord<f64>, width_m: f64) -> Result<HexGrid, CrsError> {
        let local = Crs::from_proj(&format!(
            "+proj=laea +lat_0={} +lon_0={} +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
            centre.y, centre.x
        ))?;
        Ok(HexGrid {
            to_local: Transformer::new(&Crs::wgs84(), &local)?,
            to_wgs: Transformer::new(&local, &Crs::wgs84())?,
            size: width_m / SQRT3,
        })
    }

    /// Area of one cell in km².
    pub fn cell_area_km2(&self) -> f64 {
        1.5 * SQRT3 * self.size * self.size / 1e6
    }

    /// The cell a point (longitude, latitude) lies in.
    pub fn cell_of(&self, lonlat: Coord<f64>) -> Option<Cell> {
        let p = self.to_local.convert(lonlat)?;
        let q = (SQRT3 / 3.0 * p.x - p.y / 3.0) / self.size;
        let r = (2.0 / 3.0 * p.y) / self.size;
        Some(cube_round(q, r))
    }

    /// A corner on the lattice of half-steps.
    fn corner(&self, i: i32, j: i32) -> Option<Coord<f64>> {
        self.to_wgs.convert(Coord {
            x: i as f64 * self.size * SQRT3 / 2.0,
            y: j as f64 * self.size / 2.0,
        })
    }

    /// The cell's outline in longitude/latitude, counter-clockwise.
    pub fn polygon(&self, (q, r): Cell) -> Option<Polygon<f64>> {
        let (ci, cj) = (2 * q + r, 3 * r);
        let corners = [
            (ci + 1, cj + 1),
            (ci, cj + 2),
            (ci - 1, cj + 1),
            (ci - 1, cj - 1),
            (ci, cj - 2),
            (ci + 1, cj - 1),
            (ci + 1, cj + 1),
        ];
        let ring = corners
            .iter()
            .map(|&(i, j)| self.corner(i, j))
            .collect::<Option<Vec<_>>>()?;
        Some(Polygon::new(LineString(ring), vec![]))
    }

    /// The cell's centre in longitude/latitude.
    pub fn centre(&self, (q, r): Cell) -> Option<Coord<f64>> {
        self.corner(2 * q + r, 3 * r)
    }

    /// Every cell whose centre lies in the box (longitude/latitude).
    pub fn cells_in(&self, bbox: Rect<f64>) -> Vec<Cell> {
        // The box's outline in local metres, sampled: it is curved there.
        let (min, max) = (bbox.min(), bbox.max());
        let mut lo = Coord {
            x: f64::INFINITY,
            y: f64::INFINITY,
        };
        let mut hi = Coord {
            x: f64::NEG_INFINITY,
            y: f64::NEG_INFINITY,
        };
        for k in 0..=20 {
            let t = k as f64 / 20.0;
            for c in [
                Coord {
                    x: min.x + t * (max.x - min.x),
                    y: min.y,
                },
                Coord {
                    x: min.x + t * (max.x - min.x),
                    y: max.y,
                },
                Coord {
                    x: min.x,
                    y: min.y + t * (max.y - min.y),
                },
                Coord {
                    x: max.x,
                    y: min.y + t * (max.y - min.y),
                },
            ] {
                if let Some(p) = self.to_local.convert(c) {
                    lo = Coord {
                        x: lo.x.min(p.x),
                        y: lo.y.min(p.y),
                    };
                    hi = Coord {
                        x: hi.x.max(p.x),
                        y: hi.y.max(p.y),
                    };
                }
            }
        }
        if !lo.x.is_finite() {
            return Vec::new();
        }
        let row = 1.5 * self.size;
        let col = SQRT3 * self.size;
        let (r0, r1) = (
            (lo.y / row).floor() as i32 - 1,
            (hi.y / row).ceil() as i32 + 1,
        );
        let mut cells = Vec::new();
        for r in r0..=r1 {
            let shift = r as f64 / 2.0;
            let (q0, q1) = (
                (lo.x / col - shift).floor() as i32 - 1,
                (hi.x / col - shift).ceil() as i32 + 1,
            );
            for q in q0..=q1 {
                let x = col * (q as f64 + shift);
                let y = row * r as f64;
                if x >= lo.x && x <= hi.x && y >= lo.y && y <= hi.y {
                    cells.push((q, r));
                }
            }
        }
        cells
    }
}

fn cube_round(q: f64, r: f64) -> Cell {
    let s = -q - r;
    let (mut rq, mut rr, rs) = (q.round(), r.round(), s.round());
    let (dq, dr, ds) = ((rq - q).abs(), (rr - r).abs(), (rs - s).abs());
    if dq > dr && dq > ds {
        rq = -rr - rs;
    } else if dr > ds {
        rr = -rq - rs;
    }
    (rq as i32, rr as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Area;

    #[test]
    fn cells_tile_without_gaps() {
        let g = HexGrid::new(Coord { x: 8.0, y: 47.0 }, 5_000.0).unwrap();
        // A point is in the cell whose centre is nearest.
        let c = g.centre((3, -2)).unwrap();
        assert_eq!(g.cell_of(c), Some((3, -2)));
        // Neighbours share two identical corners.
        let a = g.polygon((0, 0)).unwrap();
        let b = g.polygon((1, 0)).unwrap();
        let shared = a.exterior().0[..6]
            .iter()
            .filter(|p| b.exterior().0.contains(p))
            .count();
        assert_eq!(shared, 2);
        // Equal area: about 21.65 km² for 5 km across.
        assert!(
            (g.cell_area_km2() - 21.65).abs() < 0.01,
            "{}",
            g.cell_area_km2()
        );
        let geodesic = crate::area(&geo_types::MultiPolygon(vec![a.clone()]), true) / 1e6;
        assert!(
            (geodesic - g.cell_area_km2()).abs() / g.cell_area_km2() < 0.01,
            "{geodesic}"
        );
        assert!(a.unsigned_area() > 0.0);
        // A box of about 50 × 55 km holds about 100 cells.
        let cells = g.cells_in(Rect::new(
            Coord { x: 7.7, y: 46.75 },
            Coord { x: 8.35, y: 47.25 },
        ));
        assert!((90..140).contains(&cells.len()), "{}", cells.len());
    }
}
