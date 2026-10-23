//! Colour ramps. The named ones are ColorBrewer's (Cynthia Brewer, Mark
//! Harrower, Penn State; Apache 2.0) plus Viridis and Tableau 10.

use serde::{Deserialize, Serialize};

use crate::color::Rgba;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampKind {
    /// Light to dark: amounts.
    Sequential,
    /// Two hues around a neutral middle: above/below a reference.
    Diverging,
    /// Distinct hues: categories.
    Qualitative,
}

#[derive(Debug)]
pub struct Ramp {
    pub name: &'static str,
    pub kind: RampKind,
    pub stops: &'static [u32],
    /// Readable with the common forms of colour blindness.
    pub colorblind_safe: bool,
}

const fn hex(v: u32) -> Rgba {
    Rgba([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

impl Ramp {
    pub fn stop_colors(&self) -> Vec<Rgba> {
        self.stops.iter().map(|&v| hex(v)).collect()
    }

    /// The colour at `t` (0..1) along the ramp.
    pub fn sample(&self, t: f32) -> Rgba {
        sample(&self.stop_colors(), t)
    }

    /// `n` colours for `n` classes or categories.
    pub fn colors(&self, n: usize) -> Vec<Rgba> {
        let stops = self.stop_colors();
        match self.kind {
            RampKind::Qualitative => (0..n).map(|i| stops[i % stops.len()]).collect(),
            // Skip the near-white end of light-to-dark ramps.
            RampKind::Sequential => spread(&stops, n, 0.12, 1.0),
            RampKind::Diverging => spread(&stops, n, 0.0, 1.0),
        }
    }
}

fn spread(stops: &[Rgba], n: usize, from: f32, to: f32) -> Vec<Rgba> {
    match n {
        0 => Vec::new(),
        1 => vec![sample(stops, (from + to) / 2.0 + 0.2)],
        _ => (0..n)
            .map(|i| sample(stops, from + (to - from) * i as f32 / (n - 1) as f32))
            .collect(),
    }
}

/// Linear interpolation (in Oklab) between evenly spaced stops.
pub fn sample(stops: &[Rgba], t: f32) -> Rgba {
    match stops.len() {
        0 => Rgba::TRANSPARENT,
        1 => stops[0],
        n => {
            let x = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let i = (x.floor() as usize).min(n - 2);
            stops[i].mix(stops[i + 1], x - i as f32)
        }
    }
}

macro_rules! ramps {
    ($($name:literal, $kind:ident, $safe:literal, [$($c:literal),*];)*) => {
        pub static RAMPS: &[Ramp] = &[
            $(Ramp { name: $name, kind: RampKind::$kind, stops: &[$($c),*], colorblind_safe: $safe },)*
        ];
    };
}

ramps! {
    "Blues", Sequential, true, [0xf7fbff, 0xdeebf7, 0xc6dbef, 0x9ecae1, 0x6baed6, 0x4292c6, 0x2171b5, 0x08519c, 0x08306b];
    "Greens", Sequential, true, [0xf7fcf5, 0xe5f5e0, 0xc7e9c0, 0xa1d99b, 0x74c476, 0x41ab5d, 0x238b45, 0x006d2c, 0x00441b];
    "Oranges", Sequential, true, [0xfff5eb, 0xfee6ce, 0xfdd0a2, 0xfdae6b, 0xfd8d3c, 0xf16913, 0xd94801, 0xa63603, 0x7f2704];
    "Reds", Sequential, true, [0xfff5f0, 0xfee0d2, 0xfcbba1, 0xfc9272, 0xfb6a4a, 0xef3b2c, 0xcb181d, 0xa50f15, 0x67000d];
    "Purples", Sequential, true, [0xfcfbfd, 0xefedf5, 0xdadaeb, 0xbcbddc, 0x9e9ac8, 0x807dba, 0x6a51a3, 0x54278f, 0x3f007d];
    "Greys", Sequential, true, [0xffffff, 0xf0f0f0, 0xd9d9d9, 0xbdbdbd, 0x969696, 0x737373, 0x525252, 0x252525, 0x000000];
    "Yellow-Green-Blue", Sequential, true, [0xffffd9, 0xedf8b1, 0xc7e9b4, 0x7fcdbb, 0x41b6c4, 0x1d91c0, 0x225ea8, 0x253494, 0x081d58];
    "Yellow-Orange-Red", Sequential, true, [0xffffcc, 0xffeda0, 0xfed976, 0xfeb24c, 0xfd8d3c, 0xfc4e2a, 0xe31a1c, 0xbd0026, 0x800026];
    "Yellow-Orange-Brown", Sequential, true, [0xffffe5, 0xfff7bc, 0xfee391, 0xfec44f, 0xfe9929, 0xec7014, 0xcc4c02, 0x993404, 0x662506];
    "Yellow-Green", Sequential, true, [0xffffe5, 0xf7fcb9, 0xd9f0a3, 0xaddd8e, 0x78c679, 0x41ab5d, 0x238443, 0x006837, 0x004529];
    "Blue-Purple", Sequential, true, [0xf7fcfd, 0xe0ecf4, 0xbfd3e6, 0x9ebcda, 0x8c96c6, 0x8c6bb1, 0x88419d, 0x810f7c, 0x4d004b];
    "Green-Blue", Sequential, true, [0xf7fcf0, 0xe0f3db, 0xccebc5, 0xa8ddb5, 0x7bccc4, 0x4eb3d3, 0x2b8cbe, 0x0868ac, 0x084081];
    "Orange-Red", Sequential, true, [0xfff7ec, 0xfee8c8, 0xfdd49e, 0xfdbb84, 0xfc8d59, 0xef6548, 0xd7301f, 0xb30000, 0x7f0000];
    "Purple-Red", Sequential, true, [0xf7f4f9, 0xe7e1ef, 0xd4b9da, 0xc994c7, 0xdf65b0, 0xe7298a, 0xce1256, 0x980043, 0x67001f];
    "Viridis", Sequential, true, [0xfde725, 0xb5de2b, 0x6ece58, 0x35b779, 0x1f9e89, 0x26828e, 0x31688e, 0x3e4989, 0x482878, 0x440154];
    "Red-Blue", Diverging, true, [0x67001f, 0xb2182b, 0xd6604d, 0xf4a582, 0xfddbc7, 0xf7f7f7, 0xd1e5f0, 0x92c5de, 0x4393c3, 0x2166ac, 0x053061];
    "Brown-Teal", Diverging, true, [0x543005, 0x8c510a, 0xbf812d, 0xdfc27d, 0xf6e8c3, 0xf5f5f5, 0xc7eae5, 0x80cdc1, 0x35978f, 0x01665e, 0x003c30];
    "Pink-Green", Diverging, true, [0x8e0152, 0xc51b7d, 0xde77ae, 0xf1b6da, 0xfde0ef, 0xf7f7f7, 0xe6f5d0, 0xb8e186, 0x7fbc41, 0x4d9221, 0x276419];
    "Purple-Green", Diverging, true, [0x40004b, 0x762a83, 0x9970ab, 0xc2a5cf, 0xe7d4e8, 0xf7f7f7, 0xd9f0d3, 0xa6dba0, 0x5aae61, 0x1b7837, 0x00441b];
    "Orange-Purple", Diverging, true, [0x7f3b08, 0xb35806, 0xe08214, 0xfdb863, 0xfee0b6, 0xf7f7f7, 0xd8daeb, 0xb2abd2, 0x8073ac, 0x542788, 0x2d004b];
    "Red-Yellow-Blue", Diverging, true, [0xa50026, 0xd73027, 0xf46d43, 0xfdae61, 0xfee090, 0xffffbf, 0xe0f3f8, 0xabd9e9, 0x74add1, 0x4575b4, 0x313695];
    "Red-Yellow-Green", Diverging, false, [0xa50026, 0xd73027, 0xf46d43, 0xfdae61, 0xfee08b, 0xffffbf, 0xd9ef8b, 0xa6d96a, 0x66bd63, 0x1a9850, 0x006837];
    "Spectral", Diverging, false, [0x9e0142, 0xd53e4f, 0xf46d43, 0xfdae61, 0xfee08b, 0xffffbf, 0xe6f598, 0xabdda4, 0x66c2a5, 0x3288bd, 0x5e4fa2];
    "Tableau", Qualitative, false, [0x4e79a7, 0xf28e2b, 0xe15759, 0x76b7b2, 0x59a14f, 0xedc948, 0xb07aa1, 0xff9da7, 0x9c755f, 0xbab0ac];
    "Set 2", Qualitative, true, [0x66c2a5, 0xfc8d62, 0x8da0cb, 0xe78ac3, 0xa6d854, 0xffd92f, 0xe5c494, 0xb3b3b3];
    "Dark 2", Qualitative, true, [0x1b9e77, 0xd95f02, 0x7570b3, 0xe7298a, 0x66a61e, 0xe6ab02, 0xa6761d, 0x666666];
    "Paired", Qualitative, false, [0xa6cee3, 0x1f78b4, 0xb2df8a, 0x33a02c, 0xfb9a99, 0xe31a1c, 0xfdbf6f, 0xff7f00, 0xcab2d6, 0x6a3d9a, 0xffff99, 0xb15928];
    "Set 3", Qualitative, false, [0x8dd3c7, 0xffffb3, 0xbebada, 0xfb8072, 0x80b1d3, 0xfdb462, 0xb3de69, 0xfccde5, 0xd9d9d9, 0xbc80bd, 0xccebc5, 0xffed6f];
}

pub fn by_name(name: &str) -> Option<&'static Ramp> {
    RAMPS.iter().find(|r| r.name == name)
}

/// The colours of a classification or category scheme: a named ramp or
/// the user's own stops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RampSpec {
    Named { name: String, reversed: bool },
    Custom { stops: Vec<Rgba> },
}

impl Default for RampSpec {
    fn default() -> Self {
        RampSpec::named("Blues")
    }
}

impl RampSpec {
    pub fn named(name: &str) -> RampSpec {
        RampSpec::Named {
            name: name.to_owned(),
            reversed: false,
        }
    }

    pub fn colors(&self, n: usize) -> Vec<Rgba> {
        match self {
            RampSpec::Named { name, reversed } => {
                let ramp = by_name(name).unwrap_or(&RAMPS[0]);
                let mut c = ramp.colors(n);
                if *reversed {
                    c.reverse();
                }
                c
            }
            RampSpec::Custom { stops } => spread(stops, n, 0.0, 1.0),
        }
    }

    pub fn label(&self) -> String {
        match self {
            RampSpec::Named {
                name,
                reversed: false,
            } => name.clone(),
            RampSpec::Named {
                name,
                reversed: true,
            } => format!("{name} (reversed)"),
            RampSpec::Custom { .. } => "Custom".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_for_class_counts() {
        let blues = by_name("Blues").unwrap();
        let c = blues.colors(5);
        assert_eq!(c.len(), 5);
        // Light to dark, and not starting at white.
        assert!(c[0].luminance() > c[4].luminance());
        assert!(c[0] != Rgba::rgb(0xf7, 0xfb, 0xff));
        assert_eq!(c[4], Rgba::rgb(0x08, 0x30, 0x6b));
        let q = by_name("Set 2").unwrap().colors(10);
        assert_eq!(q[8], q[0]);
        let rev = RampSpec::Named {
            name: "Blues".into(),
            reversed: true,
        }
        .colors(5);
        assert_eq!(rev[0], c[4]);
    }

    #[test]
    fn names_are_unique() {
        for (i, r) in RAMPS.iter().enumerate() {
            assert!(RAMPS[..i].iter().all(|o| o.name != r.name), "{}", r.name);
        }
    }
}
