//! The sample project opened from the welcome screen: the Swiss cantons
//! (swisstopo, via geoBoundaries) with made-up customers, coloured by
//! sales and divided into sales regions. Built by the ignored test
//! `screenshots::build_sample`:
//!
//! ```text
//! cargo test -p territoria-app build_sample -- --ignored
//! ```

use territoria_model::Project;

const SAMPLE: &[u8] = include_bytes!("../sample/sample.territoria");

#[cfg(test)]
pub const ATTRIBUTION: &str = "Cantons: © swisstopo, via geoBoundaries";

/// The sample project, untitled and not saved anywhere.
pub fn project() -> Result<Project, String> {
    let mut project = territoria_model::file::read_project(std::io::Cursor::new(SAMPLE))
        .map_err(|e| e.to_string())?;
    project.title = "Sample".into();
    Ok(project)
}

/// Swiss towns, west to east, with a rough share of the customers.
#[cfg(test)]
const TOWNS: [(&str, f64, f64, u32); 36] = [
    ("Genève", 6.143, 46.204, 9),
    ("Lausanne", 6.633, 46.520, 8),
    ("Nyon", 6.239, 46.383, 2),
    ("Yverdon-les-Bains", 6.641, 46.778, 2),
    ("Montreux", 6.912, 46.433, 2),
    ("Sion", 7.360, 46.233, 3),
    ("Martigny", 7.072, 46.102, 1),
    ("Visp", 7.882, 46.294, 1),
    ("Fribourg", 7.161, 46.806, 3),
    ("Bulle", 7.057, 46.619, 1),
    ("Neuchâtel", 6.931, 46.990, 3),
    ("La Chaux-de-Fonds", 6.826, 47.099, 2),
    ("Delémont", 7.343, 47.365, 1),
    ("Biel/Bienne", 7.247, 47.137, 3),
    ("Bern", 7.447, 46.948, 8),
    ("Thun", 7.628, 46.758, 3),
    ("Interlaken", 7.863, 46.686, 1),
    ("Solothurn", 7.537, 47.208, 2),
    ("Basel", 7.588, 47.560, 7),
    ("Liestal", 7.735, 47.484, 2),
    ("Olten", 7.903, 47.350, 2),
    ("Aarau", 8.044, 47.392, 3),
    ("Baden", 8.306, 47.473, 3),
    ("Luzern", 8.310, 47.050, 5),
    ("Zug", 8.516, 47.166, 3),
    ("Schwyz", 8.654, 47.021, 1),
    ("Altdorf", 8.644, 46.880, 1),
    ("Zürich", 8.541, 47.377, 14),
    ("Winterthur", 8.724, 47.500, 5),
    ("Schaffhausen", 8.634, 47.696, 2),
    ("Frauenfeld", 8.898, 47.558, 2),
    ("St. Gallen", 9.377, 47.424, 4),
    ("Rapperswil", 8.818, 47.226, 2),
    ("Chur", 9.532, 46.850, 2),
    ("Davos", 9.836, 46.802, 1),
    ("Lugano", 8.951, 46.004, 4),
];

/// Made-up customers around the towns: name, town, segment, sales and
/// coordinates, the same every time.
#[cfg(test)]
pub fn customers() -> territoria_data::Table {
    use territoria_data::{Column, ColumnKind, Table};
    let mut rnd = 0x5eed_u64;
    let mut next = || {
        rnd ^= rnd << 13;
        rnd ^= rnd >> 7;
        rnd ^= rnd << 17;
        (rnd % 1_000_000) as f64 / 1_000_000.0
    };
    let first = [
        "Alpen", "Berg", "See", "Tal", "Sonnen", "Linden", "Rhein", "Jura", "Eichen", "Stern",
        "Rosen", "Matten", "Brunnen", "Falken", "Garten", "Lärchen",
    ];
    let second = [
        "blick", "hof", "werk", "haus", "egg", "matt", "bühl", "wald", "feld", "stein",
    ];
    let segments = ["Retail", "Wholesale", "Hotels & restaurants", "Industry"];
    let total: u32 = TOWNS.iter().map(|t| t.3).sum();
    let mut columns: [Vec<Option<String>>; 4] = Default::default();
    let (mut lon, mut lat, mut sales) = (Vec::new(), Vec::new(), Vec::new());
    for (i, &(town, x, y, share)) in TOWNS.iter().enumerate() {
        let n = (share * 400).div_ceil(total);
        for _ in 0..n {
            let id = columns[0].len() + 1;
            let name = format!(
                "{}{}",
                first[(next() * first.len() as f64) as usize],
                second[(next() * second.len() as f64) as usize]
            );
            // French and Italian legal forms west and south.
            let form = match (i, next()) {
                (0..=12, f) if f < 0.6 => "SA",
                (35, f) if f < 0.6 => "SA",
                (_, f) if f < 0.55 => "AG",
                (_, f) if f < 0.85 => "GmbH",
                _ => "& Co.",
            };
            let segment = segments[(next() * next() * segments.len() as f64) as usize];
            let scale = match segment {
                "Wholesale" => 4.0,
                "Industry" => 2.5,
                _ => 1.0,
            };
            columns[0].push(Some(format!("K{id:04}")));
            columns[1].push(Some(format!("{name} {form}")));
            columns[2].push(Some(town.to_owned()));
            columns[3].push(Some(segment.to_owned()));
            // Within about two kilometres of the town centre.
            let (a, r) = (next() * std::f64::consts::TAU, 0.02 * next().sqrt());
            lon.push(Some(((x + r * a.cos() * 1.45) * 1e5).round() / 1e5));
            lat.push(Some(((y + r * a.sin()) * 1e5).round() / 1e5));
            sales.push(Some(
                ((2_000.0 + 38_000.0 * next() * next()) * scale / 10.0).round() * 10.0,
            ));
        }
    }
    let [ids, names, towns, segment] = columns;
    Table::new(vec![
        Column::text("Customer", ColumnKind::Key, ids),
        Column::text("Name", ColumnKind::Text, names),
        Column::text("Town", ColumnKind::Text, towns),
        Column::text("Segment", ColumnKind::Text, segment),
        Column::number("Sales", sales),
        Column::number("Longitude", lon),
        Column::number("Latitude", lat),
    ])
    .expect("customer columns")
}

#[cfg(test)]
mod tests {
    #[test]
    fn sample_opens() {
        let p = super::project().unwrap();
        assert_eq!(p.title, "Sample");
        assert_eq!(p.layers[0].len(), 26);
        assert!(p.layers[1].len() > 350);
        assert_eq!(p.plans.len(), 1);
        assert!(p.plans[0].assignment.iter().all(Option::is_some));
        assert!(!p.maps.is_empty());
    }

    #[test]
    fn customers_stay_the_same() {
        let a = super::customers();
        let b = super::customers();
        assert_eq!(a.rows(), b.rows());
        assert!(a.rows() >= 400);
        assert_eq!(
            a.column("Name").unwrap().display(7),
            b.column("Name").unwrap().display(7)
        );
    }
}
