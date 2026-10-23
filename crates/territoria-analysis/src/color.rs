//! Colours and interpolation (in Oklab, so ramps look even).

use serde::{Deserialize, Serialize};

/// sRGB with straight alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba([0, 0, 0, 0]);
    pub const WHITE: Rgba = Rgba([255, 255, 255, 255]);
    pub const BLACK: Rgba = Rgba([0, 0, 0, 255]);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba([r, g, b, 255])
    }

    pub fn r(self) -> u8 {
        self.0[0]
    }
    pub fn g(self) -> u8 {
        self.0[1]
    }
    pub fn b(self) -> u8 {
        self.0[2]
    }
    pub fn a(self) -> u8 {
        self.0[3]
    }

    pub fn with_alpha(self, a: u8) -> Rgba {
        Rgba([self.0[0], self.0[1], self.0[2], a])
    }

    /// `#rrggbb`, or `#rrggbbaa` when not opaque.
    pub fn to_hex(self) -> String {
        let [r, g, b, a] = self.0;
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }

    /// `#rgb`, `#rrggbb`, `#rrggbbaa` (with or without `#`).
    pub fn from_hex(text: &str) -> Option<Rgba> {
        let h = text.trim().trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            3 => {
                let n = |i: usize| {
                    u8::from_str_radix(h.get(i..i + 1)?, 16)
                        .ok()
                        .map(|v| v * 17)
                };
                Some(Rgba([n(0)?, n(1)?, n(2)?, 255]))
            }
            6 => Some(Rgba([byte(0)?, byte(2)?, byte(4)?, 255])),
            8 => Some(Rgba([byte(0)?, byte(2)?, byte(4)?, byte(6)?])),
            _ => None,
        }
    }

    /// Relative luminance (0 black … 1 white).
    pub fn luminance(self) -> f32 {
        let lin = |c: u8| srgb_to_linear(c as f32 / 255.0);
        0.2126 * lin(self.r()) + 0.7152 * lin(self.g()) + 0.0722 * lin(self.b())
    }

    /// Black or white, whichever reads better on this colour.
    pub fn contrast_text(self) -> Rgba {
        if self.luminance() > 0.35 {
            Rgba::rgb(20, 22, 26)
        } else {
            Rgba::WHITE
        }
    }

    /// Mix towards `other` by `t` (0 = self, 1 = other) in Oklab.
    pub fn mix(self, other: Rgba, t: f32) -> Rgba {
        let (a, b) = (to_oklab(self), to_oklab(other));
        let t = t.clamp(0.0, 1.0);
        let lab = [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ];
        let alpha = self.a() as f32 + (other.a() as f32 - self.a() as f32) * t;
        from_oklab(lab, alpha.round() as u8)
    }

    /// Lighter (positive) or darker (negative) by `amount` of Oklab lightness.
    pub fn lighten(self, amount: f32) -> Rgba {
        let mut lab = to_oklab(self);
        lab[0] = (lab[0] + amount).clamp(0.0, 1.0);
        from_oklab(lab, self.a())
    }

    /// Hue in degrees, chroma and lightness in Oklch.
    pub fn oklch(self) -> (f32, f32, f32) {
        let [l, a, b] = to_oklab(self);
        (b.atan2(a).to_degrees().rem_euclid(360.0), a.hypot(b), l)
    }

    pub fn from_oklch(hue: f32, chroma: f32, lightness: f32) -> Rgba {
        let h = hue.to_radians();
        from_oklab([lightness, chroma * h.cos(), chroma * h.sin()], 255)
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn to_oklab(c: Rgba) -> [f32; 3] {
    let r = srgb_to_linear(c.r() as f32 / 255.0);
    let g = srgb_to_linear(c.g() as f32 / 255.0);
    let b = srgb_to_linear(c.b() as f32 / 255.0);
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn from_oklab(lab: [f32; 3], alpha: u8) -> Rgba {
    let l = lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2];
    let m = lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2];
    let s = lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2];
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    let r = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
    let g = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s;
    let b = -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s;
    let to8 = |v: f32| (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8;
    Rgba([to8(r), to8(g), to8(b), alpha])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        assert_eq!(Rgba::from_hex("#3b82f6"), Some(Rgba::rgb(0x3b, 0x82, 0xf6)));
        assert_eq!(Rgba::from_hex("fff"), Some(Rgba::WHITE));
        assert_eq!(Rgba::from_hex("#88ff000080").map(|_| ()), None);
        assert_eq!(Rgba::from_hex("#ff000080"), Some(Rgba([255, 0, 0, 128])));
        assert_eq!(Rgba([255, 0, 0, 128]).to_hex(), "#ff000080");
    }

    #[test]
    fn oklab_round_trip_and_mix() {
        for c in [
            Rgba::rgb(12, 200, 99),
            Rgba::rgb(255, 255, 255),
            Rgba::rgb(0, 0, 0),
            Rgba::rgb(240, 20, 180),
        ] {
            let back = from_oklab(to_oklab(c), 255);
            for i in 0..3 {
                assert!(
                    (back.0[i] as i32 - c.0[i] as i32).abs() <= 1,
                    "{c:?} → {back:?}"
                );
            }
        }
        assert_eq!(Rgba::BLACK.mix(Rgba::WHITE, 0.0), Rgba::BLACK);
        assert_eq!(Rgba::BLACK.mix(Rgba::WHITE, 1.0), Rgba::WHITE);
        let mid = Rgba::BLACK.mix(Rgba::WHITE, 0.5);
        assert!(mid.r() > 90 && mid.r() < 140, "{mid:?}");
    }

    #[test]
    fn contrast() {
        assert_eq!(
            Rgba::rgb(250, 250, 200).contrast_text(),
            Rgba::rgb(20, 22, 26)
        );
        assert_eq!(Rgba::rgb(20, 40, 120).contrast_text(), Rgba::WHITE);
    }
}
