//! Aggregation of several values into one (rows per area, units per
//! territory).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Aggregate {
    #[default]
    Sum,
    Mean,
    Median,
    Min,
    Max,
    /// Number of rows, empty values included.
    Count,
    /// Number of non-empty values.
    CountValues,
    First,
}

impl Aggregate {
    pub const ALL: [Aggregate; 8] = [
        Aggregate::Sum,
        Aggregate::Mean,
        Aggregate::Median,
        Aggregate::Min,
        Aggregate::Max,
        Aggregate::Count,
        Aggregate::CountValues,
        Aggregate::First,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Aggregate::Sum => "Sum",
            Aggregate::Mean => "Average",
            Aggregate::Median => "Median",
            Aggregate::Min => "Minimum",
            Aggregate::Max => "Maximum",
            Aggregate::Count => "Number of rows",
            Aggregate::CountValues => "Number of values",
            Aggregate::First => "First value",
        }
    }

    /// Aggregates that also make sense for text columns.
    pub fn works_on_text(self) -> bool {
        matches!(
            self,
            Aggregate::Count | Aggregate::CountValues | Aggregate::First
        )
    }
}

/// Collects the values of one group.
#[derive(Debug, Clone, Default)]
pub struct Accumulator {
    rows: usize,
    values: Vec<f64>,
}

impl Accumulator {
    pub fn new() -> Accumulator {
        Accumulator::default()
    }

    /// Adds a row with its value (`None` for an empty cell).
    pub fn push(&mut self, value: Option<f64>) {
        self.rows += 1;
        if let Some(v) = value {
            self.values.push(v);
        }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// The result; `None` when no value went in (except for counts).
    pub fn finish(&mut self, agg: Aggregate) -> Option<f64> {
        let v = &mut self.values;
        match agg {
            Aggregate::Count => Some(self.rows as f64),
            Aggregate::CountValues => Some(v.len() as f64),
            _ if v.is_empty() => None,
            Aggregate::Sum => Some(v.iter().sum()),
            Aggregate::Mean => Some(v.iter().sum::<f64>() / v.len() as f64),
            Aggregate::Min => v.iter().copied().reduce(f64::min),
            Aggregate::Max => v.iter().copied().reduce(f64::max),
            Aggregate::First => v.first().copied(),
            Aggregate::Median => {
                v.sort_by(f64::total_cmp);
                let m = v.len() / 2;
                Some(if v.len().is_multiple_of(2) {
                    (v[m - 1] + v[m]) / 2.0
                } else {
                    v[m]
                })
            }
        }
    }
}

/// Aggregates `values` grouped by `groups` (a group index per value, `None`
/// to skip) into `n_groups` results; groups without rows give `None`.
pub fn group(
    values: impl IntoIterator<Item = Option<f64>>,
    groups: impl IntoIterator<Item = Option<u32>>,
    n_groups: usize,
    agg: Aggregate,
) -> Vec<Option<f64>> {
    let mut acc = vec![Accumulator::new(); n_groups];
    for (v, g) in values.into_iter().zip(groups) {
        if let Some(g) = g {
            acc[g as usize].push(v);
        }
    }
    acc.iter_mut()
        .map(|a| if a.rows() == 0 { None } else { a.finish(agg) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups() {
        let vals = [Some(1.0), Some(3.0), None, Some(10.0), Some(2.0)];
        let grp = [Some(0), Some(0), Some(0), Some(1), None];
        assert_eq!(
            group(vals, grp, 3, Aggregate::Sum),
            vec![Some(4.0), Some(10.0), None]
        );
        assert_eq!(
            group(vals, grp, 3, Aggregate::Mean),
            vec![Some(2.0), Some(10.0), None]
        );
        assert_eq!(
            group(vals, grp, 3, Aggregate::Count),
            vec![Some(3.0), Some(1.0), None]
        );
        assert_eq!(
            group(vals, grp, 3, Aggregate::CountValues),
            vec![Some(2.0), Some(1.0), None]
        );
        assert_eq!(
            group(vals, grp, 3, Aggregate::Median),
            vec![Some(2.0), Some(10.0), None]
        );
        assert_eq!(
            group(vals, grp, 3, Aggregate::Max),
            vec![Some(3.0), Some(10.0), None]
        );
    }
}
