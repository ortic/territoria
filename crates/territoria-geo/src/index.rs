//! Spatial index over feature bounding boxes.

use geo_types::{Coord, Rect};
use rstar::{AABB, RTree, RTreeObject};

#[derive(Debug, Clone)]
struct Entry {
    id: u32,
    bbox: AABB<[f64; 2]>,
}

impl RTreeObject for Entry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        self.bbox
    }
}

/// Finds features by bounding box; exact tests are up to the caller.
#[derive(Debug, Clone, Default)]
pub struct BoxIndex {
    tree: RTree<Entry>,
}

impl BoxIndex {
    /// `boxes[i]` is the box of feature `i`; `None` for empty features.
    pub fn new(boxes: &[Option<Rect<f64>>]) -> BoxIndex {
        let entries = boxes
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let b = (*b)?;
                Some(Entry {
                    id: i as u32,
                    bbox: AABB::from_corners([b.min().x, b.min().y], [b.max().x, b.max().y]),
                })
            })
            .collect();
        BoxIndex {
            tree: RTree::bulk_load(entries),
        }
    }

    /// Features whose box contains `p`.
    pub fn at(&self, p: Coord<f64>) -> impl Iterator<Item = u32> + '_ {
        let aabb = AABB::from_point([p.x, p.y]);
        self.tree
            .locate_in_envelope_intersecting(&aabb)
            .map(|e| e.id)
    }

    /// Features whose box intersects `r`.
    pub fn within(&self, r: Rect<f64>) -> impl Iterator<Item = u32> + '_ {
        let aabb = AABB::from_corners([r.min().x, r.min().y], [r.max().x, r.max().y]);
        self.tree
            .locate_in_envelope_intersecting(&aabb)
            .map(|e| e.id)
    }

    pub fn len(&self) -> usize {
        self.tree.size()
    }

    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }
}
