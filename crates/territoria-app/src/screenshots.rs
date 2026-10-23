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
