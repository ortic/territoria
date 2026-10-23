//! Geometry for Territoria: coordinate systems, shared-border topology,
//! simplification and spatial indexes.
//!
//! Layers keep their geometry in WGS84 longitude/latitude; maps project it
//! for display (see [`crs::Transformer`]).

pub mod crs;
pub mod index;
pub mod simplify;
pub mod topology;
pub mod wkt;

pub use crs::{Crs, CrsError, Transformer};
pub use geo_types::{
    Coord, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Rect,
};
pub use index::BoxIndex;
pub use topology::{Arc, ArcRef, OUTSIDE, TopoPolygon, Topology, haversine};

use geo::BoundingRect;

/// Bounding box of several boxes.
pub fn union_rect(boxes: impl IntoIterator<Item = Rect<f64>>) -> Option<Rect<f64>> {
    boxes.into_iter().reduce(|a, b| {
        Rect::new(
            Coord {
                x: a.min().x.min(b.min().x),
                y: a.min().y.min(b.min().y),
            },
            Coord {
                x: a.max().x.max(b.max().x),
                y: a.max().y.max(b.max().y),
            },
        )
    })
}

/// A point well inside the polygon (pole of inaccessibility) for labels,
/// taken from its largest part. Falls back to the box centre.
pub fn label_point(mp: &MultiPolygon<f64>, geographic: bool) -> Option<Coord<f64>> {
    use geo::Area;
    let largest =
        mp.0.iter()
            .max_by(|a, b| a.unsigned_area().total_cmp(&b.unsigned_area()))?;
    let bbox = largest.bounding_rect()?;
    // Work in a locally isotropic space: longitude shrinks with latitude.
    let k = if geographic {
        bbox.center().y.to_radians().cos().max(0.01)
    } else {
        1.0
    };
    let scaled = geo::MapCoords::map_coords(largest, |c| Coord { x: c.x * k, y: c.y });
    let precision = (bbox.width() * k).max(bbox.height()) / 200.0;
    match polylabel::polylabel(&scaled, &precision.max(f64::EPSILON)) {
        Ok(p) => Some(Coord {
            x: p.x() / k,
            y: p.y(),
        }),
        Err(_) => Some(bbox.center()),
    }
}

/// Area in square metres of lon/lat geometry on the sphere (or planar area
/// for projected coordinates).
pub fn area(mp: &MultiPolygon<f64>, geographic: bool) -> f64 {
    use geo::Area;
    if geographic {
        use geo::ChamberlainDuquetteArea;
        mp.chamberlain_duquette_unsigned_area()
    } else {
        mp.unsigned_area()
    }
}
