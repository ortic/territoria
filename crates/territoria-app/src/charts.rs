//! Charts on the map: pies, donuts and bars at a point, as egui shapes
//! and as SVG, from the same outlines.

use std::fmt::Write;

use egui::{Color32, Pos2, Rect, Shape, Stroke, pos2, vec2};
use territoria_model::ChartShape;

use crate::resolve::Chart;

/// Points along a circle from angle `a0` to `a1` (radians, clockwise from
/// 12 o'clock), both ends included.
fn arc(c: Pos2, r: f32, a0: f32, a1: f32, steps: usize) -> Vec<Pos2> {
    (0..=steps)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / steps as f32;
            pos2(c.x + r * a.sin(), c.y - r * a.cos())
        })
        .collect()
}

/// A pie or donut's slices as closed outlines with their colours; parts
/// are shares that add up to 1.
pub fn slices(c: Pos2, r: f32, inner: f32, parts: &[(f32, Color32)]) -> Vec<(Vec<Pos2>, Color32)> {
    let mut out = Vec::new();
    let mut a = 0.0f32;
    for &(share, color) in parts {
        if share <= 0.0 {
            continue;
        }
        let b = a + share * std::f32::consts::TAU;
        // Both arcs of a donut slice have as many points (see `fill`).
        let steps = (((b - a) * r / 3.0).ceil() as usize).clamp(2, 96);
        let mut outline = arc(c, r, a, b, steps);
        if inner > 0.0 {
            let mut back = arc(c, inner, a, b, steps);
            back.reverse();
            outline.extend(back);
        } else {
            outline.insert(0, c);
        }
        out.push((outline, color));
        a = b;
    }
    out
}

/// Bars side by side, standing on a line through the middle of the
/// chart's height; parts are heights as fractions of `size`.
pub fn bars(c: Pos2, size: f32, parts: &[(f32, Color32)]) -> Vec<(Rect, Color32)> {
    let n = parts.len().max(1) as f32;
    let w = (size * 0.32).clamp(3.0, 9.0);
    let gap = 1.0;
    let left = c.x - (n * w + (n - 1.0) * gap) / 2.0;
    let bottom = c.y + size / 2.0;
    parts
        .iter()
        .enumerate()
        .map(|(i, &(h, color))| {
            let x = left + i as f32 * (w + gap);
            (
                Rect::from_min_max(pos2(x, bottom - size * h.max(0.0)), pos2(x + w, bottom)),
                color,
            )
        })
        .collect()
}

/// One bar with the parts stacked from the bottom; parts are heights as
/// fractions of `size`.
pub fn stacked(c: Pos2, size: f32, parts: &[(f32, Color32)]) -> Vec<(Rect, Color32)> {
    let w = (size * 0.45).clamp(5.0, 12.0);
    let mut bottom = c.y + size / 2.0;
    parts
        .iter()
        .filter(|(h, _)| *h > 0.0)
        .map(|&(h, color)| {
            let top = bottom - size * h;
            let r = Rect::from_min_max(pos2(c.x - w / 2.0, top), pos2(c.x + w / 2.0, bottom));
            bottom = top;
            (r, color)
        })
        .collect()
}

/// The rectangles of a bar chart of either kind.
fn rects(chart: &Chart, c: Pos2) -> Vec<(Rect, Color32)> {
    if chart.shape == ChartShape::Stacked {
        stacked(c, chart.size, &chart.parts)
    } else {
        bars(c, chart.size, &chart.parts)
    }
}

/// The space a chart takes on screen.
pub fn bounds(chart: &Chart, c: Pos2) -> Rect {
    match chart.shape {
        ChartShape::Bars | ChartShape::Stacked => rects(chart, c)
            .iter()
            .fold(Rect::from_center_size(c, vec2(1.0, chart.size)), |r, b| {
                r.union(b.0)
            }),
        _ => Rect::from_center_size(c, vec2(2.0, 2.0) * chart.size),
    }
}

/// Fills an outline that is star-shaped around its first point (pie
/// slices) or a band of two arcs (donut slices).
fn fill(mesh: &mut egui::Mesh, outline: &[Pos2], color: Color32, donut: bool) {
    let base = mesh.vertices.len() as u32;
    for p in outline {
        mesh.colored_vertex(*p, color);
    }
    let n = outline.len() as u32;
    if donut {
        // Outer arc then inner arc backwards: pair point i with n-1-i.
        let half = n / 2;
        for i in 0..half.saturating_sub(1) {
            let (o0, o1) = (base + i, base + i + 1);
            let (i0, i1) = (base + n - 1 - i, base + n - 2 - i);
            mesh.add_triangle(o0, o1, i1);
            mesh.add_triangle(o0, i1, i0);
        }
    } else {
        for i in 1..n.saturating_sub(1) {
            mesh.add_triangle(base, base + i, base + i + 1);
        }
    }
}

/// The chart as egui shapes at `c`.
pub fn shapes(out: &mut Vec<Shape>, chart: &Chart, c: Pos2) {
    match chart.shape {
        ChartShape::Bars | ChartShape::Stacked => {
            let rects = rects(chart, c);
            for (r, color) in &rects {
                out.push(Shape::rect_filled(*r, 0.0, *color));
                out.push(Shape::rect_stroke(
                    *r,
                    0.0,
                    Stroke::new(0.5, chart.outline.color),
                    egui::StrokeKind::Outside,
                ));
            }
            if let (Some(first), Some(last)) = (rects.first(), rects.last()) {
                let y = first.0.bottom();
                out.push(Shape::line_segment(
                    [pos2(first.0.left() - 2.0, y), pos2(last.0.right() + 2.0, y)],
                    Stroke::new(1.0, Color32::from_rgb(0x55, 0x58, 0x60)),
                ));
            }
        }
        shape => {
            let inner = if shape == ChartShape::Donut {
                chart.size * 0.5
            } else {
                0.0
            };
            let mut mesh = egui::Mesh::default();
            let parts = slices(c, chart.size, inner, &chart.parts);
            for (outline, color) in &parts {
                fill(&mut mesh, outline, *color, inner > 0.0);
            }
            out.push(Shape::mesh(mesh));
            // Lines between slices and around the whole.
            if parts.len() > 1 {
                for (outline, _) in &parts {
                    let from = if inner > 0.0 {
                        outline[outline.len() - 1]
                    } else {
                        c
                    };
                    let to = outline[if inner > 0.0 { 0 } else { 1 }];
                    out.push(Shape::line_segment([from, to], chart.outline));
                }
            }
            out.push(Shape::circle_stroke(c, chart.size, chart.outline));
            if inner > 0.0 {
                out.push(Shape::circle_stroke(c, inner, chart.outline));
            }
        }
    }
}

fn hex(c: Color32) -> (String, f32) {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    (format!("#{r:02x}{g:02x}{b:02x}"), a as f32 / 255.0)
}

/// The chart as SVG at `c`.
pub fn svg(s: &mut String, chart: &Chart, c: Pos2, tip: &str) {
    let (oc, oa) = hex(chart.outline.color);
    let w = chart.outline.width;
    let _ = write!(s, "<g{tip}>");
    match chart.shape {
        ChartShape::Bars | ChartShape::Stacked => {
            let rects = rects(chart, c);
            for (r, color) in &rects {
                let (fc, fa) = hex(*color);
                let _ = write!(
                    s,
                    r#"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="{fc}" fill-opacity="{fa:.3}" stroke="{oc}" stroke-opacity="{oa:.3}" stroke-width="0.5"/>"#,
                    r.left(),
                    r.top(),
                    r.width(),
                    r.height()
                );
            }
            if let (Some(first), Some(last)) = (rects.first(), rects.last()) {
                let y = first.0.bottom();
                let _ = write!(
                    s,
                    r##"<line x1="{:.2}" y1="{y:.2}" x2="{:.2}" y2="{y:.2}" stroke="#555860" stroke-width="1"/>"##,
                    first.0.left() - 2.0,
                    last.0.right() + 2.0
                );
            }
        }
        shape => {
            let inner = if shape == ChartShape::Donut {
                chart.size * 0.5
            } else {
                0.0
            };
            let parts = slices(c, chart.size, inner, &chart.parts);
            if parts.len() == 1 && inner == 0.0 {
                // One value: a full circle (a closed slice would show a seam).
                let (fc, fa) = hex(parts[0].1);
                let _ = write!(
                    s,
                    r#"<circle cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{fc}" fill-opacity="{fa:.3}" stroke="{oc}" stroke-opacity="{oa:.3}" stroke-width="{w:.2}"/>"#,
                    c.x, c.y, chart.size
                );
            } else {
                for (outline, color) in &parts {
                    let mut d = String::new();
                    for (k, p) in outline.iter().enumerate() {
                        let _ =
                            write!(d, "{}{:.2} {:.2}", if k == 0 { "M" } else { "L" }, p.x, p.y);
                    }
                    let (fc, fa) = hex(*color);
                    let _ = write!(
                        s,
                        r#"<path d="{d}Z" fill="{fc}" fill-opacity="{fa:.3}" stroke="{oc}" stroke-opacity="{oa:.3}" stroke-width="{w:.2}" stroke-linejoin="round"/>"#
                    );
                }
            }
        }
    }
    s.push_str("</g>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_and_bars() {
        let c = pos2(50.0, 50.0);
        let red = Color32::RED;
        let blue = Color32::BLUE;
        let pie = slices(c, 10.0, 0.0, &[(0.25, red), (0.75, blue)]);
        assert_eq!(pie.len(), 2);
        // The first slice runs from 12 to 3 o'clock.
        let first = &pie[0].0;
        assert_eq!(first[0], c);
        assert!((first[1] - pos2(50.0, 40.0)).length() < 1e-3);
        assert!((first.last().unwrap().to_owned() - pos2(60.0, 50.0)).length() < 1e-3);
        let donut = slices(c, 10.0, 5.0, &[(1.0, red)]);
        assert!(donut[0].0.iter().all(|p| {
            let d = (*p - c).length();
            (4.9..=10.1).contains(&d)
        }));
        let b = bars(c, 20.0, &[(1.0, red), (0.5, blue)]);
        assert_eq!(b[0].0.bottom(), 60.0);
        assert_eq!(b[0].0.top(), 40.0);
        assert_eq!(b[1].0.top(), 50.0);
        assert!(b[0].0.right() < b[1].0.left());
        // Stacked: the second part sits on the first.
        let s = stacked(c, 20.0, &[(0.5, red), (0.25, blue)]);
        assert_eq!(s[0].0.bottom(), 60.0);
        assert_eq!(s[0].0.top(), 50.0);
        assert_eq!(s[1].0.bottom(), 50.0);
        assert_eq!(s[1].0.top(), 45.0);
    }
}
