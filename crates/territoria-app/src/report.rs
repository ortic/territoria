//! A territory report: an overview page with all territories and one
//! page per territory with its map, figures and largest areas. Pages are
//! SVG (A4 landscape, in points) and become one PDF.

use std::fmt::Write as _;
use std::sync::Arc;

use egui::{Color32, Rect, Stroke, pos2};
use territoria_analysis::format::{format_percent, format_value};
use territoria_geo::OUTSIDE;
use territoria_model::{Plan, PlanStats};
use territoria_render::{LayerMesh, View};

use crate::export_map::{escape, hex, map_into, text_width};
use crate::map_view::color;
use crate::resolve::{Labels, Resolved, territory_border, with_opacity};

const W: f32 = 842.0;
const H: f32 = 595.0;
const MARGIN: f32 = 36.0;
const INK: &str = "#202328";
const WEAK: &str = "#6b6f78";

/// Everything a report needs, gathered on the UI thread.
pub struct ReportData {
    pub title: String,
    pub plan: Plan,
    /// The plan's areas (or the cells of its points) in the map's CRS.
    pub mesh: Arc<LayerMesh>,
    pub stats: Arc<PlanStats>,
    /// The balancing measure (unless it's the number of areas): its name
    /// and value per unit.
    pub measure: Option<(String, Vec<Option<f64>>)>,
    /// Per territory, the unit its name goes on.
    pub label_units: Vec<Option<u32>>,
    pub tolerance: f64,
    /// Per unit, its name.
    pub names: Vec<String>,
    pub credits: Vec<String>,
    pub date: String,
    /// The company's logo, top right of each page.
    pub logo: Option<crate::export_map::Logo>,
    /// The CRS `mesh` is in.
    pub crs: territoria_geo::Crs,
}

#[allow(clippy::too_many_arguments)]
fn text(s: &mut String, x: f32, y: f32, size: f32, fill: &str, anchor: &str, bold: bool, t: &str) {
    let _ = write!(
        s,
        r#"<text x="{x:.2}" y="{y:.2}" font-size="{size}" fill="{fill}" text-anchor="{anchor}"{}>{}</text>"#,
        if bold { r#" font-weight="bold""# } else { "" },
        escape(t)
    );
}

/// Shortens `t` to fit `width` at `size`.
fn fit(t: &str, size: f32, width: f32) -> String {
    if text_width(t, size) <= width {
        return t.to_owned();
    }
    let mut out: String = t.to_owned();
    while !out.is_empty() && text_width(&format!("{out}…"), size) > width {
        out.pop();
    }
    format!("{out}…")
}

fn swatch(s: &mut String, x: f32, y: f32, size: f32, c: Color32) {
    let (fill, _) = hex(c);
    let _ = write!(
        s,
        r#"<rect x="{x:.2}" y="{y:.2}" width="{size}" height="{size}" rx="2" fill="{fill}"/>"#
    );
}

fn page_start(s: &mut String, d: &ReportData) {
    let _ = write!(
        s,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" font-family="sans-serif"><rect width="{W}" height="{H}" fill="#ffffff"/>"##
    );
    if let Some(logo) = &d.logo {
        logo.svg(s, W - MARGIN - logo.width, 28.0);
    }
}

/// Room for the title beside the logo.
fn title_width(d: &ReportData, left: f32) -> f32 {
    W - MARGIN - left - d.logo.as_ref().map_or(0.0, |l| l.width + 16.0)
}

fn footer(s: &mut String, d: &ReportData, page: usize, pages: usize) {
    let y = H - 18.0;
    let left = if d.credits.is_empty() {
        d.date.clone()
    } else {
        format!("{} · {}", d.date, d.credits.join(" · "))
    };
    text(
        s,
        MARGIN,
        y,
        7.5,
        WEAK,
        "start",
        false,
        &fit(&left, 7.5, W - 2.0 * MARGIN - 80.0),
    );
    text(
        s,
        W - MARGIN,
        y,
        7.5,
        WEAK,
        "end",
        false,
        &format!("Page {page} of {pages}"),
    );
}

/// The map frame on the left of a page.
fn map_rect() -> Rect {
    Rect::from_min_max(pos2(MARGIN, 84.0), pos2(W - MARGIN - 250.0, H - 36.0))
}

fn frame(s: &mut String, r: Rect) {
    let _ = write!(
        s,
        r##"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="none" stroke="#c9cbd1" stroke-width="0.6"/>"##,
        r.min.x,
        r.min.y,
        r.width(),
        r.height()
    );
}

/// A view on `units` (all when empty) filling `rect` with some room.
fn view_on(mesh: &LayerMesh, units: &[u32], rect: Rect) -> Option<View> {
    let mut b = territoria_render::Bounds::EMPTY;
    let all: Vec<u32>;
    let units = if units.is_empty() {
        all = (0..mesh.bounds.len() as u32).collect();
        &all
    } else {
        units
    };
    for &u in units {
        if let Some(fb) = mesh.bounds.get(u as usize).filter(|b| !b.is_empty()) {
            b.add(fb.min);
            b.add(fb.max);
        }
    }
    if b.is_empty() {
        return None;
    }
    let mut v = View::fit(mesh.to_display(b.min), mesh.to_display(b.max), rect);
    v.scale *= 1.12;
    Some(v)
}

/// The plan as on the map: territories in their colours with labels.
fn overview_layer(d: &ReportData) -> Resolved {
    let p = &d.plan;
    let index: std::collections::HashMap<_, usize> = p
        .territories
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id, i))
        .collect();
    let of = |u: u32| {
        p.assignment
            .get(u as usize)
            .copied()
            .flatten()
            .and_then(|t| index.get(&t).copied())
    };
    let n = d.mesh.bounds.len();
    let fills = (0..n as u32)
        .map(|u| {
            Some(
                of(u).map_or(Color32::from_rgba_unmultiplied(0xd8, 0xd8, 0xdc, 90), |t| {
                    with_opacity(color(p.territories[t].color), 0.85)
                }),
            )
        })
        .collect();
    let border = territory_border(false);
    let arcs = d
        .mesh
        .geometry
        .topology
        .arcs
        .iter()
        .map(|a| {
            let (ta, tb) = (of(a.a), if a.b == OUTSIDE { None } else { of(a.b) });
            if a.a == a.b {
                None
            } else if ta != tb {
                Some(Stroke::new(1.0, border))
            } else {
                Some(Stroke::new(0.25, with_opacity(Color32::WHITE, 0.6)))
            }
        })
        .collect();
    let labels = d
        .label_units
        .iter()
        .zip(&p.territories)
        .filter_map(|(u, t)| u.map(|u| (u, t.name.clone())))
        .collect();
    Resolved {
        mesh: d.mesh.clone(),
        fills,
        arcs,
        line_width: 1.0,
        point_radius: 3.0,
        point_stroke: Stroke::new(0.8, border),
        labels: Labels::Territories(labels),
        sites: Vec::new(),
        symbols: Vec::new(),
        symbol_fill: Color32::TRANSPARENT,
        symbol_stroke: Stroke::NONE,
        symbol_colors: Vec::new(),
        charts: Vec::new(),
        flows: Vec::new(),
        rings: Vec::new(),
    }
}

/// One territory in colour, the others faded, its units named.
fn territory_layer(d: &ReportData, t: usize) -> Resolved {
    let p = &d.plan;
    let id = p.territories[t].id;
    let n = d.mesh.bounds.len();
    let mine = |u: u32| p.assignment.get(u as usize).copied().flatten() == Some(id);
    let colours: std::collections::HashMap<_, Color32> = p
        .territories
        .iter()
        .map(|t| (t.id, color(t.color)))
        .collect();
    let fills = (0..n as u32)
        .map(|u| {
            Some(match p.assignment.get(u as usize).copied().flatten() {
                Some(x) if x == id => with_opacity(colours[&x], 0.9),
                Some(x) => with_opacity(colours[&x], 0.2),
                None => Color32::from_rgba_unmultiplied(0xd8, 0xd8, 0xdc, 60),
            })
        })
        .collect();
    let border = territory_border(false);
    let arcs = d
        .mesh
        .geometry
        .topology
        .arcs
        .iter()
        .map(|a| {
            let inside_a = mine(a.a);
            let inside_b = a.b != OUTSIDE && mine(a.b);
            if a.a == a.b {
                None
            } else if inside_a != inside_b {
                Some(Stroke::new(1.6, border))
            } else if inside_a {
                Some(Stroke::new(0.35, with_opacity(Color32::WHITE, 0.8)))
            } else {
                Some(Stroke::new(
                    0.25,
                    Color32::from_rgba_unmultiplied(0x9a, 0x9d, 0xa5, 120),
                ))
            }
        })
        .collect();
    let labels = Labels::Features(Arc::new(
        (0..n as u32)
            .map(|u| mine(u).then(|| d.names.get(u as usize).cloned().unwrap_or_default()))
            .collect(),
    ));
    let mut sites = Vec::new();
    if let Some([lon, lat]) = p.territories[t].site
        && let Ok(to) = territoria_geo::Transformer::new(&territoria_geo::Crs::wgs84(), &d.crs)
        && let Some(c) = to.convert(geo_types::Coord { x: lon, y: lat })
    {
        sites.push(([c.x, c.y], color(p.territories[t].color).to_opaque()));
    }
    Resolved {
        mesh: d.mesh.clone(),
        fills,
        arcs,
        line_width: 1.0,
        point_radius: 3.0,
        point_stroke: Stroke::new(0.8, border),
        labels,
        sites,
        symbols: Vec::new(),
        symbol_fill: Color32::TRANSPARENT,
        symbol_stroke: Stroke::NONE,
        symbol_colors: Vec::new(),
        charts: Vec::new(),
        flows: Vec::new(),
        rings: Vec::new(),
    }
}

fn deviation_text(dev: f64) -> String {
    format!(
        "{}{}",
        if dev > 0.0 { "+" } else { "" },
        format_percent(dev)
    )
}

fn overview(d: &ReportData, pages: usize) -> String {
    let mut s = String::new();
    page_start(&mut s, d);
    text(
        &mut s,
        MARGIN,
        52.0,
        20.0,
        INK,
        "start",
        true,
        &fit(&d.title, 20.0, title_width(d, MARGIN)),
    );
    let subtitle = format!(
        "{} territories · {} areas",
        d.plan.territories.len(),
        format_value(d.plan.assignment.len() as f64)
    );
    text(&mut s, MARGIN, 70.0, 10.0, WEAK, "start", false, &subtitle);
    let r = map_rect();
    if let Some(view) = view_on(&d.mesh, &[], r) {
        map_into(&mut s, &[overview_layer(d)], &view, "overview");
    }
    frame(&mut s, r);
    // The territories with their figures.
    let x = r.max.x + 18.0;
    let right = W - MARGIN;
    let rows = d.plan.territories.len() + 1;
    let row = ((H - 36.0 - 100.0) / rows as f32).clamp(8.0, 16.0);
    let size = (row * 0.62).clamp(5.5, 9.0);
    let mut y = 96.0;
    // Columns: name, areas, the measure (if not the areas), deviation.
    let (areas_x, value_x) = if d.measure.is_some() {
        (right - 104.0, right - 46.0)
    } else {
        (right - 46.0, right - 46.0)
    };
    text(&mut s, x + 12.0, y, size, WEAK, "start", false, "Territory");
    text(&mut s, areas_x, y, size, WEAK, "end", false, "Areas");
    if let Some((name, _)) = &d.measure {
        text(
            &mut s,
            value_x,
            y,
            size,
            WEAK,
            "end",
            false,
            &fit(name, size, 50.0),
        );
    }
    text(&mut s, right, y, size, WEAK, "end", false, "±");
    for (t, ts) in d.plan.territories.iter().zip(&d.stats.territories) {
        y += row;
        swatch(&mut s, x, y - size * 0.85, size, color(t.color));
        text(
            &mut s,
            x + 12.0,
            y,
            size,
            INK,
            "start",
            false,
            &fit(&t.name, size, areas_x - 40.0 - x - 12.0),
        );
        text(
            &mut s,
            areas_x,
            y,
            size,
            INK,
            "end",
            false,
            &format_value(ts.units as f64),
        );
        if d.measure.is_some()
            && let Some(value) = ts.values.first()
        {
            text(
                &mut s,
                value_x,
                y,
                size,
                INK,
                "end",
                false,
                &format_value(*value),
            );
        }
        if let Some(dev) = ts.deviations.first() {
            let fill = if dev.abs() > d.tolerance {
                "#c2410c"
            } else {
                "#15803d"
            };
            text(
                &mut s,
                right,
                y,
                size,
                fill,
                "end",
                false,
                &deviation_text(*dev),
            );
        }
    }
    footer(&mut s, d, 1, pages);
    s.push_str("</svg>");
    s
}

fn territory_page(d: &ReportData, t: usize, page: usize, pages: usize) -> String {
    let p = &d.plan;
    let territory = &p.territories[t];
    let ts = &d.stats.territories[t];
    let members: Vec<u32> = (0..p.assignment.len() as u32)
        .filter(|&u| p.assignment[u as usize] == Some(territory.id))
        .collect();
    let mut s = String::new();
    page_start(&mut s, d);
    swatch(&mut s, MARGIN, 36.0, 18.0, color(territory.color));
    text(
        &mut s,
        MARGIN + 26.0,
        52.0,
        20.0,
        INK,
        "start",
        true,
        &fit(&territory.name, 20.0, title_width(d, MARGIN + 26.0)),
    );
    let mut subtitle = d.title.clone();
    if !territory.code.is_empty() && territory.code != territory.name {
        subtitle = format!("{} · {subtitle}", territory.code);
    }
    text(
        &mut s,
        MARGIN + 26.0,
        70.0,
        10.0,
        WEAK,
        "start",
        false,
        &subtitle,
    );
    let r = map_rect();
    if let Some(view) = view_on(&d.mesh, &members, r) {
        map_into(&mut s, &[territory_layer(d, t)], &view, &format!("t{t}"));
    }
    frame(&mut s, r);

    // Figures.
    let x = r.max.x + 18.0;
    let right = W - MARGIN;
    let mut y = 100.0;
    let mut row = |s: &mut String, label: &str, value: String, fill: &str| {
        text(s, x, y, 9.5, WEAK, "start", false, label);
        text(s, right, y, 9.5, fill, "end", false, &value);
        y += 16.0;
    };
    if let Some(r) = territory.region.and_then(|r| p.region(r)) {
        row(&mut s, "Region", fit(&r.name, 9.5, 140.0), INK);
    }
    row(&mut s, "Areas", format_value(ts.units as f64), INK);
    row(
        &mut s,
        "Area",
        format!("{} km²", format_value(ts.area)),
        INK,
    );
    if let (Some((name, _)), Some(v)) = (&d.measure, ts.values.first()) {
        row(&mut s, &fit(name, 9.5, 120.0), format_value(*v), INK);
    }
    if let Some(target) = ts.targets.first() {
        row(&mut s, "Target", format_value(*target), INK);
    }
    if let Some(dev) = ts.deviations.first() {
        let fill = if dev.abs() > d.tolerance {
            "#c2410c"
        } else {
            "#15803d"
        };
        row(&mut s, "Deviation", deviation_text(*dev), fill);
    }
    if ts.parts > 1 {
        row(
            &mut s,
            "Pieces",
            format!("{} (not contiguous)", ts.parts),
            "#c2410c",
        );
    }
    if let Some([lon, lat]) = territory.site {
        row(&mut s, "Site", format!("{lat:.4}°, {lon:.4}°"), INK);
    }
    if let Some(r) = ts.site_reach {
        row(
            &mut s,
            "Farthest from the site",
            format!("{} km", format_value(r)),
            INK,
        );
    }
    if !territory.notes.is_empty() {
        row(&mut s, "Notes", fit(&territory.notes, 9.5, 140.0), INK);
    }

    // The largest areas, by the measure (or by size).
    y += 10.0;
    let (heading, key): (String, Box<dyn Fn(u32) -> f64>) = match &d.measure {
        Some((name, values)) => (
            format!("Largest areas by {name}"),
            Box::new(|u: u32| values.get(u as usize).copied().flatten().unwrap_or(0.0)),
        ),
        _ => (
            "Largest areas (km²)".to_owned(),
            Box::new(|u: u32| {
                d.mesh
                    .geometry
                    .areas
                    .get(u as usize)
                    .copied()
                    .unwrap_or(0.0)
            }),
        ),
    };
    text(
        &mut s,
        x,
        y,
        9.5,
        INK,
        "start",
        true,
        &fit(&heading, 9.5, right - x),
    );
    y += 15.0;
    let mut top = members.clone();
    top.sort_by(|&a, &b| key(b).total_cmp(&key(a)));
    let room = ((H - 50.0 - y) / 12.5).max(0.0) as usize;
    for &u in top.iter().take(room) {
        let name = d.names.get(u as usize).cloned().unwrap_or_default();
        text(
            &mut s,
            x,
            y,
            8.5,
            INK,
            "start",
            false,
            &fit(&name, 8.5, right - x - 60.0),
        );
        text(
            &mut s,
            right,
            y,
            8.5,
            INK,
            "end",
            false,
            &format_value(key(u)),
        );
        y += 12.5;
    }
    if top.len() > room {
        text(
            &mut s,
            x,
            y,
            8.0,
            WEAK,
            "start",
            false,
            &format!("and {} more", top.len() - room),
        );
    }
    footer(&mut s, d, page, pages);
    s.push_str("</svg>");
    s
}

/// All pages: the overview, then each territory.
pub fn pages(d: &ReportData) -> Vec<String> {
    let n = d.plan.territories.len().min(d.stats.territories.len());
    let total = n + 1;
    let mut out = vec![overview(d, total)];
    for t in 0..n {
        out.push(territory_page(d, t, t + 2, total));
    }
    out
}
