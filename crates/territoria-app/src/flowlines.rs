//! Flow lines on the map: straight or bent, with an arrowhead, as egui
//! shapes and as SVG from the same outline.

use std::fmt::Write;

use egui::{Pos2, Shape, Stroke, pos2, vec2};
use territoria_render::View;

use crate::resolve::Flow;

/// The line's points on screen and its arrowhead (tip and two corners).
pub fn outline(flow: &Flow, view: &View) -> (Vec<Pos2>, Option<[Pos2; 3]>) {
    let (a, b) = (view.to_screen(flow.from), view.to_screen(flow.to));
    let d = b - a;
    let len = d.length();
    if len < 1.0 {
        return (Vec::new(), None);
    }
    // Bent to the left of the direction, so flows both ways stay apart.
    let bend = if flow.curved { 0.18 } else { 0.0 };
    let control = a + d * 0.5 + vec2(d.y, -d.x) * bend;
    let steps = if flow.curved {
        ((len / 12.0).ceil() as usize).clamp(4, 32)
    } else {
        1
    };
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let u = 1.0 - t;
            pos2(
                u * u * a.x + 2.0 * u * t * control.x + t * t * b.x,
                u * u * a.y + 2.0 * u * t * control.y + t * t * b.y,
            )
        })
        .collect();
    let arrow = flow.arrow.then(|| {
        let dir = (b - if flow.curved { control } else { a }).normalized();
        let size = (flow.width * 2.2).max(6.0).min(len * 0.4);
        let base = b - dir * size;
        let side = vec2(-dir.y, dir.x) * size * 0.5;
        [b, base + side, base - side]
    });
    (points, arrow)
}

pub fn shapes(out: &mut Vec<Shape>, flow: &Flow, view: &View) {
    let (points, arrow) = outline(flow, view);
    if points.len() < 2 {
        return;
    }
    out.push(Shape::line(points, Stroke::new(flow.width, flow.color)));
    if let Some(tri) = arrow {
        out.push(Shape::convex_polygon(
            tri.to_vec(),
            flow.color,
            Stroke::NONE,
        ));
    }
}

fn hex(c: egui::Color32) -> (String, f32) {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    (format!("#{r:02x}{g:02x}{b:02x}"), a as f32 / 255.0)
}

pub fn svg(s: &mut String, flow: &Flow, view: &View) {
    let (points, arrow) = outline(flow, view);
    if points.len() < 2 {
        return;
    }
    let (c, a) = hex(flow.color);
    let mut d = String::new();
    for (k, p) in points.iter().enumerate() {
        let _ = write!(d, "{}{:.2} {:.2}", if k == 0 { "M" } else { "L" }, p.x, p.y);
    }
    let _ = write!(
        s,
        r#"<path d="{d}" fill="none" stroke="{c}" stroke-opacity="{a:.3}" stroke-width="{:.2}" stroke-linecap="round"/>"#,
        flow.width
    );
    if let Some([t, l, r]) = arrow {
        let _ = write!(
            s,
            r#"<path d="M{:.2} {:.2}L{:.2} {:.2}L{:.2} {:.2}Z" fill="{c}" fill-opacity="{a:.3}"/>"#,
            t.x, t.y, l.x, l.y, r.x, r.y
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bends_left_and_points_at_the_end() {
        let view = View {
            center: [0.0, 0.0],
            scale: 1.0,
            rect: egui::Rect::from_center_size(Pos2::ZERO, vec2(400.0, 400.0)),
        };
        let flow = Flow {
            from: [-100.0, 0.0],
            to: [100.0, 0.0],
            width: 2.0,
            color: egui::Color32::RED,
            curved: true,
            arrow: true,
        };
        let (points, arrow) = outline(&flow, &view);
        let (a, b) = (points[0], *points.last().unwrap());
        assert!((a.x + 100.0).abs() < 1e-3 && (b.x - 100.0).abs() < 1e-3);
        // Left of a rightward line is up on screen.
        let mid = points[points.len() / 2];
        assert!(mid.y < -10.0, "{mid:?}");
        let [tip, ..] = arrow.unwrap();
        assert_eq!(tip, b);
    }
}
