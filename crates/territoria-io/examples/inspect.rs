//! Reads a boundary file and prints what Territoria makes of it:
//! `cargo run --release -p territoria-io --example inspect -- file.shp`
use std::time::Instant;

use territoria_io::{Geometries, VectorOptions, read_vector};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("path to a .shp, .zip or .geojson");
    let t = Instant::now();
    let layers = read_vector(path.as_ref(), &VectorOptions::default()).expect("readable file");
    println!("read in {:?}", t.elapsed());
    for layer in layers {
        println!(
            "{}: {} {} · {}",
            layer.name,
            layer.geometries.len(),
            layer.geometries.kind().label(),
            layer.crs_note
        );
        for w in &layer.warnings {
            println!("  warning: {w}");
        }
        for c in &layer.attributes.columns {
            println!("  {:<12} {:?} e.g. {}", c.name, c.kind, c.display(0));
        }
        if let Geometries::Areas(areas) = &layer.geometries {
            let t = Instant::now();
            let topo = territoria_geo::Topology::build(areas);
            let shared = topo.arcs.iter().filter(|a| a.is_shared()).count();
            let points: usize = topo.arcs.iter().map(|a| a.points.len()).sum();
            println!(
                "  topology: {} arcs ({} shared), {} points, in {:?}",
                topo.arcs.len(),
                shared,
                points,
                t.elapsed()
            );
            let t = Instant::now();
            let n = topo.neighbors(layer.is_geographic());
            let avg = n.iter().map(Vec::len).sum::<usize>() as f64 / n.len().max(1) as f64;
            println!("  neighbours: {avg:.1} on average, in {:?}", t.elapsed());
        }
    }
}
