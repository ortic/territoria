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

/// What goes around the map.
pub struct Decorations {
    pub title: String,
    pub legends: Vec<(String, Vec<LegendEntry>)>,
    pub credits: Vec<String>,
    pub scale_bar: bool,
}

/// Builds the SVG for `view` (its rect is the page, in points).
pub fn svg(
    resolved: &[Resolved],
    view: &View,
    crs: &Crs,
    deco: &Decorations,
    font: &str,
) -> String {
    let (w, h) = (view.rect.width(), view.rect.height());
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="{font}">"#
    );
    let _ = write!(s, r##"<rect width="{w}" height="{h}" fill="#f3f1ec"/>"##);
    let _ = write!(
        s,
        r#"<clipPath id="map"><rect width="{w}" height="{h}"/></clipPath><g clip-path="url(#map)">"#
    );
    for r in resolved {
        layer(&mut s, r, view);
    }
    let mut occupied: Vec<Rect> = Vec::new();
    for r in resolved.iter().rev() {
        labels(&mut s, r, view, &mut occupied);
    }
    for r in resolved {
        sites(&mut s, r, view);
    }
    s.push_str("</g>");
    decorations(&mut s, view, crs, deco);
    s.push_str("</svg>");
    s
}

fn hex(c: Color32) -> (String, f32) {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    (format!("#{r:02x}{g:02x}{b:02x}"), a as f32 / 255.0)
}

fn level<'a>(mesh: &'a LayerMesh, view: &View) -> &'a Level {
    // Finer than on screen: exports get zoomed into.
    mesh.level_for(view.scale / 2.0)
}

struct Frame {
    origin: [f64; 2],
    k: f64,
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

fn path(out: &mut String, f: &Frame, points: &[[f32; 2]], close: bool) {
    for (i, p) in points.iter().enumerate() {
        let (x, y) = f.xy(*p);
        let _ = write!(out, "{}{x:.2} {y:.2}", if i == 0 { "M" } else { "L" });
    }
    if close {
        out.push('Z');
    }
}

fn layer(s: &mut String, r: &Resolved, view: &View) {
    let mesh = &r.mesh;
    let lvl = level(mesh, view);
    let f = Frame::new(mesh, view);
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
                // A hairline in the fill colour hides seams between neighbours.
                let _ = write!(
                    s,
                    r#"<path d="{d}" fill="{c}" fill-opacity="{a:.3}" fill-rule="evenodd" stroke="{c}" stroke-opacity="{a:.3}" stroke-width="0.3"/>"#
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
                for p in pts {
                    let (x, y) = f.xy(*p);
                    if !view.rect.expand(10.0).contains(pos2(x, y)) {
                        continue;
                    }
                    let _ = write!(
                        s,
                        r#"<circle cx="{x:.2}" cy="{y:.2}" r="{:.2}" fill="{c}" fill-opacity="{a:.3}" stroke="{sc}" stroke-opacity="{sa:.3}" stroke-width="{:.2}"/>"#,
                        r.point_radius, r.point_stroke.width
                    );
                }
            }
        }
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

fn escape(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Rough text width for collision checks (no shaping in the exporter).
fn text_width(t: &str, size: f32) -> f32 {
    t.chars().count() as f32 * size * 0.48
}

fn labels(s: &mut String, r: &Resolved, view: &View, occupied: &mut Vec<Rect>) {
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
                let (x, y) = f.xy(lp);
                let tw = text_width(name, size);
                let rect = Rect::from_center_size(pos2(x, y), vec2(tw + 8.0, size + 6.0));
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
