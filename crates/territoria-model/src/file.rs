//! The project file: a zip archive with `project.json` (structure, styles,
//! plans) and one part per layer, table and assignment.
//!
//! ```text
//! project.json
//! layers/<id>.geom          binary geometry (see `write_geometry`)
//! layers/<id>.attributes.json
//! tables/<id>.json
//! plans/<id>.assignment.json
//! ```

use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;

use geo_types::{Coord, LineString, MultiLineString, MultiPoint, Point};
use territoria_data::Table;
use territoria_geo::{Arc as TopoArc, ArcRef, TopoPolygon, Topology};
use territoria_io::GeometryKind;

use crate::{LayerGeometry, Project, TerritoryId};

pub const EXTENSION: &str = "territoria";
const FORMAT: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not a Territoria project: {0}")]
    Format(String),
    #[error("this project was saved by a newer version of Territoria (format {0})")]
    TooNew(u32),
}

impl From<zip::result::ZipError> for FileError {
    fn from(e: zip::result::ZipError) -> Self {
        FileError::Format(e.to_string())
    }
}

impl From<serde_json::Error> for FileError {
    fn from(e: serde_json::Error) -> Self {
        FileError::Format(e.to_string())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Manifest {
    format: u32,
    project: Project,
}

/// Saves atomically: a temporary file next to `path`, then renamed.
#[cfg(not(target_arch = "wasm32"))]
pub fn save(project: &Project, path: &Path) -> Result<(), FileError> {
    let tmp = path.with_extension(format!("{EXTENSION}.saving"));
    {
        let file = std::fs::File::create(&tmp)?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
        write_project(project, &mut zip)?;
        zip.finish()?.flush()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Saves into the browser's session files (see [`territoria_io::files`]).
#[cfg(target_arch = "wasm32")]
pub fn save(project: &Project, path: &Path) -> Result<(), FileError> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    write_project(project, &mut zip)?;
    territoria_io::files::write(path, zip.finish()?.get_ref())?;
    Ok(())
}

pub fn write_project<W: Write + Seek>(
    project: &Project,
    zip: &mut zip::ZipWriter<W>,
) -> Result<(), FileError> {
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("project.json", opts)?;
    serde_json::to_writer_pretty(
        &mut *zip,
        &Manifest {
            format: FORMAT,
            project: project.clone(),
        },
    )?;
    for layer in &project.layers {
        zip.start_file(format!("layers/{}.geom", layer.id.0), opts)?;
        let mut buf = Vec::new();
        write_geometry(&layer.geometry, &mut buf);
        zip.write_all(&buf)?;
        zip.start_file(format!("layers/{}.attributes.json", layer.id.0), opts)?;
        serde_json::to_writer(&mut *zip, &*layer.attributes)?;
    }
    for table in &project.tables {
        zip.start_file(format!("tables/{}.json", table.id.0), opts)?;
        serde_json::to_writer(&mut *zip, &*table.table)?;
    }
    for plan in &project.plans {
        zip.start_file(format!("plans/{}.assignment.json", plan.id.0), opts)?;
        let a: Vec<Option<u32>> = plan.assignment.iter().map(|t| t.map(|t| t.0)).collect();
        let pinned: Vec<u32> = plan
            .pinned
            .iter()
            .enumerate()
            .filter(|(_, p)| **p)
            .map(|(i, _)| i as u32)
            .collect();
        serde_json::to_writer(&mut *zip, &(a, pinned))?;
    }
    Ok(())
}

pub fn load(path: &Path) -> Result<Project, FileError> {
    let file = territoria_io::files::open(path)?;
    read_project(std::io::BufReader::new(file))
}

pub fn read_project<R: Read + Seek>(reader: R) -> Result<Project, FileError> {
    let mut zip = zip::ZipArchive::new(reader)?;
    let manifest: Manifest = serde_json::from_reader(zip.by_name("project.json")?)?;
    if manifest.format > FORMAT {
        return Err(FileError::TooNew(manifest.format));
    }
    let mut project = manifest.project;
    for layer in &mut project.layers {
        let mut buf = Vec::new();
        zip.by_name(&format!("layers/{}.geom", layer.id.0))?
            .read_to_end(&mut buf)?;
        let mut geometry = read_geometry(&buf)?;
        // Neighbours are derived again: barriers cut them as when saved.
        geometry.cut_neighbors(&layer.barriers);
        layer.geometry = Arc::new(geometry);
        let attributes: Table = serde_json::from_reader(
            zip.by_name(&format!("layers/{}.attributes.json", layer.id.0))?,
        )?;
        layer.attributes = Arc::new(attributes);
    }
    for table in &mut project.tables {
        let t: Table =
            serde_json::from_reader(zip.by_name(&format!("tables/{}.json", table.id.0))?)?;
        table.table = Arc::new(t);
    }
    for plan in &mut project.plans {
        let (a, pinned): (Vec<Option<u32>>, Vec<u32>) =
            serde_json::from_reader(zip.by_name(&format!("plans/{}.assignment.json", plan.id.0))?)?;
        let mut p = vec![false; a.len()];
        for i in pinned {
            if let Some(slot) = p.get_mut(i as usize) {
                *slot = true;
            }
        }
        plan.assignment = Arc::new(a.into_iter().map(|t| t.map(TerritoryId)).collect());
        plan.pinned = Arc::new(p);
    }
    Ok(project)
}

// Binary geometry: little-endian, `TGEO`, version, kind, flags, then the
// kind's data. Coordinates are f64 pairs.

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_coords(out: &mut Vec<u8>, coords: &[Coord<f64>]) {
    put_u32(out, coords.len() as u32);
    for c in coords {
        out.extend_from_slice(&c.x.to_le_bytes());
        out.extend_from_slice(&c.y.to_le_bytes());
    }
}

pub fn write_geometry(g: &LayerGeometry, out: &mut Vec<u8>) {
    out.extend_from_slice(b"TGEO");
    put_u32(out, 1);
    out.push(match g.kind {
        GeometryKind::Area => 0,
        GeometryKind::Line => 1,
        GeometryKind::Point => 2,
    });
    out.push(u8::from(g.geographic));
    match g.kind {
        GeometryKind::Area => {
            let t = &g.topology;
            put_u32(out, t.arcs.len() as u32);
            for arc in &t.arcs {
                put_u32(out, arc.a);
                put_u32(out, arc.b);
                put_coords(out, &arc.points);
            }
            put_u32(out, t.features.len() as u32);
            for polys in &t.features {
                put_u32(out, polys.len() as u32);
                for p in polys {
                    put_u32(out, p.rings.len() as u32);
                    for ring in &p.rings {
                        put_u32(out, ring.len() as u32);
                        for r in ring {
                            put_u32(out, (r.arc << 1) | u32::from(r.reversed));
                        }
                    }
                }
            }
        }
        GeometryKind::Line => {
            put_u32(out, g.lines.len() as u32);
            for ml in &g.lines {
                put_u32(out, ml.0.len() as u32);
                for l in &ml.0 {
                    put_coords(out, &l.0);
                }
            }
        }
        GeometryKind::Point => {
            put_u32(out, g.points.len() as u32);
            for mp in &g.points {
                let coords: Vec<Coord<f64>> = mp.0.iter().map(|p| p.0).collect();
                put_coords(out, &coords);
            }
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8], FileError> {
        let s = self
            .b
            .get(self.pos..self.pos + n)
            .ok_or_else(|| FileError::Format("geometry is cut short".into()))?;
        self.pos += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, FileError> {
        Ok(self.bytes(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, FileError> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn count(&mut self) -> Result<usize, FileError> {
        let n = self.u32()? as usize;
        // Every element takes at least four bytes: reject absurd counts.
        if n > self.b.len() {
            return Err(FileError::Format("geometry is damaged".into()));
        }
        Ok(n)
    }

    fn coords(&mut self) -> Result<Vec<Coord<f64>>, FileError> {
        let n = self.count()?;
        let raw = self.bytes(n * 16)?;
        Ok(raw
            .as_chunks::<16>()
            .0
            .iter()
            .map(|c| Coord {
                x: f64::from_le_bytes(c[..8].try_into().unwrap()),
                y: f64::from_le_bytes(c[8..].try_into().unwrap()),
            })
            .collect())
    }
}

pub fn read_geometry(b: &[u8]) -> Result<LayerGeometry, FileError> {
    let mut r = Reader { b, pos: 0 };
    if r.bytes(4)? != b"TGEO" {
        return Err(FileError::Format("unknown geometry data".into()));
    }
    let version = r.u32()?;
    if version > 1 {
        return Err(FileError::TooNew(version));
    }
    let kind = match r.u8()? {
        0 => GeometryKind::Area,
        1 => GeometryKind::Line,
        2 => GeometryKind::Point,
        k => return Err(FileError::Format(format!("unknown geometry kind {k}"))),
    };
    let geographic = r.u8()? != 0;
    let mut topology = Topology::default();
    let mut lines = Vec::new();
    let mut points = Vec::new();
    match kind {
        GeometryKind::Area => {
            for _ in 0..r.count()? {
                let a = r.u32()?;
                let b = r.u32()?;
                topology.arcs.push(TopoArc {
                    points: r.coords()?,
                    a,
                    b,
                });
            }
            for _ in 0..r.count()? {
                let mut polys = Vec::new();
                for _ in 0..r.count()? {
                    let mut rings = Vec::new();
                    for _ in 0..r.count()? {
                        let mut refs = Vec::new();
                        for _ in 0..r.count()? {
                            let v = r.u32()?;
                            if (v >> 1) as usize >= topology.arcs.len() {
                                return Err(FileError::Format(
                                    "geometry refers to a missing arc".into(),
                                ));
                            }
                            refs.push(ArcRef {
                                arc: v >> 1,
                                reversed: v & 1 == 1,
                            });
                        }
                        rings.push(refs);
                    }
                    polys.push(TopoPolygon { rings });
                }
                topology.features.push(polys);
            }
        }
        GeometryKind::Line => {
            for _ in 0..r.count()? {
                let mut parts = Vec::new();
                for _ in 0..r.count()? {
                    parts.push(LineString(r.coords()?));
                }
                lines.push(MultiLineString(parts));
            }
        }
        GeometryKind::Point => {
            for _ in 0..r.count()? {
                points.push(MultiPoint(r.coords()?.into_iter().map(Point).collect()));
            }
        }
    }
    Ok(LayerGeometry::from_parts(
        kind, geographic, topology, lines, points,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LabelMode, MapLayer, MapLayerKind, SourceInfo};
    use geo_types::{MultiPolygon, polygon};
    use territoria_data::{Column, ColumnKind};
    use territoria_io::Geometries;

    #[test]
    fn project_round_trip() {
        let areas: Vec<MultiPolygon<f64>> = (0..4)
            .map(|i| {
                let x = i as f64;
                MultiPolygon(vec![polygon![(x: x, y: 0.0), (x: x + 1.0, y: 0.0), (x: x + 1.0, y: 1.0), (x: x, y: 1.0), (x: x, y: 0.0)]])
            })
            .collect();
        let mut p = Project::new();
        let attrs = Table::new(vec![Column::text(
            "code",
            ColumnKind::Key,
            (1..=4).map(|i| Some(format!("0{i}"))).collect(),
        )])
        .unwrap();
        let geom = LayerGeometry::from_imported(Geometries::Areas(areas), true);
        let layer = p.add_layer("Areas", geom, attrs, SourceInfo::default());
        let plan = p.add_plan("Plan", layer, 2);
        let t = p.plan(plan).unwrap().territories[0].id;
        p.plan_mut(plan).unwrap().assign(&[0, 1], Some(t));
        p.plan_mut(plan).unwrap().set_pinned(&[1], true);
        let map = p.add_map("Map");
        let ml = p.new_map_layer_id();
        p.map_mut(map).unwrap().layers.push(MapLayer {
            id: ml,
            name: "Areas".into(),
            visible: true,
            opacity: 1.0,
            zoom: Default::default(),
            kind: MapLayerKind::Boundaries {
                layer,
                style: crate::BoundaryStyle {
                    labels: LabelMode::Key,
                    ..Default::default()
                },
            },
        });

        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            write_project(&p, &mut zip).unwrap();
            zip.finish().unwrap();
        }
        buf.set_position(0);
        let back = read_project(buf).unwrap();
        assert_eq!(back.layers.len(), 1);
        let l = &back.layers[0];
        assert_eq!(l.key_column.as_deref(), Some("code"));
        assert_eq!(l.geometry.topology, p.layers[0].geometry.topology);
        assert_eq!(l.geometry.neighbors[1].len(), 2);
        assert_eq!(l.attributes.rows(), 4);
        let bp = back.plan(plan).unwrap();
        assert_eq!(bp.assignment[0], Some(t));
        assert_eq!(*bp.pinned, vec![false, true, false, false]);
        assert_eq!(back.map(map).unwrap().layers.len(), 1);
        // New ids continue after the loaded ones.
        let mut back = back;
        assert!(back.next_id() > ml.0);
    }

    #[test]
    fn rejects_damaged_geometry() {
        assert!(read_geometry(b"XXXX").is_err());
        let mut buf = Vec::new();
        write_geometry(&LayerGeometry::default(), &mut buf);
        assert!(read_geometry(&buf[..buf.len() - 1]).is_err());
        buf.truncate(10);
        buf.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_geometry(&buf).is_err());
    }
}
