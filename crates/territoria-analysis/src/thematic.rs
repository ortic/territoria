//! Turning values into colours: classified, continuous and by category,
//! with the legend that goes with it.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::classify::{self, Method};
use crate::color::Rgba;
use crate::format::format_value;
use crate::ramps::RampSpec;

/// How numbers become colours.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorScale {
    pub mode: ScaleMode,
    pub method: Method,
    pub classes: usize,
    /// Inner breaks for [`Method::Manual`].
    pub manual: Vec<f64>,
    pub round: bool,
    pub ramp: RampSpec,
    /// Areas that have data but no number.
    pub no_value: Rgba,
}

impl Default for ColorScale {
    fn default() -> Self {
        ColorScale {
            mode: ScaleMode::Classified,
            method: Method::Quantile,
            classes: 5,
            manual: Vec::new(),
            round: true,
            ramp: RampSpec::default(),
            no_value: Rgba::rgb(0xd4, 0xd4, 0xd8),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ScaleMode {
    #[default]
    Classified,
    /// A smooth gradient from minimum to maximum.
    Continuous,
}

/// One row of a legend.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendEntry {
    pub color: Rgba,
    pub label: String,
    /// Features in this class.
    pub count: usize,
}

/// The computed colours of a scale for a set of values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scaled {
    /// Per value its colour; `None` for missing values (no data).
    pub colors: Vec<Option<Rgba>>,
    pub edges: Vec<f64>,
    pub legend: Vec<LegendEntry>,
    /// Smooth gradient for continuous scales: (colour stops, min, max).
    pub gradient: Option<(Vec<Rgba>, f64, f64)>,
}

impl ColorScale {
    /// Colours for `values` (one per feature; `None` = no data for it,
    /// `Some(NaN)` = data without a number).
    pub fn apply(&self, values: &[Option<f64>]) -> Scaled {
        let finite: Vec<f64> = values
            .iter()
            .flatten()
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        match self.mode {
            ScaleMode::Classified => {
                let mut edges = classify::breaks(&finite, self.method, self.classes, &self.manual);
                if self.round && self.method != Method::Manual {
                    edges = classify::round_breaks(&edges);
                }
                let n = edges.len().saturating_sub(1).max(1);
                let colors = self.ramp.colors(n);
                let mut counts = vec![0usize; n];
                let mut no_value = 0;
                let out = values
                    .iter()
                    .map(|v| {
                        let v = (*v)?;
                        match classify::class_of(v, &edges) {
                            Some(c) => {
                                counts[c] += 1;
                                Some(colors[c])
                            }
                            None => {
                                no_value += 1;
                                Some(self.no_value)
                            }
                        }
                    })
                    .collect();
                let mut legend: Vec<LegendEntry> = (0..n)
                    .filter(|_| edges.len() >= 2)
                    .map(|c| LegendEntry {
                        color: colors[c],
                        label: format!(
                            "{} – {}",
                            format_value(edges[c]),
                            format_value(edges[c + 1])
                        ),
                        count: counts[c],
                    })
                    .collect();
                if no_value > 0 {
                    legend.push(LegendEntry {
                        color: self.no_value,
                        label: "No value".into(),
                        count: no_value,
                    });
                }
                Scaled {
                    colors: out,
                    edges,
                    legend,
                    gradient: None,
                }
            }
            ScaleMode::Continuous => {
                let (min, max) = finite
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| {
                        (a.min(v), b.max(v))
                    });
                let stops = self.ramp.colors(9);
                let span = (max - min).max(f64::MIN_POSITIVE);
                let colors = values
                    .iter()
                    .map(|v| {
                        let v = (*v)?;
                        Some(if v.is_finite() {
                            crate::ramps::sample(&stops, ((v - min) / span) as f32)
                        } else {
                            self.no_value
                        })
                    })
                    .collect();
                let legend = if finite.is_empty() {
                    Vec::new()
                } else {
                    vec![
                        LegendEntry {
                            color: stops[0],
                            label: format_value(min),
                            count: 0,
                        },
                        LegendEntry {
                            color: stops[8],
                            label: format_value(max),
                            count: 0,
                        },
                    ]
                };
                Scaled {
                    colors,
                    edges: vec![min, max],
                    legend,
                    gradient: (!finite.is_empty()).then_some((stops, min, max)),
                }
            }
        }
    }
}

/// Colours by category (text values): one colour per distinct value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryScale {
    pub ramp: RampSpec,
    /// Colours the user picked for particular values.
    pub fixed: Vec<(String, Rgba)>,
    /// More distinct values than this go into "Other".
    pub max_categories: usize,
    pub other: Rgba,
}

impl Default for CategoryScale {
    fn default() -> Self {
        CategoryScale {
            ramp: RampSpec::named("Tableau"),
            fixed: Vec::new(),
            max_categories: 12,
            other: Rgba::rgb(0xb0, 0xb0, 0xb8),
        }
    }
}

impl CategoryScale {
    pub fn apply(&self, values: &[Option<&str>]) -> Scaled {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for v in values.iter().flatten() {
            *counts.entry(v).or_default() += 1;
        }
        // Most frequent first; ties alphabetical.
        let mut cats: Vec<(&str, usize)> = counts.into_iter().collect();
        cats.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let keep = cats.len().min(self.max_categories);
        let palette = self.ramp.colors(keep);
        let mut color_of: HashMap<&str, Rgba> = HashMap::new();
        for (i, (c, _)) in cats.iter().take(keep).enumerate() {
            let fixed = self.fixed.iter().find(|(v, _)| v == c).map(|(_, col)| *col);
            color_of.insert(c, fixed.unwrap_or(palette[i]));
        }
        let colors = values
            .iter()
            .map(|v| v.map(|v| color_of.get(v).copied().unwrap_or(self.other)))
            .collect();
        let mut legend: Vec<LegendEntry> = cats
            .iter()
            .take(keep)
            .map(|(c, n)| LegendEntry {
                color: color_of[c],
                label: (*c).to_owned(),
                count: *n,
            })
            .collect();
        let other: usize = cats.iter().skip(keep).map(|(_, n)| n).sum();
        if other > 0 {
            legend.push(LegendEntry {
                color: self.other,
                label: format!("Other ({} values)", cats.len() - keep),
                count: other,
            });
        }
        Scaled {
            colors,
            edges: Vec::new(),
            legend,
            gradient: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classified_scale() {
        let s = ColorScale {
            classes: 2,
            method: Method::EqualInterval,
            round: false,
            ..Default::default()
        };
        let r = s.apply(&[Some(0.0), Some(10.0), None, Some(f64::NAN), Some(4.0)]);
        assert_eq!(r.edges, vec![0.0, 5.0, 10.0]);
        assert_eq!(r.colors[0], r.colors[4]);
        assert_ne!(r.colors[0], r.colors[1]);
        assert_eq!(r.colors[2], None);
        assert_eq!(r.colors[3], Some(s.no_value));
        assert_eq!(r.legend.len(), 3);
        assert_eq!(r.legend[0].count, 2);
        assert_eq!(r.legend[2].label, "No value");
    }

    #[test]
    fn categories_with_other() {
        let s = CategoryScale {
            max_categories: 2,
            ..Default::default()
        };
        let r = s.apply(&[Some("A"), Some("B"), Some("A"), Some("C"), None]);
        assert_eq!(r.legend[0].label, "A");
        assert_eq!(r.legend[0].count, 2);
        assert_eq!(r.colors[3], Some(s.other));
        assert_eq!(r.legend.last().unwrap().label, "Other (1 values)");
        assert_eq!(r.colors[4], None);
    }
}
