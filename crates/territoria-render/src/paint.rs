//! Drawing layer meshes with egui: fills as one big mesh, borders and
//! lines as polylines, labels with collision avoidance.

use egui::epaint::{Mesh, PathShape, Vertex, WHITE_UV};
use egui::{Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::mesh::{Bounds, LayerMesh, Level};
use crate::view::View;

/// The visible part of the map as a mesh-relative box, plus the transform.
struct Frame {
    origin_screen: [f64; 2],
    k: f64,
    visible: Bounds,
}

impl Frame {
    fn new(mesh: &LayerMesh, view: &View) -> Frame {
        let c = view.rect.center();
        let k = 1.0 / view.scale;
        let origin_screen = [
            c.x as f64 + (mesh.origin[0] - view.center[0]) * k,
            c.y as f64 - (mesh.origin[1] - view.center[1]) * k,
        ];
        let (min, max) = view.visible();
        let visible = Bounds {
            min: [
                (min[0] - mesh.origin[0]) as f32,
                (min[1] - mesh.origin[1]) as f32,
            ],
            max: [
                (max[0] - mesh.origin[0]) as f32,
                (max[1] - mesh.origin[1]) as f32,
            ],
        };
        Frame {
            origin_screen,
            k,
            visible,
        }
    }

    #[inline]
    fn screen(&self, p: [f32; 2]) -> Pos2 {
        pos2(
            (self.origin_screen[0] + p[0] as f64 * self.k) as f32,
            (self.origin_screen[1] - p[1] as f64 * self.k) as f32,
        )
    }
}

/// The level of detail for the view (physical pixels decide).
pub fn level<'a>(mesh: &'a LayerMesh, view: &View, pixels_per_point: f32) -> &'a Level {
    mesh.level_for(view.scale / pixels_per_point as f64)
}

/// Fills areas with the colour `color(feature)` gives (`None` = not drawn).
pub fn fills(
    shapes: &mut Vec<Shape>,
    mesh: &LayerMesh,
    level: &Level,
    view: &View,
    color: impl Fn(usize) -> Option<Color32>,
) {
    let f = Frame::new(mesh, view);
    let mut m = Mesh::default();
    for (i, fill) in level.fills.iter().enumerate() {
        if fill.indices.is_empty() || !mesh.bounds[i].intersects(&f.visible) {
            continue;
        }
        let Some(c) = color(i) else { continue };
        if c.a() == 0 {
            continue;
        }
        // Split before u32 indices of very large meshes get unwieldy.
        if m.vertices.len() > 4_000_000 {
            shapes.push(Shape::mesh(std::mem::take(&mut m)));
        }
        let base = m.vertices.len() as u32;
        m.vertices.extend(fill.vertices.iter().map(|&p| Vertex {
            pos: f.screen(p),
            uv: WHITE_UV,
            color: c,
        }));
        m.indices.extend(fill.indices.iter().map(|i| base + i));
    }
    if !m.is_empty() {
        shapes.push(Shape::mesh(m));
    }
}

/// Draws topology arcs with the stroke `style(arc index)` gives.
pub fn arcs(
    shapes: &mut Vec<Shape>,
    mesh: &LayerMesh,
    level: &Level,
    view: &View,
    style: impl Fn(usize) -> Option<Stroke>,
) {
    let f = Frame::new(mesh, view);
    for (i, pts) in level.arcs.iter().enumerate() {
        if pts.len() < 2 || !level.arc_bounds[i].intersects(&f.visible) {
            continue;
        }
        let Some(stroke) = style(i) else {
            continue;
        };
        if stroke.width <= 0.0 || stroke.color.a() == 0 {
            continue;
        }
        shapes.push(Shape::Path(PathShape::line(
            pts.iter().map(|&p| f.screen(p)).collect(),
            stroke,
        )));
    }
}

/// Draws line features.
pub fn lines(
    shapes: &mut Vec<Shape>,
    mesh: &LayerMesh,
    level: &Level,
    view: &View,
    style: impl Fn(usize) -> Option<Stroke>,
) {
    let f = Frame::new(mesh, view);
    for (feature, range) in level.line_parts.iter().enumerate() {
        if !mesh.bounds[feature].intersects(&f.visible) {
            continue;
        }
        let Some(stroke) = style(feature) else {
            continue;
        };
        for part in &level.arcs[range.start as usize..range.end as usize] {
            if part.len() >= 2 {
                shapes.push(Shape::Path(PathShape::line(
                    part.iter().map(|&p| f.screen(p)).collect(),
                    stroke,
                )));
            }
        }
    }
}

/// Draws point features as circles.
pub fn points(
    shapes: &mut Vec<Shape>,
    mesh: &LayerMesh,
    view: &View,
    style: impl Fn(usize) -> Option<(f32, Color32, Stroke)>,
) {
    let f = Frame::new(mesh, view);
    for (feature, pts) in mesh.points.iter().enumerate() {
        if mesh
            .bounds
            .get(feature)
            .is_none_or(|b| !b.intersects(&f.visible))
        {
            continue;
        }
        let Some((radius, fill, stroke)) = style(feature) else {
            continue;
        };
        for &p in pts {
            shapes.push(Shape::circle_filled(f.screen(p), radius, fill));
            if stroke.width > 0.0 {
                shapes.push(Shape::circle_stroke(f.screen(p), radius, stroke));
            }
        }
    }
}

/// Outline of one feature (hover, selection).
pub fn outline(
    shapes: &mut Vec<Shape>,
    mesh: &LayerMesh,
    level: &Level,
    view: &View,
    feature: usize,
    stroke: Stroke,
) {
    let f = Frame::new(mesh, view);
    match mesh.kind {
        territoria_io::GeometryKind::Area => {
            for ring in mesh.rings(level, feature) {
                shapes.push(Shape::Path(PathShape::closed_line(
                    ring.iter().map(|&p| f.screen(p)).collect(),
                    stroke,
                )));
            }
        }
        territoria_io::GeometryKind::Line => {
            if let Some(range) = level.line_parts.get(feature) {
                for part in &level.arcs[range.start as usize..range.end as usize] {
                    shapes.push(Shape::Path(PathShape::line(
                        part.iter().map(|&p| f.screen(p)).collect(),
                        stroke,
                    )));
                }
            }
        }
        territoria_io::GeometryKind::Point => {
            for &p in mesh
                .points
                .get(feature)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                shapes.push(Shape::circle_stroke(f.screen(p), 6.0, stroke));
            }
        }
    }
}

/// Fill of one feature in a single colour (highlights).
pub fn highlight(
    shapes: &mut Vec<Shape>,
    mesh: &LayerMesh,
    level: &Level,
    view: &View,
    features: &[u32],
    color: Color32,
) {
    let set: std::collections::HashSet<usize> = features.iter().map(|&f| f as usize).collect();
    fills(shapes, mesh, level, view, |i| {
        set.contains(&i).then_some(color)
    });
}

/// Places labels at features' label points where they fit and don't
/// overlap labels placed before. `text(feature)` gives the label and
/// whether it's important (placed first).
pub struct LabelStyle {
    pub font: FontId,
    pub color: Color32,
    pub halo: Color32,
    /// Only label areas at least this many points wide on screen.
    pub min_width: f32,
}

pub fn labels(
    painter: &egui::Painter,
    mesh: &LayerMesh,
    view: &View,
    style: &LabelStyle,
    occupied: &mut Vec<Rect>,
    text: impl Fn(usize) -> Option<String>,
) {
    let base = style.font.size;
    labels_sized(painter, mesh, view, style, occupied, text, |_| base);
}

/// Labels of their own sizes (font size in points per feature): bigger
/// ones are placed first.
pub fn labels_sized(
    painter: &egui::Painter,
    mesh: &LayerMesh,
    view: &View,
    style: &LabelStyle,
    occupied: &mut Vec<Rect>,
    text: impl Fn(usize) -> Option<String>,
    size: impl Fn(usize) -> f32,
) {
    let f = Frame::new(mesh, view);
    // Bigger areas first: they get the space.
    let mut order: Vec<(usize, f32)> = mesh
        .label_points
        .iter()
        .enumerate()
        .filter_map(|(i, lp)| {
            let lp = (*lp)?;
            let b = &mesh.bounds[i];
            if !b.intersects(&f.visible) {
                return None;
            }
            let p = f.screen(lp);
            if !view.rect.contains(p) {
                return None;
            }
            let w = b.width() as f64 * f.k;
            let w = if mesh.kind == territoria_io::GeometryKind::Area {
                w as f32
            } else {
                f32::MAX
            };
            (w >= style.min_width).then_some((i, w))
        })
        .collect();
    order.sort_by(|a, b| size(b.0).total_cmp(&size(a.0)).then(b.1.total_cmp(&a.1)));
    let max_labels = 600;
    let mut placed = 0;
    for (i, width) in order {
        if placed >= max_labels {
            break;
        }
        let Some(t) = text(i) else { continue };
        if t.is_empty() {
            continue;
        }
        let font = egui::FontId::new(size(i), style.font.family.clone());
        let galley = painter.layout_no_wrap(t, font, style.color);
        let size = galley.size();
        if mesh.kind == territoria_io::GeometryKind::Area && size.x > width * 1.15 {
            continue;
        }
        let at = f.screen(mesh.label_points[i].unwrap());
        let rect = Align2::CENTER_CENTER.anchor_size(at, size).expand(2.0);
        if occupied.iter().any(|r| r.intersects(rect)) {
            continue;
        }
        occupied.push(rect);
        placed += 1;
        let pos = rect.min + vec2(2.0, 2.0);
        // A thin halo keeps labels readable on any fill.
        for d in [
            vec2(-1.0, 0.0),
            vec2(1.0, 0.0),
            vec2(0.0, -1.0),
            vec2(0.0, 1.0),
        ] {
            painter.galley_with_override_text_color(pos + d, galley.clone(), style.halo);
        }
        painter.galley(pos, galley, style.color);
    }
}
