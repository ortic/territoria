//! Value classes: where to put the breaks of a classified map.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Method {
    /// About the same number of areas per class.
    #[default]
    Quantile,
    /// Classes of equal width.
    EqualInterval,
    /// Breaks where the data has gaps (Jenks).
    NaturalBreaks,
    /// Classes one standard deviation wide around the mean.
    StandardDeviation,
    /// About the same total per class: a few big values get their own.
    EqualSum,
    /// Breaks set by hand.
    Manual,
}

impl Method {
    pub const ALL: [Method; 6] = [
        Method::Quantile,
        Method::EqualInterval,
        Method::NaturalBreaks,
        Method::StandardDeviation,
        Method::EqualSum,
        Method::Manual,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Method::Quantile => "Equal count (quantiles)",
            Method::EqualInterval => "Equal intervals",
            Method::NaturalBreaks => "Natural breaks (Jenks)",
            Method::StandardDeviation => "Standard deviation",
            Method::EqualSum => "Equal sums",
            Method::Manual => "Manual",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Method::Quantile => {
                "Each class gets about the same number of areas. Good for ranking; classes can be very uneven in value."
            }
            Method::EqualInterval => {
                "Every class spans the same range of values. Easy to read; skewed data leaves most areas in one class."
            }
            Method::NaturalBreaks => {
                "Breaks fall into the gaps of the data, so similar values share a class."
            }
            Method::StandardDeviation => {
                "Classes one standard deviation wide around the average: above and below normal."
            }
            Method::EqualSum => {
                "Each class holds about the same total. Shows the few areas that carry most of the volume."
            }
            Method::Manual => "You set the breaks.",
        }
    }
}

/// Class edges for `values`: `classes + 1` ascending numbers from the
/// minimum to the maximum (fewer when the data has fewer distinct values).
/// `manual` gives the inner breaks for [`Method::Manual`].
pub fn breaks(values: &[f64], method: Method, classes: usize, manual: &[f64]) -> Vec<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return Vec::new();
    }
    v.sort_by(f64::total_cmp);
    let (min, max) = (v[0], v[v.len() - 1]);
    let k = classes.max(1);
    if min == max {
        return vec![min, max];
    }
    let inner: Vec<f64> = match method {
        Method::Quantile => (1..k)
            .map(|i| {
                let pos = i as f64 * v.len() as f64 / k as f64;
                v[(pos.ceil() as usize).min(v.len() - 1)]
            })
            .collect(),
        Method::EqualInterval => (1..k)
            .map(|i| min + (max - min) * i as f64 / k as f64)
            .collect(),
        Method::NaturalBreaks => jenks(&v, k),
        Method::StandardDeviation => {
            let n = v.len() as f64;
            let mean = v.iter().sum::<f64>() / n;
            let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
            // Even counts break at the mean; odd ones centre a class on it.
            (1..k)
                .map(|i| mean + sd * (i as f64 - k as f64 / 2.0))
                .collect()
        }
        Method::EqualSum => {
            let positive: f64 = v.iter().filter(|x| **x > 0.0).sum();
            let mut out = Vec::new();
            let mut acc = 0.0;
            let mut next = 1;
            for &x in &v {
                acc += x.max(0.0);
                while next < k && acc >= positive * next as f64 / k as f64 {
                    out.push(x);
                    next += 1;
                }
            }
            out
        }
        Method::Manual => manual.to_vec(),
    };
    let mut edges = vec![min];
    for b in inner {
        if b > *edges.last().unwrap() && b < max {
            edges.push(b);
        }
    }
    edges.push(max);
    edges
}

/// Jenks natural breaks (Fisher's exact dynamic programme) on sorted
/// values; large inputs are sampled to keep it fast.
fn jenks(sorted: &[f64], k: usize) -> Vec<f64> {
    const MAX: usize = 2000;
    let data: Vec<f64> = if sorted.len() > MAX {
        (0..MAX)
            .map(|i| sorted[i * (sorted.len() - 1) / (MAX - 1)])
            .collect()
    } else {
        sorted.to_vec()
    };
    let n = data.len();
    let k = k.min(n);
    if k < 2 {
        return Vec::new();
    }
    // lower[i][j]: first index of the last class for i values in j classes.
    let mut lower = vec![vec![0usize; k + 1]; n + 1];
    let mut cost = vec![vec![f64::INFINITY; k + 1]; n + 1];
    for j in 1..=k {
        lower[1][j] = 1;
        cost[1][j] = 0.0;
    }
    for l in 2..=n {
        let (mut s1, mut s2, mut w) = (0.0, 0.0, 0.0);
        let mut variance = 0.0;
        for m in 1..=l {
            let i3 = l - m + 1;
            let val = data[i3 - 1];
            s2 += val * val;
            s1 += val;
            w += 1.0;
            variance = s2 - (s1 * s1) / w;
            let i4 = i3 - 1;
            if i4 != 0 {
                for j in 2..=k {
                    if cost[l][j] >= variance + cost[i4][j - 1] {
                        lower[l][j] = i3;
                        cost[l][j] = variance + cost[i4][j - 1];
                    }
                }
            }
        }
        lower[l][1] = 1;
        cost[l][1] = variance;
    }
    let mut out = vec![0.0; k - 1];
    let mut idx = n;
    for j in (2..=k).rev() {
        let id = lower[idx][j] - 1;
        out[j - 2] = data[id];
        idx = lower[idx][j] - 1;
        if idx == 0 {
            break;
        }
    }
    out
}

/// Rounds inner breaks to round numbers at a precision that fits the range
/// (`12_431.7` → `12_000` in a range of 0…80_000), keeping them in order.
pub fn round_breaks(edges: &[f64]) -> Vec<f64> {
    if edges.len() <= 2 {
        return edges.to_vec();
    }
    let (min, max) = (edges[0], edges[edges.len() - 1]);
    let step = nice_step((max - min) / (edges.len() - 1) as f64 / 10.0);
    let mut out = vec![min];
    for &b in &edges[1..edges.len() - 1] {
        let r = (b / step).round() * step;
        if r > *out.last().unwrap() && r < max {
            out.push(r);
        }
    }
    out.push(max);
    out
}

/// A round step near `x`: 1, 2 or 5 times a power of ten.
pub fn nice_step(x: f64) -> f64 {
    if x <= 0.0 || !x.is_finite() {
        return 1.0;
    }
    let p = 10f64.powf(x.log10().floor());
    let f = x / p;
    let n = if f < 1.5 {
        1.0
    } else if f < 3.5 {
        2.0
    } else if f < 7.5 {
        5.0
    } else {
        10.0
    };
    n * p
}

/// The class of `value` for `edges` (lower edge inclusive, the last class
/// includes the maximum); `None` outside.
pub fn class_of(value: f64, edges: &[f64]) -> Option<usize> {
    if edges.len() < 2 || !value.is_finite() {
        return None;
    }
    if value < edges[0] || value > edges[edges.len() - 1] {
        return None;
    }
    let classes = edges.len() - 1;
    let i = edges[1..classes].partition_point(|&b| b <= value);
    Some(i.min(classes - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_and_intervals() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(
            breaks(&v, Method::Quantile, 4, &[]),
            vec![1.0, 26.0, 51.0, 76.0, 100.0]
        );
        assert_eq!(
            breaks(&v, Method::EqualInterval, 3, &[]),
            vec![1.0, 34.0, 67.0, 100.0]
        );
        assert_eq!(
            breaks(&[5.0, 5.0], Method::Quantile, 4, &[]),
            vec![5.0, 5.0]
        );
        assert!(breaks(&[], Method::Quantile, 4, &[]).is_empty());
    }

    #[test]
    fn natural_breaks_find_gaps() {
        let v = [1.0, 2.0, 3.0, 50.0, 51.0, 52.0, 100.0, 101.0];
        let e = breaks(&v, Method::NaturalBreaks, 3, &[]);
        assert_eq!(e, vec![1.0, 50.0, 100.0, 101.0]);
    }

    #[test]
    fn equal_sum_isolates_big_values() {
        let mut v = vec![1.0; 99];
        v.push(1000.0);
        let e = breaks(&v, Method::EqualSum, 2, &[]);
        // The single big value is its own class.
        assert_eq!(e, vec![1.0, 1000.0]);
        assert_eq!(class_of(999.0, &e), Some(0));
    }

    #[test]
    fn standard_deviation_centres_on_mean() {
        let v = [0.0, 10.0, 20.0, 30.0, 40.0];
        let e = breaks(&v, Method::StandardDeviation, 4, &[]);
        assert_eq!(e.len(), 5);
        assert!((e[2] - 20.0).abs() < 1e-9, "{e:?}");
    }

    #[test]
    fn classes_and_rounding() {
        let e = [0.0, 10.0, 20.0, 30.0];
        assert_eq!(class_of(0.0, &e), Some(0));
        assert_eq!(class_of(10.0, &e), Some(1));
        assert_eq!(class_of(30.0, &e), Some(2));
        assert_eq!(class_of(31.0, &e), None);
        assert_eq!(
            round_breaks(&[0.0, 12_431.7, 40_100.0, 80_000.0]),
            vec![0.0, 12_000.0, 40_000.0, 80_000.0]
        );
        assert_eq!(nice_step(0.034), 0.02);
        assert_eq!(nice_step(0.04), 0.05);
    }
}
