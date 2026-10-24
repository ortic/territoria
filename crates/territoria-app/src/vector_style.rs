//! Vector background maps: a MapLibre style (fills, lines, circles and
//! the background; no labels yet) drawn into tile images.

use std::collections::HashMap;
use std::sync::Arc;

use egui::ColorImage;
use resvg::tiny_skia::{
    self, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform,
};
use serde_json::Value;
use territoria_io::vectortiles::{FeatureKind, TileArchive, TileFeature, VectorTile};

/// Pixels of a drawn tile, for 256 points on screen: sharp on fine screens.
pub const TILE_PX: u32 = 512;

/// A light map for the OpenMapTiles and Protomaps schemas, when no style
/// is given.
const BUILT_IN_STYLE: &str = r##"{
  "version": 8,
  "layers": [
    {"id": "background", "type": "background", "paint": {"background-color": "#f4f3ef"}},
    {"id": "earth", "type": "fill", "source-layer": "earth", "paint": {"fill-color": "#f4f3ef"}},
    {"id": "landcover", "type": "fill", "source-layer": "landcover", "paint": {"fill-color": "#e6eddc", "fill-opacity": 0.7}},
    {"id": "park", "type": "fill", "source-layer": "park", "paint": {"fill-color": "#e1ead2"}},
    {"id": "landuse", "type": "fill", "source-layer": "landuse",
     "filter": ["match", ["get", "class"], ["residential", "suburb", "neighbourhood"], false, true],
     "paint": {"fill-color": "#ebeae4", "fill-opacity": 0.8}},
    {"id": "water", "type": "fill", "source-layer": "water", "paint": {"fill-color": "#c9dbe8"}},
    {"id": "waterway", "type": "line", "source-layer": "waterway", "minzoom": 8,
     "paint": {"line-color": "#c9dbe8", "line-width": ["interpolate", ["linear"], ["zoom"], 8, 0.5, 14, 2]}},
    {"id": "building", "type": "fill", "source-layer": "building", "minzoom": 13, "paint": {"fill-color": "#e2dfd8"}},
    {"id": "buildings", "type": "fill", "source-layer": "buildings", "minzoom": 13, "paint": {"fill-color": "#e2dfd8"}},
    {"id": "roads-minor", "type": "line", "source-layer": "transportation", "minzoom": 11,
     "filter": ["match", ["get", "class"], ["minor", "service", "track", "path"], true, false],
     "layout": {"line-cap": "round", "line-join": "round"},
     "paint": {"line-color": "#ffffff", "line-width": ["interpolate", ["linear"], ["zoom"], 11, 0.4, 16, 4]}},
    {"id": "roads-major", "type": "line", "source-layer": "transportation",
     "filter": ["match", ["get", "class"], ["motorway", "trunk", "primary", "secondary", "tertiary"], true, false],
     "layout": {"line-cap": "round", "line-join": "round"},
     "paint": {"line-color": "#ffffff", "line-width": ["interpolate", ["linear"], ["zoom"], 5, 0.5, 10, 1.5, 16, 8]}},
    {"id": "roads", "type": "line", "source-layer": "roads",
     "layout": {"line-cap": "round", "line-join": "round"},
     "paint": {"line-color": "#ffffff", "line-width": ["interpolate", ["linear"], ["zoom"], 5, 0.5, 10, 1.2, 16, 6]}},
    {"id": "rail", "type": "line", "source-layer": "transportation", "minzoom": 10,
     "filter": ["==", ["get", "class"], "rail"],
     "paint": {"line-color": "#d4d2cc", "line-width": 1, "line-dasharray": [3, 2]}},
    {"id": "boundary", "type": "line", "source-layer": "boundary",
     "filter": ["<=", ["get", "admin_level"], 4],
     "paint": {"line-color": "#b9b6c4", "line-width": ["interpolate", ["linear"], ["zoom"], 3, 0.6, 10, 1.4], "line-dasharray": [4, 2]}},
    {"id": "boundaries", "type": "line", "source-layer": "boundaries",
     "filter": ["<=", ["get", "kind_detail"], 4],
     "paint": {"line-color": "#b9b6c4", "line-width": ["interpolate", ["linear"], ["zoom"], 3, 0.6, 10, 1.4], "line-dasharray": [4, 2]}}
  ]
}"##;

/// An RGBA colour, components 0–1, not premultiplied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba(pub [f32; 4]);

impl Rgba {
    fn with_opacity(self, o: f32) -> Rgba {
        let [r, g, b, a] = self.0;
        Rgba([r, g, b, a * o.clamp(0.0, 1.0)])
    }

    fn lerp(self, other: Rgba, t: f32) -> Rgba {
        let mut c = [0.0; 4];
        for (i, v) in c.iter_mut().enumerate() {
            *v = self.0[i] + (other.0[i] - self.0[i]) * t;
        }
        Rgba(c)
    }

    fn paint(self) -> Paint<'static> {
        let [r, g, b, a] = self.0;
        let mut p = Paint::default();
        p.set_color(
            tiny_skia::Color::from_rgba(
                r.clamp(0.0, 1.0),
                g.clamp(0.0, 1.0),
                b.clamp(0.0, 1.0),
                a.clamp(0.0, 1.0),
            )
            .unwrap_or(tiny_skia::Color::BLACK),
        );
        p.anti_alias = true;
        p
    }
}

/// A CSS colour: `#rgb`, `#rrggbb(aa)`, `rgb()`, `rgba()`, `hsl()`,
/// `hsla()` or a few names.
pub fn parse_color(s: &str) -> Option<Rgba> {
    let s = s.trim().to_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<_>>()?;
        let byte = |hi: u8, lo: u8| (hi * 16 + lo) as f32 / 255.0;
        return match digits.len() {
            3 | 4 => {
                let a = if digits.len() == 4 {
                    byte(digits[3], digits[3])
                } else {
                    1.0
                };
                Some(Rgba([
                    byte(digits[0], digits[0]),
                    byte(digits[1], digits[1]),
                    byte(digits[2], digits[2]),
                    a,
                ]))
            }
            6 | 8 => {
                let a = if digits.len() == 8 {
                    byte(digits[6], digits[7])
                } else {
                    1.0
                };
                Some(Rgba([
                    byte(digits[0], digits[1]),
                    byte(digits[2], digits[3]),
                    byte(digits[4], digits[5]),
                    a,
                ]))
            }
            _ => None,
        };
    }
    if let Some((name, rest)) = s.split_once('(') {
        let args: Vec<f32> = rest
            .trim_end_matches(')')
            .split([',', ' ', '/'])
            .filter(|a| !a.is_empty())
            .map(|a| match a.strip_suffix('%') {
                Some(p) => p.parse::<f32>().map(|v| v / 100.0),
                None => a.parse::<f32>(),
            })
            .collect::<Result<_, _>>()
            .ok()?;
        let alpha = args.get(3).copied().unwrap_or(1.0);
        return match name.trim() {
            "rgb" | "rgba" if args.len() >= 3 => Some(Rgba([
                args[0] / 255.0,
                args[1] / 255.0,
                args[2] / 255.0,
                alpha,
            ])),
            "hsl" | "hsla" if args.len() >= 3 => {
                let (h, sat, l) = (args[0].rem_euclid(360.0) / 360.0, args[1], args[2]);
                let q = if l < 0.5 {
                    l * (1.0 + sat)
                } else {
                    l + sat - l * sat
                };
                let p = 2.0 * l - q;
                let hue = |mut t: f32| {
                    t = t.rem_euclid(1.0);
                    if t < 1.0 / 6.0 {
                        p + (q - p) * 6.0 * t
                    } else if t < 0.5 {
                        q
                    } else if t < 2.0 / 3.0 {
                        p + (q - p) * (2.0 / 3.0 - t) * 6.0
                    } else {
                        p
                    }
                };
                Some(Rgba([
                    hue(h + 1.0 / 3.0),
                    hue(h),
                    hue(h - 1.0 / 3.0),
                    alpha,
                ]))
            }
            _ => None,
        };
    }
    let rgb = |r: u8, g: u8, b: u8| {
        Some(Rgba([
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            1.0,
        ]))
    };
    match s.as_str() {
        "transparent" => Some(Rgba([0.0; 4])),
        "white" => rgb(255, 255, 255),
        "black" => rgb(0, 0, 0),
        "gray" | "grey" => rgb(128, 128, 128),
        "red" => rgb(255, 0, 0),
        "green" => rgb(0, 128, 0),
        "blue" => rgb(0, 0, 255),
        "yellow" => rgb(255, 255, 0),
        "orange" => rgb(255, 165, 0),
        _ => None,
    }
}

/// What an expression is evaluated for.
struct Ctx<'a> {
    zoom: f32,
    feature: Option<&'a TileFeature>,
}

impl Ctx<'_> {
    fn get(&self, key: &str) -> Value {
        match key {
            "$type" => Value::String(self.geometry_type().into()),
            _ => self
                .feature
                .and_then(|f| f.properties.get(key).cloned())
                .unwrap_or(Value::Null),
        }
    }

    fn geometry_type(&self) -> &'static str {
        match self.feature.map(|f| f.kind) {
            Some(FeatureKind::Point) => "Point",
            Some(FeatureKind::Line) => "LineString",
            _ => "Polygon",
        }
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(*b as u8 as f64),
        _ => None,
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        _ => true,
    }
}

fn json(n: f64) -> Value {
    serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
}

/// Values compare as numbers when both are, else as text.
fn compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Number(_), Value::Number(_)) => num(a)?.partial_cmp(&num(b)?),
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

fn equal(a: &Value, b: &Value) -> bool {
    match (num(a), num(b)) {
        (Some(x), Some(y)) if a.is_number() && b.is_number() => x == y,
        _ => a == b,
    }
}

/// Interpolates between two outputs: numbers, colours or arrays of numbers.
fn lerp(a: &Value, b: &Value, t: f64) -> Value {
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        return json(x + (y - x) * t);
    }
    if let (Some(x), Some(y)) = (
        a.as_str().and_then(parse_color),
        b.as_str().and_then(parse_color),
    ) {
        let c = x.lerp(y, t as f32).0;
        return Value::String(format!(
            "rgba({}, {}, {}, {})",
            c[0] * 255.0,
            c[1] * 255.0,
            c[2] * 255.0,
            c[3]
        ));
    }
    if let (Some(x), Some(y)) = (a.as_array(), b.as_array())
        && x.len() == y.len()
    {
        return Value::Array(x.iter().zip(y).map(|(x, y)| lerp(x, y, t)).collect());
    }
    if t < 0.5 { a.clone() } else { b.clone() }
}

/// Where `x` lies between stops, with exponential easing for `base` ≠ 1.
fn stop_t(x: f64, lo: f64, hi: f64, base: f64) -> f64 {
    let span = hi - lo;
    if span <= 0.0 {
        return 0.0;
    }
    let p = x - lo;
    if (base - 1.0).abs() < 1e-9 {
        p / span
    } else {
        (base.powf(p) - 1.0) / (base.powf(span) - 1.0)
    }
}

/// Picks or interpolates among `(input, output)` stops.
fn by_stops(x: f64, stops: &[(f64, Value)], base: f64, step: bool) -> Value {
    let Some(first) = stops.first() else {
        return Value::Null;
    };
    if x <= first.0 {
        return first.1.clone();
    }
    for w in stops.windows(2) {
        let ((lo, a), (hi, b)) = (&w[0], &w[1]);
        if x < *hi {
            return if step {
                a.clone()
            } else {
                lerp(a, b, stop_t(x, *lo, *hi, base))
            };
        }
    }
    stops.last().map_or(Value::Null, |s| s.1.clone())
}

/// Evaluates a style value: a literal, a legacy function (`stops`), or
/// an expression.
fn eval(v: &Value, cx: &Ctx) -> Value {
    match v {
        Value::Object(o) if o.contains_key("stops") => {
            let base = o.get("base").and_then(Value::as_f64).unwrap_or(1.0);
            let input = match o.get("property").and_then(Value::as_str) {
                Some(p) => cx.get(p),
                None => json(cx.zoom as f64),
            };
            let stops: Vec<(f64, Value)> = o["stops"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    let s = s.as_array()?;
                    // Zoom-and-property stops keep their value part.
                    let key = s.first()?;
                    let k = key
                        .as_f64()
                        .or_else(|| key.get("zoom").and_then(Value::as_f64))?;
                    Some((k, s.get(1)?.clone()))
                })
                .collect();
            let categorical = o.get("type").and_then(Value::as_str) == Some("categorical");
            if categorical {
                return stops
                    .iter()
                    .find(|(k, _)| num(&input) == Some(*k))
                    .map_or_else(
                        || o.get("default").cloned().unwrap_or(Value::Null),
                        |s| s.1.clone(),
                    );
            }
            let step = o.get("type").and_then(Value::as_str) == Some("interval");
            match num(&input) {
                Some(x) => by_stops(x, &stops, base, step),
                None => o.get("default").cloned().unwrap_or(Value::Null),
            }
        }
        Value::Array(a) => match a.first().and_then(Value::as_str) {
            Some(op) => expression(op, &a[1..], cx),
            None => v.clone(),
        },
        _ => v.clone(),
    }
}

fn expression(op: &str, args: &[Value], cx: &Ctx) -> Value {
    let arg = |i: usize| args.get(i).map_or(Value::Null, |a| eval(a, cx));
    match op {
        "literal" => args.first().cloned().unwrap_or(Value::Null),
        "get" => match arg(0) {
            Value::String(k) => cx.get(&k),
            _ => Value::Null,
        },
        "has" => Value::Bool(matches!(arg(0), Value::String(k) if !cx.get(&k).is_null())),
        "!has" => Value::Bool(!matches!(arg(0), Value::String(k) if !cx.get(&k).is_null())),
        "geometry-type" => Value::String(cx.geometry_type().into()),
        "zoom" => json(cx.zoom as f64),
        "id" => Value::Null,
        "!" => Value::Bool(!truthy(&arg(0))),
        "all" => Value::Bool(args.iter().all(|a| truthy(&eval(a, cx)))),
        "any" => Value::Bool(args.iter().any(|a| truthy(&eval(a, cx)))),
        "none" => Value::Bool(!args.iter().any(|a| truthy(&eval(a, cx)))),
        "==" => Value::Bool(equal(&arg(0), &arg(1))),
        "!=" => Value::Bool(!equal(&arg(0), &arg(1))),
        "<" | "<=" | ">" | ">=" => {
            use std::cmp::Ordering::*;
            Value::Bool(match compare(&arg(0), &arg(1)) {
                Some(o) => match op {
                    "<" => o == Less,
                    "<=" => o != Greater,
                    ">" => o == Greater,
                    _ => o != Less,
                },
                None => false,
            })
        }
        "in" => {
            let needle = arg(0);
            match arg(1) {
                Value::Array(h) => Value::Bool(h.iter().any(|x| equal(x, &needle))),
                Value::String(h) => Value::Bool(needle.as_str().is_some_and(|n| h.contains(n))),
                _ => Value::Bool(false),
            }
        }
        "match" => {
            let input = arg(0);
            let rest = &args[1.min(args.len())..];
            let (pairs, default) = rest.as_chunks::<2>();
            for [label, output] in pairs {
                let hit = match label {
                    Value::Array(labels) => labels.iter().any(|l| equal(l, &input)),
                    l => equal(l, &input),
                };
                if hit {
                    return eval(output, cx);
                }
            }
            default.first().map_or(Value::Null, |d| eval(d, cx))
        }
        "case" => {
            let (pairs, default) = args.as_chunks::<2>();
            for [condition, output] in pairs {
                if truthy(&eval(condition, cx)) {
                    return eval(output, cx);
                }
            }
            default.first().map_or(Value::Null, |d| eval(d, cx))
        }
        "coalesce" => args
            .iter()
            .map(|a| eval(a, cx))
            .find(|v| !v.is_null())
            .unwrap_or(Value::Null),
        "interpolate" | "interpolate-hcl" | "interpolate-lab" => {
            let base = match args.first() {
                Some(Value::Array(t))
                    if t.first().and_then(Value::as_str) == Some("exponential") =>
                {
                    t.get(1).and_then(Value::as_f64).unwrap_or(1.0)
                }
                _ => 1.0,
            };
            let Some(x) = num(&arg(1)) else {
                return Value::Null;
            };
            let stops: Vec<(f64, Value)> = args
                .get(2..)
                .unwrap_or_default()
                .as_chunks::<2>()
                .0
                .iter()
                .filter_map(|p| Some((p[0].as_f64()?, eval(&p[1], cx))))
                .collect();
            by_stops(x, &stops, base, false)
        }
        "step" => {
            let Some(x) = num(&arg(0)) else {
                return Value::Null;
            };
            let mut stops = vec![(f64::NEG_INFINITY, arg(1))];
            stops.extend(
                args.get(2..)
                    .unwrap_or_default()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .filter_map(|p| Some((p[0].as_f64()?, eval(&p[1], cx)))),
            );
            by_stops(x, &stops, 1.0, true)
        }
        "to-number" | "number" => args
            .iter()
            .map(|a| eval(a, cx))
            .find_map(|v| num(&v))
            .map_or(json(0.0), json),
        "to-string" | "string" => match arg(0) {
            Value::String(s) => Value::String(s),
            Value::Null => Value::String(String::new()),
            v => Value::String(v.to_string()),
        },
        "to-boolean" | "boolean" => Value::Bool(truthy(&arg(0))),
        "to-color" | "color" => arg(0),
        "rgb" | "rgba" => {
            let c: Vec<f64> = (0..args.len()).filter_map(|i| num(&arg(i))).collect();
            match c.as_slice() {
                [r, g, b] => Value::String(format!("rgb({r}, {g}, {b})")),
                [r, g, b, a, ..] => Value::String(format!("rgba({r}, {g}, {b}, {a})")),
                _ => Value::Null,
            }
        }
        "concat" => Value::String(
            (0..args.len())
                .map(|i| match arg(i) {
                    Value::String(s) => s,
                    Value::Null => String::new(),
                    v => v.to_string(),
                })
                .collect(),
        ),
        "downcase" => Value::String(arg(0).as_str().unwrap_or_default().to_lowercase()),
        "upcase" => Value::String(arg(0).as_str().unwrap_or_default().to_uppercase()),
        "+" | "*" => {
            let vals: Vec<f64> = (0..args.len()).filter_map(|i| num(&arg(i))).collect();
            json(if op == "+" {
                vals.iter().sum()
            } else {
                vals.iter().product()
            })
        }
        "-" | "/" => match (num(&arg(0)), num(&arg(1))) {
            (Some(a), Some(b)) => json(if op == "-" {
                a - b
            } else if b != 0.0 {
                a / b
            } else {
                0.0
            }),
            (Some(a), None) if op == "-" => json(-a),
            _ => Value::Null,
        },
        _ => Value::Null,
    }
}

/// Whether a filter keeps a feature: an expression, or the older form
/// with keys as plain strings (`["==", "class", "lake"]`).
fn filter_keeps(f: &Value, cx: &Ctx) -> bool {
    let Some(a) = f.as_array() else {
        return truthy(f);
    };
    let Some(op) = a.first().and_then(Value::as_str) else {
        return true;
    };
    let legacy_key = a.get(1).and_then(Value::as_str);
    match (op, legacy_key) {
        ("all", _) => a[1..].iter().all(|x| filter_keeps(x, cx)),
        ("any", _) => a[1..].iter().any(|x| filter_keeps(x, cx)),
        ("none", _) => !a[1..].iter().any(|x| filter_keeps(x, cx)),
        ("has", Some(k)) if a.len() == 2 => !cx.get(k).is_null(),
        ("!has", Some(k)) if a.len() == 2 => cx.get(k).is_null(),
        ("in", Some(k)) if a.len() != 3 || !a[2].is_array() => {
            let v = cx.get(k);
            a[2..].iter().any(|x| equal(x, &v))
        }
        ("!in", Some(k)) => {
            let v = cx.get(k);
            !a[2..].iter().any(|x| equal(x, &v))
        }
        ("==" | "!=" | "<" | "<=" | ">" | ">=", Some(k)) if a.len() == 3 && !a[2].is_array() => {
            let v = cx.get(k);
            let lit = &a[2];
            match op {
                "==" => equal(&v, lit),
                "!=" => !equal(&v, lit),
                _ => expression(op, &[v, lit.clone()], cx)
                    .as_bool()
                    .unwrap_or(false),
            }
        }
        _ => truthy(&eval(f, cx)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LayerKind {
    Background,
    Fill,
    Line,
    Circle,
}

#[derive(Debug, Clone)]
struct StyleLayer {
    kind: LayerKind,
    source_layer: String,
    min_zoom: f32,
    max_zoom: f32,
    filter: Option<Value>,
    paint: serde_json::Map<String, Value>,
    layout: serde_json::Map<String, Value>,
}

/// A MapLibre style: what is drawn, in order.
#[derive(Debug, Clone)]
pub struct Style {
    layers: Vec<StyleLayer>,
    /// The first vector source: its tile addresses or TileJSON address.
    pub source_tiles: Option<String>,
    pub source_url: Option<String>,
    pub source_max_zoom: Option<u8>,
}

impl Style {
    pub fn built_in() -> Style {
        Style::parse(BUILT_IN_STYLE).expect("the built-in style")
    }

    pub fn parse(text: &str) -> Result<Style, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("style: {e}"))?;
        let layers = v
            .get("layers")
            .and_then(Value::as_array)
            .ok_or("style: no layers")?
            .iter()
            .filter_map(|l| {
                let kind = match l.get("type")?.as_str()? {
                    "background" => LayerKind::Background,
                    "fill" | "fill-extrusion" => LayerKind::Fill,
                    "line" => LayerKind::Line,
                    "circle" => LayerKind::Circle,
                    _ => return None,
                };
                let layout = l
                    .get("layout")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                if layout.get("visibility").and_then(Value::as_str) == Some("none") {
                    return None;
                }
                let mut paint = l
                    .get("paint")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                // Extrusions drawn flat, in their colour.
                for (from, to) in [
                    ("fill-extrusion-color", "fill-color"),
                    ("fill-extrusion-opacity", "fill-opacity"),
                ] {
                    if let Some(c) = paint.get(from).cloned() {
                        paint.entry(to).or_insert(c);
                    }
                }
                // Patterns need sprites: those fills are left out.
                if paint.contains_key("fill-pattern") && !paint.contains_key("fill-color") {
                    return None;
                }
                Some(StyleLayer {
                    kind,
                    source_layer: l
                        .get("source-layer")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    min_zoom: l.get("minzoom").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                    max_zoom: l.get("maxzoom").and_then(Value::as_f64).unwrap_or(30.0) as f32,
                    filter: l.get("filter").cloned(),
                    paint,
                    layout,
                })
            })
            .collect();
        let source = v.get("sources").and_then(Value::as_object).and_then(|s| {
            s.values()
                .find(|s| s.get("type").and_then(Value::as_str) == Some("vector"))
        });
        Ok(Style {
            layers,
            source_tiles: source
                .and_then(|s| s.get("tiles")?.as_array()?.first()?.as_str())
                .map(str::to_owned),
            source_url: source
                .and_then(|s| s.get("url")?.as_str())
                .map(str::to_owned),
            source_max_zoom: source
                .and_then(|s| s.get("maxzoom")?.as_u64())
                .map(|z| z.min(30) as u8),
        })
    }

    fn color(&self, layer: &StyleLayer, key: &str, default: &str, cx: &Ctx) -> Option<Rgba> {
        match layer.paint.get(key).map(|v| eval(v, cx)) {
            Some(Value::String(s)) => parse_color(&s),
            Some(_) => None,
            None => parse_color(default),
        }
    }

    fn number(&self, layer: &StyleLayer, key: &str, default: f64, cx: &Ctx) -> f64 {
        layer
            .paint
            .get(key)
            .map(|v| eval(v, cx))
            .and_then(|v| num(&v))
            .unwrap_or(default)
    }

    /// Draws `tile` for a view at style zoom `zoom`. `part` is the square
    /// of the tile shown (x, y, size in tile units) when a coarser tile is
    /// drawn larger.
    pub fn render(&self, tile: &VectorTile, zoom: f32, part: [f32; 3]) -> ColorImage {
        let mut pixmap = Pixmap::new(TILE_PX, TILE_PX).expect("a tile pixmap");
        // Line widths are in pixels of 512-pixel tiles.
        let px = TILE_PX as f32 / 512.0;
        for layer in &self.layers {
            if zoom < layer.min_zoom || zoom >= layer.max_zoom {
                continue;
            }
            let base = Ctx {
                zoom,
                feature: None,
            };
            if layer.kind == LayerKind::Background {
                let o = self.number(layer, "background-opacity", 1.0, &base) as f32;
                if let Some(c) = self.color(layer, "background-color", "#000000", &base) {
                    let c = c.with_opacity(o).0;
                    if let Some(c) = tiny_skia::Color::from_rgba(c[0], c[1], c[2], c[3]) {
                        pixmap.fill(c);
                    }
                }
                continue;
            }
            let Some(source) = tile.layer(&layer.source_layer) else {
                continue;
            };
            let k = TILE_PX as f32 / (source.extent as f32 * part[2]);
            let (ox, oy) = (
                part[0] * source.extent as f32,
                part[1] * source.extent as f32,
            );
            let to_px = |p: &[f32; 2]| ((p[0] - ox) * k, (p[1] - oy) * k);
            let cap = match layer.layout.get("line-cap").and_then(Value::as_str) {
                Some("round") => LineCap::Round,
                Some("square") => LineCap::Square,
                _ => LineCap::Butt,
            };
            let join = match layer.layout.get("line-join").and_then(Value::as_str) {
                Some("round") => LineJoin::Round,
                Some("bevel") => LineJoin::Bevel,
                _ => LineJoin::Miter,
            };
            for f in &source.features {
                let cx = Ctx {
                    zoom,
                    feature: Some(f),
                };
                if layer
                    .filter
                    .as_ref()
                    .is_some_and(|flt| !filter_keeps(flt, &cx))
                {
                    continue;
                }
                match layer.kind {
                    LayerKind::Fill if f.kind == FeatureKind::Polygon => {
                        let o = self.number(layer, "fill-opacity", 1.0, &cx) as f32;
                        let Some(c) = self.color(layer, "fill-color", "#000000", &cx) else {
                            continue;
                        };
                        let mut pb = PathBuilder::new();
                        for ring in &f.parts {
                            for (i, p) in ring.iter().enumerate() {
                                let (x, y) = to_px(p);
                                if i == 0 {
                                    pb.move_to(x, y)
                                } else {
                                    pb.line_to(x, y)
                                }
                            }
                            pb.close();
                        }
                        if let Some(path) = pb.finish() {
                            pixmap.fill_path(
                                &path,
                                &c.with_opacity(o).paint(),
                                FillRule::EvenOdd,
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                    LayerKind::Line if f.kind != FeatureKind::Point => {
                        let o = self.number(layer, "line-opacity", 1.0, &cx) as f32;
                        let w = self.number(layer, "line-width", 1.0, &cx) as f32 * px;
                        let Some(c) = self.color(layer, "line-color", "#000000", &cx) else {
                            continue;
                        };
                        if w <= 0.0 {
                            continue;
                        }
                        let dash = layer
                            .paint
                            .get("line-dasharray")
                            .map(|v| eval(v, &cx))
                            .and_then(|v| {
                                v.as_array().map(|a| {
                                    a.iter()
                                        .filter_map(Value::as_f64)
                                        .map(|d| (d as f32 * w).max(0.5))
                                        .collect::<Vec<_>>()
                                })
                            })
                            .filter(|d| d.len() >= 2 && d.len() % 2 == 0)
                            .and_then(|d| tiny_skia::StrokeDash::new(d, 0.0));
                        let stroke = Stroke {
                            width: w,
                            line_cap: cap,
                            line_join: join,
                            dash,
                            ..Default::default()
                        };
                        let mut pb = PathBuilder::new();
                        for line in &f.parts {
                            for (i, p) in line.iter().enumerate() {
                                let (x, y) = to_px(p);
                                if i == 0 {
                                    pb.move_to(x, y)
                                } else {
                                    pb.line_to(x, y)
                                }
                            }
                        }
                        if let Some(path) = pb.finish() {
                            pixmap.stroke_path(
                                &path,
                                &c.with_opacity(o).paint(),
                                &stroke,
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                    LayerKind::Circle if f.kind == FeatureKind::Point => {
                        let o = self.number(layer, "circle-opacity", 1.0, &cx) as f32;
                        let r = self.number(layer, "circle-radius", 5.0, &cx) as f32 * px;
                        let Some(c) = self.color(layer, "circle-color", "#000000", &cx) else {
                            continue;
                        };
                        for part in &f.parts {
                            for p in part {
                                let (x, y) = to_px(p);
                                if let Some(path) = PathBuilder::from_circle(x, y, r.max(0.5)) {
                                    pixmap.fill_path(
                                        &path,
                                        &c.with_opacity(o).paint(),
                                        FillRule::Winding,
                                        Transform::identity(),
                                        None,
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        ColorImage::from_rgba_premultiplied([TILE_PX as usize, TILE_PX as usize], pixmap.data())
    }
}

/// Where vector tiles come from.
pub enum TileSource {
    Archive(TileArchive),
    /// An address with `{z}`, `{x}` and `{y}`.
    Url {
        template: String,
        max_zoom: u8,
    },
}

/// A vector background map: tiles and their style, ready to draw.
pub struct VectorMap {
    pub source: TileSource,
    pub style: Style,
}

impl VectorMap {
    /// Opens `source` (a .pmtiles or .mbtiles file, or a tile address) with
    /// `style` (a style file or address; empty: the built-in one). Without
    /// a source, the style's own vector source is used.
    pub fn open(
        source: &str,
        style: &str,
        agent: &ureq::Agent,
        key: &territoria_io::ApiKey,
    ) -> Result<VectorMap, String> {
        let fetch = |url: &str| -> Result<String, String> {
            agent
                .get(key.apply(url, false))
                .call()
                .map_err(|e| format!("{url}: {e}"))?
                .body_mut()
                .with_config()
                .limit(16 << 20)
                .read_to_string()
                .map_err(|e| format!("{url}: {e}"))
        };
        let is_url = |s: &str| s.starts_with("http://") || s.starts_with("https://");
        let style = match style.trim() {
            "" => Style::built_in(),
            s if is_url(s) => Style::parse(&fetch(s)?)?,
            s => Style::parse(&std::fs::read_to_string(s).map_err(|e| format!("{s}: {e}"))?)?,
        };
        let source = source.trim();
        let source = if source.is_empty() {
            match (&style.source_tiles, &style.source_url) {
                (Some(t), _) => TileSource::Url {
                    template: t.clone(),
                    max_zoom: style.source_max_zoom.unwrap_or(14),
                },
                (None, Some(u)) if is_url(u) => {
                    // A TileJSON with the tile addresses.
                    let tj: Value = serde_json::from_str(&fetch(u)?).map_err(|e| format!("{u}: {e}"))?;
                    let template = tj["tiles"][0].as_str().ok_or(format!("{u}: no tiles"))?.to_owned();
                    TileSource::Url {
                        template,
                        max_zoom: tj["maxzoom"].as_u64().unwrap_or(14).min(30) as u8,
                    }
                }
                _ => return Err("Choose a tile file (.pmtiles, .mbtiles) or address, or a style with its own source.".into()),
            }
        } else if is_url(source) {
            TileSource::Url {
                template: source.to_owned(),
                max_zoom: style.source_max_zoom.unwrap_or(14),
            }
        } else {
            TileSource::Archive(
                TileArchive::open(std::path::Path::new(source)).map_err(|e| e.to_string())?,
            )
        };
        Ok(VectorMap { source, style })
    }

    fn max_zoom(&self) -> u8 {
        match &self.source {
            TileSource::Archive(a) => a.max_zoom(),
            TileSource::Url { max_zoom, .. } => *max_zoom,
        }
    }

    /// The image of tile `z`/`x`/`y` (256 points, Web Mercator). Vector
    /// tiles are made for 512 pixels: one zoom level coarser covers the
    /// same view, drawn larger beyond the source's finest level.
    pub fn tile_image(
        &self,
        z: u8,
        x: u32,
        y: u32,
        fetch: &dyn Fn(&str) -> Option<Vec<u8>>,
        cache: &Cache,
    ) -> Option<ColorImage> {
        let sz = z.saturating_sub(1).min(self.max_zoom());
        let d = z - sz;
        let (sx, sy) = (x >> d, y >> d);
        let tile = cache.get_or(sz, sx, sy, || {
            let bytes = match &self.source {
                TileSource::Archive(a) => a.tile(sz, sx, sy).ok().flatten(),
                TileSource::Url { template, .. } => fetch(
                    &template
                        .replace("{z}", &sz.to_string())
                        .replace("{x}", &sx.to_string())
                        .replace("{y}", &sy.to_string()),
                ),
            };
            // Missing tiles are empty, e.g. open sea.
            bytes
                .map(|b| territoria_io::vectortiles::decode(&b).ok())
                .unwrap_or(Some(VectorTile::default()))
        })?;
        let n = (1u32 << d) as f32;
        let part = [
            (x % (1 << d)) as f32 / n,
            (y % (1 << d)) as f32 / n,
            1.0 / n,
        ];
        Some(self.style.render(&tile, z as f32 - 1.0, part))
    }
}

/// Decoded tiles shared by the finer views drawn from them.
#[derive(Default)]
pub struct Cache(std::sync::Mutex<HashMap<(u8, u32, u32), Arc<VectorTile>>>);

impl Cache {
    fn get_or(
        &self,
        z: u8,
        x: u32,
        y: u32,
        load: impl FnOnce() -> Option<VectorTile>,
    ) -> Option<Arc<VectorTile>> {
        if let Some(t) = self.0.lock().unwrap().get(&(z, x, y)) {
            return Some(t.clone());
        }
        let t = Arc::new(load()?);
        let mut m = self.0.lock().unwrap();
        if m.len() > 64 {
            m.clear();
        }
        m.insert((z, x, y), t.clone());
        Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn feature(kind: FeatureKind, props: Value) -> TileFeature {
        TileFeature {
            kind,
            parts: vec![vec![
                [0.0, 0.0],
                [4096.0, 0.0],
                [4096.0, 4096.0],
                [0.0, 4096.0],
                [0.0, 0.0],
            ]],
            properties: props.as_object().unwrap().clone().into_iter().collect(),
        }
    }

    /// A real style and its tiles from the network: Zürich at zoom 12.
    /// `TERRITORIA_SCREENSHOT_DIR=… cargo test -p territoria-app openfreemap -- --ignored`
    #[test]
    #[ignore]
    fn openfreemap_tile() {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .new_agent();
        let key = territoria_io::ApiKey::default();
        let map = VectorMap::open(
            "",
            "https://tiles.openfreemap.org/styles/positron",
            &agent,
            &key,
        )
        .unwrap();
        assert!(map.style.layers.len() > 10);
        let fetch = |url: &str| -> Option<Vec<u8>> {
            agent
                .get(url)
                .call()
                .ok()?
                .body_mut()
                .with_config()
                .limit(16 << 20)
                .read_to_vec()
                .ok()
        };
        let (z, x, y) = (12, 2144, 1434);
        let img = map
            .tile_image(z, x, y, &fetch, &Cache::default())
            .expect("a tile");
        let distinct: std::collections::HashSet<_> = img.pixels.iter().collect();
        assert!(distinct.len() > 20, "more than a plain background");
        if let Some(dir) = std::env::var_os("TERRITORIA_SCREENSHOT_DIR") {
            let rgba: Vec<u8> = img
                .pixels
                .iter()
                .flat_map(|c| c.to_srgba_unmultiplied())
                .collect();
            image::RgbaImage::from_raw(TILE_PX, TILE_PX, rgba)
                .unwrap()
                .save(std::path::Path::new(&dir).join("vector-tile.png"))
                .unwrap();
        }
    }

    #[test]
    fn colours() {
        assert_eq!(parse_color("#fff"), Some(Rgba([1.0; 4])));
        assert_eq!(
            parse_color("rgba(255, 0, 0, 0.5)"),
            Some(Rgba([1.0, 0.0, 0.0, 0.5]))
        );
        let c = parse_color("hsl(120, 100%, 50%)").unwrap().0;
        assert!((c[1] - 1.0).abs() < 1e-5 && c[0].abs() < 1e-5);
        assert_eq!(parse_color("nonsense"), None);
    }

    #[test]
    fn expressions_and_filters() {
        let f = feature(
            FeatureKind::Polygon,
            json!({"class": "lake", "admin_level": 2}),
        );
        let cx = Ctx {
            zoom: 10.0,
            feature: Some(&f),
        };
        assert!(filter_keeps(&json!(["==", "class", "lake"]), &cx));
        assert!(filter_keeps(&json!(["in", "class", "sea", "lake"]), &cx));
        assert!(!filter_keeps(&json!(["!in", "class", "lake"]), &cx));
        assert!(filter_keeps(&json!(["==", "$type", "Polygon"]), &cx));
        assert!(filter_keeps(
            &json!([
                "all",
                ["==", ["get", "class"], "lake"],
                ["<=", ["get", "admin_level"], 4]
            ]),
            &cx
        ));
        assert!(filter_keeps(
            &json!(["match", ["get", "class"], ["lake", "sea"], true, false]),
            &cx
        ));
        assert!(filter_keeps(
            &json!(["in", ["get", "class"], ["literal", ["lake"]]]),
            &cx
        ));
        assert_eq!(
            eval(
                &json!(["interpolate", ["linear"], ["zoom"], 5, 1, 15, 3]),
                &cx
            ),
            json!(2.0)
        );
        assert_eq!(
            eval(&json!(["step", ["zoom"], 1, 8, 2, 12, 3]), &cx),
            json!(2)
        );
        assert_eq!(
            eval(&json!({"base": 1, "stops": [[5, 1], [15, 3]]}), &cx),
            json!(2.0)
        );
        let c = eval(
            &json!([
                "interpolate",
                ["linear"],
                ["zoom"],
                0,
                "#000000",
                20,
                "#ffffff"
            ]),
            &cx,
        );
        let c = parse_color(c.as_str().unwrap()).unwrap().0;
        assert!((c[0] - 0.5).abs() < 0.01);
    }

    #[test]
    fn draws_fills_and_the_background() {
        let style = Style::parse(
            r##"{"version": 8, "sources": {"t": {"type": "vector", "tiles": ["https://t.example/{z}/{x}/{y}.pbf"], "maxzoom": 12}},
                 "layers": [
                   {"id": "bg", "type": "background", "paint": {"background-color": "#0000ff"}},
                   {"id": "w", "type": "fill", "source-layer": "water", "filter": ["==", "class", "lake"], "paint": {"fill-color": "#ff0000"}},
                   {"id": "labels", "type": "symbol", "source-layer": "water"}
                 ]}"##,
        )
        .unwrap();
        assert_eq!(style.layers.len(), 2, "labels aren't drawn");
        assert_eq!(style.source_max_zoom, Some(12));
        let mut lake = feature(FeatureKind::Polygon, json!({"class": "lake"}));
        // The left half of the tile.
        lake.parts = vec![vec![
            [0.0, 0.0],
            [2048.0, 0.0],
            [2048.0, 4096.0],
            [0.0, 4096.0],
            [0.0, 0.0],
        ]];
        let tile = VectorTile {
            layers: vec![territoria_io::vectortiles::TileLayer {
                name: "water".into(),
                extent: 4096,
                features: vec![lake],
            }],
        };
        let img = style.render(&tile, 10.0, [0.0, 0.0, 1.0]);
        let px = |x: usize, y: usize| img.pixels[y * TILE_PX as usize + x];
        assert_eq!(px(100, 200), egui::Color32::from_rgb(255, 0, 0));
        assert_eq!(px(400, 200), egui::Color32::from_rgb(0, 0, 255));
        // The right half of the tile, drawn larger: all background.
        let img = style.render(&tile, 11.0, [0.5, 0.0, 0.5]);
        assert_eq!(img.pixels[100], egui::Color32::from_rgb(0, 0, 255));
    }
}
