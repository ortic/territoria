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
    let sheets = dir.join("sheets.xlsx");
    h.state_mut()
        .export(Export::PlanSheets(plan), sheets.clone());
    settle(&mut h);
    // A sheet per territory with its areas.
    let (_, info) = territoria_io::read_table(&sheets, &Default::default()).unwrap();
    assert_eq!(info.sheets.len(), 5, "{:?}", info.sheets);
    assert_eq!(info.sheets[1], "01 Territory 01");
    let first = territoria_io::read_table(
        &sheets,
        &territoria_io::TableOptions {
            sheet: Some("01 Territory 01".into()),
            ..Default::default()
        },
    )
    .unwrap()
    .0;
    let members = h.state().project.plan(plan).unwrap().members();
    assert_eq!(first.rows(), members[0].len());
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
    // The dialog's search, not the toolbar's.
    h.get_all_by_role(egui::accesskit::Role::TextInput)
        .last()
        .expect("the dialog's search")
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
            page: Default::default(),
            footer: String::new(),
            overview: false,
            figures: false,
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
    h.state_mut().export(
        Export::Data(DataRef::Layer(points), false, None),
        csv.clone(),
    );
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
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
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
            zoom: Default::default(),
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
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
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
            zoom: Default::default(),
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
    let groups =
        crate::territories::group_regions(&app.project, &stats, plan, 1, 3).expect("regions");
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
    // A level above: the three regions grouped into two divisions.
    let app = h.state_mut();
    let groups =
        crate::territories::group_regions(&app.project, &stats, plan, 2, 2).expect("divisions");
    assert_eq!(groups.len(), 3, "one entry per region");
    {
        let p = app.project.plan_mut(plan).unwrap();
        let mut members = vec![Vec::new(); 2];
        for (i, g) in groups.iter().enumerate() {
            members[g.expect("every region in a division")].push(i);
        }
        assert!(members.iter().all(|m| !m.is_empty()));
        p.regroup(2, members.into_iter().map(|m| (None, m)).collect());
        p.rev += 1;
    }
    app.project.bump();
    let p = app.project.plan(plan).unwrap();
    assert_eq!(p.top_level(), 2);
    assert_eq!(p.regions_at(1).count(), 3, "the regions stay");
    let unit_divisions = p.unit_regions_at(2);
    for d in p.regions_at(2) {
        let units: Vec<u32> = (0..unit_divisions.len() as u32)
            .filter(|&u| unit_divisions[u as usize] == Some(d.id))
            .collect();
        assert_eq!(
            g_components(app, layer, &units),
            1,
            "{} in one piece",
            d.name
        );
    }
    let rs = p.region_stats(&stats);
    let divisions: usize = rs
        .iter()
        .filter(|s| p.region(s.id).is_some_and(|r| r.level == 2))
        .map(|s| s.territories)
        .sum();
    assert_eq!(divisions, 8, "every territory counts once per level");
    assert!(
        p.territory_table(&app.project, &stats)
            .column("Division")
            .is_some()
    );
    settle(&mut h);
    save(&mut h, "regions-levels");
}

fn g_components(app: &crate::app::TerritoriaApp, layer: LayerId, units: &[u32]) -> usize {
    app.project.layer(layer).unwrap().geometry.components(units)
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

/// The map as a web page: every area with its hover text.
#[test]
fn map_as_web_page() {
    use crate::app::Export;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, Some("Revenue"));
    h.state_mut().add_balanced_plan_now(layer, 3);
    settle(&mut h);
    let dir = std::env::temp_dir().join(format!("territoria-html-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("map.html");
    h.state_mut().export(Export::Html, path.clone());
    settle(&mut h);
    let page = std::fs::read_to_string(&path).unwrap();
    assert!(page.starts_with("<!doctype html>"));
    let n = h.state().project.layer(layer).unwrap().len();
    let tips = page.matches("data-tip=").count();
    assert!(tips >= n, "{tips} hover texts for {n} areas");
    assert!(
        page.contains("Revenue (Sales 2026): "),
        "values in the hover texts"
    );
    assert!(
        page.contains("Sales territories: Territory 0"),
        "territories in the hover texts"
    );
    if let Some(out) = out_dir() {
        std::fs::copy(&path, out.join("map.html")).unwrap();
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Newer boundaries replace a layer's; territories carry over.
#[test]
fn replace_with_newer_boundaries() {
    let (mut h, _lock) = harness(false);
    let file = boundaries();
    let layer = h.state_mut().add_boundaries_now(&file).expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 3);
    let before: Vec<_> = app.project.plan(plan).unwrap().assignment.to_vec();
    let n = before.len();
    // The "new version": the same file read again.
    app.read_replacement(layer, file.clone());
    settle(&mut h);
    let app = h.state();
    let after = app.project.plan(plan).unwrap().assignment.to_vec();
    assert_eq!(after.len(), n);
    assert_eq!(after, before, "every area keeps its territory");
    let toasts = app.toasts_for_test();
    assert!(
        toasts.iter().any(|t| t.starts_with("Replaced:")),
        "{toasts:?}"
    );
    // Undo brings the old version back.
    h.state_mut()
        .run_command_for_test(crate::commands::Command::Undo);
    assert_eq!(h.state().project.plan(plan).unwrap().assignment.len(), n);
}

/// A table reloaded from its changed file: new figures, kinds kept,
/// calculated columns computed again.
#[test]
fn reload_table_from_file() {
    use crate::dialogs::Placement;
    let (mut h, _lock) = harness(false);
    let dir = std::env::temp_dir().join(format!("territoria-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sales.csv");
    std::fs::write(&path, "Area,Revenue,Region\n0811,100,A\n8001,200,B\n").unwrap();
    let (table, _) = territoria_io::read_table(&path, &Default::default()).unwrap();
    let app = h.state_mut();
    app.add_table("Sales".into(), table, path.clone(), Placement::None, None);
    let id = app.project.tables[0].id;
    // A calculated column and a changed kind.
    app.apply_formula_for_test(crate::app::DataRef::Table(id), "Double", "[Revenue] * 2");
    // Next month: new figures, a new column, Region gone.
    std::fs::write(
        &path,
        "Area,Revenue,Customers\n0811,150,3\n8001,250,4\n3011,50,1\n",
    )
    .unwrap();
    app.reload_table(id);
    settle(&mut h);
    let t = &h.state().project.tables[0].table;
    assert_eq!(t.rows(), 3);
    assert_eq!(t.column("Area").unwrap().display(0), "0811");
    assert_eq!(
        t.column("Double").unwrap().f64(2),
        Some(100.0),
        "recomputed"
    );
    let toasts = h.state().toasts_for_test();
    assert!(
        toasts
            .iter()
            .any(|m| m.contains("new columns: Customers") && m.contains("gone: Region")),
        "{toasts:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A segment: customers with big sales only, counted per area.
#[test]
fn filtered_analysis() {
    use crate::dialogs::Placement;
    use territoria_model::{MapLayer, MapLayerKind, ValueSource};
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
    let all = ValueSource::Points {
        layer: points,
        column: None,
        agg: territoria_data::Aggregate::Count,
    };
    let big = ValueSource::filtered(all.clone(), "[Sales] > 8000");
    let mut values = territoria_model::Cache::default();
    let n_all: f64 = values
        .values(&app.project, layer, &all)
        .iter()
        .flatten()
        .sum();
    let n_big: f64 = values
        .values(&app.project, layer, &big)
        .iter()
        .flatten()
        .sum();
    assert!(n_big > 0.0 && n_big < n_all * 0.4, "{n_big} of {n_all}");
    let map = app.project.maps[0].id;
    let id = territoria_model::MapLayerId(3001);
    app.project.map_mut(map).unwrap().layers.insert(
        0,
        MapLayer {
            id,
            name: "Customers where [Sales] > 8000".into(),
            visible: true,
            opacity: 1.0,
            zoom: Default::default(),
            kind: MapLayerKind::Choropleth {
                layer,
                source: big,
                normalize: Default::default(),
                scale: Default::default(),
                style: Default::default(),
            },
        },
    );
    app.selection = crate::app::Selection::MapLayer(map, id);
    settle(&mut h);
    save(&mut h, "filtered");
}

/// A territory split into contiguous, balanced parts.
#[test]
fn split_territory() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 2);
    let t = app.project.plan(plan).unwrap().territories[0].id;
    let mut values = territoria_model::Cache::default();
    let parts =
        crate::territories::split_territory(&app.project, &mut values, plan, t, 3).expect("parts");
    let g = &app.project.layer(layer).unwrap().geometry;
    let total = parts.len() as f64;
    for k in 0..3 {
        let units: Vec<u32> = parts
            .iter()
            .filter(|(_, p)| *p == k)
            .map(|(u, _)| *u)
            .collect();
        assert_eq!(g.components(&units), 1, "part {k} in one piece");
        let share = units.len() as f64 / total;
        assert!(
            (share - 1.0 / 3.0).abs() < 0.25 / 3.0 + 0.02,
            "part {k}: {share}"
        );
    }
}

/// A plan's territories as a table.
#[test]
fn territory_table_tab() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 6);
    {
        let p = app.project.plan_mut(plan).unwrap();
        let north = p.add_region(Some("North".into()));
        let south = p.add_region(Some("South".into()));
        for (i, t) in p.territories.iter_mut().enumerate() {
            t.region = Some(if i < 3 { north } else { south });
        }
        p.territories[5].weight = 0.5;
        p.rev += 1;
    }
    app.apply_action_for_test(crate::app::Action::OpenTab(crate::app::Tab::Plan(plan)));
    settle(&mut h);
    save(&mut h, "territory-table");
    assert!(
        h.state()
            .open_tabs_for_test()
            .contains(&crate::app::Tab::Plan(plan))
    );
}

/// A map in another projection: national grid, then back.
#[test]
fn map_projection() {
    use crate::app::Action;
    let (mut h, _lock) = harness(false);
    h.state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries");
    settle(&mut h);
    let app = h.state_mut();
    let map = app.project.maps[0].id;
    let lv95 = territoria_geo::Crs::parse("EPSG:2056").unwrap();
    app.apply_action_for_test(Action::SetMapCrs(map, lv95.clone()));
    settle(&mut h);
    let app = h.state_mut();
    assert_eq!(app.project.maps[0].crs, lv95);
    // Drawn and fitted in metres of the national grid.
    let (center, _) = app.view_for_test().expect("a view");
    assert!(
        center[0] > 2_000_000.0 || center[0].abs() < 1e6,
        "{center:?}"
    );
    save(&mut h, "projection");
    h.state_mut()
        .run_command_for_test(crate::commands::Command::Undo);
    assert!(h.state().project.maps[0].crs.is_web_mercator());
}

/// Columns from the map: the area a customer lies in, the nearest rep,
/// customers per area.
#[test]
fn columns_from_the_map() {
    use crate::app::DataRef;
    use crate::dialogs::{JoinDialog, JoinMode, Placement};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
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
    let reps = Table::new(vec![
        Column::text(
            "Rep",
            ColumnKind::Key,
            vec![Some("North".into()), Some("South".into())],
        ),
        Column::number(
            "Lon",
            vec![
                Some(
                    app.project
                        .layer(layer)
                        .unwrap()
                        .geometry
                        .bbox
                        .unwrap()
                        .center()
                        .x
                );
                2
            ],
        ),
        Column::number("Lat", {
            let b = app.project.layer(layer).unwrap().geometry.bbox.unwrap();
            vec![Some(b.max().y), Some(b.min().y)]
        }),
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
    // The area each customer lies in.
    let mut d = JoinDialog::new(&app.project, DataRef::Layer(points));
    assert_eq!(d.mode, JoinMode::AreaOf);
    d.layer = Some(layer);
    app.apply_join(&d);
    // The nearest rep.
    d.mode = JoinMode::Nearest;
    d.layer = Some(reps);
    app.apply_join(&d);
    let c = &app.project.layer(points).unwrap().attributes;
    let area_key = c
        .columns
        .iter()
        .find(|c| {
            c.name
                .ends_with(&format!("({})", app.project.layer(layer).unwrap().name))
        })
        .expect("area key");
    let inside = (0..c.rows())
        .filter(|&r| !area_key.display(r).is_empty())
        .count();
    assert!(inside > 150, "{inside} customers in an area");
    let rep = c.column("Nearest Reps").unwrap();
    assert!((0..c.rows()).all(|r| rep.display(r) == "North" || rep.display(r) == "South"));
    assert!(c.column("km to the nearest Reps").unwrap().f64(0).unwrap() > 0.0);
    // Customers per area.
    let d = JoinDialog::new(&app.project, DataRef::Layer(layer));
    assert_eq!(d.mode, JoinMode::PointsIn);
    app.apply_join(&d);
    let a = &app.project.layer(layer).unwrap().attributes;
    let counted: f64 = (0..a.rows())
        .filter_map(|r| a.column("Number of Customers").unwrap().f64(r))
        .sum();
    assert_eq!(
        counted as usize, inside,
        "every customer inside counted once"
    );
    assert!(a.column("Sales (Customers)").is_some());
}

/// Editing a table: a cell, a new row, a deleted row; wrong text refused.
#[test]
fn edit_table_cells() {
    use crate::app::{Action, DataRef};
    use crate::dialogs::Placement;
    let (mut h, _lock) = harness(false);
    let table = Table::new(vec![
        Column::text(
            "Code",
            ColumnKind::Key,
            vec![Some("01".into()), Some("02".into())],
        ),
        Column::number("Sales", vec![Some(1.0), Some(2.0)]),
    ])
    .unwrap();
    let app = h.state_mut();
    app.add_table(
        "Sales".into(),
        table,
        PathBuf::from("sales.csv"),
        Placement::None,
        None,
    );
    let data = DataRef::Table(app.project.tables[0].id);
    app.apply_formula_for_test(data, "Double", "[Sales] * 2");
    app.apply_action_for_test(Action::EditCell(data, 1, "Sales".into(), "2'500".into()));
    let t = &app.project.tables[0].table;
    assert_eq!(t.column("Sales").unwrap().f64(1), Some(2500.0));
    assert_eq!(
        t.column("Double").unwrap().f64(1),
        Some(5000.0),
        "recomputed"
    );
    app.apply_action_for_test(Action::EditCell(data, 0, "Sales".into(), "many".into()));
    assert_eq!(
        app.project.tables[0].table.column("Sales").unwrap().f64(0),
        Some(1.0),
        "refused"
    );
    assert!(
        app.toasts_for_test()
            .iter()
            .any(|t| t.contains("isn't a number"))
    );
    app.apply_action_for_test(Action::AddRow(data));
    app.apply_action_for_test(Action::EditCell(data, 2, "Code".into(), "03".into()));
    assert_eq!(app.project.tables[0].table.rows(), 3);
    app.apply_action_for_test(Action::DeleteRows(data, vec![0]));
    let t = &app.project.tables[0].table;
    assert_eq!(t.rows(), 2);
    assert_eq!(t.column("Code").unwrap().display(1), "03");
    // Each a step of its own.
    app.run_command_for_test(crate::commands::Command::Undo);
    assert_eq!(app.project.tables[0].table.rows(), 3);
}

/// Coordinates in one column ("lat, lon") become points.
#[test]
fn lat_lon_in_one_column() {
    let (mut h, _lock) = harness(false);
    h.state_mut().paste_table(
        "Branch\tLocation\nBern\t46.9480, 7.4474\nZurich\t47°22'36.8\"N 8°32'30.1\"E\n",
    );
    settle(&mut h);
    let (table, _) = h
        .state()
        .import_table_for_test()
        .expect("the import dialog");
    let table = table.clone();
    let app = h.state_mut();
    // The dialog suggested the column; add it that way.
    app.add_table(
        "Branches".into(),
        table,
        PathBuf::from("branches.txt"),
        crate::dialogs::Placement::LatLon("Location".into()),
        None,
    );
    let l = app.project.layers.last().expect("a points layer");
    assert_eq!(l.kind(), territoria_io::GeometryKind::Point);
    let p = l.geometry.points[1].0[0];
    assert!(
        (p.x() - 8.5417).abs() < 1e-3 && (p.y() - 47.3769).abs() < 1e-3,
        "{p:?}"
    );
}

/// A summary table: sales per segment.
#[test]
fn summarise_table() {
    use crate::app::DataRef;
    use crate::dialogs::{Placement, SummariseDialog};
    let (mut h, _lock) = harness(false);
    let table = Table::new(vec![
        Column::text(
            "Customer",
            ColumnKind::Key,
            ["1", "2", "3", "4"].map(|s| Some(s.to_owned())).to_vec(),
        ),
        Column::text(
            "Segment",
            ColumnKind::Text,
            ["Retail", "Retail", "Wholesale", "Retail"]
                .map(|s| Some(s.to_owned()))
                .to_vec(),
        ),
        Column::number("Sales", vec![Some(1.0), Some(2.0), Some(30.0), Some(4.0)]),
    ])
    .unwrap();
    let app = h.state_mut();
    app.add_table(
        "Customers".into(),
        table,
        PathBuf::from("c.csv"),
        Placement::None,
        None,
    );
    let data = DataRef::Table(app.project.tables[0].id);
    let mut d = SummariseDialog::new(&app.project, data);
    d.by = vec!["Segment".into()];
    d.filter = "[Sales] > 1".into();
    d.name = "Sales per segment".into();
    app.create_summary(&d);
    let t = &app.project.tables.last().unwrap();
    assert_eq!(t.name, "Sales per segment");
    let s = &t.table;
    assert_eq!(s.rows(), 2);
    assert_eq!(
        s.column("Sales").unwrap().f64(0),
        Some(6.0),
        "Retail without the 1"
    );
    assert_eq!(s.column("Rows").unwrap().f64(1), Some(1.0));
    assert!(
        h.state()
            .open_tabs_for_test()
            .contains(&crate::app::Tab::Table(DataRef::Table(
                h.state().project.tables[1].id
            )))
    );
}

/// A score of two figures, weighted 3:1, as shares of their totals.
#[test]
fn score_column() {
    use crate::app::DataRef;
    use crate::dialogs::{Placement, ScoreDialog, ScoreScale};
    let (mut h, _lock) = harness(false);
    let table = Table::new(vec![
        Column::text(
            "Area",
            ColumnKind::Key,
            vec![Some("A".into()), Some("B".into())],
        ),
        Column::number("Revenue", vec![Some(75.0), Some(25.0)]),
        Column::number("Customers", vec![Some(1.0), Some(3.0)]),
    ])
    .unwrap();
    let app = h.state_mut();
    app.add_table(
        "T".into(),
        table,
        PathBuf::from("t.csv"),
        Placement::None,
        None,
    );
    let data = DataRef::Table(app.project.tables[0].id);
    let mut d = ScoreDialog::new(&app.project, data);
    d.weights = vec![("Revenue".into(), 3.0), ("Customers".into(), 1.0)];
    d.scale = ScoreScale::Share;
    let f = d.formula();
    assert_eq!(f, "share([Revenue]) * 0.75 + share([Customers]) * 0.25");
    app.apply_formula_for_test(data, "Score", &f);
    let s = app.project.tables[0].table.column("Score").unwrap();
    // A: 0.75·0.75 + 0.25·0.25 = 0.625
    assert!((s.f64(0).unwrap() - 0.625).abs() < 1e-12);
}

/// A heat map of customers weighted by sales, on screen and in exports.
#[test]
fn heat_map() {
    use crate::app::Export;
    use crate::dialogs::Placement;
    use crate::export_map::Format;
    use territoria_model::{MapLayer, MapLayerKind};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let customers = customers(h.state(), layer, 1200);
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
    let b = app.project.layer(layer).unwrap().geometry.bbox.unwrap();
    let span = territoria_geo::haversine(b.min(), b.max()) / 1000.0;
    let map = app.project.maps[0].id;
    let m = app.project.map_mut(map).unwrap();
    // Hide the points themselves: the density shows them.
    for ml in &mut m.layers {
        if ml.kind.layer() == Some(points) {
            ml.visible = false;
        }
    }
    m.layers.push(MapLayer {
        id: territoria_model::MapLayerId(4001),
        name: "Density of Sales".into(),
        visible: true,
        opacity: 0.85,
        zoom: Default::default(),
        kind: MapLayerKind::Heatmap {
            layer: points,
            weight: Some("Sales".into()),
            radius_km: span / 30.0,
            ramp: crate::heat::default_ramp(),
        },
    });
    settle(&mut h);
    save(&mut h, "heatmap");
    let dir = std::env::temp_dir().join(format!("territoria-heat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("heat.svg");
    let app = h.state_mut();
    app.set_map_export_for_test(crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: true,
        scale_bar: false,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
    });
    app.export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(
        svg.contains("data:image/png;base64,"),
        "the density as an image"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Pie and bar charts of three values per region, on screen and in exports.
#[test]
fn charts_per_area() {
    use crate::app::Export;
    use crate::export_map::Format;
    use territoria_model::{ChartShape, MapLayerKind, ValueSource};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let key = app
        .project
        .layer(layer)
        .unwrap()
        .key_column
        .clone()
        .unwrap();
    app.merge_areas_for_test(layer, &key, Some(2), "Regions");
    let regions = app.project.layers.last().unwrap().id;
    let r = app.project.layer_mut(regions).unwrap();
    let n = r.len();
    let wave = |f: usize, k: f64| ((f as f64 * k).sin() * 0.5 + 0.6) * 100.0;
    for (name, k) in [("Software", 1.3), ("Hardware", 2.9), ("Services", 0.7)] {
        std::sync::Arc::make_mut(&mut r.attributes)
            .columns
            .push(Column::number(
                name,
                (0..n)
                    .map(|f| Some((wave(f, k) * (1.0 + f as f64 % 3.0)).round()))
                    .collect(),
            ));
    }
    let values: Vec<(String, ValueSource)> = ["Software", "Hardware", "Services"]
        .iter()
        .map(|c| {
            (
                c.to_string(),
                ValueSource::Attribute {
                    column: c.to_string(),
                },
            )
        })
        .collect();
    app.add_charts(regions, ChartShape::Pie, values);
    settle(&mut h);
    save(&mut h, "charts-pie");
    let app = h.state_mut();
    let map = app.project.maps[0].id;
    let ml = app.project.maps[0].layers.last().unwrap();
    assert!(matches!(ml.kind, MapLayerKind::Charts { .. }));
    assert_eq!(ml.name, "Software and 2 more");
    let id = ml.id;
    if let Some(MapLayerKind::Charts { chart, .. }) = app
        .project
        .map_mut(map)
        .and_then(|m| m.layer_mut(id))
        .map(|l| &mut l.kind)
    {
        chart.shape = ChartShape::Bars;
    }
    settle(&mut h);
    save(&mut h, "charts-bars");
    let app = h.state_mut();
    if let Some(MapLayerKind::Charts { chart, .. }) = app
        .project
        .map_mut(map)
        .and_then(|m| m.layer_mut(id))
        .map(|l| &mut l.kind)
    {
        chart.shape = ChartShape::Stacked;
    }
    settle(&mut h);
    save(&mut h, "charts-stacked");

    let dir = std::env::temp_dir().join(format!("territoria-charts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("charts.svg");
    let app = h.state_mut();
    app.set_map_export_for_test(crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: true,
        scale_bar: false,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
    });
    app.export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(
        svg.matches("<rect").count() > 3 * (n / 2),
        "bars in the export"
    );
    assert!(svg.contains("Hardware"), "legend names the values");
    let _ = std::fs::remove_dir_all(dir);
}

/// Areas coloured by two values at once, with the grid legend on screen
/// and in exports.
#[test]
fn two_values_at_once() {
    use crate::app::Export;
    use crate::export_map::Format;
    use territoria_model::ValueSource;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, None);
    let app = h.state_mut();
    let link = app.project.links[0].id;
    app.add_bivariate(
        layer,
        ValueSource::Linked {
            link,
            column: "Customers".into(),
            agg: territoria_data::Aggregate::Sum,
        },
        ValueSource::Area,
        "Customers and area".into(),
    );
    settle(&mut h);
    save(&mut h, "bivariate");
    let app = h.state_mut();
    let ml = app.project.maps[0].layers.last().unwrap().clone();
    let mut values = territoria_model::Cache::default();
    let scaled = app
        .analyses
        .get(&app.project, &mut values, ml.id, &ml.kind)
        .unwrap();
    let grid = scaled.grid.as_ref().unwrap();
    assert_eq!(grid.size, 3);
    let n = app.project.layer(layer).unwrap().len();
    assert_eq!(grid.counts.iter().sum::<usize>(), n, "every area in a cell");

    let dir = std::env::temp_dir().join(format!("territoria-bivariate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bivariate.svg");
    app.set_map_export_for_test(crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: true,
        scale_bar: false,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
    });
    app.export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(svg.contains("↑ Area"), "grid legend in the export");
    let _ = std::fs::remove_dir_all(dir);
}

/// A layer limited to closer zoom levels shows only when zoomed in, on
/// screen and in exports.
#[test]
fn zoom_dependent_layer() {
    use crate::app::{Action, Export};
    use crate::export_map::Format;
    use territoria_model::{LabelMode, MapLayerKind};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    settle(&mut h);
    let app = h.state_mut();
    let (center, scale) = app.view_for_test().expect("a view");
    let map = app.project.maps[0].id;
    // Key labels only closer than a quarter of the full view's scale.
    let m = app.project.map_mut(map).unwrap();
    let ml = m
        .layers
        .iter_mut()
        .find(|l| l.kind.layer() == Some(layer))
        .unwrap();
    if let MapLayerKind::Boundaries { style, .. } = &mut ml.kind {
        style.labels = LabelMode::Key;
    }
    ml.zoom.farthest = Some(scale / 4.0);
    let key = app.project.layer(layer).unwrap().key(0);
    let dir = std::env::temp_dir().join(format!("territoria-zoom-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("zoom.svg");
    let settings = || crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: true,
        scale_bar: false,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
    };
    app.set_map_export_for_test(settings());
    settle(&mut h);
    save(&mut h, "zoom-out");
    h.state_mut().export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let far = std::fs::read_to_string(&path).unwrap();
    assert!(!far.contains("<path"), "no areas when zoomed out");
    h.state_mut()
        .apply_action_for_test(Action::GoToView(map, center, scale / 8.0));
    settle(&mut h);
    save(&mut h, "zoom-in");
    let path = dir.join("zoom-in.svg");
    h.state_mut().set_map_export_for_test(settings());
    h.state_mut().export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let near = std::fs::read_to_string(&path).unwrap();
    assert!(near.contains("<path"), "areas when zoomed in");
    assert!(near.contains("<text") || key.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// Selected regions carried to the areas and customers inside them.
#[test]
fn select_in_another_layer() {
    use crate::app::Action;
    use crate::dialogs::Placement;
    use crate::map_view::MapEvent;
    use territoria_model::transfer::Relation;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let customers = customers(h.state(), layer, 400);
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
    let key = app
        .project
        .layer(layer)
        .unwrap()
        .key_column
        .clone()
        .unwrap();
    app.merge_areas_for_test(layer, &key, Some(2), "Regions");
    let regions = app.project.layers.last().unwrap().id;
    settle(&mut h);
    let app = h.state_mut();
    // The two regions with the most areas.
    let r = app.project.layer(regions).unwrap();
    let areas = r.attributes.column("Areas").unwrap();
    let mut order: Vec<u32> = (0..r.len() as u32).collect();
    order.sort_by(|a, b| {
        areas
            .f64(*b as usize)
            .unwrap_or(0.0)
            .total_cmp(&areas.f64(*a as usize).unwrap_or(0.0))
    });
    let chosen = order[..2].to_vec();
    let expected: f64 = chosen.iter().filter_map(|u| areas.f64(*u as usize)).sum();
    let transfer = |h: &mut Harness<'_, TerritoriaApp>, to, relation| -> usize {
        h.state_mut()
            .apply_action_for_test(Action::SelectUnits(regions, chosen.clone()));
        h.state_mut()
            .map_events_for_test(vec![MapEvent::TransferSelection { to, relation }]);
        let (l, units) = h.state().selected.clone().expect("a selection");
        assert_eq!(l, to);
        units.len()
    };
    let contained = transfer(&mut h, layer, Relation::Contained);
    let centre = transfer(&mut h, layer, Relation::CentreInside);
    let touching = transfer(&mut h, layer, Relation::Touching);
    assert!(
        (contained as f64 - expected).abs() <= expected * 0.02,
        "{contained} areas wholly inside, {expected} merged"
    );
    assert!(
        (centre as f64 - expected).abs() <= expected * 0.02,
        "{centre}"
    );
    assert!(touching > centre, "neighbours touch too: {touching}");
    let inside = transfer(&mut h, points, Relation::CentreInside);
    assert!(inside > 0 && inside < 400, "{inside} customers");
    settle(&mut h);
    save(&mut h, "select-in");
}

/// Customers' lines to their branch, guessed from a column naming the
/// branch, on screen and in exports.
#[test]
fn flow_lines() {
    use crate::app::Export;
    use crate::dialogs::{FlowDialog, Placement};
    use crate::export_map::Format;
    use territoria_model::flows::{FlowEnd, FlowRows};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let b = app.project.layer(layer).unwrap().geometry.bbox.unwrap();
    let spots = [(0.3, 0.35), (0.65, 0.6), (0.8, 0.25), (0.45, 0.8)];
    let at = |(u, v): (f64, f64)| (b.min().x + u * b.width(), b.min().y + v * b.height());
    let branches = Table::new(vec![
        Column::text(
            "Branch",
            ColumnKind::Key,
            (0..4).map(|i| Some(format!("Branch {}", i + 1))).collect(),
        ),
        Column::number("Lon", spots.iter().map(|s| Some(at(*s).0)).collect()),
        Column::number("Lat", spots.iter().map(|s| Some(at(*s).1)).collect()),
    ])
    .unwrap();
    app.add_table(
        "Branches".into(),
        branches,
        PathBuf::from("branches.csv"),
        Placement::Points {
            x: "Lon".into(),
            y: "Lat".into(),
        },
        None,
    );
    let branches = app.project.layers.last().unwrap().id;
    // Each customer served by the nearest branch.
    let mut customers = customers(app, layer, 300);
    let nearest: Vec<Option<String>> = (0..customers.rows())
        .map(|r| {
            let (x, y) = (
                customers.column("Longitude").unwrap().f64(r).unwrap(),
                customers.column("Latitude").unwrap().f64(r).unwrap(),
            );
            let i = (0..4)
                .min_by(|a, b| {
                    let d = |i: usize| {
                        let (bx, by) = at(spots[i]);
                        (bx - x).hypot(by - y)
                    };
                    d(*a).total_cmp(&d(*b))
                })
                .unwrap();
            Some(format!("Branch {}", i + 1))
        })
        .collect();
    customers
        .columns
        .push(Column::text("Served by", ColumnKind::Text, nearest));
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
    let mut d = {
        // Choosing the rows guesses the ends.
        let mut fresh = FlowDialog::new(&app.project);
        fresh.rows = Some(FlowRows::Layer(points));
        fresh.guess_for_test(&app.project);
        fresh
    };
    assert_eq!(d.from, FlowEnd::Own);
    assert_eq!(
        d.to,
        FlowEnd::Column {
            column: "Served by".into(),
            layer: branches
        }
    );
    d.value = Some("Sales".into());
    app.add_flows(&d);
    settle(&mut h);
    save(&mut h, "flows");
    let app = h.state_mut();
    let ml = app.project.maps[0].layers.last().unwrap().clone();
    let lines = app.analyses.flows(&app.project, ml.id, &ml.kind).unwrap();
    assert_eq!(lines.len(), 300, "a line per customer");

    let dir = std::env::temp_dir().join(format!("territoria-flows-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("flows.svg");
    app.set_map_export_for_test(crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: true,
        scale_bar: false,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
    });
    app.export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(svg.contains("Line widths follow Sales"), "legend");
    assert!(
        svg.matches("stroke-linecap=\"round\"").count() >= 250,
        "lines"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Circles sized by sales and coloured by a customer segment.
#[test]
fn circles_by_category() {
    use crate::app::Export;
    use crate::dialogs::Placement;
    use crate::export_map::Format;
    use territoria_model::{MapLayer, MapLayerKind, TextSource, ValueSource};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let mut customers = customers(app, layer, 300);
    let segments = ["Retail", "Wholesale", "Online"];
    customers.columns.push(Column::text(
        "Segment",
        ColumnKind::Text,
        (0..300).map(|i| Some(segments[i % 3].to_owned())).collect(),
    ));
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
    let map = app.project.maps[0].id;
    let m = app.project.map_mut(map).unwrap();
    for ml in &mut m.layers {
        if ml.kind.layer() == Some(points) {
            ml.visible = false;
        }
    }
    m.layers.push(MapLayer {
        id: territoria_model::MapLayerId(5001),
        name: "Sales by segment".into(),
        visible: true,
        opacity: 1.0,
        zoom: Default::default(),
        kind: MapLayerKind::Symbols {
            layer: points,
            source: ValueSource::Attribute {
                column: "Sales".into(),
            },
            normalize: Default::default(),
            symbol: territoria_model::SymbolStyle {
                color_by: Some((
                    TextSource::Attribute {
                        column: "Segment".into(),
                    },
                    Default::default(),
                )),
                ..territoria_model::SymbolStyle::for_count(300)
            },
            style: Default::default(),
        },
    });
    settle(&mut h);
    save(&mut h, "circles-by-category");
    let dir = std::env::temp_dir().join(format!("territoria-segments-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("segments.svg");
    let app = h.state_mut();
    app.set_map_export_for_test(crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: true,
        scale_bar: false,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
    });
    app.export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(svg.contains("Wholesale"), "categories in the legend");
    assert!(svg.matches("<circle cx").count() > 100, "circles");
    let fills: std::collections::HashSet<&str> = svg
        .split("<circle ")
        .skip(1)
        .filter_map(|c| c.split("fill=\"").nth(1)?.split('"').next())
        .collect();
    assert!(fills.len() >= 3, "{fills:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// F1 opens help on the topic of what is selected.
#[test]
fn help_on_the_selection() {
    use crate::commands::Command;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, None);
    settle(&mut h);
    let app = h.state_mut();
    let map = app.project.maps[0].id;
    let id = app.project.maps[0].layers.last().unwrap().id;
    app.selection = crate::app::Selection::MapLayer(map, id);
    app.run_command_for_test(Command::Help);
    assert_eq!(h.state().help_topic_for_test(), Some("boundaries"));
    // A coloured layer: the colouring topic.
    let app = h.state_mut();
    let link = app.project.links[0].id;
    let id = app
        .add_choropleth_for_test(
            map,
            layer,
            territoria_model::ValueSource::Linked {
                link,
                column: "Revenue".into(),
                agg: territoria_data::Aggregate::Sum,
            },
        )
        .unwrap();
    app.selection = crate::app::Selection::MapLayer(map, id);
    app.run_command_for_test(Command::Help);
    assert_eq!(h.state().help_topic_for_test(), Some("colour"));
    settle(&mut h);
    save(&mut h, "help");
}

/// A shortcut changed in Settings: clicked, then the new keys pressed.
#[test]
fn change_a_shortcut() {
    use egui::{Key, Modifiers};
    use egui_kittest::kittest::Queryable;
    let (mut h, _lock) = harness(false);
    h.state_mut().open_settings_for_test();
    h.run_steps(3);
    h.get_by_label("Keyboard shortcuts").click();
    h.state_mut().find_shortcut_for_test("find an");
    h.run_steps(3);
    h.get_by_label("Ctrl+F").click();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    save(&mut h, "settings-shortcuts");
    let app = h.state();
    assert_eq!(
        app.settings.shortcuts.get("Find").map(String::as_str),
        Some("Ctrl+K")
    );
    let k = egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::K);
    assert_eq!(
        app.keymap_for_test().command_of(&k),
        Some(crate::commands::Command::Find)
    );
}

/// A filter kept as a segment: a yes/no column used by an analysis and
/// by the table export.
#[test]
fn segments() {
    use crate::app::{Action, DataRef, Export};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, None);
    let app = h.state_mut();
    let t = app.project.tables[0].id;
    app.apply_action_for_test(Action::SaveSegment {
        data: DataRef::Table(t),
        name: "Big".into(),
        formula: "[Revenue] > 20000".into(),
    });
    let table = &app.project.table(t).unwrap().table;
    let big = table.column("Big").expect("the segment column");
    assert_eq!(big.kind, territoria_data::ColumnKind::Bool);
    let n = (0..table.rows())
        .filter(|&r| matches!(big.get(r), territoria_data::table::Value::Bool(true)))
        .count();
    assert!(n > 0 && n < table.rows(), "{n}");
    // An analysis of the segment's rows only.
    let link = app.project.links[0].id;
    let source = territoria_model::ValueSource::filtered(
        territoria_model::ValueSource::RowCount { link },
        "[Big]",
    );
    let mut values = territoria_model::Cache::default();
    let counted: f64 = values
        .values(&app.project, layer, &source)
        .iter()
        .flatten()
        .sum();
    assert_eq!(counted as usize, n);
    // The table export of the segment.
    let dir = std::env::temp_dir().join(format!("territoria-segment-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("big.csv");
    app.export(
        Export::Data(DataRef::Table(t), false, Some("Big".into())),
        csv.clone(),
    );
    app.open_table_for_test(DataRef::Table(t));
    settle(&mut h);
    let text = std::fs::read_to_string(&csv).unwrap();
    assert_eq!(text.lines().count(), n + 1, "header and the segment's rows");
    save(&mut h, "segments");
    let _ = std::fs::remove_dir_all(dir);
}

/// Each area's travel time by road to the nearest branch, from a routing
/// server (a stand-in here).
#[test]
fn travel_time_by_road() {
    use crate::app::DataRef;
    use crate::dialogs::{JoinDialog, JoinMode, Placement};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    app.settings.routing.url = territoria_io::routing::mock_server();
    app.settings.routing.interval_ms = 0;
    let b = app.project.layer(layer).unwrap().geometry.bbox.unwrap();
    let at = |u: f64, v: f64| (b.min().x + u * b.width(), b.min().y + v * b.height());
    let spots = [at(0.25, 0.3), at(0.75, 0.7)];
    let branches = Table::new(vec![
        Column::text(
            "Branch",
            ColumnKind::Key,
            vec![Some("West".into()), Some("East".into())],
        ),
        Column::number("Lon", spots.iter().map(|s| Some(s.0)).collect()),
        Column::number("Lat", spots.iter().map(|s| Some(s.1)).collect()),
    ])
    .unwrap();
    app.add_table(
        "Branches".into(),
        branches,
        PathBuf::from("branches.csv"),
        Placement::Points {
            x: "Lon".into(),
            y: "Lat".into(),
        },
        None,
    );
    let points = app.project.layers.last().unwrap().id;
    let mut d = JoinDialog::new(&app.project, DataRef::Layer(layer));
    d.mode = JoinMode::ByRoad;
    d.layer = Some(points);
    app.apply_join(&d);
    settle(&mut h);
    let l = h.state().project.layer(layer).unwrap();
    let t = &l.attributes;
    let minutes = t.column("Minutes to Branches").expect("travel times");
    let nearest = t.column("Nearest Branches by road").unwrap();
    let n = l.len();
    assert!(
        (0..n).all(|f| minutes.f64(f).is_some()),
        "a time for every area"
    );
    // The area at the west branch goes there in no time.
    let g = &l.geometry;
    let west = g
        .feature_at(
            geo_types::Coord {
                x: spots[0].0,
                y: spots[0].1,
            },
            0.0,
        )
        .unwrap();
    assert_eq!(nearest.display(west), "West");
    assert!(minutes.f64(west).unwrap() < 1.0);
    assert!(t.column("Road km to Branches").is_some());
}

/// The quickest round through selected customers: a line on the map and
/// each customer's place in it.
#[test]
fn round_through_customers() {
    use crate::app::Action;
    use crate::dialogs::Placement;
    use crate::map_view::MapEvent;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    app.settings.routing.url = territoria_io::routing::mock_server();
    let customers = customers(app, layer, 60);
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
    let units: Vec<u32> = (0..12).map(|i| i * 5).collect();
    app.apply_action_for_test(Action::SelectUnits(points, units.clone()));
    settle(&mut h);
    h.state_mut().map_events_for_test(vec![MapEvent::Round]);
    settle(&mut h);
    save(&mut h, "round");
    let app = h.state();
    let round = app.project.layers.last().unwrap();
    assert_eq!(round.name, "Round of Customers");
    assert_eq!(round.kind(), territoria_io::GeometryKind::Line);
    let order = app
        .project
        .layer(points)
        .unwrap()
        .attributes
        .column("Visit order")
        .expect("visit order");
    let mut ranks: Vec<u32> = units
        .iter()
        .map(|&u| order.f64(u as usize).unwrap() as u32)
        .collect();
    assert_eq!(ranks[0], 1, "starts at the first");
    ranks.sort_unstable();
    assert_eq!(ranks, (1..=12).collect::<Vec<u32>>());
    assert!(order.f64(1).is_none(), "others not in the round");
}

/// The company's name and logo on exported maps.
#[test]
fn company_logo_on_exports() {
    use crate::app::Export;
    use crate::export_map::Format;
    let (mut h, _lock) = harness(false);
    h.state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries");
    let dir = std::env::temp_dir().join(format!("territoria-logo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A red logo, 200 × 50 pixels.
    let logo = dir.join("logo.png");
    image::RgbaImage::from_pixel(200, 50, image::Rgba([220, 20, 40, 255]))
        .save(&logo)
        .unwrap();
    let app = h.state_mut();
    app.settings.company = "Acme Sales AG".into();
    app.settings.logo = Some(logo);
    settle(&mut h);
    let settings = || crate::dialogs::MapExportSettings {
        format: Format::Png,
        scale: 1.0,
        title: String::new(),
        legend: false,
        scale_bar: false,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
    };
    let svg_path = dir.join("map.svg");
    h.state_mut().set_map_export_for_test(settings());
    h.state_mut()
        .export(Export::Map(Format::Svg), svg_path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&svg_path).unwrap();
    assert!(svg.contains("Acme Sales AG"), "the company in the credits");
    assert!(svg.contains("<image") && svg.contains("data:image/png;base64,"));
    // Drawn into PNGs too: red bottom left.
    let png_path = dir.join("map.png");
    h.state_mut().set_map_export_for_test(settings());
    h.state_mut()
        .export(Export::Map(Format::Png), png_path.clone());
    settle(&mut h);
    let img = image::open(&png_path).unwrap().to_rgba8();
    let found = (0..img.height())
        .rev()
        .take(80)
        .any(|y| (10..60).any(|x| img.get_pixel(x, y).0[..3] == [220, 20, 40]));
    assert!(found, "the logo in the PNG");
    let _ = std::fs::remove_dir_all(dir);
}

/// New plans take the territory colours chosen in Settings; a plan's
/// colours can change and Recolour keeps to them.
#[test]
fn territory_palettes() {
    use crate::commands::Command;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    app.settings.territory_colors = Some("Tableau".into());
    let plan = app.add_balanced_plan_now(layer, 6);
    settle(&mut h);
    save(&mut h, "palette-tableau");
    let tableau = crate::territories::palettes()
        .into_iter()
        .find(|(n, _)| *n == "Tableau")
        .unwrap()
        .1;
    let app = h.state_mut();
    let p = app.project.plan(plan).unwrap();
    assert_eq!(p.colors(), tableau.as_slice());
    assert!(p.territories.iter().all(|t| tableau.contains(&t.color)));
    app.run_command_for_test(Command::Recolor);
    let p = app.project.plan(plan).unwrap();
    assert!(
        p.territories.iter().all(|t| tableau.contains(&t.color)),
        "recoloured from the palette"
    );
    // Back to the pastels.
    let id = plan;
    app.apply_action_for_test(crate::app::Action::Edit(
        "Territory colours".into(),
        Box::new(move |p| {
            if let Some(pl) = p.plan_mut(id) {
                pl.set_palette(Vec::new());
            }
        }),
    ));
    let p = app.project.plan(plan).unwrap();
    assert!(
        p.territories
            .iter()
            .all(|t| territoria_model::plan::TERRITORY_COLORS.contains(&t.color))
    );
}

/// A table column dragged onto the Contents panel goes on the map, and
/// the Properties panel switches how it is shown.
#[test]
fn drag_column_onto_the_map() {
    use crate::app::DataRef;
    use egui_kittest::kittest::Queryable;
    use territoria_model::MapLayerKind;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    h.state_mut()
        .add_linked_table_now("Sales 2026", table, "Area", layer, None);
    let t = h.state().project.tables[0].id;
    h.state_mut().open_table_for_test(DataRef::Table(t));
    settle(&mut h);
    let start = h.get_by_label("Revenue").rect().center();
    let before = h.state().project.maps[0].layers.len();
    h.hover_at(start);
    h.run_steps(1);
    h.drag_at(start);
    h.run_steps(1);
    for k in 1..=5 {
        h.hover_at(start + egui::vec2(0.0, 12.0 * k as f32));
        h.run_steps(1);
    }
    let zone = h
        .get_by_label("⤵ Drop here to show Revenue on the map")
        .rect()
        .center();
    h.hover_at(zone);
    h.run_steps(2);
    save(&mut h, "drag-column");
    h.drop_at(zone);
    h.run_steps(3);
    let app = h.state();
    let layers = &app.project.maps[0].layers;
    assert_eq!(layers.len(), before + 1, "a new map layer");
    let ml = layers.last().unwrap();
    assert!(matches!(ml.kind, MapLayerKind::Choropleth { .. }));
    assert_eq!(ml.name, "Revenue (Sales 2026)");
    // Shown as circles instead, from the Properties panel.
    h.get_by_label("Circles").click();
    settle(&mut h);
    save(&mut h, "dropped-column");
    let ml = h.state().project.maps[0].layers.last().unwrap().clone();
    assert!(
        matches!(ml.kind, MapLayerKind::Symbols { .. }),
        "{:?}",
        ml.kind.type_label()
    );
}

/// An atlas of the map: a PDF page per region, zoomed to it.
#[test]
fn map_atlas() {
    use crate::app::Export;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    let app = h.state_mut();
    app.add_linked_table_now("Sales 2026", table, "Area", layer, None);
    // Regions by the first two characters of the key.
    let l = app.project.layer_mut(layer).unwrap();
    let n = l.len();
    let keys: Vec<Option<String>> = (0..n)
        .map(|f| Some(l.key(f).chars().take(2).collect()))
        .collect();
    std::sync::Arc::make_mut(&mut l.attributes)
        .columns
        .push(Column::text("Region", ColumnKind::Text, keys));
    let link = app.project.links[0].id;
    let map = app.project.maps[0].id;
    app.add_choropleth_for_test(
        map,
        layer,
        territoria_model::ValueSource::Linked {
            link,
            column: "Revenue".into(),
            agg: territoria_data::Aggregate::Sum,
        },
    );
    settle(&mut h);
    let app = h.state_mut();
    let d = crate::dialogs::AtlasDialog::new(&app.project, app.project.map(map).unwrap());
    assert_eq!(d.layer, Some(layer));
    assert_eq!(d.column.as_deref(), Some("Region"));
    let groups = app.atlas_groups(layer, &["Region"]);
    assert!(groups.len() >= 2);
    let dir = std::env::temp_dir().join(format!("territoria-atlas-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("atlas.pdf");
    app.export(
        Export::Atlas(crate::app::AtlasSpec::new(layer, Some("Region"))),
        path.clone(),
    );
    settle(&mut h);
    let pdf = std::fs::read(&path).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let text = String::from_utf8_lossy(&pdf);
    let pages = text.matches("/Type /Page").count() - text.matches("/Type /Pages").count();
    assert_eq!(pages, groups.len(), "a page per region");
    if let Some(out) = out_dir() {
        let svg = h.state_mut().atlas_page_for_test(layer, "Region", 1);
        let font = egui::FontDefinitions::default().font_data["Ubuntu-Light"]
            .font
            .to_vec();
        let png =
            crate::export_map::encode(&svg, crate::export_map::Format::Png, 1.5, &font).unwrap();
        std::fs::write(out.join("atlas-page.png"), png).unwrap();
    }
    let _ = std::fs::remove_dir_all(dir);
    // The dialog, with a second column.
    h.state_mut()
        .run_command_for_test(crate::commands::Command::ExportAtlas);
    h.run_steps(3);
    save(&mut h, "atlas-dialog");
}

/// Classes kept by name and used again on another map layer.
#[test]
fn class_sets() {
    use crate::app::{Action, Selection};
    use egui_kittest::kittest::Queryable;
    use territoria_model::MapLayerKind;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    let app = h.state_mut();
    app.add_linked_table_now("Sales 2026", table, "Area", layer, None);
    let link = app.project.links[0].id;
    let map = app.project.maps[0].id;
    let kept = territoria_analysis::ColorScale {
        method: territoria_analysis::Method::Manual,
        classes: 3,
        manual: vec![5000.0, 20000.0],
        labels: vec!["Low".into(), "Middle".into(), "High".into()],
        ..Default::default()
    };
    app.apply_action_for_test(Action::SaveClassSet("Revenue bands".into(), kept.clone()));
    assert_eq!(app.settings.class_sets.len(), 1);
    let id = app
        .add_choropleth_for_test(
            map,
            layer,
            territoria_model::ValueSource::Linked {
                link,
                column: "Revenue".into(),
                agg: territoria_data::Aggregate::Sum,
            },
        )
        .unwrap();
    app.selection = Selection::MapLayer(map, id);
    settle(&mut h);
    h.get_by_label("Class sets").click();
    h.run_steps(2);
    h.get_by_label("Revenue bands").click();
    settle(&mut h);
    save(&mut h, "class-set");
    let ml = h
        .state()
        .project
        .map(map)
        .unwrap()
        .layer(id)
        .unwrap()
        .clone();
    let MapLayerKind::Choropleth { scale, .. } = &ml.kind else {
        panic!("a choropleth");
    };
    assert_eq!(scale, &kept);
}

/// Labels sized by a number, on screen and in exports.
#[test]
fn labels_sized_by_value() {
    use crate::app::Export;
    use crate::export_map::Format;
    use territoria_model::{LabelMode, MapLayerKind};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    let app = h.state_mut();
    app.add_linked_table_now("Sales 2026", table, "Area", layer, None);
    let link = app.project.links[0].id;
    let map = app.project.maps[0].id;
    let m = app.project.map_mut(map).unwrap();
    for ml in &mut m.layers {
        if let MapLayerKind::Boundaries { style, .. } = &mut ml.kind {
            style.labels = LabelMode::Key;
            style.label_size = Some(territoria_model::map::LabelSize {
                source: territoria_model::ValueSource::Linked {
                    link,
                    column: "Revenue".into(),
                    agg: territoria_data::Aggregate::Sum,
                },
                min: 7.0,
                max: 20.0,
            });
        }
    }
    settle(&mut h);
    // Close enough for the areas to take labels.
    let (center, scale) = h.state().view_for_test().unwrap();
    h.state_mut()
        .apply_action_for_test(crate::app::Action::GoToView(map, center, scale / 2.5));
    settle(&mut h);
    save(&mut h, "labels-sized");
    let dir = std::env::temp_dir().join(format!("territoria-labelsize-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("labels.svg");
    let app = h.state_mut();
    app.set_map_export_for_test(crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: false,
        scale_bar: false,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
    });
    app.export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&path).unwrap();
    let sizes: std::collections::BTreeSet<String> = svg
        .split("<text font-size=\"")
        .skip(1)
        .filter_map(|t| t.split('"').next().map(str::to_owned))
        .collect();
    assert!(sizes.len() > 3, "labels of several sizes: {sizes:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The map on a printed page: frame, legend beside it, north arrow, footer.
#[test]
fn printed_page_layout() {
    use crate::app::Export;
    use crate::export_map::{Format, PageSize};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    let app = h.state_mut();
    app.add_linked_table_now("Sales 2026", table, "Area", layer, None);
    app.project.title = "Sales 2026".into();
    let link = app.project.links[0].id;
    let map = app.project.maps[0].id;
    app.add_choropleth_for_test(
        map,
        layer,
        territoria_model::ValueSource::Linked {
            link,
            column: "Revenue".into(),
            agg: territoria_data::Aggregate::Sum,
        },
    );
    settle(&mut h);
    // Zoomed in: the overview shows where the page is.
    let (center, scale) = h.state().view_for_test().unwrap();
    h.state_mut()
        .apply_action_for_test(crate::app::Action::GoToView(map, center, scale / 2.0));
    settle(&mut h);
    let dir = std::env::temp_dir().join(format!("territoria-page-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (page, name) in [
        (PageSize::A4Landscape, "page-a4-landscape"),
        (PageSize::A4Portrait, "page-a4-portrait"),
    ] {
        let settings = || crate::dialogs::MapExportSettings {
            format: Format::Png,
            scale: 1.5,
            title: "Revenue by area".into(),
            legend: true,
            scale_bar: true,
            page,
            footer: "{project} · {map} · {date}".into(),
            overview: true,
            figures: false,
        };
        let svg_path = dir.join(format!("{name}.svg"));
        h.state_mut().set_map_export_for_test(settings());
        h.state_mut()
            .export(Export::Map(Format::Svg), svg_path.clone());
        settle(&mut h);
        let svg = std::fs::read_to_string(&svg_path).unwrap();
        let size = page.size().unwrap();
        assert!(svg.starts_with(&format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}""#,
            size.x, size.y
        )));
        assert!(svg.contains(">N</text>"), "north arrow");
        assert!(
            svg.contains(r##"stroke="#d62728""##),
            "the page's part on the overview"
        );
        assert!(svg.contains("Sales 2026 · Map · 20"), "footer filled in");
        let png_path = dir.join(format!("{name}.png"));
        h.state_mut().set_map_export_for_test(settings());
        h.state_mut()
            .export(Export::Map(Format::Png), png_path.clone());
        settle(&mut h);
        let img = image::open(&png_path).unwrap();
        assert_eq!(img.width(), (size.x * 1.5).round() as u32);
        if let Some(out) = out_dir() {
            std::fs::copy(&png_path, out.join(format!("{name}.png"))).unwrap();
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// The guide's customer list reads cleanly and its address columns are
/// recognised, as the tutorial says.
#[test]
fn tutorial_customers() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/tutorial/customers.csv");
    let (table, _) = territoria_io::read_table(&path, &Default::default()).unwrap();
    assert_eq!(table.rows(), 30);
    assert_eq!(table.column("City").unwrap().display(0), "Zürich");
    assert_eq!(table.column("Sales").unwrap().f64(0), Some(48200.0));
    let a = crate::dialogs::AddressColumns::suggest(&table);
    assert_eq!(a.postcode.as_deref(), Some("Postcode"));
    assert_eq!(a.city.as_deref(), Some("City"));
    assert_eq!(a.country.as_deref(), Some("Country"));
}

/// A territory dragged from one region onto another moves there.
#[test]
fn drag_territory_to_region() {
    use egui_kittest::kittest::Queryable;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 4);
    {
        let p = app.project.plan_mut(plan).unwrap();
        let a = p.add_region(None);
        let b = p.add_region(None);
        for (i, t) in p.territories.iter_mut().enumerate() {
            t.region = Some(if i < 2 { a } else { b });
        }
        p.rev += 1;
    }
    app.project.bump();
    settle(&mut h);
    let (first, target, second_region) = {
        let p = h.state().project.plan(plan).unwrap();
        (
            p.territories[0].name.clone(),
            p.territories[3].name.clone(),
            p.regions[1].id,
        )
    };
    // The chips: the territory names in the regions section (the last
    // labels with these names; the list above has them too).
    let from = h.get_all_by_label(&first).last().unwrap().rect().center();
    let to = h.get_all_by_label(&target).last().unwrap().rect().center();
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for k in 1..=6 {
        h.hover_at(from + (to - from) * (k as f32 / 6.0));
        h.run_steps(1);
    }
    save(&mut h, "drag-territory");
    h.drop_at(to);
    h.run_steps(3);
    let p = h.state().project.plan(plan).unwrap();
    assert_eq!(p.territories[0].region, Some(second_region));
}

/// The project kept as a template and a new project started from it.
#[test]
fn project_templates() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = sales_table(h.state(), layer);
    let app = h.state_mut();
    app.add_linked_table_now("Sales 2026", table, "Area", layer, None);
    app.project.title = "Sales 2026".into();
    let dir = app.templates_dir();
    let _ = std::fs::remove_dir_all(&dir);
    app.save_template("Sales map");
    settle(&mut h);
    let app = h.state_mut();
    let templates = app.templates();
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].0, "Sales map");
    // A new, untitled project like the template.
    app.run_command_for_test(crate::commands::Command::Templates);
    settle(&mut h);
    save(&mut h, "templates");
    let app = h.state_mut();
    app.set_project(territoria_model::Project::new(), None);
    let (name, path) = templates[0].clone();
    app.new_from_template(name, path);
    settle(&mut h);
    let app = h.state();
    assert_eq!(app.project.layers.len(), 1);
    assert_eq!(app.project.tables.len(), 1);
    assert_eq!(app.project.title, "Untitled");
    assert!(app.path_for_test().is_none(), "not saved anywhere yet");
    let _ = std::fs::remove_dir_all(dir);
}

/// Diamonds sized logarithmically, on screen and in exports.
#[test]
fn symbol_shapes() {
    use crate::app::Export;
    use crate::dialogs::Placement;
    use crate::export_map::Format;
    use territoria_model::{MapLayer, MapLayerKind, SymbolScaling, SymbolShape, ValueSource};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let customers = customers(app, layer, 120);
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
    let map = app.project.maps[0].id;
    let m = app.project.map_mut(map).unwrap();
    for ml in &mut m.layers {
        if ml.kind.layer() == Some(points) {
            ml.visible = false;
        }
    }
    m.layers.push(MapLayer {
        id: territoria_model::MapLayerId(6001),
        name: "Sales".into(),
        visible: true,
        opacity: 1.0,
        zoom: Default::default(),
        kind: MapLayerKind::Symbols {
            layer: points,
            source: ValueSource::Attribute {
                column: "Sales".into(),
            },
            normalize: Default::default(),
            symbol: territoria_model::SymbolStyle {
                shape: SymbolShape::Diamond,
                scaling: SymbolScaling::Log,
                ..territoria_model::SymbolStyle::for_count(120)
            },
            style: Default::default(),
        },
    });
    settle(&mut h);
    save(&mut h, "symbol-shapes");
    let dir = std::env::temp_dir().join(format!("territoria-shapes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shapes.svg");
    let app = h.state_mut();
    app.set_map_export_for_test(crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: false,
        scale_bar: false,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
    });
    app.export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(
        svg.matches("stroke-linejoin=\"round\"/>").count() >= 100,
        "diamonds in the export"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Lines as wide as a number, on screen and in exports.
#[test]
fn line_widths_by_value() {
    use crate::app::Export;
    use crate::export_map::Format;
    use territoria_model::MapLayerKind;
    let dir = std::env::temp_dir().join(format!("territoria-lines-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Ten roads, each busier than the one before.
    let features: Vec<String> = (0..10)
        .map(|i| {
            let y = 46.5 + i as f64 * 0.05;
            format!(
                r#"{{"type":"Feature","properties":{{"Road":"R{i}","Traffic":{}}},"geometry":{{"type":"LineString","coordinates":[[7.0,{y}],[7.5,{:.3}],[8.0,{y}]]}}}}"#,
                (i + 1) * 1000,
                y + 0.02
            )
        })
        .collect();
    let path = dir.join("roads.geojson");
    std::fs::write(
        &path,
        format!(
            r#"{{"type":"FeatureCollection","features":[{}]}}"#,
            features.join(",")
        ),
    )
    .unwrap();
    let (mut h, _lock) = harness(false);
    let layer = h.state_mut().add_boundaries_now(&path).expect("roads")[0];
    let app = h.state_mut();
    let map = app.project.maps[0].id;
    for ml in &mut app.project.map_mut(map).unwrap().layers {
        if let MapLayerKind::Boundaries { style, .. } = &mut ml.kind {
            style.width_by = Some(territoria_model::map::LabelSize {
                source: territoria_model::ValueSource::Attribute {
                    column: "Traffic".into(),
                },
                min: 1.0,
                max: 10.0,
            });
        }
    }
    assert_eq!(
        app.project.layer(layer).unwrap().kind(),
        territoria_io::GeometryKind::Line
    );
    settle(&mut h);
    save(&mut h, "line-widths");
    let svg_path = dir.join("roads.svg");
    let app = h.state_mut();
    app.set_map_export_for_test(crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: false,
        scale_bar: false,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
    });
    app.export(Export::Map(Format::Svg), svg_path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&svg_path).unwrap();
    assert!(
        svg.contains(r#"stroke-width="1.00" stroke-linejoin"#),
        "thinnest"
    );
    assert!(
        svg.contains(r#"stroke-width="10.00" stroke-linejoin"#),
        "widest"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A round per territory of customers, from each territory's site.
#[test]
fn rounds_for_all_territories() {
    use crate::dialogs::Placement;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let table = customers(h.state(), layer, 90);
    let app = h.state_mut();
    app.settings.routing.url = territoria_io::routing::mock_server();
    app.settings.routing.interval_ms = 0;
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
    let plan = app.add_balanced_plan_now(points, 3);
    // Each territory's site at its centre.
    let g = app.project.layer(points).unwrap().geometry.clone();
    let centres = app.project.plan(plan).unwrap().centres(&g, None);
    {
        let p = app.project.plan_mut(plan).unwrap();
        for (t, c) in p.territories.iter_mut().zip(&centres) {
            t.site = c.map(|c| [c.x, c.y]);
        }
    }
    // The menu asks how long visits take first.
    app.apply_action_for_test(crate::app::Action::PlanRounds(plan));
    settle(&mut h);
    save(&mut h, "rounds-dialog");
    h.state_mut().close_dialogs();
    h.state_mut().plan_rounds(
        plan,
        &crate::dialogs::VisitTime {
            minutes: 15.0,
            column: None,
        },
    );
    settle(&mut h);
    save(&mut h, "plan-rounds");
    let app = h.state();
    let rounds = app.project.layers.last().unwrap();
    assert_eq!(rounds.name, "Rounds of Sales territories");
    assert_eq!(rounds.len(), 3, "a round per territory");
    assert!(rounds.attributes.column("Km").unwrap().f64(0).unwrap() > 0.0);
    // Workload: travel plus 15 minutes per customer.
    let t = &rounds.attributes;
    let stops = t.column("Stops").unwrap().f64(0).unwrap();
    let travel = t.column("Travel minutes").unwrap().f64(0).unwrap();
    assert_eq!(
        t.column("Visit minutes").unwrap().f64(0),
        Some(stops * 15.0)
    );
    let hours = t.column("Workload hours").unwrap().f64(0).unwrap();
    assert!(
        (hours - (travel + stops * 15.0) / 60.0).abs() < 0.06,
        "{hours}"
    );
    // Each territory's customers are visited 1, 2, … in some order.
    let p = app.project.plan(plan).unwrap();
    let order = app
        .project
        .layer(points)
        .unwrap()
        .attributes
        .column("Visit order")
        .expect("visit order");
    for members in p.members() {
        let mut ranks: Vec<usize> = members
            .iter()
            .map(|&u| order.f64(u as usize).unwrap() as usize)
            .collect();
        ranks.sort_unstable();
        assert_eq!(ranks, (1..=members.len()).collect::<Vec<_>>());
    }
}

/// Areas coloured by their minutes by road to the nearest branch.
#[test]
fn drive_time_zones() {
    use crate::dialogs::Placement;
    use territoria_model::MapLayerKind;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    app.settings.routing.url = territoria_io::routing::mock_server();
    app.settings.routing.interval_ms = 0;
    let b = app.project.layer(layer).unwrap().geometry.bbox.unwrap();
    let at = |u: f64, v: f64| (b.min().x + u * b.width(), b.min().y + v * b.height());
    let spots = [at(0.25, 0.35), at(0.75, 0.65)];
    let branches = Table::new(vec![
        Column::text(
            "Branch",
            ColumnKind::Key,
            vec![Some("West".into()), Some("East".into())],
        ),
        Column::number("Lon", spots.iter().map(|s| Some(s.0)).collect()),
        Column::number("Lat", spots.iter().map(|s| Some(s.1)).collect()),
    ])
    .unwrap();
    app.add_table(
        "Branches".into(),
        branches,
        PathBuf::from("branches.csv"),
        Placement::Points {
            x: "Lon".into(),
            y: "Lat".into(),
        },
        None,
    );
    let points = app.project.layers.last().unwrap().id;
    app.drive_zones_for(
        layer,
        territoria_model::catchment::Centres::Points(points),
        vec![2.0, 4.0, 6.0],
        true,
    );
    settle(&mut h);
    save(&mut h, "drive-zones");
    let app = h.state_mut();
    // Under the branches' points.
    let ml = app.project.maps[0]
        .layers
        .iter()
        .find(|l| l.name == "Drive time to Branches")
        .expect("the zones")
        .clone();
    let MapLayerKind::Choropleth { scale, .. } = &ml.kind else {
        panic!("zones");
    };
    assert_eq!(scale.labels[0], "under 2 min");
    let mut values = territoria_model::Cache::default();
    let scaled = app
        .analyses
        .get(&app.project, &mut values, ml.id, &ml.kind)
        .unwrap();
    assert_eq!(scaled.legend.len(), 4, "four zones");
    assert!(
        scaled.legend.iter().all(|e| e.count > 0),
        "{:?}",
        scaled.legend
    );
    // The zones' outlines: one shape per zone.
    let outlines = app
        .project
        .layers
        .iter()
        .find(|l| l.name == "Drive-time zones to Branches")
        .expect("the outlines");
    assert_eq!(outlines.len(), 4);
}

/// The interface in German.
#[test]
fn german_interface() {
    use egui_kittest::kittest::Queryable;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    app.settings.language = territoria_config::Language::German;
    app.add_balanced_plan_now(layer, 4);
    settle(&mut h);
    save(&mut h, "german");
    h.get_by_label("Datei");
    h.get_by_label("⬣ Neue Gebiete");
    h.get_by_label("Neu einfärben");
    // Commands too, e.g. in the palette.
    assert_eq!(
        crate::commands::Command::SaveProject.label(),
        "Datei: Speichern"
    );
    assert_eq!(
        crate::commands::Command::SaveProject.short_label(),
        "Speichern"
    );
    // Dialogs too.
    h.state_mut()
        .run_command_for_test(crate::commands::Command::ExportMap);
    h.run_steps(3);
    save(&mut h, "german-dialog");
    h.get_by_label("Karte exportieren");
    // Help as well.
    h.state_mut()
        .run_command_for_test(crate::commands::Command::Help);
    h.run_steps(3);
    h.get_by_label("Erste Schritte");
    // Messages and their values too.
    h.state_mut()
        .run_command_for_test(crate::commands::Command::Undo);
    assert!(
        h.state()
            .toasts_for_test()
            .iter()
            .any(|t| t.starts_with("Rückgängig: ") || t == "Nichts rückgängig zu machen"),
        "{:?}",
        h.state().toasts_for_test()
    );
    crate::i18n::set_language(territoria_config::Language::English);
}

/// Each site takes its nearest areas up to capacity: a scenario shows
/// what three sites can cover of four territories' worth.
#[test]
fn coverage_from_sites() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 4);
    let g = app.project.layer(layer).unwrap().geometry.clone();
    let centres = app.project.plan(plan).unwrap().centres(&g, None);
    {
        let p = app.project.plan_mut(plan).unwrap();
        // Three of the four territories have a site; no tolerance.
        for (t, c) in p.territories.iter_mut().zip(&centres).take(3) {
            t.site = c.map(|c| [c.x, c.y]);
        }
        p.measures[0].tolerance = 0.0;
    }
    app.apply_action_for_test(crate::app::Action::Coverage(plan));
    settle(&mut h);
    save(&mut h, "coverage");
    let app = h.state();
    let scenario = app.project.plans.last().unwrap();
    assert_eq!(scenario.name, "Sales territories – coverage");
    let members = scenario.members();
    let n = scenario.assignment.len();
    let target = n as f64 / 4.0;
    for m in &members[..3] {
        assert!(
            m.len() as f64 <= target.ceil(),
            "{} areas over {target}",
            m.len()
        );
        assert!(m.len() as f64 >= target - 2.0, "{} areas", m.len());
    }
    assert!(members[3].is_empty(), "no site, nothing taken");
    let left = scenario.assignment.iter().filter(|a| a.is_none()).count();
    assert!(left as f64 >= target - 3.0, "{left} left over");
}

/// A line drawn with the measure tool becomes a barrier: areas across it
/// are no longer neighbours, also after saving and opening.
#[test]
fn barriers_between_areas() {
    use crate::map_view::MapEvent;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    settle(&mut h);
    let app = h.state_mut();
    let g = app.project.layer(layer).unwrap().geometry.clone();
    let b = g.bbox.unwrap();
    let x = b.min().x + b.width() / 2.0;
    let line = vec![[x, b.min().y - 0.01], [x, b.max().y + 0.01]];
    let crossing = |g: &territoria_model::LayerGeometry| {
        (0..g.len())
            .flat_map(|a| g.neighbors[a].iter().map(move |&(n, _)| (a, n as usize)))
            .filter(|&(a, n)| {
                let (p, q) = (g.label_points[a].unwrap(), g.label_points[n].unwrap());
                (p.x < x) != (q.x < x)
            })
            .count()
    };
    assert!(crossing(&g) > 0);
    app.measure_for_test(line.clone());
    app.map_events_for_test(vec![MapEvent::AddBarrier {
        layer,
        line: line.clone(),
    }]);
    settle(&mut h);
    save(&mut h, "barrier");
    let app = h.state();
    let l = app.project.layer(layer).unwrap();
    assert_eq!(l.barriers.len(), 1);
    assert_eq!(crossing(&l.geometry), 0, "no neighbours across");
    // Kept in the file.
    let path = std::env::temp_dir().join(format!(
        "territoria-barrier-{}.territoria",
        std::process::id()
    ));
    territoria_model::file::save(&app.project, &path).unwrap();
    let back = territoria_model::file::load(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let l = back.layer(layer).unwrap();
    assert_eq!(l.barriers.len(), 1);
    assert_eq!(crossing(&l.geometry), 0, "still cut after opening");
}

/// Balancing with distances to the sites by road asks the routing service
/// first, and reports when it can't.
#[test]
fn balance_by_road() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 3);
    let g = app.project.layer(layer).unwrap().geometry.clone();
    app.project
        .plan_mut(plan)
        .unwrap()
        .sites_at_centres(&g, true, None);
    let routing = territoria_io::routing::RoutingSettings {
        url: territoria_io::routing::mock_server(),
        interval_ms: 0,
        ..Default::default()
    };
    let settings = crate::territories::BalanceSettings {
        start_over: true,
        by_road: Some(routing),
        ..Default::default()
    };
    let mut values = territoria_model::Cache::default();
    let input =
        crate::territories::balance_input(&app.project, &mut values, plan, &settings).unwrap();
    let proposal = crate::territories::run_balance(input, &mut |_| true).expect("balanced by road");
    assert_eq!(proposal.after.split, 0);
    assert!(proposal.after.out_of_tolerance.iter().all(|&n| n == 0));
    // No service: an error, not a silent straight-line result.
    let settings = crate::territories::BalanceSettings {
        start_over: true,
        by_road: Some(territoria_io::routing::RoutingSettings {
            url: "http://127.0.0.1:9".into(),
            interval_ms: 0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let input =
        crate::territories::balance_input(&app.project, &mut values, plan, &settings).unwrap();
    assert!(crate::territories::run_balance(input, &mut |_| true).is_err());
}

/// Regions like another layer's areas: each territory joins the area most
/// of its areas lie in.
#[test]
fn regions_like_a_layer() {
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&hexagons())
        .expect("boundaries")[0];
    let app = h.state_mut();
    app.merge_areas_for_test(layer, "code", Some(2), "Districts");
    let districts = app.project.layers.last().unwrap().id;
    let plan = app.add_balanced_plan_now(layer, 8);
    let (groups, names) = {
        let p = app.project.plan(plan).unwrap();
        let units = app.project.layer(layer).unwrap();
        let other = app.project.layer(districts).unwrap();
        let groups = p.regions_like(&units.geometry, other, 1);
        let names: Vec<String> = (0..other.len()).map(|f| other.display_name(f)).collect();
        (groups, names)
    };
    assert!(groups.len() >= 2, "{groups:?}");
    assert_eq!(
        groups.iter().map(|g| g.1.len()).sum::<usize>(),
        8,
        "every territory once"
    );
    assert!(groups.iter().all(|(n, _)| names.contains(n)));
}

/// A printed page of a territory plan with its figures beside the map.
#[test]
fn printed_page_with_figures() {
    use crate::app::Export;
    use crate::export_map::{Format, PageSize};
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    h.state_mut().add_balanced_plan_now(layer, 5);
    settle(&mut h);
    let dir = std::env::temp_dir().join(format!("territoria-figures-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let settings = |format| crate::dialogs::MapExportSettings {
        format,
        scale: 1.5,
        title: "Sales territories 2027".into(),
        legend: true,
        scale_bar: true,
        page: PageSize::A4Landscape,
        footer: "{date}".into(),
        overview: false,
        figures: true,
    };
    let svg_path = dir.join("page.svg");
    h.state_mut().set_map_export_for_test(settings(Format::Svg));
    h.state_mut()
        .export(Export::Map(Format::Svg), svg_path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&svg_path).unwrap();
    assert!(svg.contains(">Territory 05</text>"), "a row per territory");
    assert!(svg.contains(">±</text>"), "the deviation column");
    if let Some(out) = out_dir() {
        let png = dir.join("page.png");
        h.state_mut().set_map_export_for_test(settings(Format::Png));
        h.state_mut().export(Export::Map(Format::Png), png.clone());
        settle(&mut h);
        std::fs::copy(&png, out.join("page-figures.png")).unwrap();
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Areas shared with a second territory: half their figures count there,
/// they show striped, and sharing can stop again.
#[test]
fn shared_areas() {
    use crate::app::{Action, Export};
    use crate::export_map::Format;
    use crate::map_view::MapEvent;
    let (mut h, _lock) = harness(false);
    let layer = h
        .state_mut()
        .add_boundaries_now(&boundaries())
        .expect("boundaries")[0];
    let app = h.state_mut();
    let plan = app.add_balanced_plan_now(layer, 4);
    let (first, second, units) = {
        let p = app.project.plan(plan).unwrap();
        let members = p.members();
        (
            p.territories[0].id,
            p.territories[1].id,
            members[0].iter().take(10).copied().collect::<Vec<u32>>(),
        )
    };
    let mut values = territoria_model::Cache::default();
    let before = app
        .project
        .plan(plan)
        .unwrap()
        .stats(&app.project, &mut values);
    app.apply_action_for_test(Action::SelectUnits(layer, units.clone()));
    app.apply_action_for_test(Action::ActivateTerritory(Some(second)));
    app.map_events_for_test(vec![MapEvent::Share {
        plan,
        with: Some(second),
    }]);
    settle(&mut h);
    save(&mut h, "shared-areas");
    let app = h.state_mut();
    let p = app.project.plan(plan).unwrap();
    assert_eq!(p.shared.len(), 10);
    assert!(p.shared.iter().all(|s| s.with == second));
    let after = p.stats(&app.project, &mut values);
    let v = |s: &territoria_model::PlanStats, i: usize| s.territories[i].values[0];
    assert!(
        (v(&before, 0) - v(&after, 0) - 5.0).abs() < 1e-9,
        "half of 10 areas leave"
    );
    assert!(
        (v(&after, 1) - v(&before, 1) - 5.0).abs() < 1e-9,
        "and arrive"
    );
    assert_eq!(
        p.assignment[units[0] as usize],
        Some(first),
        "still in its own territory"
    );
    // Striped in exports too.
    let dir = std::env::temp_dir().join(format!("territoria-shared-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shared.svg");
    app.set_map_export_for_test(crate::dialogs::MapExportSettings {
        format: Format::Svg,
        scale: 1.0,
        title: String::new(),
        legend: false,
        scale_bar: false,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
    });
    app.export(Export::Map(Format::Svg), path.clone());
    settle(&mut h);
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(svg.contains(r#"fill="none" stroke=""#) && svg.contains(r#"stroke-width="2"/>"#));
    let _ = std::fs::remove_dir_all(dir);
    h.state_mut()
        .map_events_for_test(vec![MapEvent::Share { plan, with: None }]);
    assert!(h.state().project.plan(plan).unwrap().shared.is_empty());
}
