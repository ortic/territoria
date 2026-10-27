//! Addresses to coordinates through a geocoding service: Nominatim (the
//! OpenStreetMap one, or your own) or Photon. Results are cached on disk,
//! so a run that was stopped resumes where it ended and repeated addresses
//! cost nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use web_time::Instant;

use crate::ImportError;
use crate::http::Client;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GeocoderKind {
    #[default]
    Nominatim,
    Photon,
}

impl GeocoderKind {
    pub fn label(self) -> &'static str {
        match self {
            GeocoderKind::Nominatim => "Nominatim",
            GeocoderKind::Photon => "Photon",
        }
    }

    pub fn default_url(self) -> &'static str {
        match self {
            GeocoderKind::Nominatim => "https://nominatim.openstreetmap.org",
            GeocoderKind::Photon => "https://photon.komoot.io",
        }
    }
}

/// Which service, where, and how politely.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeocoderSettings {
    pub kind: GeocoderKind,
    pub url: String,
    /// Contact address sent along, as public services ask for.
    pub email: String,
    /// Pause between requests in milliseconds (the public Nominatim allows
    /// one per second).
    pub interval_ms: u64,
    /// For services that need one: in place of `{key}` in the address,
    /// else sent as the `key` parameter.
    #[serde(skip)]
    pub key: crate::ApiKey,
}

impl Default for GeocoderSettings {
    fn default() -> Self {
        GeocoderSettings {
            kind: GeocoderKind::Nominatim,
            url: GeocoderKind::Nominatim.default_url().into(),
            email: String::new(),
            interval_ms: 1100,
            key: Default::default(),
        }
    }
}

/// How precisely an address was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Quality {
    /// The building.
    House,
    Street,
    Postcode,
    /// Town or village.
    Place,
    /// Anything coarser.
    Region,
}

impl Quality {
    pub fn label(self) -> &'static str {
        match self {
            Quality::House => "house",
            Quality::Street => "street",
            Quality::Postcode => "postcode",
            Quality::Place => "town",
            Quality::Region => "region",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub lon: f64,
    pub lat: f64,
    pub quality: Quality,
    /// What the service found, as it names it.
    pub label: String,
}

/// An address in parts; empty parts are left out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Address {
    pub street: String,
    pub postcode: String,
    pub city: String,
    pub country: String,
    /// The whole address in one text (instead of the parts).
    pub text: String,
}

impl Address {
    pub fn is_empty(&self) -> bool {
        [&self.street, &self.postcode, &self.city, &self.text]
            .iter()
            .all(|s| s.trim().is_empty())
    }

    /// One line, for free-text services and the cache.
    pub fn line(&self) -> String {
        if !self.text.trim().is_empty() {
            return [self.text.trim(), self.country.trim()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(", ");
        }
        let place = [self.postcode.trim(), self.city.trim()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        [self.street.trim(), place.as_str(), self.country.trim()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Answers by query, kept in one JSON file.
struct DiskCache {
    file: PathBuf,
    entries: HashMap<String, Option<Hit>>,
    unsaved: usize,
}

impl DiskCache {
    fn open(dir: &Path) -> DiskCache {
        let file = dir.join("geocoding-cache.json");
        let entries = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        DiskCache {
            file,
            entries,
            unsaved: 0,
        }
    }

    fn save(&mut self) {
        if self.unsaved == 0 {
            return;
        }
        if let Some(dir) = self.file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&self.entries) {
            let tmp = self.file.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, &self.file);
            }
        }
        self.unsaved = 0;
    }
}

fn cache_key(settings: &GeocoderSettings, a: &Address) -> String {
    format!(
        "{}|{}|{}",
        settings.kind.label(),
        settings.url.trim_end_matches('/'),
        a.line().to_lowercase()
    )
}

fn nominatim_quality(v: &serde_json::Value) -> Quality {
    let kind = v["addresstype"]
        .as_str()
        .or(v["type"].as_str())
        .unwrap_or_default();
    let rank = v["place_rank"].as_u64().unwrap_or(0);
    match kind {
        "house" | "building" | "house_number" => Quality::House,
        "postcode" => Quality::Postcode,
        "road" | "street" | "residential" => Quality::Street,
        _ if rank >= 30 => Quality::House,
        _ if rank >= 26 => Quality::Street,
        _ if rank >= 13 => Quality::Place,
        _ => Quality::Region,
    }
}

/// The first result of a Nominatim `jsonv2` answer.
pub(crate) fn parse_nominatim(text: &str) -> Result<Option<Hit>, ImportError> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| ImportError::Format(format!("geocoder: {e}")))?;
    let Some(first) = v.as_array().and_then(|a| a.first()) else {
        return Ok(None);
    };
    let num = |k: &str| {
        first[k]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .or(first[k].as_f64())
    };
    Ok(match (num("lon"), num("lat")) {
        (Some(lon), Some(lat)) => Some(Hit {
            lon,
            lat,
            quality: nominatim_quality(first),
            label: first["display_name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        }),
        _ => None,
    })
}

/// The first feature of a Photon GeoJSON answer.
pub(crate) fn parse_photon(text: &str) -> Result<Option<Hit>, ImportError> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| ImportError::Format(format!("geocoder: {e}")))?;
    let Some(f) = v["features"].as_array().and_then(|a| a.first()) else {
        return Ok(None);
    };
    let c = &f["geometry"]["coordinates"];
    let p = &f["properties"];
    let quality = match p["type"].as_str().unwrap_or_default() {
        "house" => Quality::House,
        "street" => Quality::Street,
        "postcode" => Quality::Postcode,
        "city" | "district" | "locality" => Quality::Place,
        _ if p["housenumber"].is_string() => Quality::House,
        _ => Quality::Region,
    };
    let label = ["name", "street", "postcode", "city", "country"]
        .iter()
        .filter_map(|k| p[*k].as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(match (c[0].as_f64(), c[1].as_f64()) {
        (Some(lon), Some(lat)) => Some(Hit {
            lon,
            lat,
            quality,
            label,
        }),
        _ => None,
    })
}

fn ask(
    client: &Client,
    settings: &GeocoderSettings,
    a: &Address,
) -> Result<Option<Hit>, ImportError> {
    let url = settings.key.apply(&settings.url, false);
    let base = url.trim_end_matches('/');
    // Without `{key}` in the address, the key goes along as a parameter.
    let key = settings
        .key
        .get()
        .filter(|_| !settings.url.contains("{key}"));
    let net = |e: String| ImportError::Format(format!("geocoder: {e}"));
    let line = a.line();
    match settings.kind {
        GeocoderKind::Nominatim => {
            let mut query = vec![("format", "jsonv2"), ("limit", "1")];
            if a.text.trim().is_empty() {
                // Structured search: parts are matched for what they are.
                for (k, v) in [
                    ("street", &a.street),
                    ("postalcode", &a.postcode),
                    ("city", &a.city),
                    ("country", &a.country),
                ] {
                    if !v.trim().is_empty() {
                        query.push((k, v.trim()));
                    }
                }
            } else {
                query.push(("q", &line));
            }
            if !settings.email.trim().is_empty() {
                query.push(("email", settings.email.trim()));
            }
            if let Some(k) = key {
                query.push(("key", k));
            }
            let text = client
                .text(&format!("{base}/search"), &query)
                .map_err(net)?;
            parse_nominatim(&text)
        }
        GeocoderKind::Photon => {
            let mut query = vec![("q", line.as_str()), ("limit", "1")];
            if let Some(k) = key {
                query.push(("key", k));
            }
            let text = client.text(&format!("{base}/api"), &query).map_err(net)?;
            parse_photon(&text)
        }
    }
}

/// How a geocoding run went.
#[derive(Debug, Clone, Default)]
pub struct Geocoded {
    pub hits: Vec<Option<Hit>>,
    /// Addresses answered from the cache.
    pub cached: usize,
    /// Stopped before the end: the rest have no answer yet.
    pub stopped: bool,
}

/// Geocodes `addresses` (empty ones are skipped). `progress(done, total)`
/// returns `false` to stop; what was found so far stays cached.
pub fn geocode(
    settings: &GeocoderSettings,
    addresses: &[Address],
    cache_dir: &Path,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Geocoded, ImportError> {
    let mut cache = DiskCache::open(cache_dir);
    let user_agent = if settings.email.trim().is_empty() {
        crate::online::USER_AGENT.to_owned()
    } else {
        format!("{} ({})", crate::online::USER_AGENT, settings.email.trim())
    };
    let client = Client::new(&user_agent, None, Duration::from_secs(30));
    let mut out = Geocoded {
        hits: vec![None; addresses.len()],
        ..Default::default()
    };
    // The same address once.
    let mut asked: HashMap<String, Option<Hit>> = HashMap::new();
    let mut last: Option<Instant> = None;
    let mut failures = 0;
    for (i, a) in addresses.iter().enumerate() {
        if !progress(i, addresses.len()) {
            out.stopped = true;
            break;
        }
        if a.is_empty() {
            continue;
        }
        let key = cache_key(settings, a);
        if let Some(h) = cache.entries.get(&key).or(asked.get(&key)) {
            out.hits[i] = h.clone();
            out.cached += 1;
            continue;
        }
        if let Some(t) = last {
            let wait = Duration::from_millis(settings.interval_ms).saturating_sub(t.elapsed());
            crate::http::pause(wait);
        }
        last = Some(Instant::now());
        match ask(&client, settings, a) {
            Ok(hit) => {
                failures = 0;
                out.hits[i] = hit.clone();
                cache.entries.insert(key.clone(), hit.clone());
                asked.insert(key, hit);
                cache.unsaved += 1;
                if cache.unsaved >= 20 {
                    cache.save();
                }
            }
            Err(e) => {
                failures += 1;
                // A service that keeps failing: stop, keep what we have.
                if failures >= 3 {
                    cache.save();
                    return Err(e);
                }
            }
        }
    }
    cache.save();
    progress(addresses.len(), addresses.len());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    #[test]
    fn parses_answers() {
        let n = r#"[{"lat":"46.9480","lon":"7.4474","display_name":"Bundesplatz 3, 3011 Bern","addresstype":"building","place_rank":30}]"#;
        let h = parse_nominatim(n).unwrap().unwrap();
        assert_eq!((h.lon, h.lat, h.quality), (7.4474, 46.948, Quality::House));
        assert_eq!(parse_nominatim("[]").unwrap(), None);
        let p = r#"{"features":[{"geometry":{"coordinates":[8.54,47.37]},"properties":{"type":"city","name":"Zürich","country":"Switzerland"}}]}"#;
        let h = parse_photon(p).unwrap().unwrap();
        assert_eq!((h.lon, h.quality), (8.54, Quality::Place));
        assert_eq!(h.label, "Zürich, Switzerland");
        let a = Address {
            street: "Bundesplatz 3".into(),
            postcode: "3011".into(),
            city: "Bern".into(),
            country: "CH".into(),
            text: String::new(),
        };
        assert_eq!(a.line(), "Bundesplatz 3, 3011 Bern, CH");
    }

    /// A tiny Nominatim stand-in that counts the requests it answers.
    fn server(answers: usize) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for stream in listener.incoming().take(answers) {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                reader.read_line(&mut line).unwrap();
                // Skip the headers.
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap() <= 2 {
                        break;
                    }
                }
                let body = if line.contains("city=Nowhere") {
                    "[]".to_owned()
                } else {
                    r#"[{"lat":"46.95","lon":"7.45","display_name":"Bern","addresstype":"city","place_rank":16}]"#.to_owned()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
                requests.push(line);
            }
            requests
        });
        (url, handle)
    }

    #[test]
    fn geocodes_with_cache() {
        let dir = std::env::temp_dir().join(format!("territoria-geocode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (url, server) = server(2);
        let settings = GeocoderSettings {
            url,
            interval_ms: 0,
            ..Default::default()
        };
        let bern = Address {
            postcode: "3011".into(),
            city: "Bern".into(),
            ..Default::default()
        };
        let nowhere = Address {
            city: "Nowhere".into(),
            ..Default::default()
        };
        // Bern twice and an empty address: two requests only.
        let list = [
            bern.clone(),
            nowhere.clone(),
            Address::default(),
            bern.clone(),
        ];
        let r = geocode(&settings, &list, &dir, &mut |_, _| true).unwrap();
        assert_eq!(r.hits[0].as_ref().unwrap().quality, Quality::Place);
        assert_eq!(r.hits[1], None);
        assert_eq!(r.hits[2], None);
        assert_eq!(r.hits[3], r.hits[0]);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("postalcode=3011"), "{}", requests[0]);
        // Again: all from the cache, no server needed.
        let r = geocode(&settings, &list, &dir, &mut |_, _| true).unwrap();
        assert_eq!(r.cached, 3);
        assert_eq!(r.hits[0].as_ref().unwrap().lon, 7.45);
        let _ = std::fs::remove_dir_all(dir);
    }
}
