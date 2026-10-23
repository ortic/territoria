//! Renders the real app offscreen (wgpu) into PNGs for visual review.
//!
//! Skipped unless `TERRITORIA_SCREENSHOT_DIR` is set. Boundaries come from
//! `TERRITORIA_TEST_BOUNDARIES` (any Shapefile, zip or GeoJSON) or, without
//! it, a generated grid of hexagons:
//!
//! ```text
//! TERRITORIA_SCREENSHOT_DIR=/tmp/shots cargo test -p territoria-app screenshots -- --nocapture
//! ```

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use territoria_data::{Column, ColumnKind, Table};
use territoria_model::LayerId;

use crate::app::TerritoriaApp;

fn out_dir() -> Option<PathBuf> {
    std::env::var_os("TERRITORIA_SCREENSHOT_DIR").map(PathBuf::from)
}

/// One harness at a time: several wgpu renderers in one process are flaky.
static UI_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn harness(
    dark: bool,
) -> (
    Harness<'static, TerritoriaApp>,
    std::sync::MutexGuard<'static, ()>,
) {
    let lock = UI_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let builder = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_pixels_per_point(1.5);
    let builder = if out_dir().is_some() {
        builder.wgpu()
    } else {
        builder
    };
    let h = builder.build_eframe(|cc| TerritoriaApp::new(cc, None, None));
    h.ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    (h, lock)
}

fn save(h: &mut Harness<'_, TerritoriaApp>, name: &str) {
    let Some(dir) = out_dir() else { return };
    std::fs::create_dir_all(&dir).unwrap();
    h.run_steps(4);
    let image = h.render().expect("render");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).unwrap();
    eprintln!("wrote {}", path.display());
}

/// Steps until background jobs (mesh building) are done.
fn settle(h: &mut Harness<'_, TerritoriaApp>) {
    let start = Instant::now();
    h.run_steps(2);
    while h.state().jobs.is_busy() {
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "jobs didn't finish"
        );
        h.step();
        std::thread::sleep(Duration::from_millis(15));
    }
    h.run_steps(4);
}

/// A GeoJSON grid of hexagons with codes, written to a temp file.
fn hexagons() -> PathBuf {
    let path = std::env::temp_dir().join(format!("territoria-hex-{}.geojson", std::process::id()));
    let (cols, rows, r) = (34, 26, 0.03f64);
    // Longitude degrees are shorter at this latitude: stretch x by 1/0.68.
    let w = 3f64.sqrt() * r / 0.68;
    let mut features = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let cx = 6.0 + col as f64 * w + if row % 2 == 1 { w / 2.0 } else { 0.0 };
            let cy = 46.2 + row as f64 * 1.5 * r;
            // A rough coastline: skip a corner so the outline isn't a box.
            if (col as f64 - 4.0) + (rows - row) as f64 * 0.6 < 4.0 || (col + row) % 37 == 5 {
                continue;
            }
            let ring: Vec<String> = (0..=6)
                .map(|k| {
                    let a = (30.0 + 60.0 * (k % 6) as f64).to_radians();
                    format!("[{:.9},{:.9}]", cx + r * a.cos() / 0.68, cy + r * a.sin())
                })
                .collect();
            let code = format!("{:04}", 100 + row * cols + col);
            features.push(format!(
                r#"{{"type":"Feature","properties":{{"code":"{code}","name":"Area {code}"}},"geometry":{{"type":"Polygon","coordinates":[[{}]]}}}}"#,
                ring.join(",")
            ));
        }
    }
    std::fs::write(
        &path,
        format!(
            r#"{{"type":"FeatureCollection","features":[{}]}}"#,
            features.join(",")
        ),
    )
    .unwrap();
    path
}

fn boundaries() -> PathBuf {
    std::env::var_os("TERRITORIA_TEST_BOUNDARIES")
        .map(PathBuf::from)
        .unwrap_or_else(hexagons)
}

/// Customers and revenue per area: a few hot spots and noise.
fn sales_table(app: &TerritoriaApp, layer: LayerId) -> Table {
    let l = app.project.layer(layer).unwrap();
    let mut keys = Vec::new();
    let mut customers = Vec::new();
    let mut revenue = Vec::new();
    let g = &l.geometry;
    let hot = |x: f64, y: f64| -> f64 {
        let spots = [
            (0.25, 0.3, 1.0),
            (0.7, 0.65, 0.8),
            (0.55, 0.2, 0.5),
            (0.4, 0.8, 0.4),
        ];
        let b = g.bbox.unwrap();
        let (u, v) = ((x - b.min().x) / b.width(), (y - b.min().y) / b.height());
        spots
            .iter()
            .map(|(sx, sy, w)| w * (-((u - sx).powi(2) + (v - sy).powi(2)) / 0.012).exp())
            .sum::<f64>()
    };
    for f in 0..l.len() {
        let Some(p) = g.label_points[f] else { continue };
        let noise = ((f as f64 * 12.9898).sin() * 43758.5453).fract().abs();
        let c = (3.0 + 120.0 * hot(p.x, p.y) + 6.0 * noise).round();
        keys.push(Some(l.key(f)));
        customers.push(Some(c));
        revenue.push(Some((c * (900.0 + 400.0 * noise)).round()));
    }
    Table::new(vec![
        Column::text("Area", ColumnKind::Key, keys),
        {
            let mut c = Column::number("Customers", customers);
            c.kind = ColumnKind::Integer;
            c
        },
        Column::number("Revenue", revenue),
    ])
    .unwrap()
}

#[test]
fn screenshots() {
    if out_dir().is_none() {
        return;
    }
    // Welcome screen.
    {
        let (mut h, _lock) = harness(false);
        h.run_steps(3);
        save(&mut h, "welcome");
    }
    // The import dialog, as after File → Add Boundaries.
    {
        let (mut h, _lock) = harness(false);
        h.state_mut()
            .import_boundaries(boundaries(), territoria_io::VectorOptions::default());
        settle(&mut h);
        save(&mut h, "import-dialog");
    }
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    settle(&mut h);
    save(&mut h, "boundaries");

    // The link dialog for a fresh table.
    {
        let table = sales_table(h.state(), layer);
        let app = h.state_mut();
        let id = app
            .project
            .add_table("Sales 2026", table, Default::default());
        app.open_link_dialog(id);
        h.run_steps(4);
        save(&mut h, "link-dialog");
        let app = h.state_mut();
        app.close_dialogs();
        app.project.remove_table(id);
        h.run_steps(2);
    }

    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, Some("Revenue"));
    settle(&mut h);
    save(&mut h, "choropleth");

    h.state_mut().add_balanced_plan_now(layer, 9);
    settle(&mut h);
    save(&mut h, "territories");

    h.ctx.set_theme(egui::Theme::Dark);
    settle(&mut h);
    save(&mut h, "territories-dark");
}

/// The basic flow works without rendering (also on CI without a GPU).
#[test]
fn import_link_and_plan() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&hexagons())
        .expect("boundaries")[0];
    settle(&mut h);
    let table = sales_table(h.state(), layer);
    let rows = table.rows();
    h.state_mut()
        .add_linked_table_now("Sales", table, "Area", layer, Some("Customers"));
    let app = h.state();
    let link = &app.project.links[0];
    let m = territoria_model::values::compute_matching(&app.project, link.id).unwrap();
    assert_eq!(m.stats.rows_matched, rows);
    let plan = h.state_mut().add_balanced_plan_now(layer, 6);
    settle(&mut h);
    let app = h.state();
    let p = app.project.plan(plan).unwrap();
    assert!(p.assignment.iter().all(Option::is_some));
    assert_eq!(p.territories.len(), 6);
}

/// Side panels keep their size: no content may ask for more width than it
/// has (egui panels grow to fit their content).
#[test]
fn panels_keep_their_width() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    settle(&mut h);
    let width = |h: &Harness<'_, TerritoriaApp>, id: &str| {
        egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new(id))
            .map(|s| s.outer_rect.width())
    };
    let check = |h: &mut Harness<'_, TerritoriaApp>, what: &str| {
        h.run_steps(3);
        let (l0, r0) = (width(h, "contents"), width(h, "inspector"));
        h.run_steps(20);
        let (l1, r1) = (width(h, "contents"), width(h, "inspector"));
        assert_eq!(l0, l1, "contents panel grows ({what})");
        assert_eq!(r0, r1, "inspector panel grows ({what})");
        assert!(
            r1.is_some_and(|w| w < 400.0),
            "inspector panel is {r1:?} wide ({what})"
        );
    };
    check(&mut h, "layer properties");
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales", table, "Area", layer, Some("Revenue"));
    settle(&mut h);
    check(&mut h, "choropleth properties");
    h.state_mut().add_balanced_plan_now(layer, 5);
    settle(&mut h);
    check(&mut h, "territories");
}

/// Through the dialog, as a user does it: read the file, confirm.
#[test]
fn import_dialog_adds_layer_and_map() {
    use egui_kittest::kittest::Queryable;
    let (mut h, _lock) = harness(false);
    h.state_mut()
        .import_boundaries(hexagons(), territoria_io::VectorOptions::default());
    settle(&mut h);
    h.get_by_label("Add to map").click();
    settle(&mut h);
    let app = h.state();
    assert_eq!(app.project.layers.len(), 1);
    assert_eq!(app.project.layers[0].key_column.as_deref(), Some("code"));
    assert_eq!(app.project.maps.len(), 1);
    assert_eq!(app.project.maps[0].layers.len(), 1);
}

/// Dragging over the map in paint mode assigns areas; undo takes it back.
#[test]
fn painting_assigns_areas_with_undo() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&hexagons())
        .expect("boundaries")[0];
    settle(&mut h);
    let plan = h.state_mut().project.add_plan("Plan", layer, 2);
    {
        let app = h.state_mut();
        let t = app.project.plan(plan).unwrap().territories[0].id;
        app.active_plan = Some(plan);
        app.active_territory = Some(t);
        app.tool = crate::map_view::Tool::Paint;
    }
    h.run_steps(3);
    // A horizontal stroke through the middle of the map.
    let y = 450.0;
    let press = |h: &mut Harness<'_, TerritoriaApp>, x: f32, pressed: bool| {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    };
    h.input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(500.0, y)));
    h.step();
    press(&mut h, 500.0, true);
    h.step();
    for x in (500..900).step_by(20) {
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(x as f32, y)));
        h.step();
    }
    press(&mut h, 900.0, false);
    h.run_steps(3);
    let assigned = |h: &Harness<'_, TerritoriaApp>| {
        h.state()
            .project
            .plan(plan)
            .unwrap()
            .assignment
            .iter()
            .filter(|a| a.is_some())
            .count()
    };
    let painted = assigned(&h);
    assert!(painted >= 8, "painted {painted} areas");
    assert!(
        h.state()
            .history
            .undo_label()
            .is_some_and(|l| l.starts_with("Assign"))
    );
    let app = h.state_mut();
    let mut project = std::mem::take(&mut app.project);
    app.history.undo(&mut project);
    app.project = project;
    assert_eq!(assigned(&h), 0);
}

/// Plans export as Excel (territories + assignment) and as dissolved
/// outlines in GeoJSON.
#[test]
fn exports_plan() {
    use crate::app::Export;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&hexagons())
        .expect("boundaries")[0];
    let plan = h.state_mut().add_balanced_plan_now(layer, 4);
    let dir = std::env::temp_dir().join(format!("territoria-app-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let xlsx = dir.join("plan.xlsx");
    let geojson = dir.join("plan.geojson");
    h.state_mut().export(Export::PlanExcel(plan), xlsx.clone());
    h.state_mut()
        .export(Export::PlanGeoJson(plan), geojson.clone());
    settle(&mut h);
    let (territories, info) = territoria_io::read_table(&xlsx, &Default::default()).unwrap();
    assert_eq!(
        info.sheets,
        vec!["Territories".to_owned(), "Assignment".to_owned()]
    );
    assert_eq!(territories.rows(), 4);
    let layers = territoria_io::read_vector(&geojson, &Default::default()).unwrap();
    assert_eq!(layers[0].geometries.len(), 4);
    // Outlines of contiguous territories are single polygons.
    if let territoria_io::Geometries::Areas(a) = &layers[0].geometries {
        assert!(
            a.iter().all(|mp| mp.0.len() == 1),
            "{:?}",
            a.iter().map(|m| m.0.len()).collect::<Vec<_>>()
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Merging hexagon codes by their first two digits makes coarser regions.
#[test]
fn merge_areas_by_prefix() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&hexagons())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let n = app.project.layer(layer).unwrap().len();
    app.selection = crate::app::Selection::Data(crate::app::DataRef::Layer(layer));
    app.merge_areas_for_test(layer, "code", Some(2), "Regions");
    let merged = app.project.layers.last().unwrap();
    assert_eq!(merged.name, "Regions");
    assert!(merged.len() < n / 5, "{} regions", merged.len());
    let areas = merged.attributes.column("Areas").unwrap();
    let total: f64 = (0..merged.len()).filter_map(|r| areas.f64(r)).sum();
    assert_eq!(total as usize, n);
    assert_eq!(app.project.maps[0].layers.len(), 2);
}

/// Download dialog to map, over the internet (`TERRITORIA_ONLINE=1`).
#[test]
fn download_boundaries_online() {
    use egui_kittest::kittest::Queryable;
    if std::env::var_os("TERRITORIA_ONLINE").is_none() {
        return;
    }
    let (mut h, _lock) = harness(false);
    h.state_mut()
        .run_command_for_test(crate::commands::Command::DownloadBoundaries);
    settle(&mut h);
    h.get_by_role(egui::accesskit::Role::TextInput)
        .type_text("Liechtenstein");
    h.run_steps(3);
    h.get_by_label_contains("Liechtenstein").click();
    h.run_steps(3);
    save(&mut h, "download-dialog");
    h.get_by_label("Download").click();
    settle(&mut h);
    h.get_by_label("Add to map").click();
    settle(&mut h);
    let layer = &h.state().project.layers[0];
    assert!(
        layer
            .source
            .attribution
            .as_deref()
            .is_some_and(|a| a.contains("geoBoundaries"))
    );
    save(&mut h, "downloaded");
}

/// Customers with coordinates, clustered around a few towns.
fn customers(app: &TerritoriaApp, layer: LayerId, n: usize) -> Table {
    let b = app.project.layer(layer).unwrap().geometry.bbox.unwrap();
    let towns = [
        (0.3, 0.35, 0.08),
        (0.65, 0.6, 0.12),
        (0.45, 0.8, 0.05),
        (0.8, 0.25, 0.06),
    ];
    let mut rnd = 12345u64;
    let mut next = || {
        rnd ^= rnd << 13;
        rnd ^= rnd >> 7;
        rnd ^= rnd << 17;
        (rnd % 1_000_000) as f64 / 1_000_000.0
    };
    let (mut ids, mut lon, mut lat, mut sales) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..n {
        let (cx, cy, spread) = if next() < 0.75 {
            towns[i % towns.len()]
        } else {
            (next(), next(), 0.0)
        };
        let (u, v) = (
            cx + (next() - 0.5) * 2.0 * spread,
            cy + (next() - 0.5) * 2.0 * spread,
        );
        ids.push(Some(format!("C{i:05}")));
        lon.push(Some(b.min().x + u * b.width()));
        lat.push(Some(b.min().y + v * b.height()));
        sales.push(Some((500.0 + 9500.0 * next()).round()));
    }
    Table::new(vec![
        Column::text("Customer", ColumnKind::Key, ids),
        Column::number("Longitude", lon),
        Column::number("Latitude", lat),
        Column::number("Sales", sales),
    ])
    .unwrap()
}

/// A customer table becomes points; areas count them; territories balance
/// customers.
#[test]
fn customers_as_points() {
    use crate::dialogs::Placement;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = customers(h.state(), layer, 3000);
    assert_eq!(
        territoria_data::suggest::coordinate_columns(&table),
        Some((1, 2, true))
    );
    let app = h.state_mut();
    app.add_table(
        "Customers".into(),
        table,
        PathBuf::from("customers.csv"),
        Placement::Points {
            x: "Longitude".into(),
            y: "Latitude".into(),
        },
        None,
    );
    let points = app.project.layers.last().unwrap().id;
    assert_eq!(app.project.layer(points).unwrap().len(), 3000);
    // Customers per area add up to the customers inside the areas.
    let mut values = territoria_model::Cache::default();
    let source = territoria_model::ValueSource::Points {
        layer: points,
        column: None,
        agg: territoria_data::Aggregate::Count,
    };
    let counts = values.values(&app.project, layer, &source);
    let inside: f64 = counts.iter().flatten().sum();
    assert!(inside > 2500.0 && inside <= 3000.0, "{inside}");
    // Balance territories by customers.
    let plan = app.add_balanced_plan_now(layer, 6);
    let rev = app.project.bump();
    let p = app.project.plan_mut(plan).unwrap();
    p.measures[0].source = source.clone();
    p.rev = rev;
    let settings = crate::territories::BalanceSettings::default();
    let input =
        crate::territories::balance_input(&app.project, &mut values, plan, &settings).unwrap();
    let proposal = crate::territories::run_balance(input, &mut |_| true).unwrap();
    assert!(
        proposal.after.max_deviation[0] < proposal.before.max_deviation[0],
        "{:?} → {:?}",
        proposal.before.max_deviation,
        proposal.after.max_deviation
    );
    let rev = app.project.bump();
    let p = app.project.plan_mut(plan).unwrap();
    for (u, t) in &proposal.moves {
        p.assign(&[*u], Some(*t));
    }
    p.rev = rev;
    settle(&mut h);
    save(&mut h, "customers");
}

/// A street map under translucent areas and customer points
/// (`TERRITORIA_ONLINE=1`, needs the tile server).
#[test]
fn background_map_online() {
    use crate::dialogs::Placement;
    if std::env::var_os("TERRITORIA_ONLINE").is_none() || out_dir().is_none() {
        return;
    }
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = customers(h.state(), layer, 1500);
    let app = h.state_mut();
    app.add_table(
        "Customers".into(),
        table,
        PathBuf::from("customers.csv"),
        Placement::Points {
            x: "Longitude".into(),
            y: "Latitude".into(),
        },
        None,
    );
    let map = app.project.maps[0].id;
    let preset = &crate::tiles::PRESETS[0];
    let m = app.project.map_mut(map).unwrap();
    m.background = territoria_model::map::Background::Tiles {
        url: preset.url.into(),
        attribution: preset.attribution.into(),
    };
    m.layers[0].opacity = 0.3;
    settle(&mut h);
    let start = Instant::now();
    while h.state().tiles.pending() > 0 && start.elapsed() < Duration::from_secs(60) {
        h.step();
        std::thread::sleep(Duration::from_millis(50));
    }
    h.run_steps(3);
    save(&mut h, "background");
}

/// The map exports as PNG, SVG and PDF with title and legends.
#[test]
fn exports_map_files() {
    use crate::app::Export;
    use crate::export_map::Format;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    settle(&mut h);
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, Some("Revenue"));
    h.state_mut().add_balanced_plan_now(layer, 7);
    settle(&mut h);
    let dir = out_dir().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("territoria-map-export-{}", std::process::id()))
    });
    std::fs::create_dir_all(&dir).unwrap();
    for format in [Format::Png, Format::Svg, Format::Pdf] {
        let path = dir.join(format!("map-export.{}", format.extension()));
        let app = h.state_mut();
        app.set_map_export_for_test(crate::dialogs::MapExportSettings {
            format,
            scale: 2.0,
            title: "Sales territories 2027".into(),
            legend: true,
            scale_bar: true,
        });
        app.export(Export::Map(format), path.clone());
        settle(&mut h);
        let bytes = std::fs::read(&path).unwrap();
        match format {
            Format::Png => assert_eq!(&bytes[1..4], b"PNG"),
            Format::Svg => {
                assert!(String::from_utf8_lossy(&bytes).contains("Sales territories 2027"))
            }
            Format::Pdf => assert_eq!(&bytes[..5], b"%PDF-"),
        }
    }
}

/// Sites at the territories' centres; starting over builds territories
/// around them, each site inside its own territory.
#[test]
fn territories_around_sites() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 5);
    let g = app.project.layer(layer).unwrap().geometry.clone();
    assert_eq!(
        app.project
            .plan_mut(plan)
            .unwrap()
            .sites_at_centres(&g, true, None),
        5
    );
    // Move one site to a corner, then start over around the sites.
    let corner = g.label_points[0].unwrap();
    app.project.plan_mut(plan).unwrap().territories[0].site = Some([corner.x, corner.y]);
    let settings = crate::territories::BalanceSettings {
        start_over: true,
        ..Default::default()
    };
    let mut values = territoria_model::Cache::default();
    let input =
        crate::territories::balance_input(&app.project, &mut values, plan, &settings).unwrap();
    let proposal = crate::territories::run_balance(input, &mut |_| true).unwrap();
    let rev = app.project.bump();
    let p = app.project.plan_mut(plan).unwrap();
    for (u, t) in &proposal.moves {
        p.assign(&[*u], Some(*t));
    }
    p.rev = rev;
    let p = app.project.plan(plan).unwrap();
    for (t, unit) in p.territories.iter().zip(p.site_units(&g)) {
        let unit = unit.expect("site inside an area");
        assert_eq!(
            p.assignment[unit as usize],
            Some(t.id),
            "site of {} outside",
            t.name
        );
    }
    assert_eq!(proposal.after.split, 0);
    settle(&mut h);
    save(&mut h, "sites");
}

/// Customers themselves as planning units: balanced, contiguous in the
/// Voronoi sense.
#[test]
fn plan_on_customer_points() {
    use crate::dialogs::Placement;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = customers(h.state(), layer, 1500);
    let app = h.state_mut();
    app.add_table(
        "Customers".into(),
        table,
        PathBuf::from("customers.csv"),
        Placement::Points {
            x: "Longitude".into(),
            y: "Latitude".into(),
        },
        None,
    );
    let points = app.project.layers.last().unwrap().id;
    let g = app.project.layer(points).unwrap().geometry.clone();
    let avg = g.neighbors.iter().map(Vec::len).sum::<usize>() as f64 / g.len() as f64;
    assert!(avg > 3.0 && avg < 12.0, "{avg} neighbours on average");
    let plan = app.add_balanced_plan_now(points, 6);
    let p = app.project.plan(plan).unwrap();
    assert!(p.assignment.iter().all(Option::is_some));
    let members = p.members();
    for m in &members {
        assert!((200..=300).contains(&m.len()), "{} customers", m.len());
        assert_eq!(g.components(m), 1);
    }
    settle(&mut h);
    save(&mut h, "customer-territories");
}

/// A scenario: a copy of a plan with some areas moved, compared with the
/// original; the changes are marked on the map and export to Excel.
#[test]
fn scenario_compares_with_original() {
    use crate::app::Export;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, Some("Revenue"));
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 5);
    let copy = app.duplicate_plan_for_test(plan).expect("a copy");
    assert_eq!(app.project.plans.len(), 2);
    assert_eq!(
        app.project.plan(copy).unwrap().name,
        "Sales territories – scenario"
    );
    // Only the copy is on the map.
    let map = app.project.maps.first().unwrap();
    let visible: Vec<_> = map
        .layers
        .iter()
        .filter(|l| l.visible)
        .filter_map(|l| l.kind.plan())
        .collect();
    assert_eq!(visible, vec![copy]);
    // Move areas along the border of the first two territories.
    let g = app.project.layer(layer).unwrap().geometry.clone();
    let p = app.project.plan_mut(copy).unwrap();
    let (t0, t1) = (p.territories[0].id, p.territories[1].id);
    let border: Vec<u32> = (0..g.len() as u32)
        .filter(|&u| p.assignment[u as usize] == Some(t0))
        .filter(|&u| {
            g.neighbors[u as usize]
                .iter()
                .any(|(n, _)| p.assignment[*n as usize] == Some(t1))
        })
        .take(6)
        .collect();
    assert!(!border.is_empty());
    p.assign(&border, Some(t1));
    let rev = app.project.bump();
    app.project.plan_mut(copy).unwrap().rev = rev;
    settle(&mut h);
    let mut marked = h.state().marked_for_test().expect("changes marked");
    marked.sort_unstable();
    assert_eq!(marked, border);
    save(&mut h, "scenario");

    let dir = std::env::temp_dir().join(format!("territoria-app-scenario-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let xlsx = dir.join("changes.xlsx");
    h.state_mut()
        .export(Export::Changes(plan, copy), xlsx.clone());
    settle(&mut h);
    let (changes, info) = territoria_io::read_table(&xlsx, &Default::default()).unwrap();
    assert_eq!(
        info.sheets,
        vec!["Changes".to_owned(), "Territories".to_owned()]
    );
    assert_eq!(changes.rows(), border.len());
    let _ = std::fs::remove_dir_all(dir);
}

/// A calculated column from a formula: errors point at the mistake, the
/// column is added with its formula and goes again with undo.
#[test]
fn calculated_column() {
    use egui_kittest::kittest::Queryable;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, Some("Revenue"));
    let id = h.state().project.tables[0].id;
    let data = crate::app::DataRef::Table(id);
    h.state_mut()
        .open_formula_for_test(data, "Per customer", "[Revenu] / [Customers]");
    h.run_steps(3);
    assert!(
        h.query_by_label_contains("Did you mean [Revenue]?")
            .is_some(),
        "the mistake is explained"
    );
    h.state_mut().close_dialogs();
    h.state_mut().open_formula_for_test(
        data,
        "Revenue per customer",
        "round([Revenue] / [Customers], 1)",
    );
    h.run_steps(3);
    save(&mut h, "formula");
    h.get_by_label("Add column").click();
    h.run_steps(2);
    let t = &h.state().project.tables[0].table;
    let c = t.column("Revenue per customer").expect("the new column");
    assert_eq!(
        c.formula.as_deref(),
        Some("round([Revenue] / [Customers], 1)")
    );
    let (rev, cust) = (
        t.column("Revenue").unwrap().f64(0).unwrap(),
        t.column("Customers").unwrap().f64(0).unwrap(),
    );
    assert_eq!(c.f64(0), Some((rev / cust * 10.0).round() / 10.0));
    h.state_mut()
        .run_command_for_test(crate::commands::Command::Undo);
    h.run_steps(2);
    assert!(
        h.state().project.tables[0]
            .table
            .column("Revenue per customer")
            .is_none()
    );
}

/// Customers on an area plan know their territory: as a live column in
/// their table, in the table export and in the plan's workbook.
#[test]
fn customers_know_their_territory() {
    use crate::app::{DataRef, Export};
    use crate::dialogs::Placement;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = customers(h.state(), layer, 400);
    let app = h.state_mut();
    app.add_table(
        "Customers".into(),
        table,
        PathBuf::from("customers.csv"),
        Placement::Points {
            x: "Longitude".into(),
            y: "Latitude".into(),
        },
        None,
    );
    let points = app.project.layers.last().unwrap().id;
    let plan = app.add_balanced_plan_now(layer, 4);
    app.open_table_for_test(DataRef::Layer(points));
    settle(&mut h);
    save(&mut h, "customer-table");

    let dir = std::env::temp_dir().join(format!("territoria-app-rows-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("customers.csv");
    let xlsx = dir.join("plan.xlsx");
    h.state_mut()
        .export(Export::Data(DataRef::Layer(points), false), csv.clone());
    h.state_mut().export(Export::PlanExcel(plan), xlsx.clone());
    settle(&mut h);
    let (rows, _) = territoria_io::read_table(&csv, &Default::default()).unwrap();
    assert_eq!(rows.rows(), 400);
    let t = rows
        .column("Territory (Sales territories)")
        .expect("territory column");
    // All areas are assigned: only customers outside every area have none.
    let mut values = territoria_model::Cache::default();
    let rows_units = h
        .state()
        .project
        .layer_rows(&mut values, points, layer)
        .unwrap();
    let outside = (0..400).filter(|&r| rows_units.unit(r).is_none()).count();
    assert!(outside < 200, "{outside} outside");
    assert_eq!(t.null_count(), outside);
    let (_, info) = territoria_io::read_table(&xlsx, &Default::default()).unwrap();
    assert_eq!(
        info.sheets,
        vec![
            "Territories".to_owned(),
            "Assignment".to_owned(),
            "Customers".to_owned()
        ]
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Labels and hover cards from a template, with values of a linked table;
/// several lines survive the SVG export.
#[test]
fn label_templates() {
    use crate::app::Export;
    use crate::export_map::Format;
    use territoria_model::LabelMode;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, Some("Revenue"));
    let app = h.state_mut();
    let l = app.project.layer(layer).unwrap();
    let name = l.name_column.clone().or(l.key_column.clone()).unwrap();
    let template = format!("{{[{name}]}}\n{{format([Revenue] / 1000, 0)}}k");
    let map = app.project.maps[0].id;
    for ml in &mut app.project.map_mut(map).unwrap().layers {
        if ml.kind.layer() == Some(layer)
            && let Some(s) = ml.kind.style_mut()
        {
            s.labels = LabelMode::Template(template.clone());
            s.tooltip = Some(template.clone());
        }
    }
    let mut values = territoria_model::Cache::default();
    let ft = values.feature_table(&app.project, layer).unwrap();
    for c in ["Revenue", "Customers", "Sales 2026 rows", "Area km2"] {
        assert!(ft.column(c).is_some(), "{c} in the feature table");
    }
    let texts = values
        .template_texts(&app.project, layer, &template)
        .unwrap();
    let f = (0..ft.rows())
        .find(|&f| ft.column("Revenue").unwrap().f64(f).is_some())
        .unwrap();
    let revenue = ft.column("Revenue").unwrap().f64(f).unwrap();
    assert_eq!(
        texts[f].as_deref(),
        Some(
            format!(
                "{}\n{:.0}k",
                ft.column(&name).unwrap().display(f),
                revenue / 1000.0
            )
            .as_str()
        )
    );
    let app = h.state_mut();
    let ml = app.project.maps[0]
        .layers
        .iter()
        .find(|l| matches!(l.kind, territoria_model::MapLayerKind::Choropleth { .. }))
        .unwrap()
        .id;
    app.selection = crate::app::Selection::MapLayer(map, ml);
    settle(&mut h);
    save(&mut h, "label-templates");

    // Short lines, so they fit the areas at this zoom.
    let app = h.state_mut();
    for ml in &mut app.project.map_mut(map).unwrap().layers {
        if ml.kind.layer() == Some(layer)
            && let Some(s) = ml.kind.style_mut()
        {
            s.labels =
                LabelMode::Template("{round([Customers] / 10)}\n{round([Customers] / 100)}".into());
        }
    }
    let dir = std::env::temp_dir().join(format!("territoria-labels-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("map.svg");
    app.set_map_export_for_test(crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: true,
        scale_bar: true,
    });
    app.export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(svg.contains("<tspan"), "labels on two lines");
    let _ = std::fs::remove_dir_all(dir);
}

/// Selecting areas and assigning them in one go, with undo.
#[test]
fn select_and_assign() {
    use crate::map_view::{MapEvent, SelectMode};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 4);
    let p = app.project.plan(plan).unwrap();
    let (t0, t1) = (p.territories[0].id, p.territories[1].id);
    let units: Vec<u32> = (0..p.assignment.len() as u32)
        .filter(|&u| p.assignment[u as usize] == Some(t0))
        .take(5)
        .collect();
    app.map_events_for_test(vec![MapEvent::Select {
        layer,
        units: units[..3].to_vec(),
        mode: SelectMode::Replace,
    }]);
    // Shift+click toggles: one leaves, two join.
    app.map_events_for_test(vec![MapEvent::Select {
        layer,
        units: vec![units[2], units[3], units[4]],
        mode: SelectMode::Toggle,
    }]);
    let mut expected = vec![units[0], units[1], units[3], units[4]];
    expected.sort_unstable();
    assert_eq!(app.selected, Some((layer, expected.clone())));
    app.active_territory = Some(t1);
    app.tool = crate::map_view::Tool::Select;
    settle(&mut h);
    save(&mut h, "selection");
    let app = h.state_mut();
    app.map_events_for_test(vec![MapEvent::AssignSelection { plan, to: Some(t1) }]);
    let p = app.project.plan(plan).unwrap();
    assert!(
        expected
            .iter()
            .all(|&u| p.assignment[u as usize] == Some(t1))
    );
    app.run_command_for_test(crate::commands::Command::Undo);
    let p = h.state().project.plan(plan).unwrap();
    assert!(
        expected
            .iter()
            .all(|&u| p.assignment[u as usize] == Some(t0))
    );
    // Esc-like clearing.
    h.state_mut()
        .map_events_for_test(vec![MapEvent::ClearSelection]);
    assert!(h.state().selected.is_none());
}

/// A territory report: one PDF with an overview and a page per territory.
#[test]
fn territory_report() {
    use crate::app::Export;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, Some("Revenue"));
    let plan = h.state_mut().add_balanced_plan_now(layer, 5);
    // Report by revenue rather than the number of areas.
    let app = h.state_mut();
    let link = app.project.links[0].id;
    let rev = app.project.bump();
    let p = app.project.plan_mut(plan).unwrap();
    p.measures = vec![territoria_model::Measure {
        source: territoria_model::ValueSource::Linked {
            link,
            column: "Revenue".into(),
            agg: territoria_data::Aggregate::Sum,
        },
        tolerance: 0.15,
        target: None,
    }];
    p.rev = rev;
    settle(&mut h);
    let dir = std::env::temp_dir().join(format!("territoria-report-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("report.pdf");
    h.state_mut().export(Export::Report(plan), path.clone());
    settle(&mut h);
    let bytes = std::fs::read(&path).expect("the report");
    assert_eq!(&bytes[..5], b"%PDF-");
    let text = String::from_utf8_lossy(&bytes);
    let pages = text.matches("/Type /Page").count() - text.matches("/Type /Pages").count();
    assert_eq!(pages, 6, "an overview and five territories");
    let svgs = h.state_mut().report_pages_for_test(plan);
    assert!(svgs[1].contains("Territory 01") && svgs[1].contains("Deviation"));
    assert!(svgs[1].contains("Largest areas by Revenue"));
    if let Some(out) = out_dir() {
        let font = egui::FontDefinitions::default().font_data["Ubuntu-Light"]
            .font
            .to_vec();
        for (i, svg) in svgs.iter().take(2).enumerate() {
            let png =
                crate::export_map::encode(svg, crate::export_map::Format::Png, 1.5, &font).unwrap();
            std::fs::write(out.join(format!("report-page-{}.png", i + 1)), png).unwrap();
        }
        std::fs::copy(&path, out.join("report.pdf")).unwrap();
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Clicking table rows selects their areas; Shift toggles. The table shows
/// which rows belong to the map's selection.
#[test]
fn table_rows_select_areas() {
    use crate::app::{Action, DataRef};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, Some("Revenue"));
    let app = h.state_mut();
    let t = app.project.tables[0].id;
    let link = app.project.links[0].id;
    let m = territoria_model::Cache::default()
        .matching(&app.project, link)
        .unwrap();
    let unit = |r: usize| m.row_target[r].unwrap();
    app.apply_action_for_test(Action::SelectRows(DataRef::Table(t), vec![0, 1, 2], false));
    let mut expected = vec![unit(0), unit(1), unit(2)];
    expected.sort_unstable();
    assert_eq!(app.selected, Some((layer, expected)));
    app.apply_action_for_test(Action::SelectRows(DataRef::Table(t), vec![1], true));
    let mut expected = vec![unit(0), unit(2)];
    expected.sort_unstable();
    assert_eq!(app.selected, Some((layer, expected)));
    app.open_table_for_test(DataRef::Table(t));
    settle(&mut h);
    save(&mut h, "table-selection");
}

/// Revenue per customer: a choropleth relative to another value.
#[test]
fn choropleth_relative_to() {
    use territoria_model::{MapLayerKind, Normalize, ValueSource};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, Some("Revenue"));
    let app = h.state_mut();
    let link = app.project.links[0].id;
    let customers = ValueSource::Linked {
        link,
        column: "Customers".into(),
        agg: territoria_data::Aggregate::Sum,
    };
    let map = app.project.maps[0].id;
    let mut ml_id = None;
    for ml in &mut app.project.map_mut(map).unwrap().layers {
        if let MapLayerKind::Choropleth { normalize, .. } = &mut ml.kind {
            *normalize = Normalize::By(customers.clone());
            ml.name = "Revenue per customer".into();
            ml_id = Some(ml.id);
        }
    }
    let ml_id = ml_id.unwrap();
    app.selection = crate::app::Selection::MapLayer(map, ml_id);
    let mut values = territoria_model::Cache::default();
    let revenue = ValueSource::Linked {
        link,
        column: "Revenue".into(),
        agg: territoria_data::Aggregate::Sum,
    };
    let rates = values.normalized(
        &app.project,
        layer,
        &revenue,
        &Normalize::By(customers.clone()),
    );
    let r = values.values(&app.project, layer, &revenue);
    let c = values.values(&app.project, layer, &customers);
    let f = (0..r.len()).find(|&f| r[f].is_some()).unwrap();
    assert_eq!(rates[f], Some(r[f].unwrap() / c[f].unwrap()));
    // Revenue per customer lies between 900 and 1,300 in the test data.
    assert!(rates.iter().flatten().all(|x| (850.0..1350.0).contains(x)));
    settle(&mut h);
    save(&mut h, "relative-to");
}

/// The active territory's details: size, code, notes.
#[test]
fn territory_details() {
    use egui_kittest::kittest::Queryable;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let plan = h.state_mut().add_balanced_plan_now(layer, 4);
    let t = h.state().project.plan(plan).unwrap().territories[1].id;
    h.state_mut().active_territory = Some(t);
    settle(&mut h);
    h.get_by_label_contains("Details of Territory 02").click();
    settle(&mut h);
    save(&mut h, "territory-details");
    assert!(h.query_by_label("Size").is_some());
}

/// Circles sized by value: on areas (revenue) and on points (sales), also
/// in exports.
#[test]
fn proportional_circles() {
    use crate::app::Export;
    use crate::dialogs::Placement;
    use crate::export_map::Format;
    use territoria_model::{MapLayer, MapLayerKind, ValueSource};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, None);
    let customers = customers(h.state(), layer, 300);
    let app = h.state_mut();
    app.add_table(
        "Customers".into(),
        customers,
        PathBuf::from("customers.csv"),
        Placement::Points {
            x: "Longitude".into(),
            y: "Latitude".into(),
        },
        None,
    );
    let points = app.project.layers.last().unwrap().id;
    let link = app.project.links[0].id;
    let map = app.project.maps[0].id;
    let revenue = ValueSource::Linked {
        link,
        column: "Revenue".into(),
        agg: territoria_data::Aggregate::Sum,
    };
    for (id, layer, source) in [
        (1001, layer, revenue.clone()),
        (
            1002,
            points,
            ValueSource::Attribute {
                column: "Sales".into(),
            },
        ),
    ] {
        let n = app.project.layer(layer).unwrap().len();
        app.project.map_mut(map).unwrap().layers.push(MapLayer {
            id: territoria_model::MapLayerId(id),
            name: format!("Circles {id}"),
            visible: true,
            opacity: 1.0,
            kind: MapLayerKind::Symbols {
                layer,
                source,
                normalize: Default::default(),
                symbol: territoria_model::SymbolStyle::for_count(n),
                style: Default::default(),
            },
        });
    }
    settle(&mut h);
    save(&mut h, "circles");
    // The largest revenue gets the full radius.
    let app = h.state_mut();
    let mut values = territoria_model::Cache::default();
    let v = values.values(&app.project, layer, &revenue);
    let max = v.iter().flatten().fold(0.0f64, |a, b| a.max(*b));
    let style = territoria_model::SymbolStyle::default();
    assert_eq!(style.radius(max, max), style.max_radius);
    assert!((style.radius(max / 4.0, max) - style.max_radius / 2.0).abs() < 1e-4);

    let dir = std::env::temp_dir().join(format!("territoria-circles-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("circles.svg");
    app.set_map_export_for_test(crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: true,
        scale_bar: false,
    });
    app.export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(
        svg.matches("<circle").count() > 100,
        "circles in the export"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The import dialog for a GeoPackage given in `TERRITORIA_TEST_GPKG`
/// (skipped without it).
#[test]
fn geopackage_import_dialog() {
    let Some(path) = std::env::var_os("TERRITORIA_TEST_GPKG").map(PathBuf::from) else {
        return;
    };
    let (mut h, _lock) = harness(false);
    h.state_mut()
        .import_boundaries(path, territoria_io::VectorOptions::default());
    settle(&mut h);
    save(&mut h, "gpkg-import");
}

/// A project reopens with the tabs, plan and territory it was saved with.
#[test]
fn project_remembers_workspace() {
    use crate::app::{DataRef, Tab};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, Some("Revenue"));
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 3);
    let t = app.project.plan(plan).unwrap().territories[2].id;
    app.active_territory = Some(t);
    let table = app.project.tables[0].id;
    app.open_table_for_test(DataRef::Table(table));
    settle(&mut h);
    let dir = std::env::temp_dir().join(format!("territoria-ws-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ws.territoria");
    h.state_mut().save_to_for_test(path.clone());
    settle(&mut h);

    drop(h);
    drop(_lock);
    let (mut h2, _lock2) = harness(false);
    h2.state_mut().open_path(&path);
    settle(&mut h2);
    let app = h2.state();
    assert!(
        app.open_tabs_for_test()
            .contains(&Tab::Table(DataRef::Table(table))),
        "{:?}",
        app.open_tabs_for_test()
    );
    assert_eq!(app.active_plan, Some(plan));
    assert_eq!(app.active_territory, Some(t));
    let _ = std::fs::remove_dir_all(dir);
}

/// The settings dialog.
#[test]
fn settings_dialog() {
    let (mut h, _lock) = harness(false);
    h.state_mut().open_settings_for_test();
    h.run_steps(3);
    save(&mut h, "settings");
}

/// Cells pasted from a spreadsheet open the table import dialog.
#[test]
fn paste_cells() {
    let (mut h, _lock) = harness(false);
    h.state_mut()
        .paste_table("Postcode\tRevenue\n0811\t1200,50\n8001\t900\n");
    settle(&mut h);
    save(&mut h, "paste");
    let d = h
        .state()
        .import_table_for_test()
        .expect("the import dialog");
    assert_eq!(d.0.rows(), 2);
    assert_eq!(d.0.columns[0].display(0), "0811", "leading zeros stay");
    assert_eq!(d.0.columns[1].f64(0), Some(1200.5), "decimal comma");
}

/// Distance zones: areas coloured by their distance to their territory's
/// site.
#[test]
fn distance_zones() {
    use territoria_model::{MapLayer, MapLayerKind, ValueSource};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 5);
    let g = app.project.layer(layer).unwrap().geometry.clone();
    app.project
        .plan_mut(plan)
        .unwrap()
        .sites_at_centres(&g, true, None);
    let map = app.project.maps[0].id;
    let m = app.project.map_mut(map).unwrap();
    // Right under the territories, which stay on top with their borders.
    let at = m
        .layers
        .iter()
        .position(|l| matches!(l.kind, MapLayerKind::Territories { .. }))
        .unwrap();
    m.layers.insert(
        at,
        MapLayer {
            id: territoria_model::MapLayerId(2001),
            name: "km to the site".into(),
            visible: true,
            opacity: 1.0,
            kind: MapLayerKind::Choropleth {
                layer,
                source: ValueSource::SiteDistance { plan },
                normalize: Default::default(),
                scale: Default::default(),
                style: Default::default(),
            },
        },
    );
    for ml in &mut m.layers {
        if let MapLayerKind::Territories { style, .. } = &mut ml.kind {
            style.fill_opacity = 0.0;
        }
    }
    let mut values = territoria_model::Cache::default();
    let d = values.values(&app.project, layer, &ValueSource::SiteDistance { plan });
    assert!(
        d.iter().flatten().count() > d.len() / 2,
        "distances for assigned areas"
    );
    assert!(
        d.iter().flatten().any(|x| *x == 0.0),
        "each site's own area is 0 km away"
    );
    settle(&mut h);
    save(&mut h, "distance-zones");
}

/// Hexagons over customers, counted per cell; and territories planned on
/// hexagons over the areas.
#[test]
fn hexagon_grid() {
    use crate::dialogs::Placement;
    use territoria_model::grid::{HexCover, HexSpec};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let customers = customers(h.state(), layer, 800);
    let app = h.state_mut();
    app.add_table(
        "Customers".into(),
        customers,
        PathBuf::from("customers.csv"),
        Placement::Points {
            x: "Longitude".into(),
            y: "Latitude".into(),
        },
        None,
    );
    let points = app.project.layers.last().unwrap().id;
    // Cells a 25th of the extent across.
    let b = app.project.layer(layer).unwrap().geometry.bbox.unwrap();
    let width = territoria_geo::haversine(b.min(), b.max()) / 1000.0 / 25.0;
    app.create_hex_grid(
        &HexSpec {
            width_km: width,
            cover: HexCover::Points(points),
            count: Some(points),
        },
        "Customer hexagons",
    );
    let hex = app.project.layers.last().unwrap();
    assert_eq!(hex.name, "Customer hexagons");
    let counted: f64 = (0..hex.len())
        .filter_map(|r| hex.attributes.column("Customers").unwrap().f64(r))
        .sum();
    assert_eq!(counted, 800.0, "every customer in one cell");
    settle(&mut h);
    save(&mut h, "hexagons");

    // Hexagons over the areas, as units of a plan.
    let app = h.state_mut();
    app.create_hex_grid(
        &HexSpec {
            width_km: width,
            cover: HexCover::Areas(layer),
            count: Some(points),
        },
        "Area hexagons",
    );
    let hex = app.project.layers.last().unwrap().id;
    let plan = app.add_balanced_plan_now(hex, 4);
    let p = app.project.plan(plan).unwrap();
    let g = &app.project.layer(hex).unwrap().geometry;
    for m in p.members() {
        assert!(!m.is_empty());
        assert_eq!(g.components(&m), 1, "contiguous on hexagons");
    }
}

/// A stand-in geocoder: answers `city=` queries with fixed coordinates.
fn geocoder_server() -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) <= 2 {
                    break;
                }
            }
            let place = [
                ("Bern", (7.4474, 46.948)),
                ("Zurich", (8.5417, 47.3769)),
                ("Basel", (7.5886, 47.5596)),
            ]
            .into_iter()
            .find(|(c, _)| line.contains(&format!("city={c}")));
            let body = match place {
                Some((c, (lon, lat))) => format!(
                    r#"[{{"lat":"{lat}","lon":"{lon}","display_name":"{c}","addresstype":"city","place_rank":16}}]"#
                ),
                None => "[]".to_owned(),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

/// Customers with addresses become points through geocoding.
#[test]
fn geocoded_customers() {
    use crate::dialogs::{AddressColumns, Placement};
    let (mut h, _lock) = harness(false);
    let table = Table::new(vec![
        Column::text(
            "Customer",
            ColumnKind::Key,
            ["C1", "C2", "C3", "C4"]
                .map(|s| Some(s.to_owned()))
                .to_vec(),
        ),
        Column::text(
            "Town",
            ColumnKind::Text,
            ["Bern", "Zurich", "Atlantis", "Basel"]
                .map(|s| Some(s.to_owned()))
                .to_vec(),
        ),
    ])
    .unwrap();
    let app = h.state_mut();
    app.settings.geocoder.url = geocoder_server();
    app.settings.geocoder.interval_ms = 0;
    let columns = AddressColumns::suggest(&table);
    assert_eq!(columns.city.as_deref(), Some("Town"));
    let columns = AddressColumns {
        default_country: "Switzerland".into(),
        ..columns
    };
    app.add_table(
        format!("Customers {}", std::process::id()),
        table,
        PathBuf::from("customers.csv"),
        Placement::Address(columns),
        None,
    );
    settle(&mut h);
    let layer = h.state().project.layers.last().expect("a points layer");
    assert_eq!(layer.kind(), territoria_io::GeometryKind::Point);
    let q = layer.attributes.column("Geocoding").unwrap();
    assert_eq!(
        (0..4).map(|r| q.display(r)).collect::<Vec<_>>(),
        vec!["town", "town", "not found", "town"]
    );
    let p = &layer.geometry.points[0].0[0];
    assert!((p.x() - 7.4474).abs() < 1e-9 && (p.y() - 46.948).abs() < 1e-9);
    assert!(layer.geometry.points[2].0.is_empty(), "not found: no point");
}

/// The import dialog for a table with addresses.
#[test]
fn address_import_dialog() {
    let (mut h, _lock) = harness(false);
    h.state_mut().paste_table(
        "Customer\tStreet\tZIP\tCity\tSales\nC001\tBundesplatz 3\t3011\tBern\t1200\nC002\tBahnhofstrasse 1\t8001\tZürich\t900\n",
    );
    settle(&mut h);
    h.state_mut().import_as_addresses_for_test();
    h.run_steps(3);
    save(&mut h, "address-import");
}

/// Reps' homes become the territories' sites (and names); or the best of
/// many locations are chosen.
#[test]
fn sites_from_reps() {
    use crate::dialogs::{Placement, SitesDialog};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 4);
    let g = app.project.layer(layer).unwrap().geometry.clone();
    // A rep near each territory's centre, listed in another order.
    let centres = app.project.plan(plan).unwrap().centres(&g, None);
    let order = [2, 0, 3, 1];
    let reps = Table::new(vec![
        Column::text(
            "Rep",
            ColumnKind::Key,
            order.iter().map(|i| Some(format!("Rep {i}"))).collect(),
        ),
        Column::number(
            "Lon",
            order.iter().map(|&i| Some(centres[i].unwrap().x)).collect(),
        ),
        Column::number(
            "Lat",
            order.iter().map(|&i| Some(centres[i].unwrap().y)).collect(),
        ),
    ])
    .unwrap();
    app.add_table(
        "Reps".into(),
        reps,
        PathBuf::from("reps.csv"),
        Placement::Points {
            x: "Lon".into(),
            y: "Lat".into(),
        },
        None,
    );
    let reps = app.project.layers.last().unwrap().id;
    let mut d = SitesDialog::new(&app.project, plan);
    d.layer = Some(reps);
    app.place_sites(&d);
    let p = app.project.plan(plan).unwrap();
    for (i, t) in p.territories.iter().enumerate() {
        assert_eq!(t.name, format!("Rep {i}"), "the nearest rep");
        let c = centres[i].unwrap();
        assert_eq!(t.site, Some([c.x, c.y]));
    }
    // Choosing: the best four of the reps' places are all four.
    d.best = true;
    app.place_sites(&d);
    assert!(
        app.project
            .plan(plan)
            .unwrap()
            .territories
            .iter()
            .all(|t| t.site.is_some())
    );
    // Undo goes back to the matched sites.
    app.run_command_for_test(crate::commands::Command::Undo);
    assert_eq!(app.project.plan(plan).unwrap().territories[0].name, "Rep 0");
}

/// Catchment rings around territory sites with customers and revenue
/// within them, on the map and in a table.
#[test]
fn catchment_rings() {
    use crate::dialogs::Placement;
    use territoria_model::ValueSource;
    use territoria_model::catchment::{CatchmentSpec, Centres};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, None);
    let customers = customers(h.state(), layer, 600);
    let app = h.state_mut();
    app.add_table(
        "Customers".into(),
        customers,
        PathBuf::from("customers.csv"),
        Placement::Points {
            x: "Longitude".into(),
            y: "Latitude".into(),
        },
        None,
    );
    let points = app.project.layers.last().unwrap().id;
    let plan = app.add_balanced_plan_now(layer, 4);
    let g = app.project.layer(layer).unwrap().geometry.clone();
    app.project
        .plan_mut(plan)
        .unwrap()
        .sites_at_centres(&g, true, None);
    let b = g.bbox.unwrap();
    let span = territoria_geo::haversine(b.min(), b.max()) / 1000.0;
    let (r1, r2) = (
        (span / 12.0).round().max(1.0),
        (span / 5.0).round().max(2.0),
    );
    let link = app.project.links[0].id;
    app.create_catchment(&CatchmentSpec {
        centres: Centres::Sites(plan),
        radii_km: vec![r1, r2],
        points: Some(points),
        areas: Some((
            layer,
            ValueSource::Linked {
                link,
                column: "Revenue".into(),
                agg: territoria_data::Aggregate::Sum,
            },
        )),
    });
    let t = &app.project.tables.last().unwrap().table;
    assert_eq!(t.rows(), 4, "a row per site");
    let km = territoria_data::table::format_number;
    let near = t
        .column(&format!("Customers within {} km", km(r1)))
        .unwrap();
    let far = t
        .column(&format!("Customers within {} km", km(r2)))
        .unwrap();
    for r in 0..4 {
        assert!(far.f64(r).unwrap() >= near.f64(r).unwrap());
    }
    assert!(
        t.column(&format!("Revenue (Sales 2026) within {} km", km(r2)))
            .is_some(),
        "{:?}",
        t.columns.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
    settle(&mut h);
    // Back to the map for the picture.
    let app = h.state_mut();
    let map = app.project.maps[0].id;
    app.apply_action_for_test(crate::app::Action::OpenTab(crate::app::Tab::Map(map)));
    settle(&mut h);
    save(&mut h, "catchment");
    // Removing the plan removes its rings.
    let app = h.state_mut();
    app.project.remove_plan(plan);
    assert!(
        !app.project.maps[0]
            .layers
            .iter()
            .any(|l| matches!(l.kind, territoria_model::MapLayerKind::Rings { .. }))
    );
}

/// Territories grouped into contiguous, balanced regions with wider
/// borders between them.
#[test]
fn regions_above_territories() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 8);
    let mut values = territoria_model::Cache::default();
    let stats = app
        .project
        .plan(plan)
        .unwrap()
        .stats(&app.project, &mut values);
    let groups = crate::territories::group_regions(&app.project, &stats, plan, 3).expect("regions");
    {
        let p = app.project.plan_mut(plan).unwrap();
        let ids: Vec<_> = (0..3).map(|_| p.add_region(None)).collect();
        for (t, g) in p.territories.iter_mut().zip(&groups) {
            t.region = g.map(|g| ids[g]);
        }
        p.rev += 1;
    }
    app.project.bump();
    let p = app.project.plan(plan).unwrap();
    assert!(
        p.territories.iter().all(|t| t.region.is_some()),
        "every territory in a region"
    );
    // Each region is one piece of neighbouring territories.
    let g = &app.project.layer(layer).unwrap().geometry;
    let unit_regions = p.unit_regions();
    for r in &p.regions {
        let units: Vec<u32> = (0..unit_regions.len() as u32)
            .filter(|&u| unit_regions[u as usize] == Some(r.id))
            .collect();
        assert!(!units.is_empty());
        assert_eq!(g.components(&units), 1, "{} in one piece", r.name);
    }
    let rs = p.region_stats(&stats);
    let total: usize = rs.iter().map(|r| r.units).sum();
    assert_eq!(
        total,
        stats.territories.iter().map(|t| t.units).sum::<usize>()
    );
    // Exports name the region.
    assert!(
        p.territory_table(&app.project, &stats)
            .column("Region")
            .is_some()
    );
    app.active_plan = Some(plan);
    settle(&mut h);
    save(&mut h, "regions");
}

/// The class editor: manual limits, own labels, a colour for no data.
#[test]
fn class_editor() {
    use territoria_model::MapLayerKind;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, Some("Revenue"));
    let app = h.state_mut();
    let map = app.project.maps[0].id;
    let mut ml_id = None;
    for ml in &mut app.project.map_mut(map).unwrap().layers {
        if let MapLayerKind::Choropleth { scale, .. } = &mut ml.kind {
            scale.method = territoria_analysis::classify::Method::Manual;
            scale.manual = vec![10_000.0, 50_000.0];
            scale.labels = vec!["Low".into(), "Middle".into(), "High".into()];
            scale.no_data = Some(territoria_analysis::Rgba::rgb(0xee, 0xee, 0xee));
            ml_id = Some(ml.id);
        }
    }
    app.selection = crate::app::Selection::MapLayer(map, ml_id.unwrap());
    settle(&mut h);
    save(&mut h, "class-editor");
    let app = h.state_mut();
    let ml = app.project.maps[0]
        .layers
        .iter()
        .find(|l| Some(l.id) == ml_id)
        .unwrap()
        .clone();
    let scaled = app.analyses_for_test(&ml);
    let labels: Vec<&str> = scaled.legend.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(&labels[..3], &["Low", "Middle", "High"]);
}

/// The search box finds areas by key or name and territories by name.
#[test]
fn search_box() {
    use crate::search::{Target, search};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 3);
    let key = app.project.layer(layer).unwrap().key(5);
    let hits = search(&app.project, &key, 12);
    assert_eq!(
        hits[0].target,
        Target::Feature(layer, 5),
        "the exact key first"
    );
    let t = app.project.plan(plan).unwrap().territories[1].clone();
    let hits = search(&app.project, &t.name, 12);
    assert_eq!(hits[0].target, Target::Territory(plan, t.id));
    app.go_to(&Target::Feature(layer, 5));
    assert_eq!(app.selected, Some((layer, vec![5])));
    app.go_to(&Target::Territory(plan, t.id));
    assert_eq!(app.active_territory, Some(t.id));
    // The popup with matches, as when typing.
    let name = app.project.layer(layer).unwrap().display_name(7);
    let query: String = name.chars().take(3).collect();
    app.search_for_test(&query);
    settle(&mut h);
    save(&mut h, "search");
}

/// Copying the map renders it for the clipboard.
#[test]
fn copy_map_to_clipboard() {
    let (mut h, _lock) = harness(false);
    h.state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries");
    settle(&mut h);
    h.state_mut()
        .run_command_for_test(crate::commands::Command::CopyMap);
    settle(&mut h);
    let toasts = h.state().toasts_for_test();
    assert!(toasts.iter().any(|t| t.contains("clipboard")), "{toasts:?}");
}

/// Measuring a distance along clicked points.
#[test]
fn measure_distance() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    settle(&mut h);
    let b = h
        .state()
        .project
        .layer(layer)
        .unwrap()
        .geometry
        .bbox
        .unwrap();
    let (min, max) = (b.min(), b.max());
    let mid = [(min.x + max.x) / 2.0, (min.y + max.y) / 2.0];
    h.state_mut().measure_for_test(vec![
        [min.x + 0.1 * (max.x - min.x), mid[1]],
        mid,
        [mid[0], max.y - 0.1 * (max.y - min.y)],
    ]);
    settle(&mut h);
    save(&mut h, "measure");
}

/// Kept views, focus on a territory, and presentation mode.
#[test]
fn views_focus_presentation() {
    use crate::app::Action;
    use crate::map_view::MapEvent;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let plan = h.state_mut().add_balanced_plan_now(layer, 4);
    settle(&mut h);
    // Keep the view, move away, come back.
    let app = h.state_mut();
    let (center, scale) = app.view_for_test().expect("a view");
    app.run_command_for_test(crate::commands::Command::SaveView);
    let map = app.project.maps[0].id;
    assert_eq!(app.project.maps[0].views.len(), 1);
    let p = app.project.plan(plan).unwrap();
    let units: Vec<u32> = (0..p.assignment.len() as u32)
        .filter(|&u| p.assignment[u as usize] == Some(p.territories[0].id))
        .collect();
    app.apply_action_for_test(Action::SelectUnits(layer, units));
    settle(&mut h);
    assert_ne!(h.state().view_for_test().unwrap().1, scale, "zoomed in");
    h.state_mut()
        .apply_action_for_test(Action::GoToView(map, center, scale));
    settle(&mut h);
    assert_eq!(h.state().view_for_test().unwrap(), (center, scale));
    // Focus on the selected territory.
    h.state_mut()
        .map_events_for_test(vec![MapEvent::Focus(true)]);
    settle(&mut h);
    save(&mut h, "focus");
    // Presentation: the map alone.
    h.state_mut()
        .run_command_for_test(crate::commands::Command::Presentation);
    assert!(h.state().presenting_for_test());
    settle(&mut h);
    save(&mut h, "presentation");
}
