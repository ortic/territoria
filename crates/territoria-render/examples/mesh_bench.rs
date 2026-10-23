//! Times building display meshes for a boundary file:
//! `cargo run --release -p territoria-render --example mesh_bench -- file.shp`
use std::sync::Arc;
use std::time::Instant;

use territoria_geo::Crs;
use territoria_io::{VectorOptions, read_vector};
use territoria_model::LayerGeometry;
use territoria_render::LayerMesh;

fn main() {
    let path = std::env::args().nth(1).expect("path to a boundary file");
    let layers = read_vector(path.as_ref(), &VectorOptions::default()).expect("readable file");
    let layer = layers.into_iter().last().expect("a layer");
    let geographic = layer.is_geographic();
    let t = Instant::now();
    let g = Arc::new(LayerGeometry::from_imported(layer.geometries, geographic));
    println!("geometry + derived: {:?}", t.elapsed());
    let t = Instant::now();
    let m = LayerMesh::build(g, &Crs::web_mercator(), 1);
    println!("mesh: {:?}", t.elapsed());
    for l in &m.levels {
        let pts: usize = l.arcs.iter().map(Vec::len).sum();
        let tris: usize = l.fills.iter().map(|f| f.indices.len() / 3).sum();
        println!(
            "  tolerance {:>10.1}: {:>8} arc points, {:>8} triangles",
            l.tolerance, pts, tris
        );
    }
}
