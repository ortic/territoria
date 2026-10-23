//! Background maps: XYZ raster tiles in Web Mercator, fetched on worker
//! threads and cached on disk.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, pos2};
use territoria_render::View;

/// Tile servers people can pick without typing a URL.
pub struct Preset {
    pub name: &'static str,
    pub url: &'static str,
    pub attribution: &'static str,
    pub note: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Light grey (Esri)",
        url: "https://server.arcgisonline.com/ArcGIS/rest/services/Canvas/World_Light_Gray_Base/MapServer/tile/{z}/{y}/{x}",
        attribution: "Esri, HERE, Garmin, © OpenStreetMap contributors",
        note: "Calm grey: data stands out. Esri's terms of use apply.",
    },
    Preset {
        name: "OpenStreetMap",
        url: "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
        attribution: "© OpenStreetMap contributors",
        note: "For occasional use; heavy use needs your own tile server.",
    },
    Preset {
        name: "OpenStreetMap Humanitarian",
        url: "https://a.tile.openstreetmap.fr/hot/{z}/{x}/{y}.png",
        attribution: "© OpenStreetMap contributors · Humanitarian OSM Team",
        note: "Softer colours than the standard map.",
    },
    Preset {
        name: "OpenTopoMap",
        url: "https://tile.opentopomap.org/{z}/{x}/{y}.png",
        attribution: "© OpenStreetMap contributors, SRTM · © OpenTopoMap (CC-BY-SA)",
        note: "Terrain and contours.",
    },
];

/// Vector maps people can pick: a style with its own tiles.
pub struct VectorPreset {
    pub name: &'static str,
    pub style: &'static str,
    pub attribution: &'static str,
    pub note: &'static str,
}

pub const VECTOR_PRESETS: &[VectorPreset] = &[
    VectorPreset {
        name: "Positron (OpenFreeMap, vector)",
        style: "https://tiles.openfreemap.org/styles/positron",
        attribution: "OpenFreeMap © OpenMapTiles · Data from OpenStreetMap",
        note: "Light and calm, drawn from vector tiles; free, no key.",
    },
    VectorPreset {
        name: "Liberty (OpenFreeMap, vector)",
        style: "https://tiles.openfreemap.org/styles/liberty",
        attribution: "OpenFreeMap © OpenMapTiles · Data from OpenStreetMap",
        note: "A colourful street map, drawn from vector tiles; free, no key.",
    },
];

const HALF_WORLD: f64 = 20_037_508.342_789_244;
const USER_AGENT: &str = concat!(
    "Territoria/",
    env!("CARGO_PKG_VERSION"),
    " (desktop map viewer)"
);
const WORKERS: usize = 4;
const MAX_TEXTURES: usize = 600;

type Key = (String, u8, u32, u32);

enum Slot {
    Loading,
    Ready(TextureHandle),
    Failed,
}

/// A vector map being opened, ready, or what went wrong.
enum VectorState {
    Opening,
    Ready(
        Arc<crate::vector_style::VectorMap>,
        Arc<crate::vector_style::Cache>,
    ),
    Failed(String),
}

/// The tile key of a vector map.
fn vector_id(source: &str, style: &str) -> String {
    format!("vector:{source}|{style}")
}

struct Shared {
    queue: Mutex<VecDeque<(Key, PathBuf)>>,
    /// Vector maps by their tile key.
    vectors: Mutex<HashMap<String, VectorState>>,
    /// For servers that need one, in place of `{key}` in the address.
    api_key: Mutex<territoria_io::ApiKey>,
}

/// Tiles in memory (as textures) and on disk.
pub struct Tiles {
    slots: HashMap<Key, (Slot, u64)>,
    shared: Arc<Shared>,
    rx: Receiver<(Key, Option<ColorImage>)>,
    tx: Sender<(Key, Option<ColorImage>)>,
    started: bool,
    cache_dir: PathBuf,
    frame: u64,
}

impl Tiles {
    pub fn cache_dir(&self) -> &std::path::Path {
        &self.cache_dir
    }

    pub fn new(cache_dir: PathBuf) -> Tiles {
        let (tx, rx) = channel();
        Tiles {
            slots: HashMap::new(),
            shared: Arc::new(Shared {
                queue: Mutex::new(VecDeque::new()),
                vectors: Mutex::new(HashMap::new()),
                api_key: Mutex::new(Default::default()),
            }),
            rx,
            tx,
            started: false,
            cache_dir,
            frame: 0,
        }
    }

    /// The key for tile addresses with `{key}`; tiles that failed for want
    /// of one are tried again.
    pub fn set_key(&mut self, key: &territoria_io::ApiKey) {
        let mut current = self.shared.api_key.lock().unwrap();
        if *current != *key {
            *current = key.clone();
            self.slots.retain(|_, (s, _)| !matches!(s, Slot::Failed));
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.started {
            return;
        }
        self.started = true;
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(20)))
            .user_agent(USER_AGENT)
            .build()
            .new_agent();
        for _ in 0..WORKERS {
            let shared = self.shared.clone();
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            let agent = agent.clone();
            std::thread::Builder::new()
                .name("territoria-tiles".into())
                .spawn(move || {
                    loop {
                        let job = shared.queue.lock().unwrap().pop_back();
                        let Some((key, file)) = job else {
                            std::thread::sleep(std::time::Duration::from_millis(30));
                            continue;
                        };
                        let api_key = shared.api_key.lock().unwrap().clone();
                        let image = if key.0.starts_with("vector:") {
                            draw_vector(&shared, &agent, &key, &api_key)
                        } else {
                            load(&agent, &key, &file, &api_key)
                        };
                        if tx.send((key, image)).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                })
                .expect("spawning a tile worker");
        }
    }

    /// Draws a vector map, once it is open (opening starts in the
    /// background on first use).
    pub fn paint_vector(
        &mut self,
        painter: &egui::Painter,
        view: &View,
        source: &str,
        style: &str,
        pixels_per_point: f32,
        tint: Color32,
    ) {
        let id = vector_id(source, style);
        let ready = {
            let mut vectors = self.shared.vectors.lock().unwrap();
            match vectors.get(&id) {
                Some(VectorState::Ready(..)) => true,
                Some(_) => false,
                None => {
                    vectors.insert(id.clone(), VectorState::Opening);
                    let shared = self.shared.clone();
                    let ctx = painter.ctx().clone();
                    let (source, style, id) = (source.to_owned(), style.to_owned(), id.clone());
                    let _ = std::thread::Builder::new()
                        .name("territoria-vector-map".into())
                        .spawn(move || {
                            let agent = ureq::Agent::config_builder()
                                .timeout_global(Some(std::time::Duration::from_secs(30)))
                                .user_agent(USER_AGENT)
                                .build()
                                .new_agent();
                            let key = shared.api_key.lock().unwrap().clone();
                            let state = match crate::vector_style::VectorMap::open(
                                &source, &style, &agent, &key,
                            ) {
                                Ok(m) => VectorState::Ready(Arc::new(m), Default::default()),
                                Err(e) => {
                                    tracing::warn!("vector map: {e}");
                                    VectorState::Failed(e)
                                }
                            };
                            shared.vectors.lock().unwrap().insert(id, state);
                            ctx.request_repaint();
                        });
                    false
                }
            }
        };
        if ready {
            self.paint(painter, view, &id, pixels_per_point, tint);
        }
    }

    /// Why a vector map can't be drawn, if it can't.
    pub fn vector_problem(&self, source: &str, style: &str) -> Option<String> {
        match self
            .shared
            .vectors
            .lock()
            .unwrap()
            .get(&vector_id(source, style))
        {
            Some(VectorState::Failed(e)) => Some(e.clone()),
            _ => None,
        }
    }

    /// Tiles drawn and ready.
    #[cfg(test)]
    pub fn ready(&self) -> usize {
        self.slots
            .values()
            .filter(|(s, _)| matches!(s, Slot::Ready(_)))
            .count()
    }

    /// Tiles still loading.
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.slots
            .values()
            .filter(|(s, _)| matches!(s, Slot::Loading))
            .count()
    }

    /// Draws the tiles covering `view` (Web Mercator display coordinates).
    pub fn paint(
        &mut self,
        painter: &egui::Painter,
        view: &View,
        url: &str,
        pixels_per_point: f32,
        tint: Color32,
    ) {
        self.start(painter.ctx());
        self.frame += 1;
        while let Ok((key, image)) = self.rx.try_recv() {
            let slot = match image {
                Some(img) => Slot::Ready(painter.ctx().load_texture(
                    format!("tile-{}-{}-{}", key.1, key.2, key.3),
                    img,
                    TextureOptions::LINEAR,
                )),
                None => Slot::Failed,
            };
            self.slots.insert(key, (slot, self.frame));
        }
        // The zoom level whose pixels match the screen's.
        let metres_per_pixel = view.scale / pixels_per_point as f64;
        let z = ((2.0 * HALF_WORLD / 256.0) / metres_per_pixel)
            .log2()
            .round()
            .clamp(0.0, 19.0) as u8;
        let n = 1u32 << z;
        let size = 2.0 * HALF_WORLD / n as f64;
        let (min, max) = view.visible();
        let tx = |x: f64| ((x + HALF_WORLD) / size).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let ty = |y: f64| ((HALF_WORLD - y) / size).floor().clamp(0.0, n as f64 - 1.0) as u32;
        let (x0, x1) = (tx(min[0]), tx(max[0]));
        let (y0, y1) = (ty(max[1]), ty(min[1]));
        if (x1 - x0 + 1) as u64 * (y1 - y0 + 1) as u64 > 400 {
            return;
        }
        let template_dir = self.cache_dir.join(format!("{:016x}", hash(url)));
        let mut wanted = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let rect = Rect::from_min_max(
                    view.to_screen([x as f64 * size - HALF_WORLD, HALF_WORLD - y as f64 * size]),
                    view.to_screen([
                        (x + 1) as f64 * size - HALF_WORLD,
                        HALF_WORLD - (y + 1) as f64 * size,
                    ]),
                );
                let key = (url.to_owned(), z, x, y);
                match self.slots.get_mut(&key) {
                    Some((Slot::Ready(tex), used)) => {
                        *used = self.frame;
                        painter.image(
                            tex.id(),
                            rect,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            tint,
                        );
                    }
                    Some((Slot::Loading | Slot::Failed, _)) => {
                        self.paint_parent(painter, &key, rect, tint);
                    }
                    None => {
                        self.paint_parent(painter, &key, rect, tint);
                        let file = template_dir.join(format!("{z}/{x}/{y}"));
                        wanted.push((key.clone(), file));
                        self.slots.insert(key, (Slot::Loading, self.frame));
                    }
                }
            }
        }
        if !wanted.is_empty() {
            let mut q = self.shared.queue.lock().unwrap();
            // Newest requests first; drop stale ones from earlier views.
            q.retain(|(k, _)| k.1 == z);
            q.extend(wanted);
        }
        self.evict(painter.ctx());
    }

    /// While a tile loads, a scaled-up part of a coarser one fills in.
    fn paint_parent(&self, painter: &egui::Painter, key: &Key, rect: Rect, tint: Color32) {
        let (url, z, x, y) = (&key.0, key.1, key.2, key.3);
        for up in 1..=4u8 {
            if up > z {
                break;
            }
            let parent = (url.clone(), z - up, x >> up, y >> up);
            if let Some((Slot::Ready(tex), _)) = self.slots.get(&parent) {
                let k = 1u32 << up;
                let (fx, fy) = ((x % k) as f32 / k as f32, (y % k) as f32 / k as f32);
                let w = 1.0 / k as f32;
                let uv = Rect::from_min_max(pos2(fx, fy), pos2(fx + w, fy + w));
                painter.image(tex.id(), rect, uv, tint);
                return;
            }
        }
    }

    /// Forgets the least recently used textures beyond the limit; drawn
    /// vector tiles have four times the pixels of raster ones.
    fn evict(&mut self, _ctx: &egui::Context) {
        let weight = |k: &Key| if k.0.starts_with("vector:") { 4 } else { 1 };
        let mut total: usize = self.slots.keys().map(weight).sum();
        if total <= MAX_TEXTURES {
            return;
        }
        let mut ages: Vec<(Key, u64)> = self
            .slots
            .iter()
            .map(|(k, (_, used))| (k.clone(), *used))
            .collect();
        ages.sort_by_key(|(_, used)| *used);
        for (k, _) in ages {
            if total <= MAX_TEXTURES {
                break;
            }
            if !matches!(self.slots.get(&k), Some((Slot::Loading, _))) {
                total -= weight(&k);
                self.slots.remove(&k);
            }
        }
    }
}

fn hash(s: &str) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// A tile of an open vector map, drawn.
fn draw_vector(
    shared: &Shared,
    agent: &ureq::Agent,
    key: &Key,
    api_key: &territoria_io::ApiKey,
) -> Option<ColorImage> {
    let (map, cache) = match shared.vectors.lock().unwrap().get(&key.0) {
        Some(VectorState::Ready(m, c)) => (m.clone(), c.clone()),
        _ => return None,
    };
    let fetch = |url: &str| -> Option<Vec<u8>> {
        let mut response = agent.get(api_key.apply(url, false)).call().ok()?;
        response
            .body_mut()
            .with_config()
            .limit(16 << 20)
            .read_to_vec()
            .ok()
    };
    map.tile_image(key.1, key.2, key.3, &fetch, &cache)
}

/// From the disk cache, else from the server (and cached).
fn load(
    agent: &ureq::Agent,
    key: &Key,
    file: &std::path::Path,
    api_key: &territoria_io::ApiKey,
) -> Option<ColorImage> {
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(_) => {
            let (url, z, x, y) = key;
            let url = api_key
                .apply(url, false)
                .replace("{z}", &z.to_string())
                .replace("{x}", &x.to_string())
                .replace("{y}", &y.to_string());
            let mut response = agent.get(&url).call().ok()?;
            let bytes = response
                .body_mut()
                .with_config()
                .limit(8 << 20)
                .read_to_vec()
                .ok()?;
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(file, &bytes);
            }
            bytes
        }
    };
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}
