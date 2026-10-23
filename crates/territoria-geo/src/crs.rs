//! Coordinate reference systems and transformations between them.
//!
//! A [`Crs`] is kept as a proj string, which `proj4rs` understands, plus the
//! EPSG code and a name when known. `.prj` files are resolved in three
//! steps: an EPSG authority in the WKT, the CRS name looked up in the EPSG
//! table (ESRI names normalized), and finally the WKT's own parameters.

use std::collections::HashMap;
use std::sync::OnceLock;

use geo_types::Coord;
use serde::{Deserialize, Serialize};

use crate::wkt::{self, Node};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crs {
    pub epsg: Option<u32>,
    pub name: String,
    /// The definition as a proj string.
    pub proj: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrsError {
    UnknownEpsg(u32),
    Wkt(String),
    Unsupported(String),
    Proj(String),
}

impl std::fmt::Display for CrsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrsError::UnknownEpsg(code) => {
                write!(f, "EPSG:{code} is not a known coordinate system")
            }
            CrsError::Wkt(e) => write!(f, "{e}"),
            CrsError::Unsupported(what) => write!(f, "unsupported coordinate system: {what}"),
            CrsError::Proj(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CrsError {}

pub const WGS84_PROJ: &str = "+proj=longlat +datum=WGS84 +no_defs";
pub const WEB_MERCATOR_PROJ: &str = "+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +wktext +no_defs";

impl Crs {
    pub fn wgs84() -> Crs {
        Crs {
            epsg: Some(4326),
            name: "WGS 84".into(),
            proj: WGS84_PROJ.into(),
        }
    }

    pub fn web_mercator() -> Crs {
        Crs {
            epsg: Some(3857),
            name: "WGS 84 / Pseudo-Mercator".into(),
            proj: WEB_MERCATOR_PROJ.into(),
        }
    }

    pub fn from_epsg(code: u32) -> Result<Crs, CrsError> {
        match code {
            4326 => return Ok(Crs::wgs84()),
            3857 | 900913 | 102100 => return Ok(Crs::web_mercator()),
            _ => {}
        }
        let def = u16::try_from(code)
            .ok()
            .and_then(crs_definitions::from_code)
            .ok_or(CrsError::UnknownEpsg(code))?;
        Ok(Crs {
            epsg: Some(code),
            name: wkt_name(def.wkt).unwrap_or_else(|| format!("EPSG:{code}")),
            proj: def.proj4.to_owned(),
        })
    }

    pub fn from_proj(proj: &str) -> Result<Crs, CrsError> {
        proj4rs::Proj::from_proj_string(proj).map_err(|e| CrsError::Proj(e.to_string()))?;
        Ok(Crs {
            epsg: None,
            name: proj.to_owned(),
            proj: proj.to_owned(),
        })
    }

    /// Parses user input: `EPSG:2056`, `2056`, a proj string or WKT.
    pub fn parse(text: &str) -> Result<Crs, CrsError> {
        let t = text.trim();
        let code = t
            .strip_prefix("EPSG:")
            .or_else(|| t.strip_prefix("epsg:"))
            .unwrap_or(t);
        if let Ok(code) = code.trim().parse::<u32>() {
            return Crs::from_epsg(code);
        }
        if t.starts_with('+') {
            return Crs::from_proj(t);
        }
        Crs::from_wkt(t)
    }

    /// Resolves a WKT definition, e.g. the content of a `.prj` file.
    pub fn from_wkt(text: &str) -> Result<Crs, CrsError> {
        let root = wkt::parse(text).map_err(|e| CrsError::Wkt(e.to_string()))?;
        // WKT2 wraps a CRS in BOUNDCRS; the source CRS is what we want.
        let root = match root.child(&["SOURCECRS"]) {
            Some(src) if root.keyword.eq_ignore_ascii_case("BOUNDCRS") => src
                .args
                .iter()
                .find_map(|a| match a {
                    wkt::Arg::Node(n) => Some(n.clone()),
                    _ => None,
                })
                .unwrap_or(root),
            _ => root,
        };
        if let Some(code) = authority_code(&root)
            && let Ok(crs) = Crs::from_epsg(code)
        {
            return Ok(crs);
        }
        let name = root.name().unwrap_or_default().to_owned();
        if let Some(&code) = name_index().get(&normalize_name(&name))
            && let Ok(mut crs) = Crs::from_epsg(code)
        {
            crs.name = name;
            return Ok(crs);
        }
        let proj = proj_from_wkt(&root)?;
        proj4rs::Proj::from_proj_string(&proj)
            .map_err(|e| CrsError::Proj(format!("{e} ({proj})")))?;
        Ok(Crs {
            epsg: None,
            name,
            proj,
        })
    }

    pub fn is_geographic(&self) -> bool {
        self.proj.contains("+proj=longlat") || self.proj.contains("+proj=latlong")
    }

    pub fn is_wgs84(&self) -> bool {
        self.epsg == Some(4326) || self.proj == WGS84_PROJ
    }

    pub fn is_web_mercator(&self) -> bool {
        self.epsg == Some(3857)
    }

    /// `EPSG:2056 · CH1903+ / LV95`, or the name alone.
    pub fn label(&self) -> String {
        match self.epsg {
            Some(code) => format!("EPSG:{code} · {}", self.name),
            None => self.name.clone(),
        }
    }
}

/// Converts coordinates between two coordinate systems. Geographic
/// coordinates are in degrees (x = longitude, y = latitude).
pub struct Transformer {
    kind: TransformKind,
}

enum TransformKind {
    Identity,
    /// WGS84 → Web Mercator, the common case for display.
    ToWebMercator,
    FromWebMercator,
    Proj {
        src: Box<proj4rs::Proj>,
        dst: Box<proj4rs::Proj>,
        src_geo: bool,
        dst_geo: bool,
    },
}

const EARTH_RADIUS: f64 = 6_378_137.0;
/// Web Mercator is cut off where it reaches a square.
const MAX_MERCATOR_LAT: f64 = 85.051_128_779_806_59;

impl Transformer {
    pub fn new(src: &Crs, dst: &Crs) -> Result<Transformer, CrsError> {
        let kind = if src.proj == dst.proj {
            TransformKind::Identity
        } else if src.is_wgs84() && dst.is_web_mercator() {
            TransformKind::ToWebMercator
        } else if src.is_web_mercator() && dst.is_wgs84() {
            TransformKind::FromWebMercator
        } else {
            let parse = |c: &Crs| {
                proj4rs::Proj::from_proj_string(&c.proj).map_err(|e| CrsError::Proj(e.to_string()))
            };
            TransformKind::Proj {
                src: Box::new(parse(src)?),
                dst: Box::new(parse(dst)?),
                src_geo: src.is_geographic(),
                dst_geo: dst.is_geographic(),
            }
        };
        Ok(Transformer { kind })
    }

    pub fn is_identity(&self) -> bool {
        matches!(self.kind, TransformKind::Identity)
    }

    pub fn convert(&self, c: Coord<f64>) -> Option<Coord<f64>> {
        match &self.kind {
            TransformKind::Identity => Some(c),
            TransformKind::ToWebMercator => Some(to_web_mercator(c)),
            TransformKind::FromWebMercator => Some(from_web_mercator(c)),
            TransformKind::Proj {
                src,
                dst,
                src_geo,
                dst_geo,
            } => {
                let mut p = if *src_geo {
                    (c.x.to_radians(), c.y.to_radians(), 0.0)
                } else {
                    (c.x, c.y, 0.0)
                };
                proj4rs::transform::transform(src, dst, &mut p).ok()?;
                let (x, y) = if *dst_geo {
                    (p.0.to_degrees(), p.1.to_degrees())
                } else {
                    (p.0, p.1)
                };
                (x.is_finite() && y.is_finite()).then_some(Coord { x, y })
            }
        }
    }

    /// Converts in place. Coordinates that can't be converted become NaN.
    pub fn convert_all(&self, coords: &mut [Coord<f64>]) {
        if self.is_identity() {
            return;
        }
        for c in coords {
            *c = self.convert(*c).unwrap_or(Coord {
                x: f64::NAN,
                y: f64::NAN,
            });
        }
    }
}

pub fn to_web_mercator(c: Coord<f64>) -> Coord<f64> {
    let lat = c.y.clamp(-MAX_MERCATOR_LAT, MAX_MERCATOR_LAT).to_radians();
    Coord {
        x: EARTH_RADIUS * c.x.to_radians(),
        y: EARTH_RADIUS * (std::f64::consts::FRAC_PI_4 + lat / 2.0).tan().ln(),
    }
}

pub fn from_web_mercator(c: Coord<f64>) -> Coord<f64> {
    Coord {
        x: (c.x / EARTH_RADIUS).to_degrees(),
        y: (2.0 * (c.y / EARTH_RADIUS).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees(),
    }
}

/// True when all coordinates are plausible longitude/latitude values, for
/// files without a `.prj`.
pub fn looks_geographic(bbox: geo_types::Rect<f64>) -> bool {
    let (min, max) = (bbox.min(), bbox.max());
    min.x >= -180.5 && max.x <= 360.5 && min.y >= -90.5 && max.y <= 90.5
}

/// The top-level `AUTHORITY["EPSG","2056"]` (WKT1) or `ID["EPSG",2056]` (WKT2).
fn authority_code(root: &Node) -> Option<u32> {
    let auth = root.child(&["AUTHORITY", "ID"])?;
    let values = auth.values();
    if !values.first()?.eq_ignore_ascii_case("EPSG") {
        return None;
    }
    values.get(1)?.parse::<f64>().ok().map(|v| v as u32)
}

fn wkt_name(text: &str) -> Option<String> {
    let start = text.find('"')? + 1;
    let len = text[start..].find('"')?;
    Some(text[start..start + len].to_owned())
}

/// Lowercase letters and digits only, with ESRI's spellings of common
/// datums mapped to EPSG's (`ETRS_1989_UTM_Zone_32N` and `ETRS89 / UTM zone
/// 32N` both become `etrs89utmzone32n`).
fn normalize_name(name: &str) -> String {
    let mut s: String = name
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    for prefix in ["gcs", "d"] {
        if let Some(rest) = s.strip_prefix(prefix)
            && name.to_lowercase().starts_with(&format!("{prefix}_"))
        {
            s = rest.to_owned();
        }
    }
    const ALIASES: &[(&str, &str)] = &[
        ("etrs1989", "etrs89"),
        ("wgs1984", "wgs84"),
        ("wgs1972", "wgs72"),
        ("nad1983", "nad83"),
        ("nad1927", "nad27"),
        ("gda1994", "gda94"),
        ("rgf1993", "rgf93"),
        ("sirgas2000", "sirgas2000"),
        ("webmercatorauxiliarysphere", "pseudomercator"),
    ];
    for (from, to) in ALIASES {
        if s.contains(from) {
            s = s.replace(from, to);
        }
    }
    s
}

/// Normalized CRS name → EPSG code, for every code in the table.
fn name_index() -> &'static HashMap<String, u32> {
    static INDEX: OnceLock<HashMap<String, u32>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map = HashMap::new();
        for code in 1..=u16::MAX {
            if let Some(def) = crs_definitions::from_code(code)
                && let Some(name) = wkt_name(def.wkt)
            {
                map.entry(normalize_name(&name)).or_insert(code as u32);
            }
        }
        map
    })
}

/// The datum part of a geographic CRS's proj string, looked up by name, e.g.
/// `+ellps=bessel +towgs84=674.374,15.056,405.346,0,0,0,0` for `GCS_CH1903+`.
fn datum_from_name(name: &str) -> Option<String> {
    let code = *name_index().get(&normalize_name(name))?;
    let def = crs_definitions::from_code(u16::try_from(code).ok()?)?;
    if !def.proj4.contains("+proj=longlat") {
        return None;
    }
    let parts: Vec<&str> = def
        .proj4
        .split_whitespace()
        .filter(|p| {
            [
                "+ellps=",
                "+datum=",
                "+towgs84=",
                "+a=",
                "+b=",
                "+rf=",
                "+nadgrids=",
            ]
            .iter()
            .any(|k| p.starts_with(k))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn param_key(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// Builds a proj string from the WKT's own parameters.
fn proj_from_wkt(root: &Node) -> Result<String, CrsError> {
    let kw = root.keyword.to_uppercase();
    let geog = if matches!(
        kw.as_str(),
        "GEOGCS" | "GEOGCRS" | "GEODCRS" | "BASEGEOGCRS"
    ) {
        Some(root)
    } else {
        root.child(&["GEOGCS", "BASEGEOGCRS", "BASEGEODCRS", "GEOGCRS"])
    };
    let datum = geog
        .map(datum_params)
        .unwrap_or_else(|| "+datum=WGS84".into());
    if kw.starts_with("GEOG") || kw == "GEODCRS" {
        return Ok(format!("+proj=longlat {datum} +no_defs"));
    }

    // WKT1: PROJECTION + PARAMETERs on the root; WKT2: CONVERSION[METHOD, PARAMETERs].
    let (method_node, params_node) = match root.child(&["CONVERSION"]) {
        Some(conv) => (conv.child(&["METHOD"]), conv),
        None => (root.child(&["PROJECTION"]), root),
    };
    let method = method_node
        .and_then(Node::name)
        .ok_or_else(|| CrsError::Unsupported("no projection method".into()))?;

    let to_meter = root
        .child(&["UNIT", "LENGTHUNIT"])
        .and_then(|u| u.numbers().first().copied())
        .unwrap_or(1.0);
    let mut params: HashMap<String, f64> = HashMap::new();
    for p in params_node.children(&["PARAMETER"]) {
        if let (Some(name), Some(value)) = (p.name(), p.numbers().first()) {
            params.insert(param_key(name), *value);
        }
    }
    let get = |keys: &[&str]| keys.iter().find_map(|k| params.get(*k).copied());
    let lat_0 = get(&[
        "latitudeoforigin",
        "latitudeofcenter",
        "latitudeofnaturalorigin",
        "latitudeofprojectioncentre",
        "latitudeoffalseorigin",
    ]);
    let lon_0 = get(&[
        "centralmeridian",
        "longitudeofcenter",
        "longitudeoforigin",
        "longitudeofnaturalorigin",
        "longitudeofprojectioncentre",
        "longitudeoffalseorigin",
    ]);
    let k_0 = get(&[
        "scalefactor",
        "scalefactoratnaturalorigin",
        "scalefactoroninitialline",
    ]);
    let x_0 = get(&[
        "falseeasting",
        "eastingatfalseorigin",
        "eastingatprojectioncentre",
    ]);
    let y_0 = get(&[
        "falsenorthing",
        "northingatfalseorigin",
        "northingatprojectioncentre",
    ]);
    let lat_1 = get(&["standardparallel1", "latitudeof1ststandardparallel"]);
    let lat_2 = get(&["standardparallel2", "latitudeof2ndstandardparallel"]);
    let azimuth = get(&["azimuth", "azimuthofinitialline"]);

    let m = param_key(method);
    let proj = match m.as_str() {
        "transversemercator" | "gausskruger" | "transversemercatorsouthorientated" => "tmerc",
        "lambertconformalconic"
        | "lambertconformalconic2sp"
        | "lambertconformalconic1sp"
        | "lambertconicconformal2sp"
        | "lambertconicconformal1sp" => "lcc",
        "mercator" | "mercator1sp" | "mercator2sp" | "mercatorvarianta" | "mercatorvariantb" => {
            "merc"
        }
        "mercatorauxiliarysphere" | "popularvisualisationpseudomercator" => {
            return Ok(WEB_MERCATOR_PROJ.into());
        }
        "hotineobliquemercatorazimuthcenter"
        | "hotineobliquemercatorvariantb"
        | "obliquemercator"
        | "hotineobliquemercator"
        | "swissobliquemercator"
        | "swissobliquecylindrical" => {
            // The Swiss variant is the one proj4rs implements (`somerc`).
            if azimuth.is_none_or(|a| (a - 90.0).abs() < 1e-6) {
                "somerc"
            } else {
                return Err(CrsError::Unsupported(method.into()));
            }
        }
        "obliquestereographic" | "doublestereographic" => "sterea",
        "polarstereographic" | "stereographic" | "polarstereographicvarianta" => "stere",
        "lambertazimuthalequalarea" => "laea",
        "albers" | "albersconicequalarea" | "albersequalarea" => "aea",
        "equirectangular" | "platecarree" | "equidistantcylindrical" => "eqc",
        "cylindricalequalarea" | "lambertcylindricalequalarea" => "cea",
        "krovak" => "krovak",
        "azimuthalequidistant" => "aeqd",
        "mollweide" => "moll",
        "millercylindrical" => "mill",
        _ => return Err(CrsError::Unsupported(method.into())),
    };
    let mut s = format!("+proj={proj}");
    let mut push = |key: &str, v: Option<f64>| {
        if let Some(v) = v {
            s.push_str(&format!(" +{key}={v}"));
        }
    };
    push("lat_0", lat_0);
    push("lon_0", lon_0);
    if proj == "lcc" && lat_1.is_none() {
        push("lat_1", lat_0);
    } else {
        push("lat_1", lat_1);
        push("lat_2", lat_2);
    }
    push("k_0", k_0);
    // False easting/northing are given in the CRS's linear unit.
    push("x_0", x_0.map(|v| v * to_meter));
    push("y_0", y_0.map(|v| v * to_meter));
    s.push(' ');
    s.push_str(&datum);
    if (to_meter - 1.0).abs() > 1e-12 {
        s.push_str(&format!(" +to_meter={to_meter}"));
    } else {
        s.push_str(" +units=m");
    }
    s.push_str(" +no_defs");
    Ok(s)
}

/// Ellipsoid and datum shift of a geographic CRS node.
fn datum_params(geog: &Node) -> String {
    let datum = geog.child(&["DATUM", "ENSEMBLE"]);
    // A known datum (by the GEOGCS's or the DATUM's name) brings its shift.
    for name in [geog.name(), datum.and_then(Node::name)]
        .into_iter()
        .flatten()
    {
        if let Some(found) = datum_from_name(name) {
            return found;
        }
    }
    let Some(datum) = datum else {
        return "+datum=WGS84".into();
    };
    let mut s = String::new();
    if let Some(sph) = datum.child(&["SPHEROID", "ELLIPSOID"]) {
        let n = sph.numbers();
        if let (Some(a), Some(rf)) = (n.first(), n.get(1)) {
            if *rf == 0.0 {
                s.push_str(&format!("+a={a} +b={a}"));
            } else {
                s.push_str(&format!("+a={a} +rf={rf}"));
            }
        }
    }
    if let Some(shift) = datum.child(&["TOWGS84"]) {
        let n: Vec<String> = shift.numbers().iter().map(|v| v.to_string()).collect();
        s.push_str(&format!(" +towgs84={}", n.join(",")));
    }
    if s.is_empty() {
        "+datum=WGS84".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LV95_ESRI: &str = r#"PROJCS["CH1903+_LV95",GEOGCS["GCS_CH1903+",DATUM["D_CH1903+",SPHEROID["Bessel_1841",6377397.155,299.1528128]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Hotine_Oblique_Mercator_Azimuth_Center"],PARAMETER["False_Easting",2600000.0],PARAMETER["False_Northing",1200000.0],PARAMETER["Scale_Factor",1.0],PARAMETER["Azimuth",90.0],PARAMETER["Longitude_Of_Center",7.439583333333333],PARAMETER["Latitude_Of_Center",46.95240555555556],UNIT["Meter",1.0]],VERTCS["LN_1902",VDATUM["Landesnivellement_1902"],PARAMETER["Vertical_Shift",0.0],PARAMETER["Direction",1.0],UNIT["Meter",1.0]]"#;

    fn close(a: Coord<f64>, b: Coord<f64>, tol: f64) -> bool {
        (a.x - b.x).abs() < tol && (a.y - b.y).abs() < tol
    }

    #[test]
    fn esri_prj_resolves_by_name() {
        let crs = Crs::from_wkt(LV95_ESRI).unwrap();
        assert_eq!(crs.epsg, Some(2056));
        let t = Transformer::new(&crs, &Crs::wgs84()).unwrap();
        // Reference values from swisstopo's REFRAME service.
        let p = t
            .convert(Coord {
                x: 2_600_670.0,
                y: 1_199_655.0,
            })
            .unwrap();
        assert!(
            close(
                p,
                Coord {
                    x: 7.447433,
                    y: 46.947979
                },
                1e-5
            ),
            "{p:?}"
        );
        let p = t
            .convert(Coord {
                x: 2_700_000.0,
                y: 1_250_000.0,
            })
            .unwrap();
        assert!(
            close(
                p,
                Coord {
                    x: 8.763208,
                    y: 47.393232
                },
                1e-5
            ),
            "{p:?}"
        );
    }

    #[test]
    fn esri_prj_resolves_by_parameters() {
        // Same definition under a name the EPSG table doesn't know.
        let renamed = LV95_ESRI.replace("CH1903+_LV95", "My_Swiss_Grid");
        let crs = Crs::from_wkt(&renamed).unwrap();
        assert_eq!(crs.epsg, None);
        assert!(crs.proj.contains("+proj=somerc"), "{}", crs.proj);
        assert!(crs.proj.contains("+towgs84=674.374"), "{}", crs.proj);
        let t = Transformer::new(&crs, &Crs::wgs84()).unwrap();
        let p = t
            .convert(Coord {
                x: 2_600_670.0,
                y: 1_199_655.0,
            })
            .unwrap();
        assert!(
            close(
                p,
                Coord {
                    x: 7.447433,
                    y: 46.947979
                },
                1e-5
            ),
            "{p:?}"
        );
    }

    #[test]
    fn utm_with_esri_datum_names() {
        let wkt = r#"PROJCS["ETRS_1989_UTM_Zone_32N",GEOGCS["GCS_ETRS_1989",DATUM["D_ETRS_1989",SPHEROID["GRS_1980",6378137.0,298.257222101]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["False_Easting",500000.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",9.0],PARAMETER["Scale_Factor",0.9996],PARAMETER["Latitude_Of_Origin",0.0],UNIT["Meter",1.0]]"#;
        assert_eq!(Crs::from_wkt(wkt).unwrap().epsg, Some(25832));
    }

    #[test]
    fn authority_wins() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433],AUTHORITY["EPSG","4326"]]"#;
        assert!(Crs::from_wkt(wkt).unwrap().is_wgs84());
    }

    #[test]
    fn parses_user_input() {
        assert_eq!(Crs::parse("EPSG:2154").unwrap().epsg, Some(2154));
        assert_eq!(Crs::parse(" 27700 ").unwrap().epsg, Some(27700));
        assert!(
            Crs::parse("+proj=longlat +datum=WGS84")
                .unwrap()
                .is_geographic()
        );
        assert!(Crs::parse("EPSG:999999").is_err());
    }

    #[test]
    fn web_mercator_round_trip() {
        let c = Coord { x: 10.0, y: 45.0 };
        let m = to_web_mercator(c);
        assert!(
            close(
                m,
                Coord {
                    x: 1_113_194.908,
                    y: 5_621_521.486
                },
                0.01
            ),
            "{m:?}"
        );
        assert!(close(from_web_mercator(m), c, 1e-9));
        let t = Transformer::new(&Crs::wgs84(), &Crs::web_mercator()).unwrap();
        assert!(close(t.convert(c).unwrap(), m, 1e-9));
    }

    #[test]
    fn british_national_grid_from_epsg() {
        let t = Transformer::new(&Crs::from_epsg(27700).unwrap(), &Crs::wgs84()).unwrap();
        // Greenwich observatory, roughly.
        let p = t
            .convert(Coord {
                x: 538_890.0,
                y: 177_320.0,
            })
            .unwrap();
        assert!(close(p, Coord { x: 0.0, y: 51.4778 }, 2e-3), "{p:?}");
    }
}
