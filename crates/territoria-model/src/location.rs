//! Where sites go: matching given locations to territories with the least
//! total distance (the assignment problem), and choosing the best few of
//! many candidate locations for the demand (p-median).

/// Optimal one-to-one assignment of rows to columns by least total cost
/// (Hungarian method, O(n³)); rectangular matrices are fine. Per row its
/// column, `None` when there are more rows than columns.
pub fn assign(cost: &[Vec<f64>]) -> Vec<Option<usize>> {
    let rows = cost.len();
    let cols = cost.first().map_or(0, Vec::len);
    if rows == 0 || cols == 0 {
        return vec![None; rows];
    }
    // Square it with zero-cost dummies.
    let n = rows.max(cols);
    let c = |i: usize, j: usize| -> f64 {
        if i < rows && j < cols {
            cost[i][j]
        } else {
            0.0
        }
    };
    // Potentials u, v and the matching p (1-based, e-maxx formulation).
    let inf = f64::INFINITY;
    let mut u = vec![0.0; n + 1];
    let mut v = vec![0.0; n + 1];
    let mut p = vec![0usize; n + 1];
    let mut way = vec![0usize; n + 1];
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0;
        let mut minv = vec![inf; n + 1];
        let mut used = vec![false; n + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = inf;
            let mut j1 = 0;
            for j in 1..=n {
                if !used[j] {
                    let cur = c(i0 - 1, j - 1) - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=n {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut out = vec![None; rows];
    for (j, &i) in p.iter().enumerate().skip(1) {
        if i >= 1 && i <= rows && j <= cols {
            out[i - 1] = Some(j - 1);
        }
    }
    out
}

/// The `k` candidates that serve the demand best: least sum of weight ×
/// distance to the nearest chosen one. `distance(demand, candidate)`.
/// Greedy start, then swaps while they help (Teitz–Bart), with each
/// demand's nearest and second-nearest chosen site kept up to date.
pub fn p_median(
    demands: usize,
    weights: &[f64],
    candidates: usize,
    k: usize,
    distance: &dyn Fn(usize, usize) -> f64,
) -> Vec<usize> {
    let k = k.min(candidates);
    if k == 0 || demands == 0 {
        return Vec::new();
    }
    let w: Vec<f64> = (0..demands)
        .map(|i| weights.get(i).copied().unwrap_or(1.0).max(0.0))
        .collect();
    // Greedy: add the candidate that lowers the cost most.
    let mut chosen: Vec<usize> = Vec::new();
    let mut best = vec![f64::INFINITY; demands];
    while chosen.len() < k {
        let pick = (0..candidates)
            .filter(|j| !chosen.contains(j))
            .map(|j| {
                let c: f64 = (0..demands)
                    .map(|i| w[i] * best[i].min(distance(i, j)))
                    .sum();
                (c, j)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, j)) = pick else { break };
        chosen.push(j);
        for (i, b) in best.iter_mut().enumerate() {
            *b = b.min(distance(i, j));
        }
    }
    // Nearest and second-nearest chosen distance (and the nearest's slot).
    let nearest = |chosen: &[usize]| -> Vec<(usize, f64, f64)> {
        (0..demands)
            .map(|i| {
                let (mut s1, mut d1, mut d2) = (0, f64::INFINITY, f64::INFINITY);
                for (s, &j) in chosen.iter().enumerate() {
                    let d = distance(i, j);
                    if d < d1 {
                        d2 = d1;
                        d1 = d;
                        s1 = s;
                    } else if d < d2 {
                        d2 = d;
                    }
                }
                (s1, d1, d2)
            })
            .collect()
    };
    let mut near = nearest(&chosen);
    let mut current: f64 = (0..demands).map(|i| w[i] * near[i].1).sum();
    for _ in 0..30 {
        let mut improved = false;
        for slot in 0..chosen.len() {
            let mut best_swap: Option<(f64, usize)> = None;
            for j in 0..candidates {
                if chosen.contains(&j) {
                    continue;
                }
                // Without the one in `slot`, with j instead.
                let c: f64 = (0..demands)
                    .map(|i| {
                        let (s1, d1, d2) = near[i];
                        let rest = if s1 == slot { d2 } else { d1 };
                        w[i] * rest.min(distance(i, j))
                    })
                    .sum();
                if c < current - 1e-9 && best_swap.is_none_or(|(b, _)| c < b) {
                    best_swap = Some((c, j));
                }
            }
            if let Some((c, j)) = best_swap {
                chosen[slot] = j;
                current = c;
                near = nearest(&chosen);
                improved = true;
            }
        }
        if !improved {
            break;
        }
    }
    chosen.sort_unstable();
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_is_optimal() {
        // Greedy would take (0,0)=1 then (1,1)=9: total 10; best is 2 + 3.
        let cost = vec![vec![1.0, 2.0], vec![3.0, 9.0]];
        assert_eq!(assign(&cost), vec![Some(1), Some(0)]);
        // More rows than columns: one row stays out.
        let cost = vec![vec![5.0], vec![1.0], vec![3.0]];
        assert_eq!(assign(&cost), vec![None, Some(0), None]);
        // More columns: each row gets its best free one.
        let cost = vec![vec![4.0, 1.0, 3.0], vec![2.0, 0.0, 5.0]];
        assert_eq!(assign(&cost), vec![Some(1), Some(0)]);
    }

    #[test]
    fn p_median_finds_the_clusters() {
        // Demand in two clusters on a line, candidates everywhere.
        let demand = [0.0, 1.0, 2.0, 50.0, 51.0, 52.0];
        let cands: Vec<f64> = (0..=60).map(|x| x as f64).collect();
        let chosen = p_median(demand.len(), &[1.0; 6], cands.len(), 2, &|i, j| {
            (demand[i] - cands[j]).abs()
        });
        assert_eq!(chosen, vec![1, 51]);
        // Weights pull the site: most demand at 2.
        let chosen = p_median(3, &[1.0, 1.0, 10.0], cands.len(), 1, &|i, j| {
            (demand[i] - cands[j]).abs()
        });
        assert_eq!(chosen, vec![2]);
    }
}
