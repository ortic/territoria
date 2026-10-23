//! Hexagon grids as layers: equal cells over points or areas, with the
//! points counted and summed per cell. Useful where no boundaries fit,
//! for density maps, and as units for territories.

use std::collections::HashMap;

use geo_types::{Coord, MultiPolygon};
use territoria_data::{Column, ColumnKind, Table};
use territoria_geo::hexgrid::{Cell, HexGrid};
use territoria_io::{Geometries, GeometryKind};

use crate::{LayerId, Project};

/// What the cells cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexCover {
    /// Only cells with at least one point.
    Points(LayerId),
    /// Cells over the areas of a layer (centre or near a corner inside).
    Areas(LayerId),
}

impl HexCover {
    pub fn layer(self) -> LayerId {
        match self {
            HexCover::Points(l) | HexCover::Areas(l) => l,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HexSpec {
    /// Width of a cell, flat side to flat side.
    pub width_km: f64,
    pub cover: HexCover,
    /// Points to count (and sum) per cell.
    pub count: Option<LayerId>,
}

fn grid(project: &Project, layer: LayerId, width_km: f64) -> Result<HexGrid, String> {
    let l = project.layer(layer).ok_or("the layer was removed")?;
    if !l.geometry.geographic {
        return Err(format!("“{}” has no known coordinate system.", l.name));
    }
    let b = l.geometry.bbox.ok_or("the layer is empty")?;
    HexGrid::new(b.center(), width_km * 1000.0).map_err(|e| e.to_string())
}

/// The cells `spec` gives, without building them.
pub fn hex_cells(project: &Project, spec: &HexSpec) -> Result<(HexGrid, Vec<Cell>), String> {
    if !(spec.width_km > 0.0) {
        return Err("Choose a cell size above 0.".into());
    }
    let g = grid(project, spec.cover.layer(), spec.width_km)?;
    let l = project
        .layer(spec.cover.layer())
        .ok_or("the layer was removed")?;
    let mut cells: Vec<Cell> = match spec.cover {
        HexCover::Points(_) => l
            .geometry
            .points
            .iter()
            .flat_map(|mp| mp.0.iter())
            .filter_map(|p| g.cell_of(p.0))
            .collect(),
        HexCover::Areas(_) => {
            let b = l.geometry.bbox.ok_or("the layer is empty")?;
            let inside = |c: Coord<f64>| l.geometry.feature_at(c, 0.0).is_some();
            g.cells_in(b)
                .into_iter()
                .filter(|&cell| {
                    let Some(m) = g.centre(cell) else {
                        return false;
                    };
                    // The centre, or near a corner: cells only touching an
                    // area at a corner stay out.
                    inside(m)
                        || g.polygon(cell).is_some_and(|p| {
                            p.exterior().0.iter().any(|c| {
                                inside(Coord {
                                    x: m.x + 0.85 * (c.x - m.x),
                                    y: m.y + 0.85 * (c.y - m.y),
                                })
                            })
                        })
                })
                .collect()
        }
    };
    cells.sort_unstable();
    cells.dedup();
    Ok((g, cells))
}

/// The grid's geometry and attributes: a key per cell, and per point layer
/// the count and sums of its numbers.
pub fn hex_grid(project: &Project, spec: &HexSpec) -> Result<(Geometries, Table), String> {
    let (g, cells) = hex_cells(project, spec)?;
    if cells.is_empty() {
        return Err("No cells: there is nothing to cover.".into());
    }
    if cells.len() > 200_000 {
        return Err(format!(
            "That would be {} cells; choose larger ones.",
            cells.len()
        ));
    }
    let index: HashMap<Cell, usize> = cells.iter().enumerate().map(|(i, c)| (*c, i)).collect();
    let polygons: Vec<MultiPolygon<f64>> = cells
        .iter()
        .map(|&c| MultiPolygon(g.polygon(c).into_iter().collect()))
        .collect();
    let n = cells.len();
    let mut columns = vec![Column::text(
        "Cell",
        ColumnKind::Key,
        cells
            .iter()
            .map(|(q, r)| Some(format!("{q}:{r}")))
            .collect(),
    )];
    if let Some(points) = spec.count.and_then(|p| project.layer(p))
        && points.kind() == GeometryKind::Point
    {
        let hit: Vec<Option<usize>> = points
            .geometry
            .points
            .iter()
            .map(|mp| {
                mp.0.first()
                    .and_then(|p| g.cell_of(p.0))
                    .and_then(|c| index.get(&c).copied())
            })
            .collect();
        let mut count = vec![0.0; n];
        for h in hit.iter().flatten() {
            count[*h] += 1.0;
        }
        columns.push(Column::number(
            points.name.clone(),
            count.into_iter().map(Some).collect(),
        ));
        for c in points
            .attributes
            .columns
            .iter()
            .filter(|c| c.kind.is_numeric())
        {
            let mut sum: Vec<Option<f64>> = vec![None; n];
            for (row, h) in hit.iter().enumerate() {
                if let (Some(h), Some(v)) = (h, c.f64(row)) {
                    *sum[*h].get_or_insert(0.0) += v;
                }
            }
            columns.push(Column::number(format!("{} ({})", c.name, points.name), sum));
        }
    }
    let table = Table::new(columns).map_err(|e| e.to_string())?;
    Ok((Geometries::Areas(polygons), table))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LayerGeometry;

    #[test]
    fn grid_over_points_and_areas() {
        let mut p = Project::new();
        // Points in a 0.2° square around (8, 47), one with a big sale.
        let mut pts = Vec::new();
        let mut sales = Vec::new();
        for i in 0..100 {
            let (x, y) = (8.0 + (i % 10) as f64 * 0.02, 47.0 + (i / 10) as f64 * 0.02);
            pts.push(geo_types::MultiPoint(vec![geo_types::Point::new(x, y)]));
            sales.push(Some(if i == 0 { 1000.0 } else { 1.0 }));
        }
        let table = Table::new(vec![Column::number("Sales", sales)]).unwrap();
        let g = LayerGeometry::from_imported(Geometries::Points(pts), true);
        let points = p.add_layer("Customers", g, table, Default::default());
        let spec = HexSpec {
            width_km: 5.0,
            cover: HexCover::Points(points),
            count: Some(points),
        };
        let (geometries, table) = hex_grid(&p, &spec).unwrap();
        let n = geometries.len();
        assert!((2..30).contains(&n), "{n} cells");
        let count: f64 = (0..n)
            .filter_map(|r| table.column("Customers").unwrap().f64(r))
            .sum();
        assert_eq!(count, 100.0, "every point counted once");
        let sales: f64 = (0..n)
            .filter_map(|r| table.column("Sales (Customers)").unwrap().f64(r))
            .sum();
        assert_eq!(sales, 1099.0);
        // As a layer, neighbouring cells touch.
        let layer = p.add_layer(
            "Hexagons",
            LayerGeometry::from_imported(geometries, true),
            table,
            Default::default(),
        );
        let lg = &p.layer(layer).unwrap().geometry;
        let avg = lg.neighbors.iter().map(Vec::len).sum::<usize>() as f64 / n as f64;
        assert!(avg > 2.0, "{avg} neighbours on average");
        // Over the grid's own areas: the same cells, and no others.
        let over = HexSpec {
            width_km: 5.0,
            cover: HexCover::Areas(layer),
            count: None,
        };
        assert_eq!(hex_cells(&p, &over).unwrap().1.len(), n);
    }
}
