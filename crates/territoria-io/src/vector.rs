//! Reading boundary and point files: ESRI Shapefile (also zipped),
//! GeoJSON, GeoPackage and KML. Geometry comes back in WGS84 when the coordinate system is
//! known.

use std::io::{Cursor, Read};
use std::path::Path;

use geo::{BoundingRect, Contains};
use geo_types::{
    Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Rect,
};
use territoria_data::Table;
use territoria_data::infer::{InferOptions, infer_column};
use territoria_geo::{Crs, Transformer};

use crate::ImportError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum GeometryKind {
    #[default]
    Area,
    Line,
    Point,
}

impl GeometryKind {
    pub fn label(self) -> &'static str {
        match self {
            GeometryKind::Area => "areas",
            GeometryKind::Line => "lines",
            GeometryKind::Point => "points",
        }
    }

    /// The word for `n` of them: "1 area", "806 areas".
    pub fn noun(self, n: usize) -> &'static str {
        match (self, n) {
            (GeometryKind::Area, 1) => "area",
            (GeometryKind::Line, 1) => "line",
            (GeometryKind::Point, 1) => "point",
            _ => self.label(),
        }
    }
}

/// One feature per row of [`ImportedLayer::attributes`]; features without
/// geometry are empty.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometries {
    Areas(Vec<MultiPolygon<f64>>),
    Lines(Vec<MultiLineString<f64>>),
    Points(Vec<MultiPoint<f64>>),
}

impl Geometries {
    pub fn kind(&self) -> GeometryKind {
        match self {
            Geometries::Areas(_) => GeometryKind::Area,
            Geometries::Lines(_) => GeometryKind::Line,
            Geometries::Points(_) => GeometryKind::Point,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Geometries::Areas(v) => v.len(),
            Geometries::Lines(v) => v.len(),
            Geometries::Points(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn bounding_rect(&self) -> Option<Rect<f64>> {
        let boxes: Vec<Rect<f64>> = match self {
            Geometries::Areas(v) => v.iter().filter_map(|g| g.bounding_rect()).collect(),
            Geometries::Lines(v) => v.iter().filter_map(|g| g.bounding_rect()).collect(),
            Geometries::Points(v) => v.iter().filter_map(|g| g.bounding_rect()).collect(),
        };
        territoria_geo::union_rect(boxes)
    }

    fn coords_mut(&mut self, f: &mut impl FnMut(&mut Coord<f64>)) {
        match self {
            Geometries::Areas(v) => {
                for mp in v {
                    for p in &mut mp.0 {
                        p.exterior_mut(|r| r.0.iter_mut().for_each(&mut *f));
                        p.interiors_mut(|rs| {
                            for r in rs {
                                r.0.iter_mut().for_each(&mut *f);
                            }
                        });
                    }
                }
            }
            Geometries::Lines(v) => v
                .iter_mut()
                .flat_map(|m| m.0.iter_mut())
                .for_each(|l| l.0.iter_mut().for_each(&mut *f)),
            Geometries::Points(v) => v
                .iter_mut()
                .flat_map(|m| m.0.iter_mut())
                .for_each(|p| f(&mut p.0)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportedLayer {
    pub name: String,
    pub geometries: Geometries,
    pub attributes: Table,
    /// The coordinate system the file was in; `None` when unknown, then the
    /// coordinates are kept as they are.
    pub source_crs: Option<Crs>,
    /// How the coordinate system was determined, for the import dialog.
    pub crs_note: String,
    pub warnings: Vec<String>,
}

impl ImportedLayer {
    /// True when the geometry is in WGS84 longitude/latitude.
    pub fn is_geographic(&self) -> bool {
        self.source_crs.is_some()
    }
}

#[derive(Debug, Clone, Default)]
pub struct VectorOptions {
    /// Use this coordinate system instead of the file's (or when it has none).
    pub crs: Option<Crs>,
    /// Attribute text encoding (`UTF-8`, `windows-1252`, `850`…) instead of
    /// the `.cpg` file or detection.
    pub encoding: Option<String>,
}

/// File extensions [`read_vector`] understands.
pub const VECTOR_EXTENSIONS: &[&str] = &["shp", "zip", "geojson", "json", "gpkg", "kml", "kmz"];

pub fn read_vector(
    path: &Path,
    options: &VectorOptions,
) -> Result<Vec<ImportedLayer>, ImportError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Layer")
        .to_owned();
    match ext.as_str() {
        "shp" => {
            let read = |ext: &str| std::fs::read(path.with_extension(ext)).ok();
            let shp = std::fs::read(path)?;
            let files = ShapefileParts {
                shp,
                shx: read("shx"),
                dbf: read("dbf").ok_or_else(|| ImportError::Missing(format!("{stem}.dbf")))?,
                prj: read("prj").map(|b| String::from_utf8_lossy(&b).into_owned()),
                cpg: read("cpg").map(|b| String::from_utf8_lossy(&b).into_owned()),
            };
            Ok(vec![read_shapefile(&stem, files, options)?])
        }
        "zip" => read_zip(path, options),
        "gpkg" => crate::gpkg::read_geopackage(path, options),
        "geojson" | "json" => read_geojson(&stem, &std::fs::read(path)?, options),
        "kml" => crate::kml::read_kml(&stem, &std::fs::read(path)?, options),
        "kmz" => crate::kml::read_kmz(&stem, path, options),
        _ => Err(ImportError::Unsupported(ext)),
    }
}

struct ShapefileParts {
    shp: Vec<u8>,
    shx: Option<Vec<u8>>,
    dbf: Vec<u8>,
    prj: Option<String>,
    cpg: Option<String>,
}

/// All shapefiles and GeoJSON files inside a zip archive.
fn read_zip(path: &Path, options: &VectorOptions) -> Result<Vec<ImportedLayer>, ImportError> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)
        .map_err(|e| ImportError::Format(format!("not a readable zip file: {e}")))?;
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| ImportError::Format(e.to_string()))?;
        if !file.is_file() {
            continue;
        }
        let name = file
            .name()
            .map_err(|e| ImportError::Format(e.to_string()))?
            .into_owned();
        let lower = name.to_lowercase();
        if [
            ".shp", ".shx", ".dbf", ".prj", ".cpg", ".geojson", ".json", ".gpkg", ".kml",
        ]
        .iter()
        .any(|e| lower.ends_with(e))
        {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            entries.push((name, buf));
        }
    }
    let find = |base: &str, ext: &str| {
        entries
            .iter()
            .find(|(n, _)| n.to_lowercase() == format!("{}.{ext}", base.to_lowercase()))
            .map(|(_, b)| b.clone())
    };
    let mut layers = Vec::new();
    for (name, bytes) in &entries {
        let lower = name.to_lowercase();
        let base = &name[..name.len() - name.rsplit('.').next().unwrap_or("").len() - 1];
        let stem = Path::new(base)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(base)
            .to_owned();
        if lower.ends_with(".shp") {
            let dbf =
                find(base, "dbf").ok_or_else(|| ImportError::Missing(format!("{stem}.dbf")))?;
            let parts = ShapefileParts {
                shp: bytes.clone(),
                shx: find(base, "shx"),
                dbf,
                prj: find(base, "prj").map(|b| String::from_utf8_lossy(&b).into_owned()),
                cpg: find(base, "cpg").map(|b| String::from_utf8_lossy(&b).into_owned()),
            };
            layers.push(read_shapefile(&stem, parts, options)?);
        } else if lower.ends_with(".geojson") || lower.ends_with(".json") {
            layers.extend(read_geojson(&stem, bytes, options)?);
        } else if lower.ends_with(".kml") {
            layers.extend(crate::kml::read_kml(&stem, bytes, options)?);
        } else if lower.ends_with(".gpkg") {
            // SQLite reads files: unpack it next to the other temporary files.
            let tmp =
                std::env::temp_dir().join(format!("territoria-{}-{stem}.gpkg", std::process::id()));
            std::fs::write(&tmp, bytes)?;
            let read = crate::gpkg::read_geopackage(&tmp, options);
            let _ = std::fs::remove_file(&tmp);
            layers.extend(read?);
        }
    }
    if layers.is_empty() {
        return Err(ImportError::Format(
            "the zip file contains no shapefile, GeoJSON, GeoPackage or KML".into(),
        ));
    }
    Ok(layers)
}

/// The encoding of a DBF without a known code page: UTF-8 when all its
/// records are valid UTF-8, else Windows-1252 (the common legacy one).
fn guess_dbf_encoding(dbf: &[u8]) -> &'static encoding_rs::Encoding {
    let header = dbf
        .get(8..10)
        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as usize);
    match std::str::from_utf8(dbf.get(header..).unwrap_or_default()) {
        Ok(_) => encoding_rs::UTF_8,
        Err(_) => encoding_rs::WINDOWS_1252,
    }
}

/// An encoding by `.cpg`-style name: `UTF-8`, `1252`, `CP850`, `ISO-8859-1`…
fn encoding_by_name(name: &str) -> Option<&'static encoding_rs::Encoding> {
    let n = name.trim().trim_start_matches('\u{feff}');
    let digits =
        n.trim_start_matches(|c: char| c.is_ascii_alphabetic() || c == ' ' || c == '-' || c == '_');
    let by_number = match digits {
        "65001" => Some("utf-8"),
        "1250" | "1251" | "1252" | "1253" | "1254" | "1255" | "1256" | "1257" | "1258" => None,
        "850" | "437" => Some("windows-1252"),
        "866" => Some("ibm866"),
        "932" => Some("shift_jis"),
        "936" => Some("gbk"),
        "949" => Some("euc-kr"),
        "950" => Some("big5"),
        _ => None,
    };
    if let Some(label) = by_number {
        return encoding_rs::Encoding::for_label(label.as_bytes());
    }
    if digits.len() == 4 && digits.starts_with("125") {
        return encoding_rs::Encoding::for_label(format!("windows-{digits}").as_bytes());
    }
    encoding_rs::Encoding::for_label(n.as_bytes())
}

fn read_shapefile(
    name: &str,
    parts: ShapefileParts,
    options: &VectorOptions,
) -> Result<ImportedLayer, ImportError> {
    let mut warnings = Vec::new();
    let fixed = match options.encoding.as_deref().or(parts.cpg.as_deref()) {
        Some(label) => match encoding_by_name(label) {
            Some(enc) => Some(enc),
            None => {
                warnings.push(format!(
                    "Unknown text encoding `{}`; reading as UTF-8.",
                    label.trim()
                ));
                None
            }
        },
        None => None,
    };
    let shapes = match parts.shx {
        Some(shx) => shapefile::ShapeReader::with_shx(Cursor::new(parts.shp), Cursor::new(shx)),
        None => shapefile::ShapeReader::new(Cursor::new(parts.shp)),
    }
    .map_err(|e| ImportError::Format(e.to_string()))?;
    let encoding = fixed.unwrap_or_else(|| guess_dbf_encoding(&parts.dbf));
    let dbf = dbase::ReaderBuilder::new()
        .with_encoding(dbase::encoding::EncodingRs::from(encoding))
        .build(Cursor::new(parts.dbf))
        .map_err(|e| ImportError::Format(format!("attribute table (.dbf): {e}")))?;
    let field_names: Vec<String> = dbf.fields().iter().map(|f| f.name().to_owned()).collect();
    let mut reader = shapefile::Reader::new(shapes, dbf);

    let mut columns: Vec<Vec<Option<String>>> = vec![Vec::new(); field_names.len()];
    let mut geoms: Vec<Option<Geometry<f64>>> = Vec::new();
    let mut kind = None;
    for item in reader.iter_shapes_and_records() {
        let (shape, record) = item.map_err(|e| ImportError::Format(e.to_string()))?;
        for (i, field) in field_names.iter().enumerate() {
            columns[i].push(record.get(field).and_then(field_text));
        }
        let (k, geom) = convert_shape(shape);
        if let Some(k) = k {
            match kind {
                None => kind = Some(k),
                Some(known) if known != k => {
                    warnings.push("The file mixes geometry types; extra ones are skipped.".into())
                }
                _ => {}
            }
        }
        geoms.push(geom.filter(|g| kind_of(g) == kind));
    }
    let geometries = match kind.unwrap_or(GeometryKind::Area) {
        GeometryKind::Area => Geometries::Areas(
            geoms
                .into_iter()
                .map(|g| match g {
                    Some(Geometry::MultiPolygon(mp)) => mp,
                    _ => MultiPolygon(vec![]),
                })
                .collect(),
        ),
        GeometryKind::Line => Geometries::Lines(
            geoms
                .into_iter()
                .map(|g| match g {
                    Some(Geometry::MultiLineString(ml)) => ml,
                    _ => MultiLineString(vec![]),
                })
                .collect(),
        ),
        GeometryKind::Point => Geometries::Points(
            geoms
                .into_iter()
                .map(|g| match g {
                    Some(Geometry::MultiPoint(mp)) => mp,
                    _ => MultiPoint(vec![]),
                })
                .collect(),
        ),
    };
    let attributes = build_table(&field_names, columns);
    finish_layer(
        name,
        geometries,
        attributes,
        parts.prj.as_deref(),
        options,
        warnings,
    )
}

fn field_text(value: &dbase::FieldValue) -> Option<String> {
    use dbase::FieldValue as F;
    let s = match value {
        F::Character(s) => s.as_ref().map(|s| s.trim().to_owned())?,
        F::Memo(s) => s.trim().to_owned(),
        F::Numeric(n) => territoria_data::table::format_number((*n)?),
        F::Float(n) => territoria_data::table::format_number(f64::from((*n)?)),
        F::Double(n) | F::Currency(n) => territoria_data::table::format_number(*n),
        F::Integer(n) => n.to_string(),
        F::Logical(b) => (if (*b)? { "true" } else { "false" }).to_owned(),
        F::Date(d) => {
            let d = d.as_ref()?;
            format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
        }
        F::DateTime(dt) => {
            let d = dt.date();
            format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
        }
    };
    (!s.is_empty()).then_some(s)
}

/// Converts a shape; polygon holes go into the outer ring that contains
/// them (shapefiles don't promise any ring order).
fn convert_shape(shape: shapefile::Shape) -> (Option<GeometryKind>, Option<Geometry<f64>>) {
    use shapefile::Shape as S;
    fn xy<P: shapefile::record::traits::HasXY>(p: &P) -> Coord<f64> {
        Coord { x: p.x(), y: p.y() }
    }
    fn polygon<P: shapefile::record::traits::HasXY>(
        rings: &[shapefile::PolygonRing<P>],
    ) -> MultiPolygon<f64> {
        let mut outers: Vec<Polygon<f64>> = Vec::new();
        let mut inners: Vec<LineString<f64>> = Vec::new();
        for r in rings {
            let ls = LineString(r.points().iter().map(xy).collect());
            match r {
                shapefile::PolygonRing::Outer(_) => outers.push(Polygon::new(ls, vec![])),
                shapefile::PolygonRing::Inner(_) => inners.push(ls),
            }
        }
        for hole in inners {
            let Some(&first) = hole.0.first() else {
                continue;
            };
            let owner = outers
                .iter()
                .enumerate()
                .filter(|(_, p)| p.contains(&Point(first)) || p.exterior().0.contains(&first))
                .min_by(|a, b| {
                    use geo::Area;
                    a.1.unsigned_area().total_cmp(&b.1.unsigned_area())
                })
                .map(|(i, _)| i);
            match owner {
                Some(i) => outers[i].interiors_push(hole),
                // A hole outside every outer ring is really an outer ring
                // with the wrong winding.
                None => outers.push(Polygon::new(hole, vec![])),
            }
        }
        MultiPolygon(outers)
    }
    fn lines<P: shapefile::record::traits::HasXY>(parts: &[Vec<P>]) -> MultiLineString<f64> {
        MultiLineString(
            parts
                .iter()
                .map(|p| LineString(p.iter().map(xy).collect()))
                .collect(),
        )
    }
    fn multipoint<P: shapefile::record::traits::HasXY>(points: &[P]) -> MultiPoint<f64> {
        MultiPoint(points.iter().map(|p| Point(xy(p))).collect())
    }
    match shape {
        S::NullShape => (None, None),
        S::Point(p) => (
            Some(GeometryKind::Point),
            Some(MultiPoint(vec![Point(xy(&p))]).into()),
        ),
        S::PointM(p) => (
            Some(GeometryKind::Point),
            Some(MultiPoint(vec![Point(xy(&p))]).into()),
        ),
        S::PointZ(p) => (
            Some(GeometryKind::Point),
            Some(MultiPoint(vec![Point(xy(&p))]).into()),
        ),
        S::Multipoint(m) => (
            Some(GeometryKind::Point),
            Some(multipoint(m.points()).into()),
        ),
        S::MultipointM(m) => (
            Some(GeometryKind::Point),
            Some(multipoint(m.points()).into()),
        ),
        S::MultipointZ(m) => (
            Some(GeometryKind::Point),
            Some(multipoint(m.points()).into()),
        ),
        S::Polyline(l) => (Some(GeometryKind::Line), Some(lines(l.parts()).into())),
        S::PolylineM(l) => (Some(GeometryKind::Line), Some(lines(l.parts()).into())),
        S::PolylineZ(l) => (Some(GeometryKind::Line), Some(lines(l.parts()).into())),
        S::Polygon(p) => (Some(GeometryKind::Area), Some(polygon(p.rings()).into())),
        S::PolygonM(p) => (Some(GeometryKind::Area), Some(polygon(p.rings()).into())),
        S::PolygonZ(p) => (Some(GeometryKind::Area), Some(polygon(p.rings()).into())),
        S::Multipatch(_) => (None, None),
    }
}

pub(crate) fn build_table(names: &[String], columns: Vec<Vec<Option<String>>>) -> Table {
    let mut table = Table::default();
    for (name, values) in names.iter().zip(columns) {
        let name = table.unique_name(name);
        let col = infer_column(&name, values, InferOptions::default());
        table
            .push_column(col)
            .expect("columns have one value per feature");
    }
    table
}

/// Resolves the coordinate system and converts to WGS84.
pub(crate) fn finish_layer(
    name: &str,
    mut geometries: Geometries,
    mut attributes: Table,
    prj: Option<&str>,
    options: &VectorOptions,
    mut warnings: Vec<String>,
) -> Result<ImportedLayer, ImportError> {
    if attributes.columns.is_empty() {
        attributes = Table::with_rows(geometries.len());
    }
    let bbox = geometries.bounding_rect();
    let (crs, note) = if let Some(crs) = &options.crs {
        (Some(crs.clone()), format!("{} (chosen)", crs.label()))
    } else if let Some(prj) = prj.filter(|p| !p.trim().is_empty()) {
        match Crs::from_wkt(prj) {
            Ok(crs) => {
                let note = format!("{} (from the .prj file)", crs.label());
                (Some(crs), note)
            }
            Err(e) => {
                warnings.push(format!(
                    "The coordinate system in the .prj file isn't supported ({e})."
                ));
                guess_crs(bbox)
            }
        }
    } else {
        guess_crs(bbox)
    };
    if let Some(crs) = &crs
        && !crs.is_wgs84()
    {
        let t =
            Transformer::new(crs, &Crs::wgs84()).map_err(|e| ImportError::Format(e.to_string()))?;
        let mut failed = 0usize;
        geometries.coords_mut(&mut |c| match t.convert(*c) {
            Some(p) => *c = p,
            None => {
                failed += 1;
                *c = Coord {
                    x: f64::NAN,
                    y: f64::NAN,
                };
            }
        });
        if failed > 0 {
            warnings.push(format!(
                "{failed} coordinates could not be converted to longitude/latitude."
            ));
        }
    }
    Ok(ImportedLayer {
        name: name.to_owned(),
        geometries,
        attributes,
        source_crs: crs,
        crs_note: note,
        warnings,
    })
}

fn guess_crs(bbox: Option<Rect<f64>>) -> (Option<Crs>, String) {
    match bbox {
        Some(b) if territoria_geo::crs::looks_geographic(b) => (
            Some(Crs::wgs84()),
            "WGS 84 longitude/latitude (assumed: no coordinate system given)".into(),
        ),
        _ => (
            None,
            "Unknown: no coordinate system given; choose one to combine it with other layers"
                .into(),
        ),
    }
}

fn read_geojson(
    name: &str,
    bytes: &[u8],
    options: &VectorOptions,
) -> Result<Vec<ImportedLayer>, ImportError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ImportError::Format("GeoJSON must be UTF-8 text".into()))?;
    let json: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ImportError::Format(format!("invalid JSON: {e}")))?;
    // Old GeoJSON names its coordinate system (`"crs": {"properties": {"name": "EPSG:2056"}}`).
    let named_crs = json
        .get("crs")
        .and_then(|c| c.get("properties"))
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .and_then(|n| n.rsplit(':').next())
        .and_then(|code| code.parse::<u32>().ok())
        .and_then(|code| Crs::from_epsg(code).ok());
    let gj = serde_json::from_value::<geojson::GeoJson>(json)
        .map_err(|e| ImportError::Format(format!("invalid GeoJSON: {e}")))?;
    let features: Vec<geojson::Feature> = match gj {
        geojson::GeoJson::FeatureCollection(fc) => fc.features,
        geojson::GeoJson::Feature(f) => vec![f],
        geojson::GeoJson::Geometry(g) => vec![geojson::Feature {
            geometry: Some(g),
            ..Default::default()
        }],
    };

    let geoms: Vec<Option<Geometry<f64>>> = features
        .iter()
        .map(|f| {
            f.geometry
                .as_ref()
                .and_then(|g| Geometry::<f64>::try_from(&g.value).ok())
        })
        .collect();
    let props: Vec<Vec<(String, Option<String>)>> = features
        .iter()
        .map(|f| {
            f.properties
                .iter()
                .flatten()
                .map(|(k, v)| (k.clone(), json_text(v)))
                .collect()
        })
        .collect();
    let mut options = options.clone();
    if options.crs.is_none() {
        options.crs = named_crs;
    }
    let layers = layers_by_kind(name, geoms, &props, &options)?;
    if layers.is_empty() {
        return Err(ImportError::Format("the GeoJSON has no features".into()));
    }
    Ok(layers)
}

/// Features as layers, one per kind of geometry (areas, lines, points),
/// each with the properties its features have.
pub(crate) fn layers_by_kind(
    name: &str,
    geoms: Vec<Option<Geometry<f64>>>,
    props: &[Vec<(String, Option<String>)>],
    options: &VectorOptions,
) -> Result<Vec<ImportedLayer>, ImportError> {
    let mut groups: Vec<(GeometryKind, Vec<usize>)> = Vec::new();
    for (i, g) in geoms.iter().enumerate() {
        let kind = g.as_ref().and_then(kind_of).unwrap_or(GeometryKind::Area);
        match groups.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, rows)) => rows.push(i),
            None => groups.push((kind, vec![i])),
        }
    }
    let multi = groups.len() > 1;
    let mut layers = Vec::new();
    for (kind, rows) in groups {
        let mut names: Vec<String> = Vec::new();
        for &r in &rows {
            for (k, _) in &props[r] {
                if !names.contains(k) {
                    names.push(k.clone());
                }
            }
        }
        let columns: Vec<Vec<Option<String>>> = names
            .iter()
            .map(|n| {
                rows.iter()
                    .map(|&r| {
                        props[r]
                            .iter()
                            .find(|(k, _)| k == n)
                            .and_then(|(_, v)| v.clone())
                    })
                    .collect()
            })
            .collect();
        let geometries = match kind {
            GeometryKind::Area => Geometries::Areas(
                rows.iter()
                    .map(|&r| match &geoms[r] {
                        Some(Geometry::Polygon(p)) => MultiPolygon(vec![p.clone()]),
                        Some(Geometry::MultiPolygon(mp)) => mp.clone(),
                        _ => MultiPolygon(vec![]),
                    })
                    .collect(),
            ),
            GeometryKind::Line => Geometries::Lines(
                rows.iter()
                    .map(|&r| match &geoms[r] {
                        Some(Geometry::LineString(l)) => MultiLineString(vec![l.clone()]),
                        Some(Geometry::MultiLineString(ml)) => ml.clone(),
                        _ => MultiLineString(vec![]),
                    })
                    .collect(),
            ),
            GeometryKind::Point => Geometries::Points(
                rows.iter()
                    .map(|&r| match &geoms[r] {
                        Some(Geometry::Point(p)) => MultiPoint(vec![*p]),
                        Some(Geometry::MultiPoint(mp)) => mp.clone(),
                        _ => MultiPoint(vec![]),
                    })
                    .collect(),
            ),
        };
        let layer_name = if multi {
            format!("{name} ({})", kind.label())
        } else {
            name.to_owned()
        };
        let table = build_table(&names, columns);
        layers.push(finish_layer(
            &layer_name,
            geometries,
            table,
            None,
            options,
            Vec::new(),
        )?);
    }
    Ok(layers)
}

fn kind_of(g: &Geometry<f64>) -> Option<GeometryKind> {
    match g {
        Geometry::Polygon(_)
        | Geometry::MultiPolygon(_)
        | Geometry::Rect(_)
        | Geometry::Triangle(_) => Some(GeometryKind::Area),
        Geometry::LineString(_) | Geometry::MultiLineString(_) | Geometry::Line(_) => {
            Some(GeometryKind::Line)
        }
        Geometry::Point(_) | Geometry::MultiPoint(_) => Some(GeometryKind::Point),
        Geometry::GeometryCollection(_) => None,
    }
}

fn json_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => n
            .as_f64()
            .map(territoria_data::table::format_number)
            .or_else(|| Some(n.to_string())),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use territoria_data::ColumnKind;

    #[test]
    fn geojson_with_mixed_types() {
        let text = r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"code":"01067","name":"Dresden","pop":1000},
             "geometry":{"type":"Polygon","coordinates":[[[13.7,51.0],[13.8,51.0],[13.8,51.1],[13.7,51.0]]]}},
            {"type":"Feature","properties":{"code":"10115","name":"Berlin","pop":2500.5},
             "geometry":{"type":"MultiPolygon","coordinates":[[[[13.3,52.5],[13.4,52.5],[13.4,52.6],[13.3,52.5]]]]}},
            {"type":"Feature","properties":{"label":"HQ"},
             "geometry":{"type":"Point","coordinates":[8.5,47.4]}}
        ]}"#;
        let layers = read_geojson("test", text.as_bytes(), &VectorOptions::default()).unwrap();
        assert_eq!(layers.len(), 2);
        let areas = &layers[0];
        assert_eq!(areas.name, "test (areas)");
        assert_eq!(areas.geometries.len(), 2);
        assert_eq!(
            areas.attributes.column("code").unwrap().kind,
            ColumnKind::Key
        );
        assert_eq!(
            areas.attributes.column("pop").unwrap().kind,
            ColumnKind::Number
        );
        assert!(areas.source_crs.as_ref().unwrap().is_wgs84());
        assert_eq!(layers[1].geometries.kind(), GeometryKind::Point);
    }

    #[test]
    fn geojson_named_crs_is_converted() {
        let text = r#"{"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::2056"}},
            "features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[2600670,1199655]}}]}"#;
        let layers = read_geojson("x", text.as_bytes(), &VectorOptions::default()).unwrap();
        let Geometries::Points(p) = &layers[0].geometries else {
            panic!()
        };
        assert!((p[0].0[0].x() - 7.447433).abs() < 1e-5);
    }

    #[test]
    fn encodings_by_cpg_name() {
        assert_eq!(encoding_by_name("UTF-8"), Some(encoding_rs::UTF_8));
        assert_eq!(encoding_by_name("1252"), Some(encoding_rs::WINDOWS_1252));
        assert_eq!(
            encoding_by_name("ANSI 1250"),
            Some(encoding_rs::WINDOWS_1250)
        );
        assert_eq!(
            encoding_by_name("ISO-8859-1"),
            Some(encoding_rs::WINDOWS_1252)
        );
    }
}
