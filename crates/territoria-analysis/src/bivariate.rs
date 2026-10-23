//! Two numbers at once: each split into 2 or 3 classes (quantiles); the
//! pair of classes picks a colour from a grid that mixes two hues, so
//! areas high in both stand out from areas high in one.

use serde::{Deserialize, Serialize};

use crate::classify::{self, Method};
use crate::color::Rgba;
use crate::thematic::Scaled;

/// A 3×3 colour grid (Joshua Stevens' bivariate schemes).
#[derive(Debug)]
pub struct BivariatePalette {
    pub name: &'static str,
    /// Row by row from low to high second value, each row from low to
    /// high first value.
    pub grid: [u32; 9],
}

pub const BIVARIATE: &[BivariatePalette] = &[
    BivariatePalette {
        name: "Teal–Pink",
        grid: [
            0xe8e8e8, 0xace4e4, 0x5ac8c8, 0xdfb0d6, 0xa5add3, 0x5698b9, 0xbe64ac, 0x8c62aa,
            0x3b4994,
        ],
    },
    BivariatePalette {
        name: "Blue–Green",
        grid: [
            0xe8e8e8, 0xb5c0da, 0x6c83b5, 0xb8d6be, 0x90b2b3, 0x567994, 0x73ae80, 0x5a9178,
            0x2a5a5b,
        ],
    },
    BivariatePalette {
        name: "Gold–Purple",
        grid: [
            0xe8e8e8, 0xe4d9ac, 0xc8b35a, 0xcbb8d7, 0xc8ada0, 0xaf8e53, 0x9972af, 0x976b82,
            0x804d36,
        ],
    },
];

/// How two numbers become colours.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BivariateScale {
    /// Classes per value: 2 or 3.
    pub classes: usize,
    pub palette: String,
    /// Colour for areas lacking either value (`None`: not filled).
    pub no_data: Option<Rgba>,
}

impl Default for BivariateScale {
    fn default() -> Self {
        BivariateScale {
            classes: 3,
            palette: BIVARIATE[0].name.to_owned(),
            no_data: None,
        }
    }
}

/// The classes of both values and their colours.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BivariateGrid {
    pub size: usize,
    pub x_edges: Vec<f64>,
    pub y_edges: Vec<f64>,
    /// `size × size` colours, row by row from low to high second value.
    pub colors: Vec<Rgba>,
    /// Features per cell, in the same order.
    pub counts: Vec<usize>,
}

impl BivariateScale {
    fn classes(&self) -> usize {
        self.classes.clamp(2, 3)
    }

    /// The grid's colours, row by row from low to high second value.
    pub fn colors(&self) -> Vec<Rgba> {
        let p = BIVARIATE
            .iter()
            .find(|p| p.name == self.palette)
            .unwrap_or(&BIVARIATE[0]);
        let hex = |v: u32| Rgba([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]);
        // Two classes: the corners of the 3×3 grid.
        let pick: &[usize] = if self.classes() == 2 {
            &[0, 2]
        } else {
            &[0, 1, 2]
        };
        pick.iter()
            .flat_map(|y| pick.iter().map(move |x| hex(p.grid[y * 3 + x])))
            .collect()
    }

    /// Colours for features with values `x` and `y` (`None`: no data).
    pub fn apply(&self, x: &[Option<f64>], y: &[Option<f64>]) -> Scaled {
        let k = self.classes();
        let finite = |v: &[Option<f64>]| -> Vec<f64> {
            v.iter()
                .flatten()
                .copied()
                .filter(|v| v.is_finite())
                .collect()
        };
        let x_edges = classify::breaks(&finite(x), Method::Quantile, k, &[]);
        let y_edges = classify::breaks(&finite(y), Method::Quantile, k, &[]);
        let colors = self.colors();
        let mut counts = vec![0usize; k * k];
        // Fewer distinct values than classes: the top classes stay empty.
        let class = |v: f64, edges: &[f64]| classify::class_of(v, edges).map(|c| c.min(k - 1));
        let out = x
            .iter()
            .zip(y)
            .map(|(a, b)| {
                let (Some(a), Some(b)) = (a, b) else {
                    return self.no_data;
                };
                match (class(*a, &x_edges), class(*b, &y_edges)) {
                    (Some(cx), Some(cy)) => {
                        counts[cy * k + cx] += 1;
                        Some(colors[cy * k + cx])
                    }
                    _ => self.no_data,
                }
            })
            .collect();
        Scaled {
            colors: out,
            grid: Some(BivariateGrid {
                size: k,
                x_edges,
                y_edges,
                colors,
                counts,
            }),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_of_classes_pick_colours() {
        let scale = BivariateScale::default();
        // Distinct values, three per class of each.
        let x: Vec<Option<f64>> = (0..9).map(|i| Some((i % 3 * 10 + i / 3) as f64)).collect();
        let y: Vec<Option<f64>> = (0..9).map(|i| Some((i / 3 * 10 + i % 3) as f64)).collect();
        let s = scale.apply(&x, &y);
        let grid = s.grid.as_ref().unwrap();
        assert_eq!(grid.counts, vec![1; 9]);
        let colors = scale.colors();
        // Low–low is the light corner, high–high the dark one.
        assert_eq!(s.colors[0], Some(colors[0]));
        assert_eq!(s.colors[8], Some(colors[8]));
        assert_eq!(s.colors[2], Some(colors[2]), "high first, low second");
        let two = BivariateScale {
            classes: 2,
            ..Default::default()
        };
        assert_eq!(two.colors().len(), 4);
        assert_eq!(two.colors()[3], colors[8]);
        let gaps = scale.apply(&[Some(1.0), None], &[Some(1.0), Some(2.0)]);
        assert_eq!(gaps.colors[1], None);
    }
}
