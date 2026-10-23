//! Catchment: what lies within some distances of centres (branches, reps'
//! homes, territory sites): points counted and summed, areas' values
//! summed, and the rings themselves for the map.

use geo_types::Coord;
use serde::{Deserialize, Serialize};
use territoria_data::{Column, ColumnKind, Table};
use territoria_geo::topology::haversine;
use territoria_io::GeometryKind;

use crate::values::{Cache, ValueSource};
use crate::{LayerId, PlanId, Project};

/// Where the rings are drawn around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Centres {
    /// Every point of a layer.
    Points(LayerId),
    /// The sites of a plan's territories.
    Sites(PlanId),
}

impl Centres {
    pub fn label(&self, project: &Project) -> String {
        match self {
            Centres::Points(l) => project.layer(*l).map_or("?".into(), |l| l.name.clone()),
            Centres::Sites(p) => project
                .plan(*p)
                .map_or("?".into(), |p| format!("sites of {}", p.name)),
        }
    }

    /// Name and location (longitude, latitude) of each centre.
    pub fn list(&self, project: &Project) -> Vec<(String, Coord<f64>)> {
        match self {
            Centres::Points(l) => project.layer(*l).map_or_else(Vec::new, |l| {
                l.geometry
                    .points
                    .iter()
                    .enumerate()
                    .filter_map(|(i, mp)| mp.0.first().map(|p| (l.display_name(i), p.0)))
                    .collect()
            }),
            Centres::Sites(p) => project.plan(*p).map_or_else(Vec::new, |p| {
                p.territories
                    .iter()
                    .filter_map(|t| t.site.map(|[x, y]| (t.name.clone(), Coord { x, y })))
                    .collect()
            }),
        }
    }
}

/// What to measure within the rings.
#[derive(Debug, Clone, PartialEq)]
pub struct CatchmentSpec {
    pub centres: Centres,
    pub radii_km: Vec<f64>,
    /// Points counted (and their numbers summed).
    pub points: Option<LayerId>,
    /// A value of areas summed over the areas whose centre is within.
    pub areas: Option<(LayerId, ValueSource)>,
}

/// One row per centre: its name, and per radius what lies within.
pub fn catchment_table(project: &Project, cache: &mut Cache, spec: &CatchmentSpec) -> Table {
    let centres = spec.centres.list(project);
    let n = centres.len();
    let mut columns = vec![Column::text(
        "Centre",
        ColumnKind::Text,
        centres.iter().map(|(name, _)| Some(name.clone())).collect(),
    )];
    let km = |r: f64| territoria_data::table::format_number(r);
    // Distances from each centre to each point, once.
    if let Some(points) = spec.points.and_then(|l| project.layer(l))
        && points.kind() == GeometryKind::Point
    {
        let located: Vec<(usize, Coord<f64>)> = points
            .geometry
            .points
            .iter()
            .enumerate()
            .filter_map(|(i, mp)| mp.0.first().map(|p| (i, p.0)))
            .collect();
        let dist: Vec<Vec<f64>> = centres
            .iter()
            .map(|(_, c)| {
                located
                    .iter()
                    .map(|(_, p)| haversine(*c, *p) / 1000.0)
                    .collect()
            })
            .collect();
        let numbers: Vec<&Column> = points
            .attributes
            .columns
            .iter()
            .filter(|c| c.kind.is_numeric())
            .collect();
        for &r in &spec.radii_km {
            columns.push(Column::number(
                format!("{} within {} km", points.name, km(r)),
                dist.iter()
                    .map(|d| Some(d.iter().filter(|x| **x <= r).count() as f64))
                    .collect(),
            ));
            for c in &numbers {
                columns.push(Column::number(
                    format!("{} within {} km", c.name, km(r)),
                    dist.iter()
                        .map(|d| {
                            Some(
                                d.iter()
                                    .zip(&located)
                                    .filter(|(x, _)| **x <= r)
                                    .filter_map(|(_, (i, _))| c.f64(*i))
                                    .sum(),
                            )
                        })
                        .collect(),
                ));
            }
        }
    }
    if let Some((layer, source)) = &spec.areas
        && let Some(l) = project.layer(*layer)
    {
        let values = cache.values(project, *layer, source);
        let label = source.label(project);
        let dist: Vec<Vec<Option<f64>>> = centres
            .iter()
            .map(|(_, c)| {
                l.geometry
                    .label_points
                    .iter()
                    .map(|p| p.map(|p| haversine(*c, p) / 1000.0))
                    .collect()
            })
            .collect();
        for &r in &spec.radii_km {
            columns.push(Column::number(
                format!("{label} within {} km", km(r)),
                dist.iter()
                    .map(|d| {
                        Some(
                            d.iter()
                                .zip(&values)
                                .filter(|(x, _)| x.is_some_and(|x| x <= r))
                                .filter_map(|(_, v)| *v)
                                .sum(),
                        )
                    })
                    .collect(),
            ));
        }
    }
    Table::new(columns).unwrap_or_else(|_| Table::with_rows(n))
}

/// A circle of `radius_km` around `centre` on the sphere, as `n` points
/// (longitude, latitude), closed.
pub fn ring(centre: Coord<f64>, radius_km: f64, n: usize) -> Vec<Coord<f64>> {
    const R: f64 = 6_371.008_8;
    let (lat1, lon1) = (centre.y.to_radians(), centre.x.to_radians());
    let d = radius_km / R;
    let mut out: Vec<Coord<f64>> = (0..n)
        .map(|k| {
            let bearing = k as f64 / n as f64 * std::f64::consts::TAU;
            let lat2 = (lat1.sin() * d.cos() + lat1.cos() * d.sin() * bearing.cos()).asin();
            let lon2 = lon1
                + (bearing.sin() * d.sin() * lat1.cos()).atan2(d.cos() - lat1.sin() * lat2.sin());
            Coord {
                x: lon2.to_degrees(),
                y: lat2.to_degrees(),
            }
        })
        .collect();
    if let Some(first) = out.first().copied() {
        out.push(first);
    }
    out
}

/// Parses radii like `5, 10, 20` (km), sorted, without duplicates.
pub fn parse_radii(text: &str) -> Vec<f64> {
    let mut r: Vec<f64> = text
        .split([',', ';', ' '])
        .filter_map(|s| s.trim().trim_end_matches("km").trim().parse::<f64>().ok())
        .filter(|r| *r > 0.0 && r.is_finite())
        .collect();
    r.sort_by(f64::total_cmp);
    r.dedup();
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LayerGeometry;

    #[test]
    fn counts_and_sums_within_rings() {
        let mut p = Project::new();
        // A branch at (8, 47) and customers 0, 1, 2, 3 degrees of latitude
        // north (about 111 km apart).
        let pts = |coords: &[(f64, f64)]| {
            territoria_io::Geometries::Points(
                coords
                    .iter()
                    .map(|&(x, y)| geo_types::MultiPoint(vec![geo_types::Point::new(x, y)]))
                    .collect(),
            )
        };
        let branch = p.add_layer(
            "Branches",
            LayerGeometry::from_imported(pts(&[(8.0, 47.0)]), true),
            Table::with_rows(1),
            Default::default(),
        );
        let sales = Table::new(vec![Column::number(
            "Sales",
            vec![Some(1.0), Some(10.0), Some(100.0), Some(1000.0)],
        )])
        .unwrap();
        let customers = p.add_layer(
            "Customers",
            LayerGeometry::from_imported(
                pts(&[(8.0, 47.0), (8.0, 48.0), (8.0, 49.0), (8.0, 50.0)]),
                true,
            ),
            sales,
            Default::default(),
        );
        let spec = CatchmentSpec {
            centres: Centres::Points(branch),
            radii_km: parse_radii("120, 50km; 250"),
            points: Some(customers),
            areas: None,
        };
        assert_eq!(spec.radii_km, vec![50.0, 120.0, 250.0]);
        let t = catchment_table(&p, &mut Cache::default(), &spec);
        let get = |c: &str| t.column(c).unwrap().f64(0).unwrap();
        assert_eq!(get("Customers within 50 km"), 1.0);
        assert_eq!(get("Customers within 120 km"), 2.0);
        assert_eq!(get("Sales within 250 km"), 111.0);
        // A ring 111 km round passes one degree north.
        let r = ring(Coord { x: 8.0, y: 47.0 }, 111.195, 64);
        assert!(
            (r[0].y - 48.0).abs() < 0.01 && (r[0].x - 8.0).abs() < 1e-9,
            "{:?}",
            r[0]
        );
        assert_eq!(r.first(), r.last());
    }
}
