//! The Territoria project: layers, tables, links between them, territory
//! plans and maps, with undo history and the project file.
//!
//! Heavy data (geometry, tables, assignments) sits behind `Arc`s, so a
//! [`Project`] clones cheaply; undo keeps whole snapshots.

pub mod catchment;
pub mod file;
pub mod flows;
pub mod grid;
pub mod history;
pub mod join;
pub mod layer;
pub mod location;
pub mod map;
pub mod plan;
pub mod transfer;
pub mod values;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use territoria_data::{MatchMode, Table};

pub use history::History;
pub use layer::{
    Layer, LayerGeometry, SourceInfo, duplicate_keys, merge_by_column, points_from_table,
};
pub use map::{
    BoundaryStyle, ChartPart, ChartShape, ChartStyle, LabelMode, Map, MapLayer, MapLayerKind,
    SymbolScaling, SymbolShape, SymbolStyle, ZoomRange,
};
pub use plan::{
    Comparison, Diagnostic, Measure, Move, Plan, PlanStats, Region, RegionStats, RowUnits,
    Territory, TerritoryChange, TerritoryStats,
};
pub use values::{Cache, Normalize, TextSource, ValueSource};

macro_rules! ids {
    ($($name:ident),*) => {$(
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub u32);
    )*};
}

ids!(
    LayerId,
    TableId,
    LinkId,
    PlanId,
    MapId,
    MapLayerId,
    TerritoryId,
    RegionId
);

/// What reloading a table changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableChanges {
    /// Rows before and after.
    pub rows: (usize, usize),
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// Links whose key column is no longer there.
    pub broken_links: usize,
}

/// How replacing a layer's areas went.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Replaced {
    /// New areas that took over an old one by key.
    pub by_key: usize,
    /// … by lying where an old one was.
    pub by_place: usize,
    /// New areas with no old one.
    pub new: usize,
    /// Old areas nothing took over.
    pub gone: usize,
}

/// A table of data the user brought in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    pub id: TableId,
    pub name: String,
    #[serde(skip)]
    pub table: Arc<Table>,
    pub source: SourceInfo,
    pub rev: u64,
}

/// Rows of a table belong to features of a layer by key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub id: LinkId,
    pub table: TableId,
    /// The table's key column.
    pub column: String,
    pub layer: LayerId,
    pub mode: MatchMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Project {
    pub title: String,
    pub layers: Vec<Layer>,
    pub tables: Vec<Dataset>,
    pub links: Vec<Link>,
    pub plans: Vec<Plan>,
    pub maps: Vec<Map>,
    next_id: u32,
    rev: u64,
    /// Where the user was (open tabs, active plan): kept for the app, not
    /// read here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<serde_json::Value>,
}

impl Project {
    pub fn new() -> Project {
        Project {
            title: "Untitled".into(),
            ..Default::default()
        }
    }

    pub fn next_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    /// A fresh revision number for changed data (cache keys).
    pub fn bump(&mut self) -> u64 {
        self.rev += 1;
        self.rev
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub fn table(&self, id: TableId) -> Option<&Dataset> {
        self.tables.iter().find(|t| t.id == id)
    }

    pub fn table_mut(&mut self, id: TableId) -> Option<&mut Dataset> {
        self.tables.iter_mut().find(|t| t.id == id)
    }

    pub fn link(&self, id: LinkId) -> Option<&Link> {
        self.links.iter().find(|l| l.id == id)
    }

    pub fn plan(&self, id: PlanId) -> Option<&Plan> {
        self.plans.iter().find(|p| p.id == id)
    }

    pub fn plan_mut(&mut self, id: PlanId) -> Option<&mut Plan> {
        self.plans.iter_mut().find(|p| p.id == id)
    }

    pub fn map(&self, id: MapId) -> Option<&Map> {
        self.maps.iter().find(|m| m.id == id)
    }

    pub fn map_mut(&mut self, id: MapId) -> Option<&mut Map> {
        self.maps.iter_mut().find(|m| m.id == id)
    }

    /// Adds an imported layer; picks key and name columns.
    pub fn add_layer(
        &mut self,
        name: &str,
        geometry: LayerGeometry,
        attributes: Table,
        source: SourceInfo,
    ) -> LayerId {
        let id = LayerId(self.next_id());
        let key_column = territoria_data::suggest::key_column(&attributes)
            .map(|i| attributes.columns[i].name.clone());
        let name_column = territoria_data::suggest::name_column(&attributes)
            .map(|i| attributes.columns[i].name.clone());
        let rev = self.bump();
        self.layers.push(Layer {
            id,
            name: self.unique_layer_name(name),
            geometry: Arc::new(geometry),
            attributes: Arc::new(attributes),
            key_column,
            name_column,
            source,
            rev,
            barriers: Vec::new(),
        });
        id
    }

    fn unique_layer_name(&self, base: &str) -> String {
        let taken = |n: &str| self.layers.iter().any(|l| l.name == n);
        if !taken(base) {
            return base.to_owned();
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|n| !taken(n))
            .unwrap()
    }

    pub fn add_table(&mut self, name: &str, table: Table, source: SourceInfo) -> TableId {
        let id = TableId(self.next_id());
        let taken = |n: &str| self.tables.iter().any(|t| t.name == n);
        let name = if taken(name) {
            (2..)
                .map(|i| format!("{name} {i}"))
                .find(|n| !taken(n))
                .unwrap()
        } else {
            name.to_owned()
        };
        let rev = self.bump();
        self.tables.push(Dataset {
            id,
            name,
            table: Arc::new(table),
            source,
            rev,
        });
        id
    }

    pub fn add_link(
        &mut self,
        table: TableId,
        column: &str,
        layer: LayerId,
        mode: MatchMode,
    ) -> LinkId {
        if let Some(existing) = self
            .links
            .iter_mut()
            .find(|l| l.table == table && l.layer == layer)
        {
            existing.column = column.to_owned();
            existing.mode = mode;
            return existing.id;
        }
        let id = LinkId(self.next_id());
        self.links.push(Link {
            id,
            table,
            column: column.to_owned(),
            layer,
            mode,
        });
        id
    }

    pub fn add_map(&mut self, name: &str) -> MapId {
        let id = MapId(self.next_id());
        self.maps.push(Map::new(id, name));
        id
    }

    pub fn new_map_layer_id(&mut self) -> MapLayerId {
        MapLayerId(self.next_id())
    }

    /// Removes a layer and everything that depends on it; returns what went
    /// with it for the confirmation dialog.
    pub fn remove_layer(&mut self, id: LayerId) -> Vec<String> {
        let mut gone = Vec::new();
        let plans: Vec<PlanId> = self
            .plans
            .iter()
            .filter(|p| p.layer == id)
            .map(|p| p.id)
            .collect();
        for p in &plans {
            if let Some(plan) = self.plan(*p) {
                gone.push(format!("territory plan “{}”", plan.name));
            }
        }
        self.plans.retain(|p| p.layer != id);
        self.links.retain(|l| l.layer != id);
        for map in &mut self.maps {
            map.layers.retain(|ml| {
                let keep =
                    ml.kind.layer() != Some(id) && !plans.iter().any(|p| ml.kind.uses_plan(*p));
                if !keep {
                    gone.push(format!("map layer “{}”", ml.name));
                }
                keep
            });
        }
        self.layers.retain(|l| l.id != id);
        gone
    }

    /// Replaces a layer's areas with a newer version (new postcodes, merged
    /// municipalities), keeping the layer: its links, map layers and plans
    /// go on. New areas take over the old ones by key, else the old area
    /// under their centre; plans' assignments and pins follow, calculated
    /// columns are computed again.
    pub fn replace_layer(
        &mut self,
        id: LayerId,
        geometry: LayerGeometry,
        mut attributes: territoria_data::Table,
        key_column: Option<String>,
    ) -> Option<Replaced> {
        let old = self.layer(id)?.clone();
        let n = geometry.len();
        if attributes.rows() != n {
            attributes = territoria_data::Table::with_rows(n);
        }
        let old_keys: Vec<Option<String>> = (0..old.len())
            .map(|u| old.key_index().map(|_| old.key(u)))
            .collect();
        let index = territoria_data::KeyIndex::new(
            old_keys.iter().map(|k| k.as_deref()),
            territoria_data::MatchMode::Loose,
        );
        let new_keys = key_column.as_deref().and_then(|k| attributes.column(k));
        let mut report = Replaced::default();
        let mut taken = vec![false; old.len()];
        let from: Vec<Option<usize>> = (0..n)
            .map(|u| {
                if let Some(o) = new_keys
                    .and_then(|c| c.get(u).to_text())
                    .and_then(|k| index.find(&k))
                {
                    report.by_key += 1;
                    taken[o as usize] = true;
                    return Some(o as usize);
                }
                let at = geometry.label_points.get(u).copied().flatten()?;
                match old.geometry.feature_at(at, 0.0) {
                    Some(o) => {
                        report.by_place += 1;
                        taken[o] = true;
                        Some(o)
                    }
                    None => {
                        report.new += 1;
                        None
                    }
                }
            })
            .collect();
        report.gone = taken.iter().filter(|t| !**t).count();
        // Calculated columns, on the new attributes.
        for c in old
            .attributes
            .columns
            .iter()
            .filter(|c| c.formula.is_some())
        {
            if attributes.column(&c.name).is_none()
                && let Some(source) = &c.formula
                && let Ok(f) = territoria_expr::Formula::compile(source, &attributes)
            {
                let col = f.column(&c.name, source, &attributes);
                let _ = attributes.push_column(col);
            }
        }
        let rev = self.bump();
        for plan in self.plans.iter_mut().filter(|p| p.layer == id) {
            let assignment = from
                .iter()
                .map(|o| o.and_then(|o| plan.assignment.get(o).copied().flatten()))
                .collect();
            let pinned = from
                .iter()
                .map(|o| o.is_some_and(|o| plan.pinned.get(o).copied().unwrap_or(false)))
                .collect();
            plan.assignment = Arc::new(assignment);
            plan.pinned = Arc::new(pinned);
            plan.rev = rev;
        }
        let layer = self.layer_mut(id)?;
        let name_column = layer
            .name_column
            .clone()
            .filter(|c| attributes.column(c).is_some());
        let mut geometry = geometry;
        geometry.cut_neighbors(&layer.barriers);
        layer.geometry = Arc::new(geometry);
        layer.attributes = Arc::new(attributes);
        layer.key_column = key_column;
        layer.name_column = name_column;
        layer.rev = rev;
        Some(report)
    }

    /// Puts newly read rows in place of a table's (a reload from its
    /// file). Columns keep the kinds they were given; calculated columns
    /// are computed again; links stay.
    pub fn replace_table(
        &mut self,
        id: TableId,
        mut table: territoria_data::Table,
    ) -> Option<TableChanges> {
        let old = self.table(id)?.table.clone();
        let mut changes = TableChanges {
            rows: (old.rows(), table.rows()),
            ..Default::default()
        };
        for c in &old.columns {
            match table.columns.iter_mut().find(|n| n.name == c.name) {
                Some(n) if n.kind != c.kind => {
                    n.convert(c.kind);
                }
                Some(_) => {}
                None if c.formula.is_some() => {}
                None => changes.removed.push(c.name.clone()),
            }
        }
        changes.added = table
            .columns
            .iter()
            .filter(|n| old.column(&n.name).is_none())
            .map(|n| n.name.clone())
            .collect();
        for c in old.columns.iter().filter(|c| c.formula.is_some()) {
            if table.column(&c.name).is_none()
                && let Some(source) = &c.formula
                && let Ok(f) = territoria_expr::Formula::compile(source, &table)
            {
                let col = f.column(&c.name, source, &table);
                let _ = table.push_column(col);
            }
        }
        changes.broken_links = self
            .links
            .iter()
            .filter(|l| l.table == id && table.column(&l.column).is_none())
            .count();
        let rev = self.bump();
        let t = self.table_mut(id)?;
        t.table = Arc::new(table);
        t.rev = rev;
        Some(changes)
    }

    /// Removes a plan and the map layers showing it.
    pub fn remove_plan(&mut self, id: PlanId) {
        self.plans.retain(|p| p.id != id);
        for map in &mut self.maps {
            map.layers.retain(|ml| !ml.kind.uses_plan(id));
        }
    }

    /// `base`, or `base 2`, … when a table has that name.
    pub fn unique_table_name(&self, base: &str) -> String {
        (1..)
            .map(|i| {
                if i == 1 {
                    base.to_owned()
                } else {
                    format!("{base} {i}")
                }
            })
            .find(|n| !self.tables.iter().any(|t| &t.name == n))
            .unwrap_or_else(|| base.to_owned())
    }

    pub fn remove_table(&mut self, id: TableId) {
        let links: Vec<LinkId> = self
            .links
            .iter()
            .filter(|l| l.table == id)
            .map(|l| l.id)
            .collect();
        self.links.retain(|l| l.table != id);
        for map in &mut self.maps {
            map.layers.retain(|ml| !ml.kind.uses_links(&links));
        }
        for plan in &mut self.plans {
            plan.measures.retain(|m| !m.source.uses_links(&links));
        }
        self.tables.retain(|t| t.id != id);
    }

    /// Links that bring data to `layer`.
    pub fn links_to(&self, layer: LayerId) -> impl Iterator<Item = &Link> {
        self.links.iter().filter(move |l| l.layer == layer)
    }
}
