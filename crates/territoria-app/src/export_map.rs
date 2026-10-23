//! Exporting a map: SVG drawn from the resolved layers (with title,
//! legend, scale bar and credits), rendered to PNG with resvg and to PDF
//! with svg2pdf.

use std::fmt::Write as _;

use egui::{Color32, Rect, Stroke, pos2, vec2};
use territoria_analysis::LegendEntry;
use territoria_geo::{Crs, Transformer};
use territoria_io::GeometryKind;
use territoria_render::{LayerMesh, Level, View};

use crate::resolve::{Labels, Resolved};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Svg,
    Pdf,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Svg => "svg",
            Format::Pdf => "pdf",
        }
    }
}

/// A logo as an image for SVG, with its size in points.
#[derive(Clone)]
pub struct Logo {
    pub uri: String,
    pub width: f32,
    pub height: f32,
}

impl Logo {
    /// The image at `path` (PNG, JPEG or SVG), sized to fit `max_w` × `max_h`.
    pub fn load(path: &std::path::Path, max_w: f32, max_h: f32) -> Option<Logo> {
        let bytes = std::fs::read(path).ok()?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase();
        let (mime, (w, h)) = if ext == "svg" {
            let tree = resvg::usvg::Tree::from_data(&bytes, &Default::default()).ok()?;
            ("image/svg+xml", (tree.size().width(), tree.size().height()))
        } else {
            let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
                .with_guessed_format()
                .ok()?;
            let mime = match reader.format()? {
                image::ImageFormat::Png => "image/png",
                image::ImageFormat::Jpeg => "image/jpeg",
                _ => return None,
            };
            let (w, h) = reader.into_dimensions().ok()?;
            (mime, (w as f32, h as f32))
        };
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let k = (max_w / w).min(max_h / h);
        Some(Logo {
            uri: format!("data:{mime};base64,{}", crate::heat::base64(&bytes)),
            width: w * k,
            height: h * k,
        })
    }

    /// The logo as an SVG image with its top-left corner at `x`, `y`.
    pub fn svg(&self, s: &mut String, x: f32, y: f32) {
        let _ = write!(
            s,
            r#"<image x="{x:.1}" y="{y:.1}" width="{:.1}" height="{:.1}" href="{}"/>"#,
            self.width, self.height, self.uri
        );
    }
}

/// What goes around the map.
pub struct Decorations {
    pub title: String,
    /// The company's logo, bottom left.
    pub logo: Option<Logo>,
    pub legends: Vec<(String, Vec<LegendEntry>)>,
    /// Legends of two values at once, below the others.
    pub grids: Vec<GridLegend>,
    pub credits: Vec<String>,
    pub scale_bar: bool,
}

/// The colour grid of a map coloured by two values.
pub struct GridLegend {
    pub title: String,
    /// What rises to the right and upward.
    pub x: String,
    pub y: String,
    pub grid: territoria_analysis::BivariateGrid,
}

/// Builds the SVG for `view` (its rect is the page, in points).
pub fn svg(
    resolved: &[Resolved],
    view: &View,
    crs: &Crs,
    deco: &Decorations,
    font: &str,
    overlays: &str,
) -> String {
    let (w, h) = (view.rect.width(), view.rect.height());
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="{font}">"#
    );
    let _ = write!(s, r##"<rect width="{w}" height="{h}" fill="#f3f1ec"/>"##);
    map_into(&mut s, resolved, view, "map");
    s.push_str(overlays);
    decorations(&mut s, view, crs, deco);
    s.push_str("</svg>");
    s
}

/// Draws the layers, labels and sites into `view.rect` (anywhere on a
/// page), clipped to it.
pub fn map_into(s: &mut String, resolved: &[Resolved], view: &View, clip: &str) {
    map_into_with(s, resolved, view, clip, &[], false);
}

/// Like [`map_into`], with hover texts per resolved layer and feature (for
/// interactive pages) and finer detail when `fine`. An empty `clip` draws
/// without clipping.
pub fn map_into_with(
    s: &mut String,
    resolved: &[Resolved],
    view: &View,
    clip: &str,
    tips: &[Option<Vec<String>>],
    fine: bool,
) {
    let r = view.rect;
    if clip.is_empty() {
        s.push_str("<g>");
    } else {
        let _ = write!(
            s,
            r#"<clipPath id="{clip}"><rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}"/></clipPath><g clip-path="url(#{clip})">"#,
            r.min.x,
            r.min.y,
            r.width(),
            r.height()
        );
    }
    for (i, r) in resolved.iter().enumerate() {
        layer(s, r, view, tips.get(i).and_then(|t| t.as_deref()), fine);
    }
    let mut occupied: Vec<Rect> = Vec::new();
    for r in resolved.iter().rev() {
        labels(s, r, view, &mut occupied);
    }
    for r in resolved {
        sites(s, r, view);
    }
    s.push_str("</g>");
}

/// The usvg options with our font as the sans-serif family.
fn usvg_options(font: &[u8]) -> resvg::usvg::Options<'static> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_font_data(font.to_vec());
    let family = options
        .fontdb
        .faces()
        .next()
        .and_then(|f| f.families.first())
        .map(|f| f.0.clone());
    if let Some(family) = family {
        // The SVG asks for sans-serif: that's our font.
        options.fontdb_mut().set_sans_serif_family(family.clone());
        options.font_family = family;
    }
    options
}

/// The SVG as pixels (for the clipboard), `scale` times its size.
pub fn rasterize(svg: &str, scale: f32, font: &[u8]) -> Result<egui::ColorImage, String> {
    let options = usvg_options(font);
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|e| e.to_string())?;
    let size = tree.size();
    let (w, h) = (
        (size.width() * scale).ceil() as u32,
        (size.height() * scale).ceil() as u32,
    );
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or("the image is too large")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(egui::ColorImage::from_rgba_premultiplied(
        [pixmap.width() as usize, pixmap.height() as usize],
        pixmap.data(),
    ))
}

/// A single-file web page with the map: zoom, pan and hover cards, the
/// title, legend and credits as HTML around it.
pub fn html(svg_body: &str, view: &View, title: &str, deco: &Decorations) -> String {
    let (w, h) = (view.rect.width(), view.rect.height());
    let mut legend = String::new();
    for (name, entries) in &deco.legends {
        let _ = write!(legend, "<div class=\"lg\"><b>{}</b>", escape(name));
        for e in entries {
            let (c, _) = hex(crate::map_view::color(e.color));
            let _ = write!(
                legend,
                "<div><span style=\"background:{c}\"></span>{}</div>",
                escape(&e.label)
            );
        }
        legend.push_str("</div>");
    }
    for g in &deco.grids {
        let k = g.grid.size;
        let _ = write!(
            legend,
            "<div class=\"lg\"><b>{}</b><div>↑ {}</div><div style=\"display:grid;grid-template-columns:repeat({k},16px);gap:1px;margin:3px 0\">",
            escape(&g.title),
            escape(&g.y)
        );
        // Top row first: the second value is high at the top.
        for row in (0..k).rev() {
            for col in 0..k {
                let (c, _) = hex(crate::map_view::color(g.grid.colors[row * k + col]));
                let _ = write!(
                    legend,
                    "<i style=\"display:block;width:16px;height:16px;background:{c}\"></i>"
                );
            }
        }
        let _ = write!(legend, "</div><div>{} →</div></div>", escape(&g.x));
    }
    let credits = escape(&deco.credits.join(" · "));
    let logo = deco.logo.as_ref().map_or(String::new(), |l| {
        format!(
            r#"<img src="{}" width="{:.0}" height="{:.0}" style="display:block;margin-bottom:8px">"#,
            l.uri, l.width, l.height
        )
    });
    let title = escape(title);
    format!(
        r##"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title}</title>
<style>
html,body{{margin:0;height:100%;font-family:system-ui,sans-serif;background:#f3f1ec;color:#202328}}
#map{{position:fixed;inset:0;cursor:grab}}#map svg{{width:100%;height:100%;display:block}}
#tip{{position:fixed;pointer-events:none;background:#fff;border:1px solid #d0d2d8;border-radius:6px;padding:6px 9px;font-size:13px;white-space:pre-line;box-shadow:0 2px 10px rgba(0,0,0,.15);display:none;max-width:320px}}
#tip::first-line{{font-weight:600}}
#side{{position:fixed;top:12px;left:12px;background:rgba(255,255,255,.92);border-radius:8px;padding:10px 12px;font-size:12px;max-height:80vh;overflow:auto;box-shadow:0 1px 6px rgba(0,0,0,.12)}}
#side h1{{font-size:16px;margin:0 0 6px}}.lg{{margin-top:8px}}.lg span{{display:inline-block;width:11px;height:11px;border-radius:2px;margin-right:6px;vertical-align:-1px}}
#foot{{position:fixed;right:10px;bottom:6px;font-size:11px;color:#666}}
[data-tip]:hover{{stroke:#101216;stroke-width:1.5;stroke-opacity:1}}
</style></head><body>
<div id="map"><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" font-family="sans-serif">{svg_body}</svg></div>
<div id="side">{logo}<h1>{title}</h1>{legend}<div style="margin-top:8px;color:#888">Scroll to zoom, drag to move.</div></div>
<div id="tip"></div><div id="foot">{credits}</div>
<script>
const svg=document.querySelector('#map svg'),tip=document.getElementById('tip');
let v={{x:0,y:0,w:{w},h:{h}}};
const apply=()=>svg.setAttribute('viewBox',v.x+' '+v.y+' '+v.w+' '+v.h);
const at=e=>{{const p=svg.createSVGPoint();p.x=e.clientX;p.y=e.clientY;return p.matrixTransform(svg.getScreenCTM().inverse());}};
svg.addEventListener('wheel',e=>{{e.preventDefault();const p=at(e),k=e.deltaY<0?0.8:1.25;v.x=p.x-(p.x-v.x)*k;v.y=p.y-(p.y-v.y)*k;v.w*=k;v.h*=k;apply();}},{{passive:false}});
let drag=null;
svg.addEventListener('mousedown',e=>{{drag=at(e);svg.style.cursor='grabbing';}});
window.addEventListener('mouseup',()=>{{drag=null;svg.style.cursor='';}});
window.addEventListener('mousemove',e=>{{
 if(drag){{const p=at(e);v.x-=p.x-drag.x;v.y-=p.y-drag.y;apply();tip.style.display='none';return;}}
 const t=e.target.closest&&e.target.closest('[data-tip]');
 if(t){{tip.textContent=t.dataset.tip;tip.style.display='block';tip.style.left=(e.clientX+14)+'px';tip.style.top=(e.clientY+14)+'px';}}else tip.style.display='none';
}});
</script></body></html>
"##
    )
}

/// Several SVG pages as one PDF.
pub fn pdf_pages(pages: &[String], font: &[u8]) -> Result<Vec<u8>, String> {
    use pdf_writer::{Content, Finish, Name, Pdf, Rect as PdfRect, Ref};
    let options = usvg_options(font);
    let mut next = 1;
    let mut alloc = || {
        let r = Ref::new(next);
        next += 1;
        r
    };
    let catalog = alloc();
    let tree_id = alloc();
    let mut pdf = Pdf::new();
    let mut page_ids = Vec::new();
    let mut chunks = Vec::new();
    for svg in pages {
        let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|e| e.to_string())?;
        let (chunk, svg_id) = svg2pdf::to_chunk(&tree, svg2pdf::ConversionOptions::default())
            .map_err(|e| e.to_string())?;
        let mut map = std::collections::HashMap::new();
        let chunk = chunk.renumber(|old| *map.entry(old).or_insert_with(&mut alloc));
        let svg_id = *map.get(&svg_id).ok_or("the page has no content")?;
        let (page_id, content_id) = (alloc(), alloc());
        let size = tree.size();
        let (w, h) = (size.width(), size.height());
        let mut page = pdf.page(page_id);
        page.media_box(PdfRect::new(0.0, 0.0, w, h));
        page.parent(tree_id);
        page.contents(content_id);
        page.resources().x_objects().pair(Name(b"P"), svg_id);
        page.finish();
        let mut content = Content::new();
        content
            .transform([w, 0.0, 0.0, h, 0.0, 0.0])
            .x_object(Name(b"P"));
        pdf.stream(content_id, &content.finish());
        page_ids.push(page_id);
        chunks.push(chunk);
    }
    pdf.catalog(catalog).pages(tree_id);
    pdf.pages(tree_id)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);
    for c in &chunks {
        pdf.extend(c);
    }
    Ok(pdf.finish())
}

pub fn hex(c: Color32) -> (String, f32) {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    (format!("#{r:02x}{g:02x}{b:02x}"), a as f32 / 255.0)
}

fn level<'a>(mesh: &'a LayerMesh, view: &View, fine: bool) -> &'a Level {
    // Finer than on screen: exports get zoomed into (pages even more).
    mesh.level_for(view.scale / 2.0 / if fine { 1.5 } else { 1.0 })
}

/// ` data-tip="…"` for a feature's hover text, if any.
fn tip_attr(tips: Option<&[String]>, i: usize) -> String {
    match tips.and_then(|t| t.get(i)).filter(|t| !t.is_empty()) {
        Some(t) => format!(r#" data-tip="{}""#, escape(t)),
        None => String::new(),
    }
}

struct Frame {
    origin: [f64; 2],
    k: f64,
    /// Decimals of coordinates in paths.
    decimals: usize,
}

impl Frame {
    fn new(mesh: &LayerMesh, view: &View) -> Frame {
        let c = view.rect.center();
        let k = 1.0 / view.scale;
        Frame {
            origin: [
                c.x as f64 + (mesh.origin[0] - view.center[0]) * k,
                c.y as f64 - (mesh.origin[1] - view.center[1]) * k,
            ],
            k,
            decimals: 2,
        }
    }

    fn xy(&self, p: [f32; 2]) -> (f32, f32) {
        (
            (self.origin[0] + p[0] as f64 * self.k) as f32,
            (self.origin[1] - p[1] as f64 * self.k) as f32,
        )
    }
}

fn visible(mesh: &LayerMesh, view: &View) -> territoria_render::Bounds {
    let (min, max) = view.visible();
    territoria_render::Bounds {
        min: [
            (min[0] - mesh.origin[0]) as f32,
            (min[1] - mesh.origin[1]) as f32,
        ],
        max: [
            (max[0] - mesh.origin[0]) as f32,
            (max[1] - mesh.origin[1]) as f32,
        ],
    }
}

/// A number with at most one decimal and no trailing zero: `3`, `-0.4`.
fn short(v: f32) -> String {
    let t = (v * 10.0).round() as i64;
    if t % 10 == 0 {
        format!("{}", t / 10)
    } else {
        format!("{:.1}", t as f32 / 10.0)
    }
}

fn path(out: &mut String, f: &Frame, points: &[[f32; 2]], close: bool) {
    if f.decimals == 1 {
        // Compact: relative steps between points rounded to tenths.
        let mut prev = (0i64, 0i64);
        for (i, p) in points.iter().enumerate() {
            let (x, y) = f.xy(*p);
            let at = ((x * 10.0).round() as i64, (y * 10.0).round() as i64);
            if i == 0 {
                let _ = write!(out, "M{} {}", short(x), short(y));
            } else {
                let (dx, dy) = (at.0 - prev.0, at.1 - prev.1);
                if dx == 0 && dy == 0 {
                    continue;
                }
                let _ = write!(
                    out,
                    "l{} {}",
                    short(dx as f32 / 10.0),
                    short(dy as f32 / 10.0)
                );
            }
            prev = at;
        }
        if close {
            out.push('z');
        }
        return;
    }
    for (i, p) in points.iter().enumerate() {
        let (x, y) = f.xy(*p);
        let d = f.decimals;
        let _ = write!(out, "{}{x:.d$} {y:.d$}", if i == 0 { "M" } else { "L" });
    }
    if close {
        out.push('Z');
    }
}

fn layer(s: &mut String, r: &Resolved, view: &View, tips: Option<&[String]>, fine: bool) {
    let mesh = &r.mesh;
    let lvl = level(mesh, view, fine);
    let mut f = Frame::new(mesh, view);
    if fine {
        // Pages: one decimal is a tenth of a point, plenty when zoomed.
        f.decimals = 1;
    }
    let vis = visible(mesh, view);
    match mesh.kind {
        GeometryKind::Area => {
            for (i, fill) in r.fills.iter().enumerate() {
                let Some(fill) = fill else { continue };
                if fill.a() == 0 || !mesh.bounds[i].intersects(&vis) {
                    continue;
                }
                let mut d = String::new();
                for ring in mesh.rings(lvl, i) {
                    path(&mut d, &f, &ring, true);
                }
                if d.is_empty() {
                    continue;
                }
                let (c, a) = hex(*fill);
                let tip = tip_attr(tips, i);
                // A hairline in the fill colour hides seams between neighbours.
                let _ = write!(
                    s,
                    r#"<path d="{d}" fill="{c}" fill-opacity="{a:.3}" fill-rule="evenodd" stroke="{c}" stroke-opacity="{a:.3}" stroke-width="0.3"{tip}/>"#
                );
            }
            // Borders, grouped by stroke.
            let mut groups: Vec<(Stroke, String)> = Vec::new();
            for (i, pts) in lvl.arcs.iter().enumerate() {
                let Some(stroke) = r.arcs.get(i).copied().flatten() else {
                    continue;
                };
                if pts.len() < 2 || stroke.width <= 0.0 || !lvl.arc_bounds[i].intersects(&vis) {
                    continue;
                }
                let d = match groups.iter_mut().find(|(st, _)| *st == stroke) {
                    Some((_, d)) => d,
                    None => {
                        groups.push((stroke, String::new()));
                        &mut groups.last_mut().unwrap().1
                    }
                };
                path(d, &f, pts, false);
            }
            // Thin borders first, territory borders on top.
            groups.sort_by(|a, b| a.0.width.total_cmp(&b.0.width));
            for (stroke, d) in groups {
                let (c, a) = hex(stroke.color);
                let _ = write!(
                    s,
                    r#"<path d="{d}" fill="none" stroke="{c}" stroke-opacity="{a:.3}" stroke-width="{:.2}" stroke-linejoin="round" stroke-linecap="round"/>"#,
                    stroke.width
                );
            }
        }
        GeometryKind::Line => {
            for (i, range) in lvl.line_parts.iter().enumerate() {
                let Some(c) = r.fills.get(i).copied().flatten() else {
                    continue;
                };
                if !mesh.bounds[i].intersects(&vis) {
                    continue;
                }
                let mut d = String::new();
                for part in &lvl.arcs[range.start as usize..range.end as usize] {
                    path(&mut d, &f, part, false);
                }
                let (c, a) = hex(c);
                let _ = write!(
                    s,
                    r#"<path d="{d}" fill="none" stroke="{c}" stroke-opacity="{a:.3}" stroke-width="{:.2}" stroke-linejoin="round"/>"#,
                    r.line_width
                );
            }
        }
        GeometryKind::Point => {
            let (sc, sa) = hex(r.point_stroke.color);
            for (i, pts) in mesh.points.iter().enumerate() {
                let Some(fill) = r.fills.get(i).copied().flatten() else {
                    continue;
                };
                let (c, a) = hex(fill);
                let tip = tip_attr(tips, i);
                for p in pts {
                    let (x, y) = f.xy(*p);
                    if !view.rect.expand(10.0).contains(pos2(x, y)) {
                        continue;
                    }
                    let _ = write!(
                        s,
                        r#"<circle cx="{x:.2}" cy="{y:.2}" r="{:.2}" fill="{c}" fill-opacity="{a:.3}" stroke="{sc}" stroke-opacity="{sa:.3}" stroke-width="{:.2}"{tip}/>"#,
                        r.point_radius, r.point_stroke.width
                    );
                }
            }
        }
    }
    // Catchment rings.
    for ring in &r.rings {
        let mut d = String::new();
        for (k, p) in ring.points.iter().enumerate() {
            let s = view.to_screen(*p);
            let _ = write!(d, "{}{:.2} {:.2}", if k == 0 { "M" } else { "L" }, s.x, s.y);
        }
        if d.is_empty() {
            continue;
        }
        let (fc, fa) = hex(ring.fill);
        let (sc, sa) = hex(ring.stroke.color);
        let _ = write!(
            s,
            r#"<path d="{d}Z" fill="{fc}" fill-opacity="{fa:.3}" stroke="{sc}" stroke-opacity="{sa:.3}" stroke-width="{:.2}"/>"#,
            ring.stroke.width
        );
    }
    // Proportional circles, largest first.
    for &(i, radius) in &r.symbols {
        let Some(lp) = mesh.label_points.get(i).copied().flatten() else {
            continue;
        };
        let (x, y) = f.xy(lp);
        if !view.rect.expand(radius).contains(pos2(x, y)) {
            continue;
        }
        let fill = r
            .symbol_colors
            .get(i)
            .copied()
            .flatten()
            .unwrap_or(r.symbol_fill);
        let (c, a) = hex(fill);
        let (sc, sa) = hex(r.symbol_stroke.color);
        let _ = write!(
            s,
            r#"<circle cx="{x:.2}" cy="{y:.2}" r="{radius:.2}" fill="{c}" fill-opacity="{a:.3}" stroke="{sc}" stroke-opacity="{sa:.3}" stroke-width="{:.2}"/>"#,
            r.symbol_stroke.width
        );
    }
    for chart in &r.charts {
        let Some(lp) = mesh.label_points.get(chart.feature).copied().flatten() else {
            continue;
        };
        let (x, y) = f.xy(lp);
        let p = pos2(x, y);
        if view.rect.intersects(crate::charts::bounds(chart, p)) {
            crate::charts::svg(s, chart, p, &tip_attr(tips, chart.feature));
        }
    }
    for flow in &r.flows {
        crate::flowlines::svg(s, flow, view);
    }
}

fn sites(s: &mut String, r: &Resolved, view: &View) {
    for (d, c) in &r.sites {
        let p = view.to_screen(*d);
        let (c, _) = hex(*c);
        let _ = write!(
            s,
            r##"<circle cx="{:.2}" cy="{:.2}" r="5.5" fill="#ffffff" stroke="#2a2d33" stroke-width="1.2"/><circle cx="{:.2}" cy="{:.2}" r="4" fill="{c}"/>"##,
            p.x, p.y, p.x, p.y
        );
    }
}

pub fn escape(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Rough text width for collision checks (no shaping in the exporter).
pub fn text_width(t: &str, size: f32) -> f32 {
    t.chars().count() as f32 * size * 0.48
}

fn labels(s: &mut String, r: &Resolved, view: &View, occupied: &mut Vec<Rect>) {
    for ring in &r.rings {
        let Some(top) = ring
            .points
            .iter()
            .map(|d| view.to_screen(*d))
            .min_by(|a, b| a.y.total_cmp(&b.y))
        else {
            continue;
        };
        let rect = Rect::from_center_size(top, vec2(text_width(&ring.label, 9.0) + 4.0, 13.0));
        if !view.rect.contains(top) || occupied.iter().any(|o| o.intersects(rect)) {
            continue;
        }
        occupied.push(rect);
        let (c, _) = hex(ring.stroke.color);
        let _ = write!(
            s,
            r##"<text x="{:.2}" y="{:.2}" font-size="9" text-anchor="middle" fill="{c}" stroke="#ffffff" stroke-opacity="0.85" stroke-width="2.5" paint-order="stroke">{}</text>"##,
            top.x,
            top.y + 3.0,
            escape(&ring.label)
        );
    }
    let mesh = &r.mesh;
    let f = Frame::new(mesh, view);
    match &r.labels {
        Labels::None => {}
        Labels::Features(texts) => {
            let size = 11.0;
            let mut order: Vec<(usize, f32)> = texts
                .iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    t.as_ref()?;
                    mesh.label_points.get(i).copied().flatten()?;
                    let w = mesh.bounds[i].width() as f64 * f.k;
                    Some((
                        i,
                        if mesh.kind == GeometryKind::Area {
                            w as f32
                        } else {
                            f32::MAX
                        },
                    ))
                })
                .collect();
            order.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (i, width) in order.into_iter().take(800) {
                let t = texts[i].as_deref().unwrap_or_default();
                // Templates may give several lines: one tspan each.
                let lines: Vec<&str> = t.lines().collect();
                let tw = lines
                    .iter()
                    .map(|l| text_width(l, size))
                    .fold(0.0, f32::max);
                if mesh.kind == GeometryKind::Area && tw > width * 1.15 {
                    continue;
                }
                let (x, y) = f.xy(mesh.label_points[i].unwrap());
                let line_height = size * 1.2;
                let height = line_height * lines.len() as f32;
                let rect = Rect::from_center_size(pos2(x, y), vec2(tw + 4.0, height + 4.0));
                if !view.rect.contains(rect.center()) || occupied.iter().any(|o| o.intersects(rect))
                {
                    continue;
                }
                occupied.push(rect);
                let top = y - height / 2.0 + line_height / 2.0 + size * 0.35;
                let _ = write!(
                    s,
                    r##"<text font-size="{size}" text-anchor="middle" fill="#202328" stroke="#ffffff" stroke-opacity="0.8" stroke-width="2.5" paint-order="stroke">"##
                );
                for (k, line) in lines.iter().enumerate() {
                    let _ = write!(
                        s,
                        r#"<tspan x="{x:.2}" y="{:.2}">{}</tspan>"#,
                        top + k as f32 * line_height,
                        escape(line)
                    );
                }
                s.push_str("</text>");
            }
        }
        Labels::Territories(names) => {
            let size = 13.0;
            for (unit, name) in names {
                let Some(lp) = mesh.label_points.get(*unit as usize).copied().flatten() else {
                    continue;
                };
                let (x, mut y) = f.xy(lp);
                let tw = text_width(name, size);
                let mut rect = Rect::from_center_size(pos2(x, y), vec2(tw + 8.0, size + 6.0));
                // A site marker under the name: the name goes below it.
                if let Some(site) = r
                    .sites
                    .iter()
                    .map(|(d, _)| view.to_screen(*d))
                    .find(|s| rect.expand(6.0).contains(*s))
                {
                    let dy = site.y + 8.0 - rect.top();
                    rect = rect.translate(vec2(0.0, dy));
                    y += dy;
                }
                if !view.rect.contains(rect.center()) || occupied.iter().any(|o| o.intersects(rect))
                {
                    continue;
                }
                occupied.push(rect);
                let _ = write!(
                    s,
                    r##"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="4" fill="#ffffff" fill-opacity="0.8"/><text x="{x:.2}" y="{:.2}" font-size="{size}" text-anchor="middle" fill="#202328">{}</text>"##,
                    rect.min.x,
                    rect.min.y,
                    rect.width(),
                    rect.height(),
                    y + size * 0.35,
                    escape(name)
                );
            }
        }
    }
}

fn decorations(s: &mut String, view: &View, crs: &Crs, deco: &Decorations) {
    let (w, h) = (view.rect.width(), view.rect.height());
    if !deco.title.is_empty() {
        let _ = write!(
            s,
            r##"<rect x="12" y="12" width="{:.1}" height="34" rx="6" fill="#ffffff" fill-opacity="0.88"/><text x="24" y="35" font-size="18" fill="#202328">{}</text>"##,
            text_width(&deco.title, 18.0) + 26.0,
            escape(&deco.title)
        );
    }
    // Legends stacked in the top right corner.
    let mut y = 12.0f32;
    for (title, entries) in &deco.legends {
        if entries.is_empty() {
            continue;
        }
        let shown: Vec<&LegendEntry> = entries.iter().take(14).collect();
        let width = shown
            .iter()
            .map(|e| text_width(&e.label, 11.0))
            .fold(text_width(title, 12.0), f32::max)
            + 44.0;
        let height = 28.0
            + shown.len() as f32 * 17.0
            + if entries.len() > shown.len() {
                14.0
            } else {
                0.0
            };
        let x = w - width - 12.0;
        let _ = write!(
            s,
            r##"<rect x="{x:.1}" y="{y:.1}" width="{width:.1}" height="{height:.1}" rx="6" fill="#ffffff" fill-opacity="0.9"/><text x="{:.1}" y="{:.1}" font-size="12" fill="#202328">{}</text>"##,
            x + 10.0,
            y + 18.0,
            escape(title)
        );
        for (i, e) in shown.iter().enumerate() {
            let ey = y + 26.0 + i as f32 * 17.0;
            let c = e.color;
            let _ = write!(
                s,
                r##"<rect x="{:.1}" y="{ey:.1}" width="14" height="12" rx="2" fill="#{:02x}{:02x}{:02x}" fill-opacity="{:.3}"/><text x="{:.1}" y="{:.1}" font-size="11" fill="#202328">{}</text>"##,
                x + 10.0,
                c.r(),
                c.g(),
                c.b(),
                c.a() as f32 / 255.0,
                x + 30.0,
                ey + 10.0,
                escape(&e.label)
            );
        }
        if entries.len() > shown.len() {
            let _ = write!(
                s,
                r##"<text x="{:.1}" y="{:.1}" font-size="10" fill="#6a717c">… {} more</text>"##,
                x + 10.0,
                y + height - 6.0,
                entries.len() - shown.len()
            );
        }
        y += height + 8.0;
    }
    for g in &deco.grids {
        let k = g.grid.size;
        let cell = 14.0;
        let side = cell * k as f32;
        let up = format!("↑ {}", g.y);
        let right = format!("{} →", g.x);
        let width = [
            text_width(&g.title, 12.0),
            text_width(&up, 11.0),
            text_width(&right, 11.0),
            side,
        ]
        .into_iter()
        .fold(0.0, f32::max)
            + 20.0;
        let height = 26.0 + 16.0 + side + 20.0;
        let x = w - width - 12.0;
        let _ = write!(
            s,
            r##"<rect x="{x:.1}" y="{y:.1}" width="{width:.1}" height="{height:.1}" rx="6" fill="#ffffff" fill-opacity="0.9"/><text x="{:.1}" y="{:.1}" font-size="12" fill="#202328">{}</text><text x="{:.1}" y="{:.1}" font-size="11" fill="#202328">{}</text>"##,
            x + 10.0,
            y + 18.0,
            escape(&g.title),
            x + 10.0,
            y + 36.0,
            escape(&up)
        );
        let top = y + 42.0;
        for row in 0..k {
            for col in 0..k {
                let c = g.grid.colors[row * k + col];
                let _ = write!(
                    s,
                    r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="#{:02x}{:02x}{:02x}"/>"##,
                    x + 10.0 + col as f32 * cell,
                    top + side - (row + 1) as f32 * cell,
                    cell - 1.0,
                    cell - 1.0,
                    c.r(),
                    c.g(),
                    c.b()
                );
            }
        }
        let _ = write!(
            s,
            r##"<text x="{:.1}" y="{:.1}" font-size="11" fill="#202328">{}</text>"##,
            x + 10.0,
            top + side + 14.0,
            escape(&right)
        );
        y += height + 8.0;
    }
    // Scale bar, bottom right.
    if deco.scale_bar
        && let Ok(t) = Transformer::new(crs, &Crs::wgs84())
    {
        let c = view.rect.center();
        let a = view.to_display(c);
        let b = view.to_display(c + vec2(100.0, 0.0));
        if let (Some(a), Some(b)) = (
            t.convert(geo_types::Coord { x: a[0], y: a[1] }),
            t.convert(geo_types::Coord { x: b[0], y: b[1] }),
        ) {
            let mpp = territoria_geo::haversine(a, b) / 100.0;
            if mpp.is_finite() && mpp > 0.0 {
                let nice = territoria_analysis::classify::nice_step(mpp * 110.0);
                let len = (nice / mpp) as f32;
                let label = if nice >= 1000.0 {
                    format!(
                        "{} km",
                        territoria_analysis::format::format_value(nice / 1000.0)
                    )
                } else {
                    format!("{} m", territoria_analysis::format::format_value(nice))
                };
                let (x1, y1) = (w - 18.0, h - 16.0);
                let x0 = x1 - len;
                let _ = write!(
                    s,
                    r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="30" rx="4" fill="#ffffff" fill-opacity="0.8"/><path d="M{x0:.1} {:.1}L{x0:.1} {y1:.1}L{x1:.1} {y1:.1}L{x1:.1} {:.1}" fill="none" stroke="#303338" stroke-width="2"/><text x="{:.1}" y="{:.1}" font-size="11" text-anchor="middle" fill="#303338">{label}</text>"##,
                    x0 - 8.0,
                    h - 40.0,
                    len + 16.0,
                    y1 - 5.0,
                    y1 - 5.0,
                    (x0 + x1) / 2.0,
                    y1 - 8.0
                );
            }
        }
    }
    if let Some(logo) = &deco.logo {
        let bottom = if deco.credits.is_empty() {
            h - 10.0
        } else {
            h - 30.0
        };
        logo.svg(s, 10.0, bottom - logo.height);
    }
    if !deco.credits.is_empty() {
        let text = deco.credits.join(" · ");
        let _ = write!(
            s,
            r##"<rect x="8" y="{:.1}" width="{:.1}" height="16" rx="3" fill="#ffffff" fill-opacity="0.75"/><text x="12" y="{:.1}" font-size="9" fill="#50545a">{}</text>"##,
            h - 24.0,
            text_width(&text, 9.0) + 8.0,
            h - 13.0,
            escape(&text)
        );
    }
}

/// The SVG as a file in `format`. `scale` multiplies the pixel size of
/// PNGs.
pub fn encode(svg: &str, format: Format, scale: f32, font: &[u8]) -> Result<Vec<u8>, String> {
    if format == Format::Svg {
        return Ok(svg.as_bytes().to_vec());
    }
    let options = usvg_options(font);
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|e| e.to_string())?;
    match format {
        Format::Png => {
            let size = tree.size();
            let (w, h) = (
                (size.width() * scale).ceil() as u32,
                (size.height() * scale).ceil() as u32,
            );
            let mut pixmap = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))
                .ok_or("the image is too large")?;
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            pixmap.encode_png().map_err(|e| e.to_string())
        }
        Format::Pdf => svg2pdf::to_pdf(
            &tree,
            svg2pdf::ConversionOptions::default(),
            svg2pdf::PageOptions::default(),
        )
        .map_err(|e| e.to_string()),
        Format::Svg => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_encodes() {
        assert_eq!(escape("A & <B>"), "A &amp; &lt;B&gt;");
        let font = egui::FontDefinitions::default().font_data["Ubuntu-Light"]
            .font
            .to_vec();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#ff0000"/><text x="2" y="15" font-size="12">Hi</text></svg>"##;
        let png = encode(svg, Format::Png, 2.0, &font).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let pdf = encode(svg, Format::Pdf, 1.0, &font).unwrap();
        assert_eq!(&pdf[..5], b"%PDF-");
    }
}
