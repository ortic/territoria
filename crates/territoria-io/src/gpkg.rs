//! GeoPackage (OGC, SQLite): every feature table becomes a layer. The
//! geometry blobs (a small header, then WKB) are read here; SQLite comes
//! compiled in.

use std::path::Path;

use geo_types::{Coord, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use rusqlite::{Connection, types::ValueRef};
use territoria_geo::Crs;

use crate::ImportError;
use crate::vector::{
    Geometries, GeometryKind, ImportedLayer, VectorOptions, build_table, finish_layer,
};

fn err(e: rusqlite::Error) -> ImportError {
    ImportError::Format(format!("GeoPackage: {e}"))
}

/// Geometry from WKB, flattened to 2D.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Wkb {
    Points(Vec<Point<f64>>),
    Lines(Vec<LineString<f64>>),
    Polygons(Vec<Polygon<f64>>),
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn bytes<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s = self.b.get(self.at..self.at + N)?;
        self.at += N;
        s.try_into().ok()
    }

    fn u32(&mut self, le: bool) -> Option<u32> {
        let b = self.bytes::<4>()?;
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }

    fn f64(&mut self, le: bool) -> Option<f64> {
        let b = self.bytes::<8>()?;
        Some(if le {
            f64::from_le_bytes(b)
        } else {
            f64::from_be_bytes(b)
        })
    }

    /// One geometry, appended to `out` (collections append their parts).
    fn geometry(&mut self, out: &mut Vec<Wkb>, depth: usize) -> Option<()> {
        if depth > 8 {
            return None;
        }
        let le = self.bytes::<1>()?[0] == 1;
        let raw = self.u32(le)?;
        // EWKB flags (PostGIS) or ISO codes (1000s for Z, M, ZM).
        let mut dims = 2;
        if raw & 0x8000_0000 != 0 {
            dims += 1;
        }
        if raw & 0x4000_0000 != 0 {
            dims += 1;
        }
        if raw & 0x2000_0000 != 0 {
            self.u32(le)?; // SRID
        }
        let code = raw & 0x0FFF_FFFF;
        let (base, iso) = (code % 1000, code / 1000);
        dims += match iso {
            1 | 2 => 1,
            3 => 2,
            _ => 0,
        };
        let point = |r: &mut Self| -> Option<Coord<f64>> {
            let x = r.f64(le)?;
            let y = r.f64(le)?;
            for _ in 2..dims {
                r.f64(le)?;
            }
            Some(Coord { x, y })
        };
        let ring = |r: &mut Self| -> Option<LineString<f64>> {
            let n = r.u32(le)? as usize;
            if n > r.b.len() {
                return None;
            }
            (0..n)
                .map(|_| point(r))
                .collect::<Option<Vec<_>>>()
                .map(LineString)
        };
        let polygon = |r: &mut Self| -> Option<Polygon<f64>> {
            let n = r.u32(le)? as usize;
            if n > r.b.len() {
                return None;
            }
            let mut rings = (0..n)
                .map(|_| ring(r))
                .collect::<Option<Vec<_>>>()?
                .into_iter();
            let exterior = rings.next()?;
            Some(Polygon::new(exterior, rings.collect()))
        };
        match base {
            1 => {
                let c = point(self)?;
                // An empty point is NaN, NaN.
                if c.x.is_finite() && c.y.is_finite() {
                    out.push(Wkb::Points(vec![Point(c)]));
                }
            }
            2 => out.push(Wkb::Lines(vec![ring(self)?])),
            3 => out.push(Wkb::Polygons(vec![polygon(self)?])),
            4..=7 => {
                let n = self.u32(le)? as usize;
                if n > self.b.len() {
                    return None;
                }
                for _ in 0..n {
                    self.geometry(out, depth + 1)?;
                }
            }
            // Curves and surfaces aren't supported.
            _ => return None,
        }
        Some(())
    }
}

/// Parses a GeoPackage geometry blob. `None`: empty or unreadable.
pub(crate) fn parse_blob(blob: &[u8]) -> Option<Vec<Wkb>> {
    let wkb = if blob.starts_with(b"GP") {
        let flags = *blob.get(3)?;
        if flags & 0b1_0000 != 0 {
            return None; // empty geometry
        }
        let envelope = match (flags >> 1) & 0b111 {
            0 => 0,
            1 => 32,
            2 | 3 => 48,
            4 => 64,
            _ => return None,
        };
        blob.get(8 + envelope..)?
    } else {
        // Plain WKB, as some writers store it.
        blob
    };
    let mut out = Vec::new();
    Reader { b: wkb, at: 0 }.geometry(&mut out, 0)?;
    Some(out)
}

fn kind_of(parts: &[Wkb]) -> Option<GeometryKind> {
    parts.first().map(|p| match p {
        Wkb::Points(_) => GeometryKind::Point,
        Wkb::Lines(_) => GeometryKind::Line,
        Wkb::Polygons(_) => GeometryKind::Area,
    })
}

/// The coordinate system of a GeoPackage `srs_id`.
fn crs_of(conn: &Connection, srs_id: i64) -> Option<Crs> {
    if srs_id <= 0 {
        return None;
    }
    let (org, id, wkt): (String, i64, String) = conn
        .query_row(
            "SELECT organization, organization_coordsys_id, definition FROM gpkg_spatial_ref_sys WHERE srs_id = ?1",
            [srs_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok()?;
    if org.eq_ignore_ascii_case("EPSG")
        && let Ok(crs) = Crs::from_epsg(id as u32)
    {
        return Some(crs);
    }
    Crs::from_wkt(&wkt).ok()
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// All feature tables of a GeoPackage.
pub fn read_geopackage(
    path: &Path,
    options: &VectorOptions,
) -> Result<Vec<ImportedLayer>, ImportError> {
    // In the browser the file is in memory, not on disk.
    #[cfg(target_arch = "wasm32")]
    return read_geopackage_bytes(&crate::files::read(path)?, options);
    #[cfg(not(target_arch = "wasm32"))]
    read_tables(
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(err)?,
        options,
    )
}

/// All feature tables of a GeoPackage held in memory (from a zip, say).
pub fn read_geopackage_bytes(
    bytes: &[u8],
    options: &VectorOptions,
) -> Result<Vec<ImportedLayer>, ImportError> {
    let mut conn = Connection::open_in_memory().map_err(err)?;
    conn.deserialize_read_exact(rusqlite::MAIN_DB, bytes, bytes.len(), true)
        .map_err(err)?;
    read_tables(conn, options)
}

fn read_tables(
    conn: Connection,
    options: &VectorOptions,
) -> Result<Vec<ImportedLayer>, ImportError> {
    let tables: Vec<(String, Option<String>, String, i64)> = {
        let mut q = conn
            .prepare(
                "SELECT c.table_name, c.identifier, g.column_name, g.srs_id \
                 FROM gpkg_contents c JOIN gpkg_geometry_columns g ON c.table_name = g.table_name \
                 WHERE c.data_type = 'features' ORDER BY c.table_name",
            )
            .map_err(|_| {
                ImportError::Format("not a GeoPackage: it has no list of feature tables".into())
            })?;
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?
    };
    let mut layers = Vec::new();
    for (table, identifier, geom_column, srs_id) in tables {
        let columns: Vec<String> = {
            let mut q = conn
                .prepare(&format!("PRAGMA table_info({})", quote(&table)))
                .map_err(err)?;
            q.query_map([], |r| r.get::<_, String>(1))
                .map_err(err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(err)?
                .into_iter()
                .filter(|c| !c.eq_ignore_ascii_case(&geom_column))
                .collect()
        };
        let select: Vec<String> = columns.iter().map(|c| quote(c)).collect();
        let sql = format!(
            "SELECT {}{}{} FROM {}",
            select.join(", "),
            if select.is_empty() { "" } else { ", " },
            quote(&geom_column),
            quote(&table)
        );
        let mut q = conn.prepare(&sql).map_err(err)?;
        let mut rows = q.query([]).map_err(err)?;
        let mut values: Vec<Vec<Option<String>>> = vec![Vec::new(); columns.len()];
        let mut geoms: Vec<Option<Vec<Wkb>>> = Vec::new();
        let mut unreadable = 0;
        while let Some(row) = rows.next().map_err(err)? {
            for (i, v) in values.iter_mut().enumerate() {
                v.push(match row.get_ref(i).map_err(err)? {
                    ValueRef::Null | ValueRef::Blob(_) => None,
                    ValueRef::Integer(n) => Some(n.to_string()),
                    ValueRef::Real(x) => Some(territoria_data::table::format_number(x)),
                    ValueRef::Text(t) => {
                        let t = String::from_utf8_lossy(t).trim().to_owned();
                        (!t.is_empty()).then_some(t)
                    }
                });
            }
            let g = match row.get_ref(columns.len()).map_err(err)? {
                ValueRef::Blob(b) => {
                    let g = parse_blob(b);
                    if g.is_none() && !b.is_empty() {
                        unreadable += 1;
                    }
                    g
                }
                _ => None,
            };
            geoms.push(g);
        }
        // One kind per layer: the most common one.
        let mut counts = [0usize; 3];
        for k in geoms.iter().flatten().filter_map(|g| kind_of(g)) {
            counts[k as usize] += 1;
        }
        let kind = [GeometryKind::Area, GeometryKind::Line, GeometryKind::Point]
            .into_iter()
            .max_by_key(|k| counts[*k as usize])
            .unwrap_or(GeometryKind::Area);
        let mut warnings = Vec::new();
        if unreadable > 0 {
            warnings.push(format!(
                "{unreadable} geometries couldn't be read (curves aren't supported) and are left empty."
            ));
        }
        let others = counts.iter().sum::<usize>() - counts[kind as usize];
        if others > 0 {
            warnings.push(format!(
                "{others} features of another geometry type are left empty."
            ));
        }
        fn of_kind(g: &Option<Vec<Wkb>>, kind: GeometryKind) -> Option<&Vec<Wkb>> {
            g.as_ref().filter(|g| kind_of(g) == Some(kind))
        }
        let pick = |g| of_kind(g, kind);
        let geometries = match kind {
            GeometryKind::Area => Geometries::Areas(
                geoms
                    .iter()
                    .map(|g| {
                        MultiPolygon(pick(g).map_or_else(Vec::new, |parts| {
                            parts
                                .iter()
                                .flat_map(|p| match p {
                                    Wkb::Polygons(ps) => ps.clone(),
                                    _ => Vec::new(),
                                })
                                .collect()
                        }))
                    })
                    .collect(),
            ),
            GeometryKind::Line => Geometries::Lines(
                geoms
                    .iter()
                    .map(|g| {
                        MultiLineString(pick(g).map_or_else(Vec::new, |parts| {
                            parts
                                .iter()
                                .flat_map(|p| match p {
                                    Wkb::Lines(ls) => ls.clone(),
                                    _ => Vec::new(),
                                })
                                .collect()
                        }))
                    })
                    .collect(),
            ),
            GeometryKind::Point => Geometries::Points(
                geoms
                    .iter()
                    .map(|g| {
                        MultiPoint(pick(g).map_or_else(Vec::new, |parts| {
                            parts
                                .iter()
                                .flat_map(|p| match p {
                                    Wkb::Points(ps) => ps.clone(),
                                    _ => Vec::new(),
                                })
                                .collect()
                        }))
                    })
                    .collect(),
            ),
        };
        let attributes = build_table(&columns, values);
        let crs = crs_of(&conn, srs_id);
        let mut opts = options.clone();
        let from_file = opts.crs.is_none() && crs.is_some();
        if opts.crs.is_none() {
            opts.crs = crs;
        }
        let name = identifier.filter(|i| !i.trim().is_empty()).unwrap_or(table);
        let mut layer = finish_layer(&name, geometries, attributes, None, &opts, warnings)?;
        if from_file && let Some(c) = &layer.source_crs {
            layer.crs_note = format!("{} (from the GeoPackage)", c.label());
        }
        layers.push(layer);
    }
    if layers.is_empty() {
        return Err(ImportError::Format(
            "the GeoPackage has no feature tables".into(),
        ));
    }
    Ok(layers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GeoPackage geometry blob: header without envelope, then WKB.
    fn blob(wkb: &[u8]) -> Vec<u8> {
        let mut b = vec![b'G', b'P', 0, 0b0000_0001];
        b.extend_from_slice(&2056i32.to_le_bytes());
        b.extend_from_slice(wkb);
        b
    }

    fn polygon_wkb(ring: &[(f64, f64)], z: bool) -> Vec<u8> {
        let mut w = vec![1u8];
        w.extend_from_slice(&(if z { 1003u32 } else { 3u32 }).to_le_bytes());
        w.extend_from_slice(&1u32.to_le_bytes());
        w.extend_from_slice(&(ring.len() as u32).to_le_bytes());
        for (x, y) in ring {
            w.extend_from_slice(&x.to_le_bytes());
            w.extend_from_slice(&y.to_le_bytes());
            if z {
                w.extend_from_slice(&500.0f64.to_le_bytes());
            }
        }
        w
    }

    #[test]
    fn blobs_with_and_without_z() {
        let ring = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)];
        for z in [false, true] {
            let g = parse_blob(&blob(&polygon_wkb(&ring, z))).unwrap();
            let Wkb::Polygons(p) = &g[0] else { panic!() };
            assert_eq!(p[0].exterior().0.len(), 4);
            assert_eq!(p[0].exterior().0[2], Coord { x: 1.0, y: 1.0 });
        }
        // A multipolygon of two, big-endian.
        let mut m = vec![0u8];
        m.extend_from_slice(&6u32.to_be_bytes());
        m.extend_from_slice(&2u32.to_be_bytes());
        for _ in 0..2 {
            m.push(0);
            m.extend_from_slice(&3u32.to_be_bytes());
            m.extend_from_slice(&1u32.to_be_bytes());
            m.extend_from_slice(&4u32.to_be_bytes());
            for (x, y) in ring {
                m.extend_from_slice(&x.to_be_bytes());
                m.extend_from_slice(&y.to_be_bytes());
            }
        }
        assert_eq!(parse_blob(&blob(&m)).unwrap().len(), 2);
        // Empty flag.
        assert!(parse_blob(&[b'G', b'P', 0, 0b0001_0001, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn reads_a_geopackage() {
        let dir = std::env::temp_dir().join(format!("territoria-gpkg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.gpkg");
        let _ = std::fs::remove_file(&path);
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE gpkg_spatial_ref_sys (srs_name TEXT, srs_id INTEGER PRIMARY KEY, organization TEXT, organization_coordsys_id INTEGER, definition TEXT, description TEXT);
                 INSERT INTO gpkg_spatial_ref_sys VALUES ('CH1903+ / LV95', 2056, 'EPSG', 2056, 'undefined', NULL);
                 CREATE TABLE gpkg_contents (table_name TEXT PRIMARY KEY, data_type TEXT, identifier TEXT);
                 INSERT INTO gpkg_contents VALUES ('plz', 'features', 'Postcodes');
                 INSERT INTO gpkg_contents VALUES ('notes', 'attributes', 'Notes');
                 CREATE TABLE gpkg_geometry_columns (table_name TEXT, column_name TEXT, geometry_type_name TEXT, srs_id INTEGER, z INTEGER, m INTEGER);
                 INSERT INTO gpkg_geometry_columns VALUES ('plz', 'geom', 'POLYGON', 2056, 1, 0);
                 CREATE TABLE plz (fid INTEGER PRIMARY KEY, geom BLOB, plz TEXT, name TEXT, pop REAL);",
            )
            .unwrap();
            // Two squares near Bern in LV95, one with a 3D polygon.
            for (i, (code, name, x, z)) in [
                ("3011", "Bern", 2_600_000.0, false),
                ("0815", "Testdorf", 2_601_000.0, true),
            ]
            .into_iter()
            .enumerate()
            {
                let ring = [
                    (x, 1_200_000.0),
                    (x + 1000.0, 1_200_000.0),
                    (x + 1000.0, 1_201_000.0),
                    (x, 1_201_000.0),
                    (x, 1_200_000.0),
                ];
                c.execute(
                    "INSERT INTO plz VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        i as i64 + 1,
                        blob(&polygon_wkb(&ring, z)),
                        code,
                        name,
                        1234.5
                    ],
                )
                .unwrap();
            }
        }
        let layers = crate::read_vector(&path, &VectorOptions::default()).unwrap();
        assert_eq!(layers.len(), 1, "only feature tables");
        let l = &layers[0];
        assert_eq!(l.name, "Postcodes");
        assert!(l.crs_note.contains("2056"), "{}", l.crs_note);
        let plz = l.attributes.column("plz").unwrap();
        assert_eq!(plz.display(1), "0815", "leading zeros stay");
        let Geometries::Areas(a) = &l.geometries else {
            panic!()
        };
        let c = a[0].0[0].exterior().0[0];
        // LV95 (2600000, 1200000) is the old Bern observatory: 7.4386 E, 46.9511 N.
        assert!(
            (c.x - 7.4386).abs() < 0.001 && (c.y - 46.9511).abs() < 0.001,
            "{c:?}"
        );

        // Zipped, it is read from memory (as in the browser).
        let zipped = dir.join("test.zip");
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&zipped).unwrap());
            zip.start_file("plz/test.gpkg", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&std::fs::read(&path).unwrap()).unwrap();
            zip.finish().unwrap();
        }
        let from_zip = crate::read_vector(&zipped, &VectorOptions::default()).unwrap();
        assert_eq!(from_zip.len(), 1);
        assert_eq!(from_zip[0].name, "Postcodes");
        assert_eq!(from_zip[0].attributes.rows(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
