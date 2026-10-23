//! Open boundaries from geoBoundaries (William & Mary geoLab): administrative
//! areas of about 230 countries, each with its own licence.
//!
//! The catalogue is fetched once and cached; downloads are cached too.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::ImportError;

const CATALOG_URL: &str = "https://www.geoboundaries.org/api/current/gbOpen/ALL/ALL/";
const USER_AGENT: &str = concat!("Territoria/", env!("CARGO_PKG_VERSION"));
const CATALOG_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);

/// One downloadable boundary set: a country at one administrative level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundarySet {
    /// ISO 3166-1 alpha-3, e.g. `CHE`.
    pub iso: String,
    pub country: String,
    /// `ADM0` (country) … `ADM5`.
    pub level: String,
    /// What the areas are called there, e.g. "Municipality", "Canton".
    pub kind: String,
    pub units: usize,
    pub year: String,
    pub source: String,
    pub license: String,
    pub url: String,
    pub simplified_url: String,
}

impl BoundarySet {
    /// File name in the cache.
    pub fn file_name(&self, simplified: bool) -> String {
        format!(
            "geoBoundaries-{}-{}{}.geojson",
            self.iso,
            self.level,
            if simplified { "-simplified" } else { "" }
        )
    }

    /// A layer name: "Switzerland · Municipality" (or the level).
    pub fn title(&self) -> String {
        let what = if self.kind.is_empty() {
            &self.level
        } else {
            &self.kind
        };
        format!("{} · {what}", self.country)
    }

    pub fn attribution(&self) -> String {
        format!("{} via geoBoundaries · {}", self.source, self.license)
    }
}

#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "boundaryISO")]
    iso: String,
    #[serde(rename = "boundaryName")]
    name: String,
    #[serde(rename = "boundaryType")]
    level: String,
    #[serde(rename = "boundaryCanonical", default)]
    canonical: Option<String>,
    #[serde(rename = "admUnitCount", default)]
    units: Option<String>,
    #[serde(rename = "boundaryYearRepresented", default)]
    year: Option<String>,
    #[serde(rename = "boundarySource", default)]
    source: Option<String>,
    #[serde(rename = "boundaryLicense", default)]
    license: Option<String>,
    #[serde(rename = "gjDownloadURL", default)]
    url: Option<String>,
    #[serde(rename = "simplifiedGeometryGeoJSON", default)]
    simplified: Option<String>,
}

/// Parses the geoBoundaries API's answer, sorted by country and level.
pub fn parse_catalog(json: &str) -> Result<Vec<BoundarySet>, ImportError> {
    let raw: Vec<Raw> = serde_json::from_str(json)
        .map_err(|e| ImportError::Format(format!("boundary catalogue: {e}")))?;
    let clean = |s: Option<String>| {
        s.filter(|s| !s.is_empty() && s != "nan" && s != "Unknown")
            .unwrap_or_default()
    };
    let mut sets: Vec<BoundarySet> = raw
        .into_iter()
        .filter_map(|r| {
            let url = clean(r.url);
            (!url.is_empty()).then(|| BoundarySet {
                iso: r.iso,
                country: r.name,
                kind: clean(r.canonical),
                units: clean(r.units).parse::<f64>().map_or(0, |v| v as usize),
                year: clean(r.year),
                source: clean(r.source),
                license: clean(r.license),
                simplified_url: clean(r.simplified),
                url,
                level: r.level,
            })
        })
        .collect();
    sets.sort_by(|a, b| a.country.cmp(&b.country).then(a.level.cmp(&b.level)));
    Ok(sets)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_global(Some(Duration::from_secs(600)))
        .user_agent(USER_AGENT)
        .build()
        .new_agent()
}

/// The catalogue, from `cache_dir` when fresh, else from the internet (and
/// cached). A stale cache is used when the internet isn't reachable.
pub fn catalog(cache_dir: &Path) -> Result<Vec<BoundarySet>, ImportError> {
    let file = cache_dir.join("geoboundaries-catalog.json");
    let fresh = std::fs::metadata(&file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < CATALOG_MAX_AGE);
    if fresh && let Ok(text) = std::fs::read_to_string(&file) {
        return parse_catalog(&text);
    }
    let fetched = agent()
        .get(CATALOG_URL)
        .call()
        .and_then(|mut r| r.body_mut().with_config().limit(64 << 20).read_to_string());
    match fetched {
        Ok(text) => {
            let sets = parse_catalog(&text)?;
            std::fs::create_dir_all(cache_dir)?;
            std::fs::write(&file, &text)?;
            Ok(sets)
        }
        Err(e) => match std::fs::read_to_string(&file) {
            Ok(text) => parse_catalog(&text),
            Err(_) => Err(ImportError::Format(format!(
                "couldn't reach geoBoundaries: {e}"
            ))),
        },
    }
}

/// Downloads a boundary set into `cache_dir` (or finds it there).
/// `progress(bytes, total)` is called while downloading; returning `false`
/// cancels.
pub fn download(
    set: &BoundarySet,
    simplified: bool,
    cache_dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
) -> Result<PathBuf, ImportError> {
    let path = cache_dir.join(set.file_name(simplified));
    if path.exists() {
        return Ok(path);
    }
    let url = if simplified && !set.simplified_url.is_empty() {
        &set.simplified_url
    } else {
        &set.url
    };
    std::fs::create_dir_all(cache_dir)?;
    let mut response = agent()
        .get(url)
        .call()
        .map_err(|e| ImportError::Format(format!("download failed: {e}")))?;
    let total = response.body().content_length();
    let tmp = path.with_extension("part");
    let mut out = std::fs::File::create(&tmp)?;
    let mut reader = response.body_mut().as_reader();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        done += n as u64;
        if !progress(done, total) {
            drop(out);
            let _ = std::fs::remove_file(&tmp);
            return Err(ImportError::Format("download cancelled".into()));
        }
    }
    out.flush()?;
    drop(out);
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_catalog_entries() {
        let json = r#"[
            {"boundaryISO":"CHE","boundaryName":"Switzerland","boundaryType":"ADM3","boundaryCanonical":"Municipality",
             "admUnitCount":"2286","boundaryYearRepresented":"2022","boundarySource":"swisstopo","boundaryLicense":"swisstopo License",
             "gjDownloadURL":"https://example.org/che3.geojson","simplifiedGeometryGeoJSON":"https://example.org/che3s.geojson"},
            {"boundaryISO":"AUT","boundaryName":"Austria","boundaryType":"ADM1","boundaryCanonical":"nan",
             "admUnitCount":"9.0","gjDownloadURL":"https://example.org/aut1.geojson"},
            {"boundaryISO":"XXX","boundaryName":"Nowhere","boundaryType":"ADM0"}
        ]"#;
        let sets = parse_catalog(json).unwrap();
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0].country, "Austria");
        assert_eq!(sets[0].units, 9);
        assert_eq!(sets[0].kind, "");
        assert_eq!(sets[1].kind, "Municipality");
        assert_eq!(
            sets[1].file_name(true),
            "geoBoundaries-CHE-ADM3-simplified.geojson"
        );
        assert!(sets[1].attribution().contains("swisstopo License"));
    }

    /// Needs the internet: `cargo test -p territoria-io -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_liechtenstein() {
        let dir = std::env::temp_dir().join(format!("territoria-gb-{}", std::process::id()));
        let sets = catalog(&dir).unwrap();
        assert!(sets.len() > 300, "{}", sets.len());
        let lie = sets
            .iter()
            .find(|s| s.iso == "LIE" && s.level == "ADM1")
            .expect("Liechtenstein");
        let mut calls = 0;
        let path = download(lie, false, &dir, &mut |_, _| {
            calls += 1;
            true
        })
        .unwrap();
        assert!(calls > 0);
        let layers = crate::read_vector(&path, &Default::default()).unwrap();
        assert_eq!(layers[0].geometries.len(), lie.units);
        let _ = std::fs::remove_dir_all(dir);
    }
}
