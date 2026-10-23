//! Heat maps: point density (optionally weighted) as a smooth colour
//! field, computed for a view on a coarse grid of screen cells.

use egui::{Color32, ColorImage, Pos2, Rect};
use territoria_analysis::RampSpec;

/// Screen points per grid cell.
pub const CELL: f32 = 3.0;

/// The density image for points (screen positions and weights) in `rect`,
/// each spread over `radius` screen points (quartic kernel), coloured by
/// `ramp` relative to the densest cell; thin densities fade out.
pub fn raster(
    points: &[(Pos2, f32)],
    rect: Rect,
    radius: f32,
    ramp: &RampSpec,
    cell: f32,
) -> ColorImage {
    let w = (rect.width() / cell).ceil().max(1.0) as usize;
    let h = (rect.height() / cell).ceil().max(1.0) as usize;
    let mut grid = vec![0.0f32; w * h];
    let r = (radius / cell).max(1.0);
    let ri = r.ceil() as i64;
    for &(p, weight) in points {
        if weight <= 0.0 {
            continue;
        }
        let (cx, cy) = ((p.x - rect.left()) / cell, (p.y - rect.top()) / cell);
        if cx < -r || cy < -r || cx > w as f32 + r || cy > h as f32 + r {
            continue;
        }
        let (x0, y0) = (cx.floor() as i64, cy.floor() as i64);
        for dy in -ri..=ri {
            let y = y0 + dy;
            if y < 0 || y >= h as i64 {
                continue;
            }
            for dx in -ri..=ri {
                let x = x0 + dx;
                if x < 0 || x >= w as i64 {
                    continue;
                }
                let (fx, fy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let d2 = (fx * fx + fy * fy) / (r * r);
                if d2 < 1.0 {
                    let k = 1.0 - d2;
                    grid[y as usize * w + x as usize] += weight * k * k;
                }
            }
        }
    }
    let max = grid.iter().copied().fold(0.0f32, f32::max).max(1e-9);
    let stops = ramp.colors(9);
    let pixels = grid
        .iter()
        .map(|&v| {
            let t = v / max;
            if t < 0.02 {
                return Color32::TRANSPARENT;
            }
            let c = territoria_analysis::ramps::sample(&stops, t);
            let [r, g, b, _] = c.0;
            // Thin densities are see-through, dense ones solid.
            Color32::from_rgba_unmultiplied(r, g, b, (60.0 + 190.0 * t.sqrt()) as u8)
        })
        .collect();
    ColorImage::new([w, h], pixels)
}

/// The points of a layer's mesh on screen with their weights.
pub fn screen_points(
    mesh: &territoria_render::LayerMesh,
    view: &territoria_render::View,
    weights: Option<&territoria_data::Column>,
) -> Vec<(Pos2, f32)> {
    mesh.label_points
        .iter()
        .enumerate()
        .filter_map(|(f, lp)| {
            let p = view.to_screen(mesh.to_display((*lp)?));
            let w = weights.map_or(Some(1.0), |c| c.f64(f))?;
            Some((p, w as f32))
        })
        .collect()
}

/// `km` as screen points at the view's centre.
pub fn radius_px(view: &territoria_render::View, crs: &territoria_geo::Crs, km: f64) -> f32 {
    use territoria_geo::{Crs, Transformer};
    let (Ok(to_wgs), Ok(to_display)) = (
        Transformer::new(crs, &Crs::wgs84()),
        Transformer::new(&Crs::wgs84(), crs),
    ) else {
        return 20.0;
    };
    let c = view.center;
    let Some(ll) = to_wgs.convert(geo_types::Coord { x: c[0], y: c[1] }) else {
        return 20.0;
    };
    let north = geo_types::Coord {
        x: ll.x,
        y: (ll.y + km / 111.2).min(89.0),
    };
    to_display
        .convert(north)
        .map_or(20.0, |d| ((d.y - c[1]).abs() / view.scale) as f32)
        .max(2.0)
}

/// The default ramp for heat maps.
pub fn default_ramp() -> RampSpec {
    RampSpec::named("Yellow-Orange-Red")
}

/// Bytes as base64 (for images inside SVG).
pub fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            A[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            A[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The image as a PNG data URI.
pub fn png_data_uri(image: &ColorImage) -> Option<String> {
    let [w, h] = image.size;
    let rgba: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|c| c.to_srgba_unmultiplied())
        .collect();
    let buffer = image::RgbaImage::from_raw(w as u32, h as u32, rgba)?;
    let mut bytes = Vec::new();
    buffer
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .ok()?;
    Some(format!("data:image/png;base64,{}", base64(&bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_peaks_where_points_are() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(90.0, 30.0));
        let pts = [
            (Pos2::new(15.0, 15.0), 1.0),
            (Pos2::new(16.0, 15.0), 1.0),
            (Pos2::new(75.0, 15.0), 1.0),
        ];
        let img = raster(&pts, rect, 9.0, &default_ramp(), 3.0);
        assert_eq!(img.size, [30, 10]);
        let alpha = |x: usize, y: usize| img.pixels[y * 30 + x].a();
        assert!(alpha(5, 5) > alpha(25, 5), "two points denser than one");
        assert_eq!(alpha(15, 5), 0, "nothing between");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert!(
            png_data_uri(&img)
                .unwrap()
                .starts_with("data:image/png;base64,iVBOR")
        );
    }
}
