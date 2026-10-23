//! The Territoria project: layers, tables, links between them, territory
//! plans and maps, with undo history and the project file.
//!
//! Heavy data (geometry, tables, assignments) sits behind `Arc`s, so a
//! [`Project`] clones cheaply; undo keeps whole snapshots.

pub mod file;
pub mod history;
pub mod layer;
pub mod map;
pub mod plan;
pub mod values;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use territoria_data::{MatchMode, Table};

pub use history::History;
pub use layer::{
    Layer, LayerGeometry, SourceInfo, duplicate_keys, merge_by_column, points_from_table,
};
pub use map::{BoundaryStyle, LabelMode, Map, MapLayer, MapLayerKind};
pub use plan::{
    Comparison, Measure, Move, Plan, PlanStats, RowUnits, Territory, TerritoryChange,
    TerritoryStats,
};
pub use values::{Cache, TextSource, ValueSource};

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
    TerritoryId
);

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
                let keep = ml.kind.layer() != Some(id)
                    && !ml.kind.plan().is_some_and(|p| plans.contains(&p));
                if !keep {
                    gone.push(format!("map layer “{}”", ml.name));
                }
                keep
            });
        }
        self.layers.retain(|l| l.id != id);
        gone
    }

    /// Removes a plan and the map layers showing it.
    pub fn remove_plan(&mut self, id: PlanId) {
        self.plans.retain(|p| p.id != id);
        for map in &mut self.maps {
            map.layers.retain(|ml| ml.kind.plan() != Some(id));
        }
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
