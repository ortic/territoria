//! The map viewport: display coordinates ↔ screen points.

use egui::{Pos2, Rect, Vec2, pos2};

/// What part of the map is on screen. Display y grows north, screen y
/// grows down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Display coordinates at the centre of `rect`.
    pub center: [f64; 2],
    /// Display units per screen point.
    pub scale: f64,
    pub rect: Rect,
}

impl View {
    pub fn to_screen(&self, p: [f64; 2]) -> Pos2 {
        let c = self.rect.center();
        pos2(
            c.x + ((p[0] - self.center[0]) / self.scale) as f32,
            c.y - ((p[1] - self.center[1]) / self.scale) as f32,
        )
    }

    pub fn to_display(&self, s: Pos2) -> [f64; 2] {
        let c = self.rect.center();
        [
            self.center[0] + (s.x - c.x) as f64 * self.scale,
            self.center[1] - (s.y - c.y) as f64 * self.scale,
        ]
    }

    /// Visible display box: (min, max).
    pub fn visible(&self) -> ([f64; 2], [f64; 2]) {
        let a = self.to_display(self.rect.left_bottom());
        let b = self.to_display(self.rect.right_top());
        (a, b)
    }

    /// A view showing `min`…`max` in `rect` with a margin.
    pub fn fit(min: [f64; 2], max: [f64; 2], rect: Rect) -> View {
        let w = (max[0] - min[0]).max(1e-9);
        let h = (max[1] - min[1]).max(1e-9);
        let scale = (w / (rect.width() as f64 * 0.92)).max(h / (rect.height() as f64 * 0.92));
        View {
            center: [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0],
            scale: if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            },
            rect,
        }
    }

    /// Zooms by `factor` (>1 = in) keeping `at` fixed on screen.
    pub fn zoom_at(&mut self, at: Pos2, factor: f64) {
        let before = self.to_display(at);
        self.scale = (self.scale / factor).clamp(1e-6, 1e9);
        let after = self.to_display(at);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
    }

    /// Moves the map with the pointer by `delta` screen points.
    pub fn pan(&mut self, delta: Vec2) {
        self.center[0] -= delta.x as f64 * self.scale;
        self.center[1] += delta.y as f64 * self.scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_zoom() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(200.0, 100.0));
        let mut v = View::fit([0.0, 0.0], [1000.0, 500.0], rect);
        let p = [250.0, 400.0];
        let s = v.to_screen(p);
        let back = v.to_display(s);
        assert!((back[0] - p[0]).abs() < 1e-3 && (back[1] - p[1]).abs() < 1e-3);
        // North is up.
        assert!(v.to_screen([0.0, 500.0]).y < v.to_screen([0.0, 0.0]).y);
        let anchor = pos2(50.0, 20.0);
        let before = v.to_display(anchor);
        v.zoom_at(anchor, 2.0);
        let after = v.to_display(anchor);
        assert!((before[0] - after[0]).abs() < 1e-6 && (before[1] - after[1]).abs() < 1e-6);
        v.pan(egui::vec2(10.0, 0.0));
        assert!(v.to_display(anchor)[0] < after[0]);
    }
}
