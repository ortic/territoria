//! Caches of things computed from the project for drawing: display meshes
//! (built in the background) and analysis colours.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use territoria_analysis::Scaled;
use territoria_geo::Crs;
use territoria_model::{Cache, Layer, LayerId, MapLayerId, MapLayerKind, Project};
use territoria_render::LayerMesh;

use crate::jobs::{JobResult, Jobs};

enum Entry {
    /// Being built for this revision; the old mesh (if any) is shown meanwhile.
    Building {
        rev: u64,
        old: Option<Arc<LayerMesh>>,
    },
    Ready(Arc<LayerMesh>),
}

/// What a mesh shows of a layer: its features, or (for points) their
/// Voronoi cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeshKind {
    Features,
    Cells,
}

#[derive(Default)]
pub struct MeshCache {
    entries: HashMap<(LayerId, String, MeshKind), Entry>,
}

impl MeshCache {
    /// The mesh for `layer` in `crs`, starting a build when missing or out
    /// of date. Returns a stale mesh while a newer one is built.
    pub fn get(&mut self, layer: &Layer, crs: &Crs, jobs: &mut Jobs) -> Option<Arc<LayerMesh>> {
        self.get_kind(layer, crs, MeshKind::Features, jobs)
    }

    /// The Voronoi cells of a point layer as an area mesh.
    pub fn get_cells(
        &mut self,
        layer: &Layer,
        crs: &Crs,
        jobs: &mut Jobs,
    ) -> Option<Arc<LayerMesh>> {
        self.get_kind(layer, crs, MeshKind::Cells, jobs)
    }

    fn get_kind(
        &mut self,
        layer: &Layer,
        crs: &Crs,
        kind: MeshKind,
        jobs: &mut Jobs,
    ) -> Option<Arc<LayerMesh>> {
        let key = (layer.id, crs.proj.clone(), kind);
        match self.entries.get(&key) {
            Some(Entry::Ready(m)) if m.rev == layer.rev => return Some(m.clone()),
            Some(Entry::Building { rev, old }) if *rev == layer.rev => return old.clone(),
            _ => {}
        }
        let old = match self.entries.remove(&key) {
            Some(Entry::Ready(m)) => Some(m),
            Some(Entry::Building { old, .. }) => old,
            None => None,
        };
        let geometry = layer.geometry.clone();
        let crs = crs.clone();
        let (id, rev) = (layer.id, layer.rev);
        let label = format!("Preparing {} for display", layer.name);
        jobs.spawn(label, move |_| {
            let geometry = match kind {
                MeshKind::Features => geometry,
                MeshKind::Cells => {
                    let cells = territoria_geo::points::voronoi_cells(
                        &geometry.label_points,
                        geometry.geographic,
                    );
                    Arc::new(territoria_model::LayerGeometry::from_imported(
                        territoria_io::Geometries::Areas(cells),
                        geometry.geographic,
                    ))
                }
            };
            JobResult::Mesh {
                layer: id,
                crs: crs.proj.clone(),
                kind,
                rev,
                mesh: Arc::new(LayerMesh::build(geometry, &crs, rev)),
            }
        });
        self.entries.insert(
            key,
            Entry::Building {
                rev,
                old: old.clone(),
            },
        );
        old
    }

    pub fn finished(
        &mut self,
        layer: LayerId,
        crs: String,
        kind: MeshKind,
        rev: u64,
        mesh: Arc<LayerMesh>,
    ) {
        let key = (layer, crs, kind);
        let current =
            matches!(self.entries.get(&key), Some(Entry::Building { rev: r, .. }) if *r == rev);
        if current || !self.entries.contains_key(&key) {
            self.entries.insert(key, Entry::Ready(mesh));
        }
    }

    /// Forgets meshes of layers that no longer exist.
    pub fn retain(&mut self, project: &Project) {
        self.entries
            .retain(|(id, _, _), _| project.layer(*id).is_some());
    }
}

/// Analysis colours per map layer, recomputed when inputs change.
#[derive(Default)]
pub struct AnalysisCache {
    entries: HashMap<MapLayerId, (u64, Arc<Scaled>)>,
}

impl AnalysisCache {
    pub fn get(
        &mut self,
        project: &Project,
        values: &mut Cache,
        id: MapLayerId,
        kind: &MapLayerKind,
    ) -> Option<Arc<Scaled>> {
        let key = input_key(project, kind)?;
        if let Some((k, s)) = self.entries.get(&id)
            && *k == key
        {
            return Some(s.clone());
        }
        let scaled = match kind {
            MapLayerKind::Choropleth {
                layer,
                source,
                scale,
                ..
            } => {
                let v = values.values(project, *layer, source);
                scale.apply(&v)
            }
            MapLayerKind::Categories {
                layer,
                source,
                scale,
                ..
            } => {
                let t = values.texts(project, *layer, source);
                let refs: Vec<Option<&str>> = t.iter().map(|s| s.as_deref()).collect();
                scale.apply(&refs)
            }
            _ => return None,
        };
        let scaled = Arc::new(scaled);
        self.entries.insert(id, (key, scaled.clone()));
        Some(scaled)
    }
}

/// Everything an analysis result depends on, hashed.
fn input_key(project: &Project, kind: &MapLayerKind) -> Option<u64> {
    let mut h = DefaultHasher::new();
    format!("{kind:?}").hash(&mut h);
    let layer = project.layer(kind.layer()?)?;
    layer.key_column.hash(&mut h);
    // Point layers can feed area values: any layer change counts.
    for l in &project.layers {
        l.rev.hash(&mut h);
    }
    for link in project.links_to(layer.id) {
        format!("{link:?}").hash(&mut h);
        if let Some(t) = project.table(link.table) {
            t.rev.hash(&mut h);
        }
    }
    Some(h.finish())
}
