//! KML and KMZ (Google Earth, Google My Maps): placemarks with their
//! name, description, extended data and folder, as points, lines and
//! areas in WGS84.

use std::io::Read;
use std::path::Path;

use geo_types::{
    Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use territoria_geo::Crs;

use crate::ImportError;
use crate::vector::{ImportedLayer, VectorOptions, layers_by_kind};

type Node<'a, 'i> = roxmltree::Node<'a, 'i>;

fn is(n: &Node, tag: &str) -> bool {
    n.is_element() && n.tag_name().name() == tag
}

fn child<'a, 'i>(n: &Node<'a, 'i>, tag: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| is(c, tag))
}

fn text(n: &Node) -> Option<String> {
    let t: String = n
        .descendants()
        .filter(|d| d.is_text())
        .filter_map(|d| d.text())
        .collect();
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// `lon,lat[,alt]` tuples separated by white space.
fn coordinates(n: &Node) -> Vec<Coord<f64>> {
    let Some(c) = child(n, "coordinates").and_then(|c| text(&c)) else {
        return Vec::new();
    };
    c.split_whitespace()
        .filter_map(|t| {
            let mut parts = t.split(',').map(|p| p.trim().parse::<f64>());
            match (parts.next(), parts.next()) {
                (Some(Ok(x)), Some(Ok(y))) => Some(Coord { x, y }),
                _ => None,
            }
        })
        .collect()
}

fn ring(boundary: Option<Node>) -> Option<LineString<f64>> {
    let r = child(&boundary?, "LinearRing")?;
    let pts = coordinates(&r);
    (pts.len() >= 3).then_some(LineString(pts))
}

/// Points, lines and polygons of a geometry element (also inside
/// MultiGeometry).
#[derive(Default)]
struct Parts {
    points: Vec<Point<f64>>,
    lines: Vec<LineString<f64>>,
    polygons: Vec<Polygon<f64>>,
}

fn collect(n: &Node, out: &mut Parts) {
    match n.tag_name().name() {
        "Point" => out.points.extend(coordinates(n).first().map(|c| Point(*c))),
        "LineString" | "LinearRing" => {
            let pts = coordinates(n);
            if pts.len() >= 2 {
                out.lines.push(LineString(pts));
            }
        }
        "Polygon" => {
            if let Some(outer) = ring(child(n, "outerBoundaryIs")) {
                let holes = n
                    .children()
                    .filter(|c| is(c, "innerBoundaryIs"))
                    .filter_map(|c| ring(Some(c)))
                    .collect();
                out.polygons.push(Polygon::new(outer, holes));
            }
        }
        "MultiGeometry" => {
            for c in n.children().filter(|c| c.is_element()) {
                collect(&c, out);
            }
        }
        _ => {}
    }
}

/// A placemark's geometry: areas win over lines over points when a
/// MultiGeometry mixes them.
fn geometry(placemark: &Node) -> Option<Geometry<f64>> {
    let mut parts = Parts::default();
    for c in placemark.children().filter(|c| c.is_element()) {
        collect(&c, &mut parts);
    }
    if !parts.polygons.is_empty() {
        Some(Geometry::MultiPolygon(MultiPolygon(parts.polygons)))
    } else if !parts.lines.is_empty() {
        Some(Geometry::MultiLineString(MultiLineString(parts.lines)))
    } else if parts.points.len() == 1 {
        Some(Geometry::Point(parts.points[0]))
    } else if !parts.points.is_empty() {
        Some(Geometry::MultiPoint(MultiPoint(parts.points)))
    } else {
        None
    }
}

/// Name, description, folder and extended data of a placemark.
fn properties(placemark: &Node) -> Vec<(String, Option<String>)> {
    let mut out = vec![
        (
            "Name".to_owned(),
            child(placemark, "name").and_then(|n| text(&n)),
        ),
        (
            "Description".to_owned(),
            child(placemark, "description").and_then(|n| text(&n)),
        ),
    ];
    // The innermost folder: My Maps puts each of its layers in one.
    if let Some(folder) = placemark
        .ancestors()
        .find(|a| is(a, "Folder"))
        .and_then(|f| child(&f, "name"))
        .and_then(|n| text(&n))
    {
        out.push(("Folder".to_owned(), Some(folder)));
    }
    if let Some(data) = child(placemark, "ExtendedData") {
        for d in data.descendants() {
            let name = d.attribute("name").map(str::to_owned);
            if is(&d, "Data")
                && let Some(name) = name
            {
                let value = child(&d, "value").and_then(|v| text(&v));
                out.push((name, value));
            } else if is(&d, "SimpleData")
                && let Some(name) = name
            {
                out.push((name, text(&d)));
            }
        }
    }
    // Empty descriptions don't make a column.
    out.retain(|(k, v)| v.is_some() || k == "Name");
    out
}

pub fn read_kml(
    name: &str,
    bytes: &[u8],
    options: &VectorOptions,
) -> Result<Vec<ImportedLayer>, ImportError> {
    let text = String::from_utf8_lossy(bytes);
    let doc = roxmltree::Document::parse(&text)
        .map_err(|e| ImportError::Format(format!("invalid KML: {e}")))?;
    let placemarks: Vec<Node> = doc.descendants().filter(|n| is(n, "Placemark")).collect();
    let geoms: Vec<Option<Geometry<f64>>> = placemarks.iter().map(geometry).collect();
    let props: Vec<Vec<(String, Option<String>)>> = placemarks.iter().map(properties).collect();
    // KML is always longitude/latitude.
    let mut options = options.clone();
    if options.crs.is_none() {
        options.crs = Some(Crs::wgs84());
    }
    let layers = layers_by_kind(name, geoms, &props, &options)?;
    if layers.is_empty() {
        return Err(ImportError::Format("the KML has no placemarks".into()));
    }
    Ok(layers)
}

/// A KMZ: a zip with the KML (`doc.kml`, else the first `.kml`).
pub fn read_kmz(
    name: &str,
    path: &Path,
    options: &VectorOptions,
) -> Result<Vec<ImportedLayer>, ImportError> {
    let mut archive = zip::ZipArchive::new(crate::files::open(path)?)
        .map_err(|e| ImportError::Format(format!("not a readable KMZ file: {e}")))?;
    let names: Vec<String> = archive
        .file_names()
        .filter_map(|n| n.ok().map(|n| n.into_owned()))
        .collect();
    let kml = names
        .iter()
        .find(|n| n.eq_ignore_ascii_case("doc.kml"))
        .or_else(|| names.iter().find(|n| n.to_lowercase().ends_with(".kml")))
        .ok_or_else(|| ImportError::Format("the KMZ file contains no KML".into()))?
        .clone();
    let mut bytes = Vec::new();
    archive
        .by_name(&kml)
        .map_err(|e| ImportError::Format(e.to_string()))?
        .read_to_end(&mut bytes)?;
    read_kml(name, &bytes, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::Geometries;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <name>Stores</name>
    <Folder>
      <name>Branches</name>
      <Placemark>
        <name>Zürich HB</name>
        <description>Main branch</description>
        <ExtendedData><Data name="Staff"><value>12</value></Data></ExtendedData>
        <Point><coordinates>8.5402,47.3782,0</coordinates></Point>
      </Placemark>
      <Placemark>
        <name>Bern</name>
        <ExtendedData><Data name="Staff"><value>7</value></Data></ExtendedData>
        <Point><coordinates> 7.4391,46.9490 </coordinates></Point>
      </Placemark>
    </Folder>
    <Folder>
      <name>Areas</name>
      <Placemark>
        <name>Zone A</name>
        <Polygon><outerBoundaryIs><LinearRing><coordinates>
          8.0,47.0 9.0,47.0 9.0,48.0 8.0,48.0 8.0,47.0
        </coordinates></LinearRing></outerBoundaryIs>
        <innerBoundaryIs><LinearRing><coordinates>
          8.4,47.4 8.6,47.4 8.6,47.6 8.4,47.4
        </coordinates></LinearRing></innerBoundaryIs></Polygon>
      </Placemark>
    </Folder>
  </Document>
</kml>"#;

    #[test]
    fn reads_placemarks_by_kind() {
        let layers = read_kml("Stores", SAMPLE.as_bytes(), &VectorOptions::default()).unwrap();
        assert_eq!(layers.len(), 2);
        let points = layers.iter().find(|l| l.name == "Stores (points)").unwrap();
        let Geometries::Points(p) = &points.geometries else {
            panic!("points");
        };
        assert_eq!(p.len(), 2);
        assert!((p[1].0[0].x() - 7.4391).abs() < 1e-9);
        let t = &points.attributes;
        assert_eq!(t.column("Name").unwrap().display(0), "Zürich HB");
        assert_eq!(t.column("Staff").unwrap().f64(1), Some(7.0));
        assert_eq!(t.column("Folder").unwrap().display(0), "Branches");
        assert!(points.is_geographic());
        let areas = layers.iter().find(|l| l.name == "Stores (areas)").unwrap();
        let Geometries::Areas(a) = &areas.geometries else {
            panic!("areas");
        };
        assert_eq!(a[0].0[0].interiors().len(), 1);
    }
}

#[cfg(test)]
mod kmz_tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn reads_kml_inside_kmz() {
        let path = std::env::temp_dir().join(format!("territoria-{}.kmz", std::process::id()));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("doc.kml", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"<kml xmlns="http://www.opengis.net/kml/2.2"><Placemark><name>A</name>
            <LineString><coordinates>8,47 9,48</coordinates></LineString></Placemark></kml>"#,
        )
        .unwrap();
        zip.finish().unwrap();
        let layers = crate::read_vector(&path, &VectorOptions::default()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].geometries.kind(), crate::GeometryKind::Line);
    }
}
