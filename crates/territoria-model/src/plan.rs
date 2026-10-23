//! Territory plans: units of a layer assigned to territories, with the
//! figures that tell whether the territories are balanced.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use territoria_analysis::Rgba;
use territoria_data::{KeyIndex, MatchMode, MatchStats, Table, keys};

use crate::values::{Cache, ValueSource, column_keys, layer_keys};
use crate::{LayerGeometry, LayerId, PlanId, Project, RegionId, TableId, TerritoryId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: PlanId,
    pub name: String,
    /// The layer whose features are the units.
    pub layer: LayerId,
    pub territories: Vec<Territory>,
    /// Per unit, its territory. Saved separately (see `file`).
    #[serde(skip)]
    pub assignment: Arc<Vec<Option<TerritoryId>>>,
    /// Per unit, whether its assignment is pinned (locked).
    #[serde(skip)]
    pub pinned: Arc<Vec<bool>>,
    /// What territories are compared by.
    pub measures: Vec<Measure>,
    next_territory: u32,
    pub rev: u64,
    /// Groups of territories (sales regions), one level above them.
    #[serde(default)]
    pub regions: Vec<Region>,
    #[serde(default)]
    next_region: u32,
}

/// A group of territories.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub id: RegionId,
    pub code: String,
    pub name: String,
    pub color: Rgba,
}

/// A region's figures: its territories' summed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegionStats {
    pub id: RegionId,
    pub territories: usize,
    pub units: usize,
    pub area: f64,
    pub values: Vec<f64>,
    pub targets: Vec<f64>,
    pub deviations: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Territory {
    pub id: TerritoryId,
    /// Short code people use (`T01`, a rep number, …).
    pub code: String,
    pub name: String,
    pub color: Rgba,
    /// Relative size: 1.0 is a normal territory, 0.5 one for a part-time rep.
    pub weight: f64,
    /// Locked territories keep their units.
    pub locked: bool,
    /// The territory's site (rep home, branch) as longitude/latitude.
    pub site: Option<[f64; 2]>,
    pub notes: String,
    /// The region it belongs to.
    #[serde(default)]
    pub region: Option<RegionId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measure {
    pub source: ValueSource,
    /// Accepted deviation from the target, e.g. 0.1 for ±10 %.
    pub tolerance: f64,
    /// Target per territory of weight 1; `None` = the average.
    pub target: Option<f64>,
}

/// Pleasant, distinct fills that keep dark labels readable.
pub const TERRITORY_COLORS: [Rgba; 12] = [
    Rgba::rgb(0x8d, 0xd3, 0xc7),
    Rgba::rgb(0xfd, 0xb4, 0x62),
    Rgba::rgb(0xbe, 0xba, 0xda),
    Rgba::rgb(0xfb, 0x80, 0x72),
    Rgba::rgb(0x80, 0xb1, 0xd3),
    Rgba::rgb(0xb3, 0xde, 0x69),
    Rgba::rgb(0xfc, 0xcd, 0xe5),
    Rgba::rgb(0xff, 0xed, 0x6f),
    Rgba::rgb(0xbc, 0x80, 0xbd),
    Rgba::rgb(0xcc, 0xeb, 0xc5),
    Rgba::rgb(0xe5, 0xc4, 0x94),
    Rgba::rgb(0x9e, 0xca, 0xe1),
];

impl Plan {
    pub fn new(id: PlanId, name: &str, layer: LayerId, units: usize) -> Plan {
        Plan {
            id,
            name: name.to_owned(),
            layer,
            territories: Vec::new(),
            assignment: Arc::new(vec![None; units]),
            pinned: Arc::new(vec![false; units]),
            measures: vec![Measure {
                source: ValueSource::Units,
                tolerance: 0.15,
                target: None,
            }],
            next_territory: 0,
            rev: 0,
            regions: Vec::new(),
            next_region: 0,
        }
    }

    pub fn region(&self, id: RegionId) -> Option<&Region> {
        self.regions.iter().find(|r| r.id == id)
    }

    /// Adds a region with the next code and a colour.
    pub fn add_region(&mut self, name: Option<String>) -> RegionId {
        self.next_region += 1;
        let id = RegionId(self.next_region);
        let n = self.regions.len();
        let code = format!("R{}", self.next_region);
        // Darker colours than territories: regions are drawn as borders.
        const REGION_COLORS: [Rgba; 6] = [
            Rgba([0x1f, 0x4e, 0x79, 255]),
            Rgba([0x7a, 0x1f, 0x4e, 0xff]),
            Rgba([0x2e, 0x6b, 0x30, 0xff]),
            Rgba([0x8a, 0x55, 0x10, 0xff]),
            Rgba([0x4b, 0x2c, 0x7a, 0xff]),
            Rgba([0x16, 0x6a, 0x6a, 0xff]),
        ];
        self.regions.push(Region {
            id,
            name: name.unwrap_or_else(|| format!("Region {}", n + 1)),
            code,
            color: REGION_COLORS[n % REGION_COLORS.len()],
        });
        id
    }

    /// Removes a region; its territories belong to none.
    pub fn remove_region(&mut self, id: RegionId) {
        self.regions.retain(|r| r.id != id);
        for t in &mut self.territories {
            if t.region == Some(id) {
                t.region = None;
            }
        }
    }

    /// Per unit its region, through its territory.
    pub fn unit_regions(&self) -> Vec<Option<RegionId>> {
        let of: HashMap<TerritoryId, Option<RegionId>> =
            self.territories.iter().map(|t| (t.id, t.region)).collect();
        self.assignment
            .iter()
            .map(|t| t.and_then(|t| of.get(&t).copied().flatten()))
            .collect()
    }

    /// Each region's figures from its territories'.
    pub fn region_stats(&self, stats: &PlanStats) -> Vec<RegionStats> {
        self.regions
            .iter()
            .map(|r| {
                let mut s = RegionStats {
                    id: r.id,
                    ..Default::default()
                };
                for (t, ts) in self.territories.iter().zip(&stats.territories) {
                    if t.region != Some(r.id) {
                        continue;
                    }
                    s.territories += 1;
                    s.units += ts.units;
                    s.area += ts.area;
                    if s.values.is_empty() {
                        s.values = vec![0.0; ts.values.len()];
                        s.targets = vec![0.0; ts.targets.len()];
                    }
                    for (a, b) in s.values.iter_mut().zip(&ts.values) {
                        *a += b;
                    }
                    for (a, b) in s.targets.iter_mut().zip(&ts.targets) {
                        *a += b;
                    }
                }
                s.deviations = s
                    .values
                    .iter()
                    .zip(&s.targets)
                    .map(|(v, t)| if *t != 0.0 { v / t - 1.0 } else { 0.0 })
                    .collect();
                s
            })
            .collect()
    }

    pub fn territory(&self, id: TerritoryId) -> Option<&Territory> {
        self.territories.iter().find(|t| t.id == id)
    }

    pub fn territory_mut(&mut self, id: TerritoryId) -> Option<&mut Territory> {
        self.territories.iter_mut().find(|t| t.id == id)
    }

    pub fn territory_index(&self, id: TerritoryId) -> Option<usize> {
        self.territories.iter().position(|t| t.id == id)
    }

    /// Adds a territory with the next free code and a colour.
    pub fn add_territory(&mut self, code: Option<String>, name: Option<String>) -> TerritoryId {
        self.next_territory += 1;
        let id = TerritoryId(self.next_territory);
        let code = code.unwrap_or_else(|| self.next_code());
        let color = TERRITORY_COLORS[self.territories.len() % TERRITORY_COLORS.len()];
        self.territories.push(Territory {
            id,
            name: name.unwrap_or_else(|| format!("Territory {code}")),
            code,
            color,
            weight: 1.0,
            locked: false,
            site: None,
            notes: String::new(),
            region: None,
        });
        id
    }

    fn next_code(&self) -> String {
        let used: HashSet<&str> = self.territories.iter().map(|t| t.code.as_str()).collect();
        (1..)
            .map(|i| format!("{i:02}"))
            .find(|c| !used.contains(c.as_str()))
            .unwrap()
    }

    /// Removes a territory; its units become unassigned.
    pub fn remove_territory(&mut self, id: TerritoryId) {
        self.territories.retain(|t| t.id != id);
        let a = Arc::make_mut(&mut self.assignment);
        for slot in a.iter_mut() {
            if *slot == Some(id) {
                *slot = None;
            }
        }
    }

    /// Assigns units (skipping pinned units and locked territories on either
    /// side). Returns how many changed.
    pub fn assign(&mut self, units: &[u32], to: Option<TerritoryId>) -> usize {
        if to.and_then(|t| self.territory(t)).is_some_and(|t| t.locked) {
            return 0;
        }
        let locked: HashSet<TerritoryId> = self
            .territories
            .iter()
            .filter(|t| t.locked)
            .map(|t| t.id)
            .collect();
        let pinned = self.pinned.clone();
        let a = Arc::make_mut(&mut self.assignment);
        let mut changed = 0;
        for &u in units {
            let u = u as usize;
            if u >= a.len() || pinned.get(u).copied().unwrap_or(false) {
                continue;
            }
            if a[u].is_some_and(|t| locked.contains(&t)) {
                continue;
            }
            if a[u] != to {
                a[u] = to;
                changed += 1;
            }
        }
        changed
    }

    pub fn set_pinned(&mut self, units: &[u32], pinned: bool) {
        let p = Arc::make_mut(&mut self.pinned);
        for &u in units {
            if let Some(slot) = p.get_mut(u as usize) {
                *slot = pinned;
            }
        }
    }

    /// Units per territory, in territory order.
    pub fn members(&self) -> Vec<Vec<u32>> {
        let index: HashMap<TerritoryId, usize> = self
            .territories
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, i))
            .collect();
        let mut out = vec![Vec::new(); self.territories.len()];
        for (u, t) in self.assignment.iter().enumerate() {
            if let Some(i) = t.and_then(|t| index.get(&t)) {
                out[*i].push(u as u32);
            }
        }
        out
    }

    /// Territories whose units touch, as index pairs.
    pub fn territory_neighbors(&self, geometry: &LayerGeometry) -> Vec<HashSet<usize>> {
        let index: HashMap<TerritoryId, usize> = self
            .territories
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, i))
            .collect();
        let mut out = vec![HashSet::new(); self.territories.len()];
        for (u, t) in self.assignment.iter().enumerate() {
            let Some(&a) = t.and_then(|t| index.get(&t)) else {
                continue;
            };
            for &(n, _) in geometry
                .neighbors
                .get(u)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                if let Some(&b) = self.assignment[n as usize].and_then(|t| index.get(&t))
                    && a != b
                {
                    out[a].insert(b);
                    out[b].insert(a);
                }
            }
        }
        out
    }

    /// Gives neighbouring territories different colours (greedy, most
    /// constrained first), from [`TERRITORY_COLORS`].
    /// Names each territory after its largest unit (by `weights` per unit,
    /// e.g. the measure; by area without). Locked territories keep their
    /// names. Returns how many were renamed.
    pub fn name_after_largest(
        &mut self,
        layer: &crate::Layer,
        weights: Option<&[Option<f64>]>,
    ) -> usize {
        let members = self.members();
        let size = |u: u32| match weights {
            Some(w) => w.get(u as usize).copied().flatten().unwrap_or(0.0),
            None => layer.geometry.areas.get(u as usize).copied().unwrap_or(0.0),
        };
        // The place name without the key, when there is one.
        let place = |u: u32| {
            layer
                .name_column
                .as_deref()
                .and_then(|c| layer.attributes.column(c))
                .map(|c| c.display(u as usize))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| layer.key(u as usize))
        };
        let mut used: HashMap<String, usize> = HashMap::new();
        let mut renamed = 0;
        for (t, units) in self.territories.iter_mut().zip(&members) {
            if t.locked {
                continue;
            }
            let Some(&best) = units.iter().max_by(|a, b| size(**a).total_cmp(&size(**b))) else {
                continue;
            };
            let base = place(best);
            let n = used.entry(base.clone()).or_insert(0);
            *n += 1;
            let name = if *n == 1 { base } else { format!("{base} {n}") };
            if t.name != name {
                t.name = name;
                renamed += 1;
            }
        }
        renamed
    }

    /// Orders and numbers territories from north to south (west to east
    /// within a band), so codes follow the map. Default names follow.
    pub fn renumber_by_position(&mut self, geometry: &LayerGeometry) {
        let members = self.members();
        let centre = |units: &Vec<u32>| -> Option<(f64, f64)> {
            let pts: Vec<_> = units
                .iter()
                .filter_map(|&u| geometry.label_points.get(u as usize).copied().flatten())
                .collect();
            (!pts.is_empty()).then(|| {
                let n = pts.len() as f64;
                (
                    pts.iter().map(|p| p.x).sum::<f64>() / n,
                    pts.iter().map(|p| p.y).sum::<f64>() / n,
                )
            })
        };
        let centres: Vec<Option<(f64, f64)>> = members.iter().map(centre).collect();
        // Bands of about a territory's height, so a row reads west to east.
        let ys: Vec<f64> = centres.iter().flatten().map(|c| c.1).collect();
        let (lo, hi) = ys
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &y| {
                (a.min(y), b.max(y))
            });
        let rows = (self.territories.len() as f64).sqrt().ceil().max(1.0);
        let band = ((hi - lo) / rows).max(1e-12);
        let mut order: Vec<usize> = (0..self.territories.len()).collect();
        order.sort_by(|&a, &b| match (centres[a], centres[b]) {
            (Some(ca), Some(cb)) => {
                let (ra, rb) = (((hi - ca.1) / band).floor(), ((hi - cb.1) / band).floor());
                ra.total_cmp(&rb).then(ca.0.total_cmp(&cb.0))
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.cmp(&b),
        });
        let width = self.territories.len().to_string().len().max(2);
        let old = std::mem::take(&mut self.territories);
        for (i, k) in order.into_iter().enumerate() {
            let mut t = old[k].clone();
            let code = format!("{:0width$}", i + 1);
            if t.name == format!("Territory {}", t.code) {
                t.name = format!("Territory {code}");
            }
            t.code = code;
            self.territories.push(t);
        }
    }

    pub fn recolor(&mut self, geometry: &LayerGeometry) {
        let adj = self.territory_neighbors(geometry);
        let n = self.territories.len();
        let mut color: Vec<Option<usize>> = vec![None; n];
        for _ in 0..n {
            // Most distinct neighbour colours first, then most neighbours.
            let next = (0..n).filter(|&i| color[i].is_none()).max_by_key(|&i| {
                let sat: HashSet<usize> = adj[i].iter().filter_map(|&j| color[j]).collect();
                (sat.len(), adj[i].len(), usize::MAX - i)
            });
            let Some(i) = next else { break };
            let used: HashSet<usize> = adj[i].iter().filter_map(|&j| color[j]).collect();
            // The least used colour among the free ones keeps the map varied.
            let mut counts = [0usize; TERRITORY_COLORS.len()];
            for c in color.iter().flatten() {
                counts[*c] += 1;
            }
            let pick = (0..TERRITORY_COLORS.len())
                .filter(|c| !used.contains(c))
                .min_by_key(|&c| (counts[c], c))
                .unwrap_or(i % TERRITORY_COLORS.len());
            color[i] = Some(pick);
        }
        for (t, c) in self.territories.iter_mut().zip(color) {
            t.color = TERRITORY_COLORS[c.unwrap_or(0)];
        }
    }

    /// Figures per territory for the plan's measures.
    pub fn stats(&self, project: &Project, cache: &mut Cache) -> PlanStats {
        let Some(layer) = project.layer(self.layer) else {
            return PlanStats::default();
        };
        let geometry = &layer.geometry;
        let members = self.members();
        let measure_values: Vec<Vec<Option<f64>>> = self
            .measures
            .iter()
            .map(|m| cache.values(project, self.layer, &m.source))
            .collect();
        let mut territories: Vec<TerritoryStats> = self
            .territories
            .iter()
            .zip(&members)
            .map(|(t, units)| TerritoryStats {
                id: t.id,
                units: units.len(),
                area: units
                    .iter()
                    .map(|&u| geometry.areas.get(u as usize).copied().unwrap_or(0.0))
                    .sum(),
                parts: geometry.components(units),
                site_reach: t.site.filter(|_| geometry.geographic).map(|[x, y]| {
                    let site = geo_types::Coord { x, y };
                    units
                        .iter()
                        .filter_map(|&u| geometry.label_points.get(u as usize).copied().flatten())
                        .map(|p| territoria_geo::topology::haversine(site, p) / 1000.0)
                        .fold(0.0, f64::max)
                }),
                values: measure_values
                    .iter()
                    .map(|v| {
                        units
                            .iter()
                            .filter_map(|&u| v.get(u as usize).copied().flatten())
                            .sum()
                    })
                    .collect(),
                targets: Vec::new(),
                deviations: Vec::new(),
            })
            .collect();
        let total_weight: f64 = self.territories.iter().map(|t| t.weight.max(0.0)).sum();
        let mut totals = Vec::new();
        let mut unassigned_values = Vec::new();
        for (m, measure) in self.measures.iter().enumerate() {
            let assigned: f64 = territories.iter().map(|t| t.values[m]).sum();
            let unassigned: f64 = self
                .assignment
                .iter()
                .enumerate()
                .filter(|(_, t)| t.is_none())
                .filter_map(|(u, _)| measure_values[m].get(u).copied().flatten())
                .sum();
            totals.push(assigned + unassigned);
            unassigned_values.push(unassigned);
            // Targets share everything, assigned or not, by weight.
            let per_weight = measure.target.unwrap_or_else(|| {
                if total_weight > 0.0 {
                    (assigned + unassigned) / total_weight
                } else {
                    0.0
                }
            });
            for (ts, t) in territories.iter_mut().zip(&self.territories) {
                let target = per_weight * t.weight;
                ts.targets.push(target);
                ts.deviations.push(if target != 0.0 {
                    ts.values[m] / target - 1.0
                } else {
                    0.0
                });
            }
        }
        PlanStats {
            unassigned: self.assignment.iter().filter(|t| t.is_none()).count(),
            territories,
            totals,
            unassigned_values,
        }
    }

    /// Puts each territory's site at its centre: the label point of the unit
    /// nearest to the territory's area-weighted centre (so it's inside).
    /// Territories with a site keep it unless `replace`.
    pub fn sites_at_centres(
        &mut self,
        geometry: &LayerGeometry,
        replace: bool,
        weights: Option<&[Option<f64>]>,
    ) -> usize {
        let members = self.members();
        let centres = self.centres(geometry, weights);
        let mut placed = 0;
        for ((t, units), centre) in self.territories.iter_mut().zip(&members).zip(centres) {
            if (t.site.is_some() && !replace) || units.is_empty() {
                continue;
            }
            let Some(c) = centre else { continue };
            // The unit nearest the centre, so the site lies in the territory.
            let best = units
                .iter()
                .filter_map(|&u| geometry.label_points.get(u as usize).copied().flatten())
                .min_by(|a, b| {
                    let da = (a.x - c.x).powi(2) + (a.y - c.y).powi(2);
                    let db = (b.x - c.x).powi(2) + (b.y - c.y).powi(2);
                    da.total_cmp(&db)
                });
            if let Some(p) = best {
                t.site = Some([p.x, p.y]);
                placed += 1;
            }
        }
        placed
    }

    /// The unit each territory's site lies in (`None` without a site or
    /// outside every unit).
    pub fn site_units(&self, geometry: &LayerGeometry) -> Vec<Option<u32>> {
        self.territories
            .iter()
            .map(|t| {
                let [x, y] = t.site?;
                geometry
                    .feature_at(geo_types::Coord { x, y }, 0.0)
                    .map(|u| u as u32)
            })
            .collect()
    }

    /// Each territory's centre: the weighted mean of its units' label
    /// points (by `weights`, else by area).
    pub fn centres(
        &self,
        geometry: &LayerGeometry,
        weights: Option<&[Option<f64>]>,
    ) -> Vec<Option<geo_types::Coord<f64>>> {
        self.members()
            .iter()
            .map(|units| {
                let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
                for &u in units {
                    let Some(p) = geometry.label_points.get(u as usize).copied().flatten() else {
                        continue;
                    };
                    let w = match weights {
                        Some(w) => w.get(u as usize).copied().flatten().unwrap_or(0.0),
                        None => geometry.areas.get(u as usize).copied().unwrap_or(1.0),
                    }
                    .max(1e-9);
                    sx += p.x * w;
                    sy += p.y * w;
                    sw += w;
                }
                (sw > 0.0).then(|| geo_types::Coord {
                    x: sx / sw,
                    y: sy / sw,
                })
            })
            .collect()
    }

    /// Sites from given locations (rep homes, branches): each territory
    /// gets one, with the least total distance from the territories'
    /// centres. Returns per territory the index of its location.
    pub fn sites_from(
        &mut self,
        geometry: &LayerGeometry,
        locations: &[geo_types::Coord<f64>],
        weights: Option<&[Option<f64>]>,
    ) -> Vec<Option<usize>> {
        let centres = self.centres(geometry, weights);
        let cost: Vec<Vec<f64>> = centres
            .iter()
            .map(|c| {
                locations
                    .iter()
                    .map(|l| c.map_or(1e12, |c| territoria_geo::topology::haversine(c, *l)))
                    .collect()
            })
            .collect();
        let matched = crate::location::assign(&cost);
        for (t, m) in self.territories.iter_mut().zip(&matched) {
            if let Some(j) = m {
                t.site = Some([locations[*j].x, locations[*j].y]);
            }
        }
        matched
    }

    /// Chooses as many of the candidate locations as there are territories,
    /// where they serve the units best (by `weights`, e.g. revenue), and
    /// gives them to the territories they are nearest to. Returns per
    /// territory the index of its location.
    pub fn best_sites(
        &mut self,
        geometry: &LayerGeometry,
        candidates: &[geo_types::Coord<f64>],
        weights: Option<&[Option<f64>]>,
    ) -> Vec<Option<usize>> {
        let units: Vec<(usize, geo_types::Coord<f64>)> = (0..geometry.len())
            .filter_map(|u| {
                geometry
                    .label_points
                    .get(u)
                    .copied()
                    .flatten()
                    .map(|p| (u, p))
            })
            .collect();
        let w: Vec<f64> = units
            .iter()
            .map(|(u, _)| weights.map_or(1.0, |w| w.get(*u).copied().flatten().unwrap_or(0.0)))
            .collect();
        let chosen = crate::location::p_median(
            units.len(),
            &w,
            candidates.len(),
            self.territories.len(),
            &|i, j| territoria_geo::topology::haversine(units[i].1, candidates[j]),
        );
        let picked: Vec<geo_types::Coord<f64>> = chosen.iter().map(|&j| candidates[j]).collect();
        self.sites_from(geometry, &picked, weights)
            .into_iter()
            .map(|m| m.map(|k| chosen[k]))
            .collect()
    }

    /// Units without a territory get the territory most of their assigned
    /// neighbours have, repeatedly, so enclosed gaps fill from outside in.
    /// Units with no assigned neighbour at all stay unassigned.
    pub fn fill_unassigned(&mut self, geometry: &LayerGeometry) -> usize {
        let mut changed = 0;
        let pinned = self.pinned.clone();
        loop {
            let a = self.assignment.clone();
            let mut updates = Vec::new();
            for (u, t) in a.iter().enumerate() {
                if t.is_some() || pinned[u] {
                    continue;
                }
                let mut votes: BTreeMap<TerritoryId, f64> = BTreeMap::new();
                for &(n, len) in &geometry.neighbors[u] {
                    if let Some(nt) = a[n as usize] {
                        *votes.entry(nt).or_default() += len;
                    }
                }
                if let Some((t, _)) = votes.into_iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
                    updates.push((u as u32, t));
                }
            }
            if updates.is_empty() {
                break;
            }
            for (u, t) in updates {
                changed += self.assign(&[u], Some(t));
            }
        }
        changed
    }
}

/// Tables and outlines for handing a plan over (CRM, spreadsheets, GIS).
impl Plan {
    /// One row per unit: its key and name, its territory's code and name.
    pub fn assignment_table(&self, project: &Project) -> Table {
        use territoria_data::{Column, ColumnKind};
        let Some(layer) = project.layer(self.layer) else {
            return Table::default();
        };
        let n = layer.len();
        let territory = |u: usize| {
            self.assignment
                .get(u)
                .copied()
                .flatten()
                .and_then(|t| self.territory(t))
        };
        let name_col = layer
            .name_column
            .as_deref()
            .and_then(|c| layer.attributes.column(c));
        let mut columns = vec![Column::text(
            layer.key_column.clone().unwrap_or_else(|| "Area".into()),
            ColumnKind::Key,
            (0..n).map(|u| Some(layer.key(u))).collect(),
        )];
        if let Some(c) = name_col {
            columns.push(Column::text(
                c.name.clone(),
                ColumnKind::Text,
                (0..n).map(|u| c.get(u).to_text()).collect(),
            ));
        }
        columns.push(Column::text(
            "Territory code",
            ColumnKind::Key,
            (0..n)
                .map(|u| territory(u).map(|t| t.code.clone()))
                .collect(),
        ));
        if !self.regions.is_empty() {
            columns.push(Column::text(
                "Region",
                ColumnKind::Text,
                (0..n)
                    .map(|u| {
                        territory(u)
                            .and_then(|t| t.region)
                            .and_then(|r| self.region(r))
                            .map(|r| r.name.clone())
                    })
                    .collect(),
            ));
        }
        columns.push(Column::text(
            "Territory",
            ColumnKind::Text,
            (0..n)
                .map(|u| territory(u).map(|t| t.name.clone()))
                .collect(),
        ));
        columns.push(Column {
            name: "Pinned".into(),
            kind: ColumnKind::Bool,
            data: territoria_data::ColumnData::Bool(
                (0..n)
                    .map(|u| Some(self.pinned.get(u).copied().unwrap_or(false)))
                    .collect(),
            ),
            formula: None,
        });
        Table::new(columns).unwrap_or_default()
    }

    /// One row per territory with its figures.
    pub fn territory_table(&self, project: &Project, stats: &PlanStats) -> Table {
        use territoria_data::{Column, ColumnKind};
        let t = &self.territories;
        let mut columns = vec![
            Column::text(
                "Code",
                ColumnKind::Key,
                t.iter().map(|t| Some(t.code.clone())).collect(),
            ),
            Column::text(
                "Territory",
                ColumnKind::Text,
                t.iter().map(|t| Some(t.name.clone())).collect(),
            ),
            Column::text(
                "Region",
                ColumnKind::Text,
                t.iter()
                    .map(|t| {
                        t.region
                            .and_then(|r| self.region(r))
                            .map(|r| r.name.clone())
                    })
                    .collect(),
            ),
            {
                let mut c = Column::number(
                    "Areas",
                    stats
                        .territories
                        .iter()
                        .map(|s| Some(s.units as f64))
                        .collect(),
                );
                c.kind = ColumnKind::Integer;
                c
            },
            Column::number(
                "Area km²",
                stats
                    .territories
                    .iter()
                    .map(|s| Some((s.area * 10.0).round() / 10.0))
                    .collect(),
            ),
            Column::number("Weight", t.iter().map(|t| Some(t.weight)).collect()),
            {
                let mut c = Column::number(
                    "Pieces",
                    stats
                        .territories
                        .iter()
                        .map(|s| Some(s.parts as f64))
                        .collect(),
                );
                c.kind = ColumnKind::Integer;
                c
            },
        ];
        for (m, measure) in self.measures.iter().enumerate() {
            let label = measure.source.label(project);
            columns.push(Column::number(
                label.clone(),
                stats
                    .territories
                    .iter()
                    .map(|s| s.values.get(m).copied())
                    .collect(),
            ));
            columns.push(Column::number(
                format!("{label} target"),
                stats
                    .territories
                    .iter()
                    .map(|s| s.targets.get(m).copied())
                    .collect(),
            ));
            columns.push(Column::number(
                format!("{label} deviation %"),
                stats
                    .territories
                    .iter()
                    .map(|s| s.deviations.get(m).map(|d| (d * 1000.0).round() / 10.0))
                    .collect(),
            ));
        }
        columns.push(Column {
            name: "Locked".into(),
            kind: ColumnKind::Bool,
            data: territoria_data::ColumnData::Bool(t.iter().map(|t| Some(t.locked)).collect()),
            formula: None,
        });
        if self.regions.is_empty() {
            columns.retain(|c| c.name != "Region");
        }
        Table::new(columns).unwrap_or_default()
    }

    /// Each territory's outline (its units dissolved), in layer coordinates.
    pub fn outlines(&self, project: &Project) -> Vec<geo_types::MultiPolygon<f64>> {
        let Some(layer) = project.layer(self.layer) else {
            return Vec::new();
        };
        self.members()
            .iter()
            .map(|m| layer.geometry.topology.dissolve(m))
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerritoryStats {
    pub id: TerritoryId,
    pub units: usize,
    /// km²
    pub area: f64,
    /// Separate pieces (1 = contiguous).
    pub parts: usize,
    /// km from the site to the farthest unit (with a site).
    pub site_reach: Option<f64>,
    /// Per measure: value, target and deviation (value / target − 1).
    pub values: Vec<f64>,
    pub targets: Vec<f64>,
    pub deviations: Vec<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanStats {
    pub territories: Vec<TerritoryStats>,
    pub unassigned: usize,
    /// Per measure: the total over all units and the unassigned part.
    pub totals: Vec<f64>,
    pub unassigned_values: Vec<f64>,
}

/// How an assignment table maps onto a layer, before creating a plan.
#[derive(Debug, Clone)]
pub struct AssignmentImport {
    /// Per unit, the territory code.
    pub codes: Vec<Option<String>>,
    /// Territory code → name (first seen).
    pub names: BTreeMap<String, String>,
    pub stats: MatchStats,
    /// Units claimed by more than one territory (the first row wins).
    pub conflicts: usize,
}

/// Reads unit → territory rows. `unit_column` holds unit keys (or lists
/// and ranges like `8000-8099; 8105`), `territory_column` the territory code,
/// `name_column` an optional territory name.
pub fn read_assignment(
    project: &Project,
    layer: LayerId,
    table: &Table,
    unit_column: &str,
    territory_column: &str,
    name_column: Option<&str>,
    mode: MatchMode,
) -> AssignmentImport {
    let units = layer_keys(project, layer);
    let index = KeyIndex::new(units.iter().map(|u| u.as_deref()), mode);
    let unit_keys = column_keys(table, unit_column);
    let codes_col = column_keys(table, territory_column);
    let names_col = name_column.map(|c| column_keys(table, c));
    let mut codes: Vec<Option<String>> = vec![None; units.len()];
    let mut names = BTreeMap::new();
    let mut stats = MatchStats {
        targets: units.len(),
        ..Default::default()
    };
    let mut conflicts = 0;
    for row in 0..table.rows() {
        stats.rows += 1;
        let (Some(key), Some(code)) = (&unit_keys[row], &codes_col[row]) else {
            stats.rows_empty += 1;
            continue;
        };
        if let Some(n) = names_col.as_ref().and_then(|n| n[row].clone()) {
            names.entry(code.clone()).or_insert(n);
        } else {
            names.entry(code.clone()).or_insert_with(|| code.clone());
        }
        let hits = expand_unit_list(key, &units, &index);
        if hits.is_empty() {
            if stats.unmatched_samples.len() < 20 {
                stats.unmatched_samples.push(key.clone());
            }
            continue;
        }
        stats.rows_matched += 1;
        for u in hits {
            match &codes[u as usize] {
                Some(existing) if existing != code => conflicts += 1,
                Some(_) => {}
                None => codes[u as usize] = Some(code.clone()),
            }
        }
    }
    stats.targets_matched = codes.iter().filter(|c| c.is_some()).count();
    AssignmentImport {
        codes,
        names,
        stats,
        conflicts,
    }
}

/// Units named by `spec`: a key, or a list like `8000-8099; 8105, 81*`
/// (ranges compare as numbers when both ends are numbers, `*` and `?`
/// are wildcards, separators `;` `,` `+` tab or new line).
pub fn expand_unit_list(spec: &str, units: &[Option<String>], index: &KeyIndex) -> Vec<u32> {
    if let Some(u) = index.find(spec) {
        return vec![u];
    }
    let mut out = Vec::new();
    for part in spec
        .split([';', ',', '+', '\t', '\n'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        if let Some(u) = index.find(part) {
            out.push(u);
            continue;
        }
        let range = part
            .split_once("...")
            .or_else(|| part.split_once('…'))
            .or_else(|| part.split_once(" bis "))
            .or_else(|| part.split_once(" to "))
            .or_else(|| part.split_once('-'));
        if let Some((from, to)) = range
            && let (Ok(a), Ok(b)) = (from.trim().parse::<u64>(), to.trim().parse::<u64>())
        {
            for (i, k) in units.iter().enumerate() {
                if let Some(v) = k.as_deref().and_then(|k| k.trim().parse::<u64>().ok())
                    && v >= a
                    && v <= b
                {
                    out.push(i as u32);
                }
            }
            continue;
        }
        if part.contains(['*', '?']) {
            for (i, k) in units.iter().enumerate() {
                if k.as_deref().is_some_and(|k| wildcard(part, k)) {
                    out.push(i as u32);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = keys::normalize(pattern, MatchMode::Loose).chars().collect();
    let t: Vec<char> = keys::normalize(text, MatchMode::Loose).chars().collect();
    fn rec(p: &[char], t: &[char]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some('*'), _) => rec(&p[1..], t) || (!t.is_empty() && rec(p, &t[1..])),
            (Some('?'), Some(_)) => rec(&p[1..], &t[1..]),
            (Some(a), Some(b)) if a == b => rec(&p[1..], &t[1..]),
            _ => false,
        }
    }
    rec(&p, &t)
}

impl Project {
    /// Creates a plan from an [`AssignmentImport`].
    pub fn add_plan_from_assignment(
        &mut self,
        name: &str,
        layer: LayerId,
        import: &AssignmentImport,
    ) -> PlanId {
        let id = PlanId(self.next_id());
        let units = import.codes.len();
        let mut plan = Plan::new(id, name, layer, units);
        let mut by_code = HashMap::new();
        for (code, tname) in &import.names {
            let t = plan.add_territory(Some(code.clone()), Some(tname.clone()));
            by_code.insert(code.clone(), t);
        }
        let a: Vec<Option<TerritoryId>> = import
            .codes
            .iter()
            .map(|c| c.as_ref().and_then(|c| by_code.get(c).copied()))
            .collect();
        plan.assignment = Arc::new(a);
        if let Some(l) = self.layer(layer) {
            let geometry = l.geometry.clone();
            plan.recolor(&geometry);
        }
        plan.rev = self.bump();
        self.plans.push(plan);
        id
    }

    pub fn add_plan(&mut self, name: &str, layer: LayerId, territories: usize) -> PlanId {
        let id = PlanId(self.next_id());
        let units = self.layer(layer).map_or(0, |l| l.len());
        let mut plan = Plan::new(id, name, layer, units);
        for _ in 0..territories {
            plan.add_territory(None, None);
        }
        plan.rev = self.bump();
        self.plans.push(plan);
        id
    }

    /// A copy of a plan to try changes on (a scenario). Territories keep
    /// their ids, so the copies can be compared.
    pub fn duplicate_plan(&mut self, id: PlanId, name: &str) -> Option<PlanId> {
        let mut plan = self.plan(id)?.clone();
        plan.id = PlanId(self.next_id());
        plan.name = name.to_owned();
        plan.rev = self.bump();
        let new = plan.id;
        self.plans.push(plan);
        Some(new)
    }

    /// How rows of a layer's attributes relate to `units`' features: the
    /// features themselves, or the area each point lies in.
    pub fn layer_rows(
        &self,
        cache: &mut Cache,
        layer: LayerId,
        units: LayerId,
    ) -> Option<RowUnits> {
        if layer == units {
            return Some(RowUnits::Same);
        }
        let (l, u) = (self.layer(layer)?, self.layer(units)?);
        if l.kind() == territoria_io::GeometryKind::Point
            && u.kind() == territoria_io::GeometryKind::Area
        {
            return cache.point_areas(self, layer, units).map(RowUnits::Points);
        }
        None
    }

    /// How rows of a table relate to `units`' features: by its link.
    pub fn table_rows(
        &self,
        cache: &mut Cache,
        table: TableId,
        units: LayerId,
    ) -> Option<RowUnits> {
        let link = self
            .links
            .iter()
            .find(|l| l.table == table && l.layer == units)?;
        cache.matching(self, link.id).map(RowUnits::Matched)
    }

    /// Takes over a scenario: `into` gets the territories and assignment of
    /// `from` and keeps its own name.
    pub fn adopt_plan(&mut self, from: PlanId, into: PlanId) -> bool {
        let Some(source) = self.plan(from).cloned() else {
            return false;
        };
        let rev = self.bump();
        let Some(target) = self.plan_mut(into).filter(|t| t.layer == source.layer) else {
            return false;
        };
        target.territories = source.territories;
        target.assignment = source.assignment;
        target.pinned = source.pinned;
        target.measures = source.measures;
        target.next_territory = target.next_territory.max(source.next_territory);
        target.regions = source.regions;
        target.next_region = target.next_region.max(source.next_region);
        target.rev = rev;
        true
    }

    /// What differs between two plans on the same layer.
    pub fn compare_plans(&self, a: PlanId, b: PlanId, cache: &mut Cache) -> Option<Comparison> {
        let (pa, pb) = (self.plan(a)?, self.plan(b)?);
        if pa.layer != pb.layer {
            return None;
        }
        let moves: Vec<Move> = pa
            .assignment
            .iter()
            .zip(pb.assignment.iter())
            .enumerate()
            .filter(|(_, (x, y))| x != y)
            .map(|(u, (x, y))| Move {
                unit: u as u32,
                from: *x,
                to: *y,
            })
            .collect();
        // Points (customers) in the areas that moved, per point layer.
        let mut points = Vec::new();
        if self
            .layer(pa.layer)
            .is_some_and(|l| l.kind() == territoria_io::GeometryKind::Area)
        {
            let moved: HashSet<u32> = moves.iter().map(|m| m.unit).collect();
            for l in self
                .layers
                .iter()
                .filter(|l| l.kind() == territoria_io::GeometryKind::Point)
            {
                if let Some(hits) = cache.point_areas(self, l.id, pa.layer) {
                    let n = hits.iter().flatten().filter(|u| moved.contains(u)).count();
                    points.push((l.id, n));
                }
            }
        }
        // Per territory (matched by id) its figures in both plans, by the
        // measure of the second plan.
        let source = pb
            .measures
            .first()
            .or(pa.measures.first())
            .map_or(ValueSource::Units, |m| m.source.clone());
        let values = cache.values(self, pa.layer, &source);
        let sum = |p: &Plan, t: TerritoryId| -> (usize, f64) {
            p.assignment
                .iter()
                .enumerate()
                .filter(|(_, x)| **x == Some(t))
                .fold((0, 0.0), |(n, s), (u, _)| {
                    (n + 1, s + values.get(u).copied().flatten().unwrap_or(0.0))
                })
        };
        let mut territories: Vec<TerritoryChange> = Vec::new();
        for t in pb.territories.iter().chain(&pa.territories) {
            if territories.iter().any(|c| c.id == t.id) {
                continue;
            }
            let (ua, va) = if pa.territory(t.id).is_some() {
                sum(pa, t.id)
            } else {
                (0, 0.0)
            };
            let (ub, vb) = if pb.territory(t.id).is_some() {
                sum(pb, t.id)
            } else {
                (0, 0.0)
            };
            territories.push(TerritoryChange {
                id: t.id,
                name: t.name.clone(),
                color: t.color,
                units: [ua, ub],
                values: [va, vb],
                gained: moves.iter().filter(|m| m.to == Some(t.id)).count(),
                lost: moves.iter().filter(|m| m.from == Some(t.id)).count(),
            });
        }
        Some(Comparison {
            a,
            b,
            moves,
            points,
            measure: source.label(self),
            territories,
        })
    }
}

/// Something about a plan's data that keeps territories from balancing
/// or explains why they don't.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// `true`: balancing can't succeed; `false`: worth knowing.
    pub serious: bool,
    pub text: String,
    /// The units it is about, to show them.
    pub units: Vec<u32>,
}

impl Plan {
    /// Checks the plan's data before balancing.
    pub fn diagnose(&self, project: &Project, cache: &mut Cache) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let Some(layer) = project.layer(self.layer) else {
            return out;
        };
        let n = layer.len();
        let fmt = territoria_analysis::format::format_value;
        let names = |units: &[u32]| -> String {
            let mut list: Vec<String> = units
                .iter()
                .take(3)
                .map(|&u| layer.display_name(u as usize))
                .collect();
            if units.len() > 3 {
                list.push(format!("{} more", units.len() - 3));
            }
            list.join(", ")
        };
        let territories = self.territories.len();
        if territories > 0 && n < territories {
            out.push(Diagnostic {
                serious: true,
                text: format!(
                    "Only {n} areas for {territories} territories: some territories stay empty."
                ),
                units: Vec::new(),
            });
        }
        if let Some(m) = self.measures.first() {
            let values = cache.values(project, self.layer, &m.source);
            let label = m.source.label(project);
            let negative: Vec<u32> = (0..n as u32)
                .filter(|&u| {
                    values
                        .get(u as usize)
                        .copied()
                        .flatten()
                        .is_some_and(|v| v < 0.0)
                })
                .collect();
            if !negative.is_empty() {
                out.push(Diagnostic {
                    serious: true,
                    text: format!("{} areas have a negative {label} ({}); balancing treats them as taking away.", negative.len(), names(&negative)),
                    units: negative,
                });
            }
            let missing: Vec<u32> = (0..n as u32)
                .filter(|&u| values.get(u as usize).copied().flatten().is_none())
                .collect();
            if !missing.is_empty() && missing.len() < n {
                out.push(Diagnostic {
                    serious: false,
                    text: format!(
                        "{} areas have no {label}; they count as 0.",
                        fmt(missing.len() as f64)
                    ),
                    units: missing,
                });
            }
            // A unit bigger than the smallest target makes its territory overshoot.
            let total: f64 = values.iter().flatten().sum();
            let weights: f64 = self.territories.iter().map(|t| t.weight.max(0.0)).sum();
            let smallest = self
                .territories
                .iter()
                .map(|t| t.weight)
                .filter(|w| *w > 0.0)
                .fold(f64::INFINITY, f64::min);
            if weights > 0.0 && smallest.is_finite() {
                let per_weight = m.target.unwrap_or(total / weights);
                let target = per_weight * smallest;
                let limit = target * (1.0 + m.tolerance);
                let mut big: Vec<u32> = (0..n as u32)
                    .filter(|&u| {
                        values
                            .get(u as usize)
                            .copied()
                            .flatten()
                            .is_some_and(|v| v > limit)
                    })
                    .collect();
                big.sort_by(|a, b| {
                    let v = |u: u32| values[u as usize].unwrap_or(0.0);
                    v(*b).total_cmp(&v(*a))
                });
                if !big.is_empty() {
                    out.push(Diagnostic {
                        serious: true,
                        text: format!(
                            "{} {} alone more {label} than a territory should have ({} with tolerance): {}.",
                            big.len(),
                            if big.len() == 1 { "area has" } else { "areas have" },
                            fmt(limit),
                            names(&big)
                        ),
                        units: big,
                    });
                }
            }
        }
        if layer.kind() == territoria_io::GeometryKind::Area && n > 1 {
            let islands: Vec<u32> = (0..n as u32)
                .filter(|&u| {
                    layer
                        .geometry
                        .neighbors
                        .get(u as usize)
                        .is_none_or(Vec::is_empty)
                })
                .collect();
            if !islands.is_empty() {
                out.push(Diagnostic {
                    serious: false,
                    text: format!(
                        "{} areas touch no other area ({}); a territory with them is in pieces.",
                        fmt(islands.len() as f64),
                        names(&islands)
                    ),
                    units: islands,
                });
            }
        }
        if self.territories.iter().any(|t| t.weight <= 0.0) {
            out.push(Diagnostic {
                serious: false,
                text: "Territories with size 0 get no share and end up empty.".into(),
                units: Vec::new(),
            });
        }
        out
    }
}

/// Which unit (feature of a plan's layer) each row of some data belongs to.
#[derive(Debug, Clone)]
pub enum RowUnits {
    /// The rows are the units.
    Same,
    /// Rows of a linked table.
    Matched(Arc<territoria_data::Matching>),
    /// Points, by the area they lie in.
    Points(crate::values::PointAreas),
}

impl RowUnits {
    pub fn unit(&self, row: usize) -> Option<u32> {
        match self {
            RowUnits::Same => Some(row as u32),
            RowUnits::Matched(m) => m.row_target.get(row).copied().flatten(),
            RowUnits::Points(p) => p.get(row).copied().flatten(),
        }
    }
}

impl Plan {
    /// The territory of a row, through `rows`.
    pub fn row_territory(&self, rows: &RowUnits, row: usize) -> Option<&Territory> {
        let unit = rows.unit(row)?;
        self.territory(self.assignment.get(unit as usize).copied().flatten()?)
    }

    /// Columns with the territory code and name of each row.
    pub fn territory_columns(&self, rows: &RowUnits, n: usize) -> [territoria_data::Column; 2] {
        use territoria_data::{Column, ColumnKind};
        let t: Vec<Option<&Territory>> = (0..n).map(|r| self.row_territory(rows, r)).collect();
        [
            Column::text(
                format!("Territory code ({})", self.name),
                ColumnKind::Key,
                t.iter().map(|t| t.map(|t| t.code.clone())).collect(),
            ),
            Column::text(
                format!("Territory ({})", self.name),
                ColumnKind::Text,
                t.iter().map(|t| t.map(|t| t.name.clone())).collect(),
            ),
        ]
    }
}

/// A territory's figures in two plans.
#[derive(Debug, Clone, PartialEq)]
pub struct TerritoryChange {
    pub id: TerritoryId,
    pub name: String,
    pub color: Rgba,
    pub units: [usize; 2],
    pub values: [f64; 2],
    /// Units it gains and loses from the first plan to the second.
    pub gained: usize,
    pub lost: usize,
}

/// One unit that belongs to another territory in the second plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    pub unit: u32,
    pub from: Option<TerritoryId>,
    pub to: Option<TerritoryId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    pub a: PlanId,
    pub b: PlanId,
    pub moves: Vec<Move>,
    /// Per point layer, how many of its points lie in moved units.
    pub points: Vec<(LayerId, usize)>,
    /// What `territories` compares.
    pub measure: String,
    pub territories: Vec<TerritoryChange>,
}

impl Comparison {
    /// One row per moved unit: key, name, territory before and after.
    pub fn table(&self, project: &Project) -> Table {
        use territoria_data::{Column, ColumnKind};
        let (Some(pa), Some(pb)) = (project.plan(self.a), project.plan(self.b)) else {
            return Table::default();
        };
        let Some(layer) = project.layer(pa.layer) else {
            return Table::default();
        };
        let name = |p: &Plan, t: Option<TerritoryId>| {
            t.and_then(|t| p.territory(t)).map(|t| t.name.clone())
        };
        let mut columns = vec![Column::text(
            layer.key_column.clone().unwrap_or_else(|| "Area".into()),
            ColumnKind::Key,
            self.moves
                .iter()
                .map(|m| Some(layer.key(m.unit as usize)))
                .collect(),
        )];
        if let Some(c) = layer
            .name_column
            .as_deref()
            .and_then(|c| layer.attributes.column(c))
        {
            columns.push(Column::text(
                c.name.clone(),
                ColumnKind::Text,
                self.moves
                    .iter()
                    .map(|m| c.get(m.unit as usize).to_text())
                    .collect(),
            ));
        }
        columns.push(Column::text(
            format!("Before ({})", pa.name),
            ColumnKind::Text,
            self.moves.iter().map(|m| name(pa, m.from)).collect(),
        ));
        columns.push(Column::text(
            format!("After ({})", pb.name),
            ColumnKind::Text,
            self.moves.iter().map(|m| name(pb, m.to)).collect(),
        ));
        Table::new(columns).unwrap_or_default()
    }

    /// One row per territory with its figures before and after.
    pub fn territory_table(&self) -> Table {
        use territoria_data::{Column, ColumnKind};
        let t = &self.territories;
        let number = |name: String, f: &dyn Fn(&TerritoryChange) -> f64| {
            Column::number(name, t.iter().map(|c| Some(f(c))).collect())
        };
        let columns = vec![
            Column::text(
                "Territory",
                ColumnKind::Text,
                t.iter().map(|c| Some(c.name.clone())).collect(),
            ),
            number("Areas before".into(), &|c| c.units[0] as f64),
            number("Areas after".into(), &|c| c.units[1] as f64),
            number("Gained".into(), &|c| c.gained as f64),
            number("Lost".into(), &|c| c.lost as f64),
            number(format!("{} before", self.measure), &|c| c.values[0]),
            number(format!("{} after", self.measure), &|c| c.values[1]),
            number("Change".into(), &|c| c.values[1] - c.values[0]),
        ];
        Table::new(columns).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(v: &[&str]) -> Vec<Option<String>> {
        v.iter().map(|s| Some(s.to_string())).collect()
    }

    #[test]
    fn unit_lists() {
        let units = keys(&["8000", "8001", "8050", "8105", "9000", "A12"]);
        let index = KeyIndex::new(units.iter().map(|u| u.as_deref()), MatchMode::Loose);
        assert_eq!(expand_unit_list("8001", &units, &index), vec![1]);
        assert_eq!(
            expand_unit_list("8000-8099; 8105", &units, &index),
            vec![0, 1, 2, 3]
        );
        assert_eq!(expand_unit_list("80*", &units, &index), vec![0, 1, 2]);
        assert_eq!(expand_unit_list("a?2, 9000", &units, &index), vec![4, 5]);
        assert!(expand_unit_list("7000", &units, &index).is_empty());
    }

    #[test]
    fn scenarios_compare() {
        use crate::layer::LayerGeometry;
        use geo_types::{MultiPolygon, polygon};
        let areas: Vec<MultiPolygon<f64>> = (0..4)
            .map(|i| {
                let x = i as f64;
                MultiPolygon(vec![polygon![(x: x, y: 0.0), (x: x + 1.0, y: 0.0), (x: x + 1.0, y: 1.0), (x: x, y: 1.0), (x: x, y: 0.0)]])
            })
            .collect();
        let mut p = Project::new();
        let g = LayerGeometry::from_imported(territoria_io::Geometries::Areas(areas), true);
        let layer = p.add_layer("A", g, Table::with_rows(4), Default::default());
        let a = p.add_plan("Now", layer, 2);
        let (t1, t2) = {
            let pl = p.plan(a).unwrap();
            (pl.territories[0].id, pl.territories[1].id)
        };
        p.plan_mut(a).unwrap().assign(&[0, 1], Some(t1));
        p.plan_mut(a).unwrap().assign(&[2, 3], Some(t2));
        let b = p.duplicate_plan(a, "Next year").unwrap();
        p.plan_mut(b).unwrap().assign(&[1], Some(t2));
        let mut cache = Cache::default();
        let c = p.compare_plans(a, b, &mut cache).unwrap();
        assert_eq!(
            c.moves,
            vec![Move {
                unit: 1,
                from: Some(t1),
                to: Some(t2)
            }]
        );
        let t = c.table(&p);
        assert_eq!(t.rows(), 1);
        assert_eq!(t.columns[0].display(0), "2");
        assert_eq!(
            t.column("After (Next year)").unwrap().display(0),
            "Territory 02"
        );
        assert_eq!(c.territories[0].units, [2, 1]);
        assert_eq!(c.territories[1].units, [2, 3]);
        assert_eq!((c.territories[1].gained, c.territories[1].lost), (1, 0));
        assert_eq!(c.territory_table().rows(), 2);
        let rows = p.layer_rows(&mut cache, layer, layer).unwrap();
        let pb = p.plan(b).unwrap();
        assert_eq!(pb.row_territory(&rows, 1).unwrap().id, t2);
        let [code, name] = pb.territory_columns(&rows, 4);
        assert_eq!(code.display(0), "01");
        assert_eq!(name.name, "Territory (Next year)");
        // Four units of 1 for two territories: nothing to complain about.
        let d = p.plan(a).unwrap().diagnose(&p, &mut cache);
        assert!(d.is_empty(), "{d:?}");
        assert!(p.adopt_plan(b, a));
        assert!(p.compare_plans(a, b, &mut cache).unwrap().moves.is_empty());
        assert_eq!(p.plan(a).unwrap().name, "Now");
    }

    #[test]
    fn diagnostics() {
        use crate::layer::LayerGeometry;
        use geo_types::{MultiPolygon, polygon};
        // Five squares in a row and one far away.
        let mut areas: Vec<MultiPolygon<f64>> = (0..5)
            .map(|i| {
                let x = i as f64;
                MultiPolygon(vec![polygon![(x: x, y: 0.0), (x: x + 1.0, y: 0.0), (x: x + 1.0, y: 1.0), (x: x, y: 1.0), (x: x, y: 0.0)]])
            })
            .collect();
        areas.push(MultiPolygon(vec![polygon![(x: 20.0, y: 0.0), (x: 21.0, y: 0.0), (x: 21.0, y: 1.0), (x: 20.0, y: 1.0), (x: 20.0, y: 0.0)]]));
        let table = Table::new(vec![territoria_data::Column::number(
            "Sales",
            vec![
                Some(1.0),
                Some(-2.0),
                Some(1.0),
                Some(40.0),
                None,
                Some(1.0),
            ],
        )])
        .unwrap();
        let mut p = Project::new();
        let g = LayerGeometry::from_imported(territoria_io::Geometries::Areas(areas), true);
        let layer = p.add_layer("A", g, table, Default::default());
        let plan = p.add_plan("Plan", layer, 3);
        p.plan_mut(plan).unwrap().measures = vec![Measure {
            source: ValueSource::Attribute {
                column: "Sales".into(),
            },
            tolerance: 0.1,
            target: None,
        }];
        let d = p.plan(plan).unwrap().diagnose(&p, &mut Cache::default());
        let find = |word: &str| {
            d.iter()
                .find(|x| x.text.contains(word))
                .unwrap_or_else(|| panic!("{word}: {d:?}"))
        };
        assert_eq!(find("negative").units, vec![1]);
        assert_eq!(find("no Sales").units, vec![4]);
        assert_eq!(find("alone").units, vec![3], "40 is more than 41 / 3 × 1.1");
        assert_eq!(find("touch no other").units, vec![5]);
        assert!(find("alone").serious && !find("touch no other").serious);
    }

    #[test]
    fn normalized_values() {
        use crate::layer::LayerGeometry;
        use crate::values::Normalize;
        use geo_types::{MultiPolygon, polygon};
        let areas: Vec<MultiPolygon<f64>> = (0..3)
            .map(|i| {
                let x = i as f64;
                MultiPolygon(vec![polygon![(x: x, y: 0.0), (x: x + 1.0, y: 0.0), (x: x + 1.0, y: 1.0), (x: x, y: 1.0), (x: x, y: 0.0)]])
            })
            .collect();
        let table = Table::new(vec![
            territoria_data::Column::number("Sales", vec![Some(10.0), Some(30.0), Some(60.0)]),
            territoria_data::Column::number("Customers", vec![Some(2.0), Some(0.0), Some(3.0)]),
        ])
        .unwrap();
        let mut p = Project::new();
        let g = LayerGeometry::from_imported(territoria_io::Geometries::Areas(areas), true);
        let layer = p.add_layer("A", g, table, Default::default());
        let mut cache = Cache::default();
        let sales = ValueSource::Attribute {
            column: "Sales".into(),
        };
        let customers = ValueSource::Attribute {
            column: "Customers".into(),
        };
        assert_eq!(
            cache.normalized(&p, layer, &sales, &Normalize::Percent),
            vec![Some(10.0), Some(30.0), Some(60.0)]
        );
        assert_eq!(
            cache.normalized(&p, layer, &sales, &Normalize::By(customers)),
            vec![Some(5.0), None, Some(20.0)],
            "no rate where there are no customers"
        );
        let area = p.layer(layer).unwrap().geometry.areas[0];
        let density = cache.normalized(&p, layer, &sales, &Normalize::PerArea);
        assert!((density[0].unwrap() - 10.0 / area).abs() < 1e-12);
        assert_eq!(Normalize::PerArea.suffix(&p), " per km²");
    }

    #[test]
    fn sites_and_distances() {
        use crate::layer::LayerGeometry;
        use geo_types::{MultiPolygon, polygon};
        // Two squares of one degree side by side at the equator.
        let areas: Vec<MultiPolygon<f64>> = (0..2)
            .map(|i| {
                let x = i as f64;
                MultiPolygon(vec![polygon![(x: x, y: 0.0), (x: x + 1.0, y: 0.0), (x: x + 1.0, y: 1.0), (x: x, y: 1.0), (x: x, y: 0.0)]])
            })
            .collect();
        let mut p = Project::new();
        let g = LayerGeometry::from_imported(territoria_io::Geometries::Areas(areas), true);
        let layer = p.add_layer("A", g, Table::with_rows(2), Default::default());
        let plan = p.add_plan("Plan", layer, 1);
        let g = p.layer(layer).unwrap().geometry.clone();
        let pl = p.plan_mut(plan).unwrap();
        let t = pl.territories[0].id;
        pl.assign(&[0, 1], Some(t));
        // Weighted by the measure, the site goes where most of it is.
        assert_eq!(
            pl.sites_at_centres(&g, true, Some(&[Some(1.0), Some(9.0)])),
            1
        );
        let site = pl.territories[0].site.unwrap();
        let second = g.label_points[1].unwrap();
        assert_eq!(site, [second.x, second.y]);
        let mut cache = Cache::default();
        let d = cache.values(&p, layer, &ValueSource::SiteDistance { plan });
        assert_eq!(d[1], Some(0.0));
        let km = d[0].unwrap();
        assert!((km - 111.2).abs() < 1.0, "{km} km for a degree");
        let stats = p.plan(plan).unwrap().stats(&p, &mut cache);
        assert!((stats.territories[0].site_reach.unwrap() - km).abs() < 1e-9);
    }

    #[test]
    fn sites_from_locations() {
        use crate::layer::LayerGeometry;
        use geo_types::{Coord, MultiPolygon, polygon};
        // Six squares in a row; territory 1 west, territory 2 east.
        let areas: Vec<MultiPolygon<f64>> = (0..6)
            .map(|i| {
                let x = i as f64 * 0.1;
                MultiPolygon(vec![polygon![(x: x, y: 0.0), (x: x + 0.1, y: 0.0), (x: x + 0.1, y: 0.1), (x: x, y: 0.1), (x: x, y: 0.0)]])
            })
            .collect();
        let mut p = Project::new();
        let g = LayerGeometry::from_imported(territoria_io::Geometries::Areas(areas), true);
        let layer = p.add_layer("A", g, Table::with_rows(6), Default::default());
        let plan = p.add_plan("Plan", layer, 2);
        let g = p.layer(layer).unwrap().geometry.clone();
        let pl = p.plan_mut(plan).unwrap();
        let (west, east) = (pl.territories[0].id, pl.territories[1].id);
        pl.assign(&[0, 1, 2], Some(west));
        pl.assign(&[3, 4, 5], Some(east));
        // Two rep homes: the eastern one listed first.
        let homes = [Coord { x: 0.55, y: 0.05 }, Coord { x: 0.05, y: 0.05 }];
        assert_eq!(pl.sites_from(&g, &homes, None), vec![Some(1), Some(0)]);
        assert_eq!(pl.territories[1].site, Some([0.55, 0.05]));
        // The best two of many candidates: one in each half.
        let candidates: Vec<Coord<f64>> = (0..13)
            .map(|i| Coord {
                x: i as f64 * 0.05,
                y: 0.05,
            })
            .collect();
        let picked = pl.best_sites(&g, &candidates, None);
        let west_x = candidates[picked[0].unwrap()].x;
        let east_x = candidates[picked[1].unwrap()].x;
        assert!(west_x < 0.3 && east_x > 0.3, "{west_x} {east_x}");
    }

    #[test]
    fn names_and_numbers() {
        use crate::layer::LayerGeometry;
        use geo_types::{MultiPolygon, polygon};
        // A 2×2 grid: units 0, 1 on the bottom row, 2, 3 on top.
        let areas: Vec<MultiPolygon<f64>> = (0..4)
            .map(|i| {
                let (x, y) = ((i % 2) as f64, (i / 2) as f64);
                MultiPolygon(vec![polygon![(x: x, y: y), (x: x + 1.0, y: y), (x: x + 1.0, y: y + 1.0), (x: x, y: y + 1.0), (x: x, y: y)]])
            })
            .collect();
        let table = Table::new(vec![
            territoria_data::Column::text(
                "Code",
                territoria_data::ColumnKind::Key,
                ["A", "B", "C", "D"].map(|s| Some(s.to_owned())).to_vec(),
            ),
            territoria_data::Column::text(
                "Place",
                territoria_data::ColumnKind::Text,
                ["Ash", "Birch", "Cedar", "Dogwood"]
                    .map(|s| Some(s.to_owned()))
                    .to_vec(),
            ),
        ])
        .unwrap();
        let mut p = Project::new();
        let g = LayerGeometry::from_imported(territoria_io::Geometries::Areas(areas), true);
        let layer = p.add_layer("A", g, table, Default::default());
        {
            let l = p.layer_mut(layer).unwrap();
            l.key_column = Some("Code".into());
            l.name_column = Some("Place".into());
        }
        let plan = p.add_plan("Plan", layer, 2);
        let l = p.layer(layer).unwrap().clone();
        let pl = p.plan_mut(plan).unwrap();
        let (t1, t2) = (pl.territories[0].id, pl.territories[1].id);
        // Territory 01 is the bottom row, 02 the top row.
        pl.assign(&[0, 1], Some(t1));
        pl.assign(&[2, 3], Some(t2));
        let weights = [Some(1.0), Some(5.0), Some(9.0), Some(2.0)];
        assert_eq!(pl.name_after_largest(&l, Some(&weights)), 2);
        assert_eq!(pl.territories[0].name, "Birch");
        assert_eq!(pl.territories[1].name, "Cedar");
        pl.territories[0].name = "Territory 01".into();
        pl.renumber_by_position(&l.geometry);
        // North first: the top row becomes 01.
        assert_eq!(pl.territories[0].id, t2);
        assert_eq!(pl.territories[0].code, "01");
        assert_eq!(pl.territories[1].code, "02");
        assert_eq!(
            pl.territories[1].name, "Territory 02",
            "default names follow"
        );
    }

    #[test]
    fn assign_respects_locks() {
        let mut p = Plan::new(PlanId(1), "p", LayerId(1), 4);
        let a = p.add_territory(None, None);
        let b = p.add_territory(None, None);
        assert_eq!(p.territories[1].code, "02");
        assert_eq!(p.assign(&[0, 1], Some(a)), 2);
        p.set_pinned(&[1], true);
        assert_eq!(p.assign(&[1, 2], Some(b)), 1);
        p.territory_mut(a).unwrap().locked = true;
        assert_eq!(p.assign(&[0], Some(b)), 0);
        assert_eq!(p.assign(&[3], Some(a)), 0);
        assert_eq!(p.members(), vec![vec![0, 1], vec![2]]);
        p.remove_territory(b);
        assert_eq!(p.assignment[2], None);
    }
}
