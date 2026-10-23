//! Vector tiles: Mapbox Vector Tile (MVT) decoding, and tiles from
//! PMTiles and MBTiles archives on disk.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

use crate::ImportError;

/// A decoded tile: its layers by name.
#[derive(Debug, Clone, Default)]
pub struct VectorTile {
    pub layers: Vec<TileLayer>,
}

#[derive(Debug, Clone)]
pub struct TileLayer {
    pub name: String,
    /// Coordinates run from 0 to `extent` across the tile.
    pub extent: u32,
    pub features: Vec<TileFeature>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureKind {
    Point,
    Line,
    Polygon,
}

#[derive(Debug, Clone)]
pub struct TileFeature {
    pub kind: FeatureKind,
    /// Points, lines, or rings (outer and holes, in order).
    pub parts: Vec<Vec<[f32; 2]>>,
    pub properties: HashMap<String, serde_json::Value>,
}

impl VectorTile {
    pub fn layer(&self, name: &str) -> Option<&TileLayer> {
        self.layers.iter().find(|l| l.name == name)
    }
}

/// Decodes a tile, gzipped or not.
pub fn decode(bytes: &[u8]) -> Result<VectorTile, ImportError> {
    let bytes = gunzip_if_needed(bytes)?;
    let mut tile = VectorTile::default();
    let mut r = Proto::new(&bytes);
    while let Some((field, wire)) = r.key()? {
        match (field, wire) {
            (3, 2) => tile.layers.push(decode_layer(r.bytes()?)?),
            _ => r.skip(wire)?,
        }
    }
    Ok(tile)
}

fn gunzip_if_needed(bytes: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>, ImportError> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes).read_to_end(&mut out)?;
        Ok(out.into())
    } else {
        Ok(bytes.into())
    }
}

fn decode_layer(bytes: &[u8]) -> Result<TileLayer, ImportError> {
    let mut name = String::new();
    let mut extent = 4096;
    let mut keys = Vec::new();
    let mut values = Vec::new();
    let mut raw_features = Vec::new();
    let mut r = Proto::new(bytes);
    while let Some((field, wire)) = r.key()? {
        match (field, wire) {
            (1, 2) => name = String::from_utf8_lossy(r.bytes()?).into_owned(),
            (2, 2) => raw_features.push(r.bytes()?),
            (3, 2) => keys.push(String::from_utf8_lossy(r.bytes()?).into_owned()),
            (4, 2) => values.push(decode_value(r.bytes()?)?),
            (5, 0) => extent = r.varint()? as u32,
            _ => r.skip(wire)?,
        }
    }
    let mut features = Vec::with_capacity(raw_features.len());
    for f in raw_features {
        if let Some(f) = decode_feature(f, &keys, &values)? {
            features.push(f);
        }
    }
    Ok(TileLayer {
        name,
        extent: extent.max(1),
        features,
    })
}

fn decode_value(bytes: &[u8]) -> Result<serde_json::Value, ImportError> {
    use serde_json::Value;
    let mut r = Proto::new(bytes);
    let mut v = Value::Null;
    while let Some((field, wire)) = r.key()? {
        v = match (field, wire) {
            (1, 2) => Value::String(String::from_utf8_lossy(r.bytes()?).into_owned()),
            (2, 5) => number(f32::from_le_bytes(r.fixed::<4>()?) as f64),
            (3, 1) => number(f64::from_le_bytes(r.fixed::<8>()?)),
            (4, 0) => number(r.varint()? as i64 as f64),
            (5, 0) => number(r.varint()? as f64),
            (6, 0) => number(zigzag(r.varint()?) as f64),
            (7, 0) => Value::Bool(r.varint()? != 0),
            _ => {
                r.skip(wire)?;
                continue;
            }
        };
    }
    Ok(v)
}

fn number(n: f64) -> serde_json::Value {
    serde_json::Number::from_f64(n).map_or(serde_json::Value::Null, serde_json::Value::Number)
}

fn zigzag(n: u64) -> i64 {
    ((n >> 1) as i64) ^ -((n & 1) as i64)
}

fn decode_feature(
    bytes: &[u8],
    keys: &[String],
    values: &[serde_json::Value],
) -> Result<Option<TileFeature>, ImportError> {
    let mut kind = None;
    let mut tags = Vec::new();
    let mut geometry = Vec::new();
    let mut r = Proto::new(bytes);
    while let Some((field, wire)) = r.key()? {
        match (field, wire) {
            (2, 2) => tags = r.packed()?,
            (3, 0) => {
                kind = match r.varint()? {
                    1 => Some(FeatureKind::Point),
                    2 => Some(FeatureKind::Line),
                    3 => Some(FeatureKind::Polygon),
                    _ => None,
                }
            }
            (4, 2) => geometry = r.packed()?,
            _ => r.skip(wire)?,
        }
    }
    let Some(kind) = kind else { return Ok(None) };
    let mut properties = HashMap::new();
    for pair in tags.chunks_exact(2) {
        if let (Some(k), Some(v)) = (keys.get(pair[0] as usize), values.get(pair[1] as usize)) {
            properties.insert(k.clone(), v.clone());
        }
    }
    // Commands: MoveTo (1), LineTo (2), ClosePath (7), each with a count.
    let mut parts: Vec<Vec<[f32; 2]>> = Vec::new();
    let (mut x, mut y) = (0i64, 0i64);
    let mut i = 0;
    while i < geometry.len() {
        let (cmd, count) = (geometry[i] & 7, (geometry[i] >> 3) as usize);
        i += 1;
        match cmd {
            1 | 2 => {
                for _ in 0..count {
                    let (Some(&dx), Some(&dy)) = (geometry.get(i), geometry.get(i + 1)) else {
                        break;
                    };
                    i += 2;
                    x += zigzag(dx);
                    y += zigzag(dy);
                    if cmd == 1 && kind != FeatureKind::Point || parts.is_empty() {
                        parts.push(Vec::new());
                    }
                    if let Some(p) = parts.last_mut() {
                        p.push([x as f32, y as f32]);
                    }
                }
            }
            7 => {
                if let Some(first) = parts.last().and_then(|p| p.first()).copied()
                    && let Some(p) = parts.last_mut()
                {
                    p.push(first);
                }
            }
            _ => break,
        }
    }
    Ok(Some(TileFeature {
        kind,
        parts,
        properties,
    }))
}

/// Just enough Protocol Buffers for vector tiles.
struct Proto<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Proto<'a> {
    fn new(b: &'a [u8]) -> Self {
        Proto { b, at: 0 }
    }

    fn eof() -> ImportError {
        ImportError::Format("vector tile: cut short".into())
    }

    fn varint(&mut self) -> Result<u64, ImportError> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.b.get(self.at).ok_or_else(Self::eof)?;
            self.at += 1;
            v |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(ImportError::Format("vector tile: bad number".into()))
    }

    fn key(&mut self) -> Result<Option<(u64, u8)>, ImportError> {
        if self.at >= self.b.len() {
            return Ok(None);
        }
        let k = self.varint()?;
        Ok(Some((k >> 3, (k & 7) as u8)))
    }

    fn bytes(&mut self) -> Result<&'a [u8], ImportError> {
        let n = self.varint()? as usize;
        let end = self
            .at
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(Self::eof)?;
        let out = &self.b[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], ImportError> {
        let end = self.at + N;
        let out: [u8; N] = self
            .b
            .get(self.at..end)
            .ok_or_else(Self::eof)?
            .try_into()
            .map_err(|_| Self::eof())?;
        self.at = end;
        Ok(out)
    }

    fn packed(&mut self) -> Result<Vec<u64>, ImportError> {
        let mut inner = Proto::new(self.bytes()?);
        let mut out = Vec::new();
        while inner.at < inner.b.len() {
            out.push(inner.varint()?);
        }
        Ok(out)
    }

    fn skip(&mut self, wire: u8) -> Result<(), ImportError> {
        match wire {
            0 => {
                self.varint()?;
            }
            1 => self.at += 8,
            2 => {
                self.bytes()?;
            }
            5 => self.at += 4,
            _ => {
                return Err(ImportError::Format(
                    "vector tile: unknown field type".into(),
                ));
            }
        }
        if self.at > self.b.len() {
            return Err(Self::eof());
        }
        Ok(())
    }
}

/// Tiles from a file: a PMTiles or an MBTiles archive.
pub enum TileArchive {
    PmTiles(PmTiles),
    MbTiles {
        db: Mutex<rusqlite::Connection>,
        max_zoom: u8,
    },
}

impl TileArchive {
    pub fn open(path: &Path) -> Result<TileArchive, ImportError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase();
        match ext.as_str() {
            "pmtiles" => Ok(TileArchive::PmTiles(PmTiles::open(path)?)),
            "mbtiles" => {
                let err =
                    |e: rusqlite::Error| ImportError::Format(format!("{}: {e}", path.display()));
                let db = rusqlite::Connection::open_with_flags(
                    path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )
                .map_err(err)?;
                let max_zoom: Option<i64> = db
                    .query_row("SELECT MAX(zoom_level) FROM tiles", [], |r| r.get(0))
                    .map_err(err)?;
                Ok(TileArchive::MbTiles {
                    db: Mutex::new(db),
                    max_zoom: max_zoom.unwrap_or(14).clamp(0, 30) as u8,
                })
            }
            _ => Err(ImportError::Unsupported(ext)),
        }
    }

    /// The highest zoom level with tiles; finer views draw these larger.
    pub fn max_zoom(&self) -> u8 {
        match self {
            TileArchive::PmTiles(p) => p.max_zoom,
            TileArchive::MbTiles { max_zoom, .. } => *max_zoom,
        }
    }

    /// The tile's bytes (maybe gzipped), if there is one.
    pub fn tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, ImportError> {
        match self {
            TileArchive::PmTiles(p) => p.tile(z, x, y),
            TileArchive::MbTiles { db, .. } => {
                // MBTiles count rows from the bottom (TMS).
                let row = (1u32 << z) - 1 - y;
                let db = db.lock().unwrap_or_else(|e| e.into_inner());
                match db.query_row(
                    "SELECT tile_data FROM tiles WHERE zoom_level = ?1 AND tile_column = ?2 AND tile_row = ?3",
                    rusqlite::params![z, x, row],
                    |r| r.get::<_, Vec<u8>>(0),
                ) {
                    Ok(b) => Ok(Some(b)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(ImportError::Format(e.to_string())),
                }
            }
        }
    }
}

/// A PMTiles (version 3) archive on disk.
pub struct PmTiles {
    file: Mutex<std::fs::File>,
    root: Vec<DirEntry>,
    leaf_offset: u64,
    data_offset: u64,
    gzip_dirs: bool,
    pub max_zoom: u8,
    /// Leaf directories read before, by offset.
    leaves: Mutex<HashMap<u64, std::sync::Arc<Vec<DirEntry>>>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DirEntry {
    tile_id: u64,
    offset: u64,
    length: u32,
    /// 0: a leaf directory; else the number of tiles sharing the data.
    run_length: u32,
}

impl PmTiles {
    pub fn open(path: &Path) -> Result<PmTiles, ImportError> {
        let mut file = std::fs::File::open(path)?;
        let mut h = [0u8; 127];
        file.read_exact(&mut h)?;
        if &h[..7] != b"PMTiles" || h[7] != 3 {
            return Err(ImportError::Format(format!(
                "{}: not a PMTiles archive of version 3",
                path.display()
            )));
        }
        let u = |at: usize| u64::from_le_bytes(h[at..at + 8].try_into().unwrap());
        let internal = h[97];
        if internal > 2 {
            return Err(ImportError::Format(
                "PMTiles directories compressed with brotli or zstd aren't supported".into(),
            ));
        }
        let gzip_dirs = internal == 2;
        let root = read_dir(&mut file, u(8), u(16), gzip_dirs)?;
        Ok(PmTiles {
            file: Mutex::new(file),
            root,
            leaf_offset: u(40),
            data_offset: u(56),
            gzip_dirs,
            max_zoom: h[101],
            leaves: Mutex::new(HashMap::new()),
        })
    }

    pub fn tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, ImportError> {
        let id = tile_id(z, x, y);
        let mut dir = std::sync::Arc::new(self.root.clone());
        for _ in 0..4 {
            let Some(e) = find(&dir, id) else {
                return Ok(None);
            };
            if e.run_length > 0 {
                let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
                file.seek(SeekFrom::Start(self.data_offset + e.offset))?;
                let mut out = vec![0; e.length as usize];
                file.read_exact(&mut out)?;
                return Ok(Some(out));
            }
            let cached = self.leaves.lock().unwrap().get(&e.offset).cloned();
            dir = match cached {
                Some(d) => d,
                None => {
                    let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
                    let d = std::sync::Arc::new(read_dir(
                        &mut file,
                        self.leaf_offset + e.offset,
                        e.length as u64,
                        self.gzip_dirs,
                    )?);
                    drop(file);
                    let mut leaves = self.leaves.lock().unwrap();
                    if leaves.len() > 256 {
                        leaves.clear();
                    }
                    leaves.insert(e.offset, d.clone());
                    d
                }
            };
        }
        Ok(None)
    }
}

/// The entry holding `id`: the last one at or before it, if it covers it.
fn find(dir: &[DirEntry], id: u64) -> Option<DirEntry> {
    let i = dir.partition_point(|e| e.tile_id <= id).checked_sub(1)?;
    let e = dir[i];
    (e.run_length == 0 || id < e.tile_id + e.run_length as u64).then_some(e)
}

fn read_dir(
    file: &mut std::fs::File,
    offset: u64,
    length: u64,
    gzip: bool,
) -> Result<Vec<DirEntry>, ImportError> {
    file.seek(SeekFrom::Start(offset))?;
    let mut raw = vec![0; length as usize];
    file.read_exact(&mut raw)?;
    let bytes = if gzip {
        gunzip_if_needed(&raw)?.into_owned()
    } else {
        raw
    };
    let mut r = Proto::new(&bytes);
    let n = r.varint()? as usize;
    let mut entries = vec![
        DirEntry {
            tile_id: 0,
            offset: 0,
            length: 0,
            run_length: 0,
        };
        n.min(1 << 24)
    ];
    let mut last = 0u64;
    for e in &mut entries {
        last += r.varint()?;
        e.tile_id = last;
    }
    for e in &mut entries {
        e.run_length = r.varint()? as u32;
    }
    for e in &mut entries {
        e.length = r.varint()? as u32;
    }
    for i in 0..entries.len() {
        let v = r.varint()?;
        entries[i].offset = if v == 0 && i > 0 {
            entries[i - 1].offset + entries[i - 1].length as u64
        } else {
            v.saturating_sub(1)
        };
    }
    Ok(entries)
}

/// The PMTiles tile id: tiles of coarser zooms first, then along a
/// Hilbert curve.
pub fn tile_id(z: u8, x: u32, y: u32) -> u64 {
    let before: u64 = (0..z as u32).map(|i| 1u64 << (2 * i)).sum();
    let n = 1u64 << z;
    let (mut x, mut y) = (x as u64, y as u64);
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = u64::from(x & s > 0);
        let ry = u64::from(y & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            // Only the bits below `s` count from here on.
            if rx == 1 {
                x = (s - 1).wrapping_sub(x);
                y = (s - 1).wrapping_sub(y);
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    before + d
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A varint as protobuf writes it.
    fn varint(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }

    fn field(n: u64, bytes: &[u8], out: &mut Vec<u8>) {
        varint(n << 3 | 2, out);
        varint(bytes.len() as u64, out);
        out.extend_from_slice(bytes);
    }

    #[test]
    fn decodes_a_square_with_a_property() {
        let zz = |v: i64| ((v << 1) ^ (v >> 63)) as u64;
        // A square: MoveTo(10,10) LineTo(+20,0)(0,+20)(-20,0) ClosePath.
        let mut geom = Vec::new();
        for v in [
            1 << 3 | 1,
            zz(10),
            zz(10),
            3 << 3 | 2,
            zz(20),
            zz(0),
            zz(0),
            zz(20),
            zz(-20),
            zz(0),
            1 << 3 | 7,
        ] {
            varint(v, &mut geom);
        }
        let mut feature = Vec::new();
        field(2, &[0, 0], &mut feature);
        feature.extend([3 << 3, 3]);
        field(4, &geom, &mut feature);
        let mut value = Vec::new();
        field(1, b"lake", &mut value);
        let mut layer = Vec::new();
        field(1, b"water", &mut layer);
        field(2, &feature, &mut layer);
        field(3, b"class", &mut layer);
        field(4, &value, &mut layer);
        layer.extend([5 << 3]);
        varint(4096, &mut layer);
        let mut tile = Vec::new();
        field(3, &layer, &mut tile);
        let t = decode(&tile).unwrap();
        let l = t.layer("water").unwrap();
        assert_eq!(l.extent, 4096);
        let f = &l.features[0];
        assert_eq!(f.kind, FeatureKind::Polygon);
        assert_eq!(
            f.parts,
            vec![vec![
                [10.0, 10.0],
                [30.0, 10.0],
                [30.0, 30.0],
                [10.0, 30.0],
                [10.0, 10.0]
            ]]
        );
        assert_eq!(f.properties["class"], "lake");
        // Gzipped the same.
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tile).unwrap();
        assert_eq!(decode(&gz.finish().unwrap()).unwrap().layers.len(), 1);
    }

    #[test]
    fn pmtiles_ids() {
        assert_eq!(tile_id(0, 0, 0), 0);
        assert_eq!(tile_id(1, 0, 0), 1);
        assert_eq!(tile_id(1, 0, 1), 2);
        assert_eq!(tile_id(1, 1, 1), 3);
        assert_eq!(tile_id(1, 1, 0), 4);
        assert_eq!(tile_id(2, 0, 0), 5);
        // From the PMTiles specification's tests.
        assert_eq!(tile_id(12, 3423, 1763), 19078479);
        // Every id of a level once.
        let mut ids: Vec<u64> = (0..8)
            .flat_map(|x| (0..8).map(move |y| tile_id(3, x, y)))
            .collect();
        ids.sort();
        assert_eq!(ids, (21..85).collect::<Vec<_>>());
    }

    #[test]
    fn pmtiles_archive_round_trip() {
        // A tiny archive: root directory with two tiles, no compression.
        let tiles: [&[u8]; 2] = [b"zero", b"one-one"];
        let mut dir = Vec::new();
        varint(2, &mut dir);
        varint(0, &mut dir); // id 0
        varint(1, &mut dir); // id 1 (delta)
        varint(1, &mut dir);
        varint(1, &mut dir);
        varint(4, &mut dir);
        varint(7, &mut dir);
        varint(1, &mut dir); // offset 0 (+1)
        varint(0, &mut dir); // right after the first
        let mut h = vec![0u8; 127];
        h[..7].copy_from_slice(b"PMTiles");
        h[7] = 3;
        let put =
            |h: &mut Vec<u8>, at: usize, v: u64| h[at..at + 8].copy_from_slice(&v.to_le_bytes());
        put(&mut h, 8, 127);
        put(&mut h, 16, dir.len() as u64);
        let data_at = 127 + dir.len() as u64;
        put(&mut h, 56, data_at);
        h[97] = 1;
        h[101] = 1;
        let mut file = h;
        file.extend(&dir);
        for t in tiles {
            file.extend_from_slice(t);
        }
        let path = std::env::temp_dir().join(format!("territoria-{}.pmtiles", std::process::id()));
        std::fs::write(&path, file).unwrap();
        let a = TileArchive::open(&path).unwrap();
        assert_eq!(a.max_zoom(), 1);
        assert_eq!(a.tile(0, 0, 0).unwrap().as_deref(), Some(&b"zero"[..]));
        assert_eq!(a.tile(1, 0, 0).unwrap().as_deref(), Some(&b"one-one"[..]));
        assert_eq!(a.tile(1, 1, 1).unwrap(), None);
        let _ = std::fs::remove_file(path);
    }
}
