//! Territory plans: units of a layer assigned to territories, with the
//! figures that tell whether the territories are balanced.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use territoria_analysis::Rgba;
use territoria_data::{KeyIndex, MatchMode, MatchStats, Table, keys};

use crate::values::{Cache, ValueSource, column_keys, layer_keys};
use crate::{LayerGeometry, LayerId, PlanId, Project, TerritoryId};

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
        }
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
    pub fn sites_at_centres(&mut self, geometry: &LayerGeometry, replace: bool) -> usize {
        let members = self.members();
        let mut placed = 0;
        for (t, units) in self.territories.iter_mut().zip(&members) {
            if (t.site.is_some() && !replace) || units.is_empty() {
                continue;
            }
            let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
            for &u in units {
                if let Some(p) = geometry.label_points.get(u as usize).copied().flatten() {
                    let w = geometry
                        .areas
                        .get(u as usize)
                        .copied()
                        .unwrap_or(1.0)
                        .max(1e-9);
                    sx += p.x * w;
                    sy += p.y * w;
                    sw += w;
                }
            }
            if sw == 0.0 {
                continue;
            }
            let (cx, cy) = (sx / sw, sy / sw);
            let best = units
                .iter()
                .filter_map(|&u| geometry.label_points.get(u as usize).copied().flatten())
                .min_by(|a, b| {
                    let da = (a.x - cx).powi(2) + (a.y - cy).powi(2);
                    let db = (b.x - cx).powi(2) + (b.y - cy).powi(2);
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
        });
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
