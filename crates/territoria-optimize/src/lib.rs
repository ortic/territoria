//! Territory optimization.
//!
//! Units (postcodes, municipalities, customers) are moved between
//! neighbouring territories by simulated annealing. The objective adds up
//!
//! - **balance**: per measure and territory the squared relative deviation
//!   from its target, with a steep extra penalty outside the tolerance band;
//! - **compactness**: the length of borders between territories, relative to
//!   that of ideal round territories;
//! - **stability**: the share of units that changed territory (only when
//!   improving an existing plan).
//!
//! Contiguity is a hard constraint: a move never splits a territory. Pinned
//! units and locked territories don't move. Everything is deterministic for
//! a given seed.

use std::collections::VecDeque;

const NONE: u32 = u32::MAX;

/// One figure territories are compared by (customers, revenue, areas…).
#[derive(Debug, Clone)]
pub struct Measure {
    /// Value per unit (missing values count as 0).
    pub values: Vec<f64>,
    /// Relative importance among measures.
    pub importance: f64,
    /// Accepted relative deviation, e.g. 0.1 for ±10 %.
    pub tolerance: f64,
}

#[derive(Debug, Clone)]
pub struct Problem {
    /// Per unit its neighbours with the length of the shared border (km).
    pub neighbors: Vec<Vec<(u32, f64)>>,
    /// Per unit a position in km (any planar system).
    pub positions: Vec<[f64; 2]>,
    /// Per unit its area in km² (for compactness reference).
    pub areas: Vec<f64>,
    pub measures: Vec<Measure>,
    /// Per territory its relative size (1.0 = normal).
    pub capacity: Vec<f64>,
    /// Territories that keep their units.
    pub locked: Vec<bool>,
    /// Units that keep their territory.
    pub pinned: Vec<bool>,
    /// Per unit its territory index, if any.
    pub initial: Vec<Option<u32>>,
    /// Per territory a unit it must keep (its site), if any.
    pub anchors: Vec<Option<u32>>,
    /// Per territory its site's position (same system as `positions`):
    /// distance to it counts towards compactness.
    pub sites: Vec<Option<[f64; 2]>>,
}

impl Problem {
    pub fn units(&self) -> usize {
        self.neighbors.len()
    }

    pub fn territories(&self) -> usize {
        self.capacity.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// Improve the existing plan with as few moves as possible.
    Balance,
    /// Start over: new territories from scratch (anchors seed them).
    Reorganize,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub mode: Mode,
    /// 0 = balance only, 1 = compact shapes matter as much as balance.
    pub compactness: f64,
    /// 0 = move freely, 1 = strongly prefer few moves (Balance mode).
    pub stability: f64,
    /// Never split a territory into pieces.
    pub contiguous: bool,
    /// Give unassigned units to territories first.
    pub assign_unassigned: bool,
    pub seed: u64,
    /// Stop after this many seconds.
    pub max_seconds: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            mode: Mode::Balance,
            compactness: 0.4,
            stability: 0.3,
            contiguous: true,
            assign_unassigned: true,
            seed: 1,
            max_seconds: 20.0,
        }
    }
}

/// How good an assignment is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Quality {
    /// Per measure, the largest relative deviation of a territory.
    pub max_deviation: Vec<f64>,
    /// Per measure, territories outside the tolerance.
    pub out_of_tolerance: Vec<usize>,
    /// Length of borders between territories in km.
    pub border_length: f64,
    /// Territories in more than one piece.
    pub split: usize,
    pub unassigned: usize,
    pub objective: f64,
}

#[derive(Debug, Clone)]
pub struct Solution {
    pub assignment: Vec<Option<u32>>,
    pub before: Quality,
    pub after: Quality,
    /// Units whose territory changed.
    pub moved: usize,
    pub iterations: u64,
    pub stopped_early: bool,
}

/// Small, fast, deterministic random numbers (xorshift64*).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The working state with incremental sums.
struct State<'a> {
    p: &'a Problem,
    a: Vec<u32>,
    /// sums[m][t]
    sums: Vec<Vec<f64>>,
    /// targets[m][t]
    targets: Vec<Vec<f64>>,
    count: Vec<u32>,
    cut: f64,
    moved: usize,
    w_compact: f64,
    /// Weight of the summed distance of units to their territory's site.
    w_distance: f64,
    distance: f64,
    w_stable: f64,
    initial: Vec<u32>,
    /// Scratch for connectivity checks.
    mark: Vec<u32>,
    stamp: u32,
}

const OUT_OF_BAND: f64 = 12.0;

fn penalty(dev: f64, tol: f64) -> f64 {
    let over = (dev.abs() - tol).max(0.0);
    dev * dev + OUT_OF_BAND * over * over
}

impl<'a> State<'a> {
    fn new(p: &'a Problem, a: Vec<u32>, options: &Options) -> State<'a> {
        let k = p.territories();
        let total_cap: f64 = p
            .capacity
            .iter()
            .map(|c| c.max(0.0))
            .sum::<f64>()
            .max(1e-12);
        let targets: Vec<Vec<f64>> = p
            .measures
            .iter()
            .map(|m| {
                let total: f64 = m.values.iter().map(|v| v.max(0.0)).sum();
                p.capacity
                    .iter()
                    .map(|c| total * c.max(0.0) / total_cap)
                    .collect()
            })
            .collect();
        // Compactness relative to ideal round territories.
        let total_area: f64 = p.areas.iter().sum::<f64>().max(1e-9);
        let ideal_perimeter = 2.0 * (std::f64::consts::PI * total_area / k.max(1) as f64).sqrt();
        let w_compact =
            options.compactness * 0.02 / (k.max(1) as f64 * ideal_perimeter).max(1e-9) * 2.0;
        let w_stable = if options.mode == Mode::Balance {
            options.stability * 0.05 / p.units().max(1) as f64
        } else {
            0.0
        };
        // Distance to the site relative to an ideal round territory's radius.
        let radius = (total_area / k.max(1) as f64 / std::f64::consts::PI).sqrt();
        let w_distance = options.compactness * 0.02 / (p.units().max(1) as f64 * radius.max(1e-9));
        let mut s = State {
            p,
            initial: p.initial.iter().map(|t| t.unwrap_or(NONE)).collect(),
            a,
            sums: vec![vec![0.0; k]; p.measures.len()],
            targets,
            count: vec![0; k],
            cut: 0.0,
            moved: 0,
            w_compact,
            w_distance,
            distance: 0.0,
            w_stable,
            mark: vec![0; p.units()],
            stamp: 0,
        };
        s.recompute();
        s
    }

    fn recompute(&mut self) {
        let k = self.p.territories();
        self.count = vec![0; k];
        for s in &mut self.sums {
            s.iter_mut().for_each(|v| *v = 0.0);
        }
        for (u, &t) in self.a.iter().enumerate() {
            if t != NONE {
                self.count[t as usize] += 1;
                for (m, measure) in self.p.measures.iter().enumerate() {
                    self.sums[m][t as usize] += measure.values[u].max(0.0);
                }
            }
        }
        self.cut = 0.0;
        for (u, ns) in self.p.neighbors.iter().enumerate() {
            for &(v, len) in ns {
                if (u as u32) < v
                    && self.a[u] != self.a[v as usize]
                    && self.a[u] != NONE
                    && self.a[v as usize] != NONE
                {
                    self.cut += len;
                }
            }
        }
        self.distance = (0..self.a.len())
            .map(|u| self.site_distance(u, self.a[u]))
            .sum();
        self.moved = self
            .a
            .iter()
            .zip(&self.initial)
            .filter(|(a, i)| **i != NONE && a != i)
            .count();
    }

    /// Distance of `u` to the site of territory `t` (0 without a site).
    fn site_distance(&self, u: usize, t: u32) -> f64 {
        if t == NONE {
            return 0.0;
        }
        match self.p.sites.get(t as usize).copied().flatten() {
            Some(s) => {
                let p = self.p.positions[u];
                ((p[0] - s[0]).powi(2) + (p[1] - s[1]).powi(2)).sqrt()
            }
            None => 0.0,
        }
    }

    fn dev(&self, m: usize, t: usize, sum: f64) -> f64 {
        let target = self.targets[m][t];
        if target > 0.0 {
            sum / target - 1.0
        } else {
            0.0
        }
    }

    fn balance_term(&self, t: usize, delta: &[f64]) -> f64 {
        let k = self.p.territories() as f64;
        self.p
            .measures
            .iter()
            .enumerate()
            .map(|(m, measure)| {
                measure.importance
                    * penalty(
                        self.dev(m, t, self.sums[m][t] + delta[m]),
                        measure.tolerance,
                    )
                    / k
            })
            .sum()
    }

    fn objective(&self) -> f64 {
        let zero = vec![0.0; self.p.measures.len()];
        let balance: f64 = (0..self.p.territories())
            .map(|t| self.balance_term(t, &zero))
            .sum();
        balance
            + self.w_compact * self.cut
            + self.w_distance * self.distance
            + self.w_stable * self.moved as f64
    }

    /// Change of the objective when `u` moves from its territory to `to`.
    fn delta(&self, u: usize, to: u32, scratch: &mut Vec<f64>) -> f64 {
        let from = self.a[u];
        let mut d = 0.0;
        scratch.clear();
        scratch.extend(self.p.measures.iter().map(|m| m.values[u].max(0.0)));
        if from != NONE {
            let neg: Vec<f64> = scratch.iter().map(|v| -v).collect();
            let f = from as usize;
            d += self.balance_term(f, &neg) - self.balance_term(f, &vec![0.0; neg.len()]);
        }
        if to != NONE {
            let t = to as usize;
            d += self.balance_term(t, scratch) - self.balance_term(t, &vec![0.0; scratch.len()]);
        }
        // Borders: edges to `from` become cut, edges to `to` stop being cut.
        let mut dcut = 0.0;
        for &(v, len) in &self.p.neighbors[u] {
            let tv = self.a[v as usize];
            if tv == NONE {
                continue;
            }
            let was = from != NONE && tv != from;
            let will = to != NONE && tv != to;
            dcut += (will as i32 - was as i32) as f64 * len;
        }
        d += self.w_compact * dcut;
        d += self.w_distance * (self.site_distance(u, to) - self.site_distance(u, from));
        let init = self.initial[u];
        if init != NONE {
            let was = (from != init) as i32;
            let will = (to != init) as i32;
            d += self.w_stable * (will - was) as f64;
        }
        d
    }

    fn apply(&mut self, u: usize, to: u32) {
        let from = self.a[u];
        self.distance += self.site_distance(u, to) - self.site_distance(u, from);
        for &(v, len) in &self.p.neighbors[u] {
            let tv = self.a[v as usize];
            if tv == NONE {
                continue;
            }
            let was = from != NONE && tv != from;
            let will = to != NONE && tv != to;
            self.cut += (will as i32 - was as i32) as f64 * len;
        }
        for (m, measure) in self.p.measures.iter().enumerate() {
            let v = measure.values[u].max(0.0);
            if from != NONE {
                self.sums[m][from as usize] -= v;
            }
            if to != NONE {
                self.sums[m][to as usize] += v;
            }
        }
        if from != NONE {
            self.count[from as usize] -= 1;
        }
        if to != NONE {
            self.count[to as usize] += 1;
        }
        let init = self.initial[u];
        if init != NONE {
            if from == init {
                self.moved += 1;
            }
            if to == init {
                self.moved -= 1;
            }
        }
        self.a[u] = to;
    }

    /// Whether the territory of `u` stays in one piece without `u`.
    fn removable(&mut self, u: usize) -> bool {
        let t = self.a[u];
        let same: Vec<u32> = self.p.neighbors[u]
            .iter()
            .map(|&(v, _)| v)
            .filter(|&v| self.a[v as usize] == t)
            .collect();
        if same.len() <= 1 {
            return true;
        }
        // Fast path: the same-territory neighbours touch each other
        // directly (the usual case around a unit on a border).
        self.stamp += 2;
        let (member, visited) = (self.stamp - 1, self.stamp);
        for &v in &same {
            self.mark[v as usize] = member;
        }
        let mut reached = 1;
        let mut stack = vec![same[0]];
        self.mark[same[0] as usize] = visited;
        while let Some(v) = stack.pop() {
            for &(w, _) in &self.p.neighbors[v as usize] {
                if self.mark[w as usize] == member {
                    self.mark[w as usize] = visited;
                    reached += 1;
                    stack.push(w);
                }
            }
        }
        if reached == same.len() {
            return true;
        }
        // Full check: breadth-first through the territory without u until
        // every same-territory neighbour is reached.
        self.stamp += 1;
        let seen = self.stamp;
        let mut remaining: std::collections::HashSet<u32> = same[1..].iter().copied().collect();
        let mut queue = VecDeque::from([same[0]]);
        self.mark[same[0] as usize] = seen;
        self.mark[u] = seen;
        while let Some(v) = queue.pop_front() {
            for &(w, _) in &self.p.neighbors[v as usize] {
                let wi = w as usize;
                if self.a[wi] == t && self.mark[wi] != seen {
                    self.mark[wi] = seen;
                    if remaining.remove(&w) && remaining.is_empty() {
                        return true;
                    }
                    queue.push_back(w);
                }
            }
        }
        remaining.is_empty()
    }

    fn quality(&self) -> Quality {
        let k = self.p.territories();
        let mut q = Quality {
            border_length: self.cut,
            unassigned: self.a.iter().filter(|&&t| t == NONE).count(),
            objective: self.objective(),
            ..Default::default()
        };
        for (m, measure) in self.p.measures.iter().enumerate() {
            let devs: Vec<f64> = (0..k)
                .filter(|&t| !self.p.locked[t])
                .map(|t| self.dev(m, t, self.sums[m][t]))
                .collect();
            q.max_deviation
                .push(devs.iter().fold(0.0, |a: f64, d| a.max(d.abs())));
            q.out_of_tolerance.push(
                devs.iter()
                    .filter(|d| d.abs() > measure.tolerance + 1e-9)
                    .count(),
            );
        }
        q.split = (0..k)
            .filter(|&t| parts(self.p, &self.a, t as u32) > 1)
            .count();
        q
    }
}

/// Number of connected pieces of territory `t`.
pub fn parts(p: &Problem, a: &[u32], t: u32) -> usize {
    let mut seen = vec![false; a.len()];
    let mut n = 0;
    for start in 0..a.len() {
        if a[start] != t || seen[start] {
            continue;
        }
        n += 1;
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(u) = stack.pop() {
            for &(v, _) in &p.neighbors[u] {
                let v = v as usize;
                if a[v] == t && !seen[v] {
                    seen[v] = true;
                    stack.push(v);
                }
            }
        }
    }
    n
}

/// Optimizes `problem`. `progress(share_done)` is called now and then; when
/// it returns `false` the search stops and returns the best so far.
pub fn optimize(
    problem: &Problem,
    options: &Options,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Solution {
    let start_time = std::time::Instant::now();
    let n = problem.units();
    let k = problem.territories();
    let initial: Vec<u32> = problem.initial.iter().map(|t| t.unwrap_or(NONE)).collect();
    let before = State::new(problem, initial.clone(), options).quality();
    if n == 0 || k == 0 {
        return Solution {
            assignment: problem.initial.clone(),
            after: before.clone(),
            before,
            moved: 0,
            iterations: 0,
            stopped_early: false,
        };
    }
    let mut rng = Rng::new(options.seed);

    let start = match options.mode {
        Mode::Balance => {
            let mut a = initial.clone();
            if options.assign_unassigned {
                fill_unassigned(problem, &mut a);
            }
            a
        }
        Mode::Reorganize => grow(problem, &mut rng),
    };
    let mut s = State::new(problem, start, options);

    let movable = |s: &State, u: usize| -> bool {
        if problem.pinned[u] {
            return false;
        }
        let t = s.a[u];
        if t != NONE
            && (problem.locked[t as usize] || problem.anchors[t as usize] == Some(u as u32))
        {
            return false;
        }
        true
    };

    // Temperature from the size of typical bad moves.
    let mut scratch = Vec::new();
    let mut samples = Vec::new();
    for _ in 0..400 {
        let u = rng.below(n);
        if s.a[u] == NONE || !movable(&s, u) {
            continue;
        }
        if let Some(&(v, _)) =
            problem.neighbors[u].get(rng.below(problem.neighbors[u].len().max(1)))
        {
            let to = s.a[v as usize];
            if to != NONE && to != s.a[u] && !problem.locked[to as usize] {
                let d = s.delta(u, to, &mut scratch);
                if d > 0.0 {
                    samples.push(d);
                }
            }
        }
    }
    samples.sort_by(f64::total_cmp);
    let t0 = samples
        .get(samples.len() / 2)
        .copied()
        .unwrap_or(1e-4)
        .max(1e-9)
        * 0.6;
    let mut temp = t0;
    let sweeps = 120usize;
    let cooling = (1e-3f64).powf(1.0 / sweeps as f64);

    let mut best = s.a.clone();
    let mut best_obj = s.objective();
    let mut iterations = 0u64;
    let mut stopped_early = false;
    let mut boundary: Vec<u32> = Vec::new();
    'outer: for sweep in 0..=sweeps {
        let greedy = sweep == sweeps;
        let t = if greedy { 0.0 } else { temp };
        boundary.clear();
        for u in 0..n {
            if s.a[u] != NONE
                && problem.neighbors[u]
                    .iter()
                    .any(|&(v, _)| s.a[v as usize] != s.a[u] && s.a[v as usize] != NONE)
            {
                boundary.push(u as u32);
            }
        }
        // Shuffle.
        for i in (1..boundary.len()).rev() {
            let j = rng.below(i + 1);
            boundary.swap(i, j);
        }
        let mut improved = false;
        for &u in &boundary {
            let u = u as usize;
            iterations += 1;
            if !movable(&s, u) || s.count[s.a[u] as usize] <= 1 {
                continue;
            }
            // Candidate territories: those of the neighbours.
            let from = s.a[u];
            let mut cands: Vec<u32> = problem.neighbors[u]
                .iter()
                .map(|&(v, _)| s.a[v as usize])
                .filter(|&t| t != NONE && t != from && !problem.locked[t as usize])
                .collect();
            cands.sort_unstable();
            cands.dedup();
            let Some((to, d)) = cands
                .into_iter()
                .map(|to| (to, s.delta(u, to, &mut scratch)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
            else {
                continue;
            };
            let accept = d < -1e-15 || (t > 0.0 && rng.unit() < (-d / t).exp());
            if !accept {
                continue;
            }
            if options.contiguous && !s.removable(u) {
                continue;
            }
            s.apply(u, to);
            improved |= d < 0.0;
            let obj = s.objective();
            if obj < best_obj - 1e-15 {
                best_obj = obj;
                best.clone_from(&s.a);
            }
        }
        temp *= cooling;
        let done = sweep as f32 / sweeps as f32;
        if !progress(done) || start_time.elapsed().as_secs_f64() > options.max_seconds {
            stopped_early = sweep < sweeps;
            break 'outer;
        }
        if greedy && !improved {
            break;
        }
    }
    // A final greedy polish from the best state.
    s.a = best;
    s.recompute();
    loop {
        let mut improved = false;
        for u in 0..n {
            if s.a[u] == NONE || !movable(&s, u) || s.count[s.a[u] as usize] <= 1 {
                continue;
            }
            let from = s.a[u];
            let mut cands: Vec<u32> = problem.neighbors[u]
                .iter()
                .map(|&(v, _)| s.a[v as usize])
                .filter(|&t| t != NONE && t != from && !problem.locked[t as usize])
                .collect();
            cands.sort_unstable();
            cands.dedup();
            for to in cands {
                if s.delta(u, to, &mut scratch) < -1e-12 && (!options.contiguous || s.removable(u))
                {
                    s.apply(u, to);
                    improved = true;
                    break;
                }
            }
        }
        if !improved || start_time.elapsed().as_secs_f64() > options.max_seconds * 1.2 {
            break;
        }
    }
    let after = s.quality();
    let assignment: Vec<Option<u32>> = s.a.iter().map(|&t| (t != NONE).then_some(t)).collect();
    let moved = assignment
        .iter()
        .zip(&problem.initial)
        .filter(|(a, b)| a != b)
        .count();
    progress(1.0);
    Solution {
        assignment,
        before,
        after,
        moved,
        iterations,
        stopped_early,
    }
}

/// Unassigned units join the territory they share most border with,
/// repeatedly (enclosed gaps fill from outside in).
fn fill_unassigned(p: &Problem, a: &mut [u32]) {
    loop {
        let mut changes = Vec::new();
        for u in 0..a.len() {
            if a[u] != NONE || p.pinned[u] {
                continue;
            }
            let mut best: Option<(u32, f64)> = None;
            for &(v, len) in &p.neighbors[u] {
                let t = a[v as usize];
                if t == NONE || p.locked[t as usize] {
                    continue;
                }
                let total: f64 = p.neighbors[u]
                    .iter()
                    .filter(|(w, _)| a[*w as usize] == t)
                    .map(|(_, l)| l)
                    .sum();
                if best.is_none_or(|(_, b)| total > b) {
                    best = Some((t, total));
                }
                let _ = len;
            }
            if let Some((t, _)) = best {
                changes.push((u, t));
            }
        }
        if changes.is_empty() {
            break;
        }
        for (u, t) in changes {
            a[u] = t;
        }
    }
}

/// New territories from scratch: seeds spread out (anchors first), then
/// the territory furthest below its target takes its nearest free
/// neighbouring unit, until all units are taken.
fn grow(p: &Problem, rng: &mut Rng) -> Vec<u32> {
    let n = p.units();
    let k = p.territories();
    let mut a = vec![NONE; n];
    // Locked territories and pinned units keep what they have.
    for (u, slot) in a.iter_mut().enumerate() {
        if let Some(t) = p.initial[u]
            && (p.pinned[u] || p.locked[t as usize])
        {
            *slot = t;
        }
    }
    let weight = |u: usize| -> f64 {
        p.measures
            .first()
            .map_or(1.0, |m| m.values[u].max(0.0))
            .max(1e-9)
    };
    // Seeds: anchors, else k-means++ style spread.
    let mut seeds: Vec<Option<usize>> = (0..k).map(|t| p.anchors[t].map(|u| u as usize)).collect();
    for t in 0..k {
        if seeds[t].is_some() {
            continue;
        }
        if let Some(u) = (0..n).find(|&u| a[u] == t as u32) {
            seeds[t] = Some(u);
            continue;
        }
        let chosen: Vec<usize> = seeds.iter().flatten().copied().collect();
        let d2: Vec<f64> = (0..n)
            .map(|u| {
                if a[u] != NONE {
                    return 0.0;
                }
                let d = chosen
                    .iter()
                    .map(|&c| dist2(p.positions[u], p.positions[c]))
                    .fold(f64::INFINITY, f64::min);
                (if d.is_finite() { d } else { 1.0 }) * weight(u).sqrt()
            })
            .collect();
        let total: f64 = d2.iter().sum();
        let mut r = rng.unit() * total;
        let mut pick = rng.below(n);
        for (u, d) in d2.iter().enumerate() {
            r -= d;
            if r <= 0.0 && *d > 0.0 {
                pick = u;
                break;
            }
        }
        seeds[t] = Some(pick);
    }
    let mut sums = vec![0.0; k];
    let targets: Vec<f64> = {
        let total: f64 = (0..n).map(weight).sum();
        let cap: f64 = p.capacity.iter().sum::<f64>().max(1e-9);
        p.capacity.iter().map(|c| total * c / cap).collect()
    };
    for (t, s) in seeds.iter().enumerate() {
        if let Some(u) = s
            && a[*u] == NONE
        {
            a[*u] = t as u32;
        }
    }
    for u in 0..n {
        if a[u] != NONE {
            sums[a[u] as usize] += weight(u);
        }
    }
    let center: Vec<[f64; 2]> = seeds
        .iter()
        .map(|s| s.map_or([0.0, 0.0], |u| p.positions[u]))
        .collect();
    loop {
        // Territories in order of how far they are below target.
        let mut order: Vec<usize> = (0..k).filter(|&t| !p.locked[t]).collect();
        order.sort_by(|&x, &y| {
            (sums[x] / targets[x].max(1e-9)).total_cmp(&(sums[y] / targets[y].max(1e-9)))
        });
        let mut grew = false;
        for t in order {
            // Its nearest free neighbouring unit.
            let mut best: Option<(usize, f64)> = None;
            for u in 0..n {
                if a[u] != t as u32 {
                    continue;
                }
                for &(v, _) in &p.neighbors[u] {
                    let v = v as usize;
                    if a[v] == NONE {
                        let d = dist2(p.positions[v], center[t]);
                        if best.is_none_or(|(_, b)| d < b) {
                            best = Some((v, d));
                        }
                    }
                }
            }
            if let Some((v, _)) = best {
                a[v] = t as u32;
                sums[t] += weight(v);
                grew = true;
                break;
            }
        }
        if !grew {
            break;
        }
    }
    // Islands nobody reached: nearest territory centre.
    for (u, slot) in a.iter_mut().enumerate() {
        if *slot == NONE && !p.pinned[u] {
            let t = (0..k).filter(|&t| !p.locked[t]).min_by(|&x, &y| {
                dist2(p.positions[u], center[x]).total_cmp(&dist2(p.positions[u], center[y]))
            });
            if let Some(t) = t {
                *slot = t as u32;
            }
        }
    }
    a
}

fn dist2(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An n×n grid of unit squares, 4-neighbourhood.
    fn grid(n: usize, k: usize) -> Problem {
        let units = n * n;
        let mut neighbors = vec![Vec::new(); units];
        for y in 0..n {
            for x in 0..n {
                let u = y * n + x;
                if x + 1 < n {
                    neighbors[u].push((u as u32 + 1, 1.0));
                    neighbors[u + 1].push((u as u32, 1.0));
                }
                if y + 1 < n {
                    neighbors[u].push(((u + n) as u32, 1.0));
                    neighbors[u + n].push((u as u32, 1.0));
                }
            }
        }
        Problem {
            positions: (0..units)
                .map(|u| [(u % n) as f64, (u / n) as f64])
                .collect(),
            areas: vec![1.0; units],
            neighbors,
            measures: vec![Measure {
                values: vec![1.0; units],
                importance: 1.0,
                tolerance: 0.05,
            }],
            capacity: vec![1.0; k],
            locked: vec![false; k],
            pinned: vec![false; units],
            initial: vec![None; units],
            anchors: vec![None; k],
            sites: vec![None; k],
        }
    }

    #[test]
    fn balances_a_lopsided_plan() {
        let mut p = grid(12, 2);
        // Territory 0 has 3 columns, territory 1 has 9.
        p.initial = (0..144)
            .map(|u| Some(if u % 12 < 3 { 0 } else { 1 }))
            .collect();
        let s = optimize(
            &p,
            &Options {
                compactness: 0.3,
                ..Default::default()
            },
            &mut |_| true,
        );
        assert!(s.before.max_deviation[0] > 0.4);
        assert!(s.after.max_deviation[0] <= 0.05, "{:?}", s.after);
        assert_eq!(s.after.split, 0);
        // 36 → 72 units needs at least 36 moves; stability keeps it near that.
        assert!((36..=44).contains(&s.moved), "moved {}", s.moved);
    }

    #[test]
    fn reorganizes_from_scratch() {
        let p = grid(16, 4);
        let s = optimize(
            &p,
            &Options {
                mode: Mode::Reorganize,
                ..Default::default()
            },
            &mut |_| true,
        );
        assert_eq!(s.after.unassigned, 0);
        assert_eq!(s.after.split, 0);
        assert!(s.after.max_deviation[0] <= 0.1, "{:?}", s.after);
        // Four 8×8-ish blocks have 48 units of inner border; allow some slack.
        assert!(s.after.border_length < 90.0, "{:?}", s.after);
    }

    #[test]
    fn respects_locks_and_pins() {
        let mut p = grid(8, 2);
        p.initial = (0..64)
            .map(|u| Some(if u % 8 < 2 { 0 } else { 1 }))
            .collect();
        p.locked[0] = true;
        let s = optimize(&p, &Options::default(), &mut |_| true);
        assert_eq!(s.assignment, p.initial);
        p.locked[0] = false;
        // Pin the whole border column of territory 1: nothing can move.
        for y in 0..8 {
            p.pinned[y * 8 + 2] = true;
            p.pinned[y * 8 + 1] = true;
        }
        let s = optimize(&p, &Options::default(), &mut |_| true);
        assert_eq!(s.assignment, p.initial);
    }

    #[test]
    fn territories_form_around_sites() {
        let mut p = grid(12, 2);
        // Sites in the left and right thirds; anchors are their units.
        p.anchors = vec![Some(6 * 12 + 2), Some(6 * 12 + 9)];
        p.sites = vec![Some([2.0, 6.0]), Some([9.0, 6.0])];
        let s = optimize(
            &p,
            &Options {
                mode: Mode::Reorganize,
                compactness: 0.8,
                ..Default::default()
            },
            &mut |_| true,
        );
        assert_eq!(s.after.split, 0);
        assert!(s.after.max_deviation[0] <= 0.05, "{:?}", s.after);
        // Each site's unit stays with its territory, and the left column
        // belongs to the left site's territory.
        let a = &s.assignment;
        assert_eq!(a[6 * 12 + 2], Some(0));
        assert_eq!(a[6 * 12 + 9], Some(1));
        assert_eq!(a[0], Some(0));
        assert_eq!(a[11], Some(1));
    }

    #[test]
    fn deterministic_for_a_seed() {
        let mut p = grid(10, 3);
        p.initial = (0..100).map(|u| Some((u % 10 / 4).min(2) as u32)).collect();
        let a = optimize(&p, &Options::default(), &mut |_| true).assignment;
        let b = optimize(&p, &Options::default(), &mut |_| true).assignment;
        assert_eq!(a, b);
    }

    #[test]
    fn stops_when_asked() {
        let mut p = grid(10, 2);
        p.initial = (0..100)
            .map(|u| Some(if u % 10 < 2 { 0 } else { 1 }))
            .collect();
        let s = optimize(&p, &Options::default(), &mut |_| false);
        assert!(s.stopped_early);
        assert_eq!(s.after.split, 0);
    }
}
