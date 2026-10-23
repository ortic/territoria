//! The Contents panel: what the map shows (with legends), the data in the
//! project and the territory plans.

use egui::{Color32, RichText, Sense, Ui, vec2};
use territoria_analysis::format::format_value;
use territoria_model::{Cache, MapId, MapLayerKind, PlanId, Project};

use crate::app::{Action, DataRef, Selection, Tab};
use crate::commands::Command;
use crate::map_view::color;
use crate::meshes::AnalysisCache;

pub struct ContentsContext<'a> {
    pub project: &'a Project,
    pub values: &'a mut Cache,
    pub analyses: &'a mut AnalysisCache,
    /// The current map's view scale (display units per point).
    pub scale: Option<f64>,
    pub current_map: Option<MapId>,
    pub selection: Selection,
    pub active_plan: Option<PlanId>,
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(title)
            .small()
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(2.0);
}

fn swatch(ui: &mut Ui, c: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    ui.painter().rect_filled(rect.shrink(1.0), 3.0, c);
}

pub fn show(ui: &mut Ui, cx: ContentsContext) -> Vec<Action> {
    let mut actions = Vec::new();
    let project = cx.project;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // The map.
            if let Some(map) = cx.current_map.and_then(|m| project.map(m)) {
                section(ui, &format!("MAP · {}", map.name.to_uppercase()));
                background_ui(ui, map, &mut actions);
                projection_ui(ui, project, map, &mut actions);
                views_ui(ui, map, &mut actions);
                let count = map.layers.len();
                for (pos, ml) in map.layers.iter().enumerate().rev() {
                    let selected = cx.selection == Selection::MapLayer(map.id, ml.id);
                    let map_id = map.id;
                    let layer_id = ml.id;
                    ui.horizontal(|ui| {
                        let mut visible = ml.visible;
                        if ui
                            .checkbox(&mut visible, "")
                            .on_hover_text("Show or hide")
                            .changed()
                        {
                            actions.push(Action::Edit(
                                if visible {
                                    "Show layer".into()
                                } else {
                                    "Hide layer".into()
                                },
                                Box::new(move |p| {
                                    if let Some(l) =
                                        p.map_mut(map_id).and_then(|m| m.layer_mut(layer_id))
                                    {
                                        l.visible = visible;
                                    }
                                }),
                            ));
                        }
                        let icon = match &ml.kind {
                            MapLayerKind::Boundaries { .. } => "▣",
                            MapLayerKind::Choropleth { .. } => "▦",
                            MapLayerKind::Bivariate { .. } => "◩",
                            MapLayerKind::Flows { .. } => "↗",
                            MapLayerKind::Categories { .. } => "◧",
                            MapLayerKind::Territories { .. } => "⬣",
                            MapLayerKind::Symbols { .. } => "●",
                            MapLayerKind::Rings { .. } => "◎",
                            MapLayerKind::Heatmap { .. } => "◍",
                            MapLayerKind::Charts { chart, .. } => {
                                if chart.shape.is_bars() {
                                    "▮"
                                } else {
                                    "◔"
                                }
                            }
                        };
                        let text = RichText::new(format!("{icon} {}", ml.name));
                        // Hidden at this zoom: greyed, with why.
                        let zoomed = cx.scale.is_some_and(|s| !ml.zoom.contains(s));
                        let text = if ml.visible && !zoomed {
                            text
                        } else {
                            text.weak()
                        };
                        let r = ui
                            .selectable_label(selected, text)
                            .on_hover_text(if ml.visible && zoomed {
                                format!("{} · not shown at this zoom", ml.kind.type_label())
                            } else {
                                ml.kind.type_label().to_owned()
                            });
                        if r.clicked() {
                            actions.push(Action::Select(Selection::MapLayer(map_id, layer_id)));
                        }
                        r.context_menu(|ui| {
                            if ui
                                .add_enabled(pos + 1 < count, egui::Button::new("Move up"))
                                .clicked()
                            {
                                actions.push(move_layer(map_id, pos, pos + 1));
                                ui.close();
                            }
                            if ui
                                .add_enabled(pos > 0, egui::Button::new("Move down"))
                                .clicked()
                            {
                                actions.push(move_layer(map_id, pos, pos - 1));
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Remove from map").clicked() {
                                actions.push(Action::Edit(
                                    "Remove from map".into(),
                                    Box::new(move |p| {
                                        if let Some(m) = p.map_mut(map_id) {
                                            m.layers.retain(|l| l.id != layer_id);
                                        }
                                    }),
                                ));
                                ui.close();
                            }
                        });
                    });
                    // Legend under analysis layers.
                    if ml.visible {
                        match &ml.kind {
                            MapLayerKind::Choropleth { .. } | MapLayerKind::Categories { .. } => {
                                if let Some(scaled) =
                                    cx.analyses.get(project, cx.values, ml.id, &ml.kind)
                                {
                                    ui.indent(("legend", ml.id.0), |ui| {
                                        if let Some((stops, min, max)) = &scaled.gradient {
                                            let (rect, _) = ui.allocate_exact_size(
                                                vec2(160.0, 12.0),
                                                Sense::hover(),
                                            );
                                            let n = stops.len().max(2);
                                            let step = rect.width() / (n - 1) as f32;
                                            for i in 0..n - 1 {
                                                let r = egui::Rect::from_min_size(
                                                    rect.min + vec2(i as f32 * step, 0.0),
                                                    vec2(step + 0.5, rect.height()),
                                                );
                                                ui.painter().rect_filled(
                                                    r,
                                                    0.0,
                                                    color(stops[i].mix(stops[i + 1], 0.5)),
                                                );
                                            }
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new(format_value(*min)).small());
                                                ui.add_space(80.0);
                                                ui.label(RichText::new(format_value(*max)).small());
                                            });
                                        }
                                        for entry in &scaled.legend {
                                            if scaled.gradient.is_some() {
                                                break;
                                            }
                                            ui.horizontal(|ui| {
                                                swatch(ui, color(entry.color));
                                                ui.label(RichText::new(&entry.label).small());
                                                ui.label(
                                                    RichText::new(format!("({})", entry.count))
                                                        .small()
                                                        .weak(),
                                                );
                                            });
                                        }
                                    });
                                }
                            }
                            MapLayerKind::Territories { plan, .. } => {
                                if let Some(p) = project.plan(*plan) {
                                    ui.indent(("legend", ml.id.0), |ui| {
                                        for t in p.territories.iter().take(12) {
                                            ui.horizontal(|ui| {
                                                swatch(ui, color(t.color));
                                                ui.label(RichText::new(&t.name).small());
                                            });
                                        }
                                        if p.territories.len() > 12 {
                                            ui.label(
                                                RichText::new(format!(
                                                    "… {} more",
                                                    p.territories.len() - 12
                                                ))
                                                .small()
                                                .weak(),
                                            );
                                        }
                                    });
                                }
                            }
                            MapLayerKind::Symbols {
                                layer,
                                source,
                                normalize,
                                symbol,
                                ..
                            } => {
                                let v = cx.values.normalized(project, *layer, source, normalize);
                                let max = v.iter().flatten().fold(0.0f64, |a, b| a.max(*b));
                                let categories = symbol
                                    .color_by
                                    .is_some()
                                    .then(|| cx.analyses.get(project, cx.values, ml.id, &ml.kind))
                                    .flatten();
                                ui.indent(("legend", ml.id.0), |ui| {
                                    if max > 0.0 {
                                        // Sizes in grey when colours mean categories.
                                        let neutral = territoria_model::SymbolStyle {
                                            color: territoria_analysis::Rgba::rgb(0x8a, 0x8d, 0x96),
                                            ..symbol.clone()
                                        };
                                        size_legend(
                                            ui,
                                            if categories.is_some() { &neutral } else { symbol },
                                            max,
                                        );
                                    }
                                    for entry in categories.iter().flat_map(|s| s.legend.iter()) {
                                        ui.horizontal(|ui| {
                                            let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                                            ui.painter().circle_filled(rect.center(), 5.5, color(entry.color));
                                            ui.label(RichText::new(&entry.label).small());
                                            ui.label(RichText::new(format!("({})", entry.count)).small().weak());
                                        });
                                    }
                                });
                            }
                            MapLayerKind::Flows { style, .. } => {
                                if let Some(lines) = cx.analyses.flows(project, ml.id, &ml.kind) {
                                    let max = lines.iter().map(|l| l.value).fold(0.0, f64::max);
                                    if max > 0.0 {
                                        ui.indent(("legend", ml.id.0), |ui| {
                                            width_legend(ui, style, max);
                                        });
                                    }
                                }
                            }
                            MapLayerKind::Bivariate { x, y, .. } => {
                                if let Some(grid) = cx
                                    .analyses
                                    .get(project, cx.values, ml.id, &ml.kind)
                                    .and_then(|s| s.grid.clone())
                                {
                                    ui.indent(("legend", ml.id.0), |ui| {
                                        grid_legend(ui, &grid, &x.label(project), &y.label(project));
                                    });
                                }
                            }
                            MapLayerKind::Charts {
                                layer, parts, chart, ..
                            } => {
                                ui.indent(("legend", ml.id.0), |ui| {
                                    for p in parts {
                                        ui.horizontal(|ui| {
                                            swatch(ui, color(p.color));
                                            ui.label(RichText::new(&p.label).small());
                                        });
                                    }
                                    // Pies sized by their total: what sizes mean.
                                    if chart.sized && !chart.shape.is_bars() {
                                        let n = project.layer(*layer).map_or(0, |l| l.len());
                                        let mut totals = vec![0.0f64; n];
                                        for p in parts {
                                            let v = cx.values.values(project, *layer, &p.source);
                                            for (t, x) in totals.iter_mut().zip(&v) {
                                                *t += x.unwrap_or(0.0).max(0.0);
                                            }
                                        }
                                        let max = totals.iter().copied().fold(0.0f64, f64::max);
                                        if max > 0.0 {
                                            ui.label(RichText::new("Total").small().weak());
                                            let symbol = territoria_model::SymbolStyle {
                                                color: territoria_analysis::Rgba::rgb(0x8a, 0x8d, 0x96),
                                                max_radius: chart.max_size,
                                                ..Default::default()
                                            };
                                            size_legend(ui, &symbol, max);
                                        }
                                    }
                                });
                            }
                            MapLayerKind::Heatmap { ramp, .. } => {
                                ui.indent(("legend", ml.id.0), |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("fewer").small());
                                        let (rect, _) = ui.allocate_exact_size(vec2(90.0, 10.0), Sense::hover());
                                        let stops = ramp.colors(9);
                                        let step = rect.width() / stops.len() as f32;
                                        for (i, c) in stops.iter().enumerate() {
                                            let r = egui::Rect::from_min_size(rect.min + vec2(i as f32 * step, 0.0), vec2(step + 0.5, rect.height()));
                                            ui.painter().rect_filled(r, 0.0, color(*c));
                                        }
                                        ui.label(RichText::new("more").small());
                                    });
                                });
                            }
                            MapLayerKind::Rings {
                                radii_km, color: c, ..
                            } => {
                                ui.indent(("legend", ml.id.0), |ui| {
                                    ui.horizontal(|ui| {
                                        let (rect, _) = ui
                                            .allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                                        ui.painter().circle(
                                            rect.center(),
                                            6.0,
                                            color(*c).gamma_multiply(0.15),
                                            egui::Stroke::new(1.2, color(*c)),
                                        );
                                        let radii: Vec<String> = radii_km
                                            .iter()
                                            .map(|r| territoria_data::table::format_number(*r))
                                            .collect();
                                        ui.label(
                                            RichText::new(format!(
                                                "within {} km",
                                                radii.join(" / ")
                                            ))
                                            .small(),
                                        );
                                    });
                                });
                            }
                            MapLayerKind::Boundaries { .. } => {}
                        }
                    }
                }
                ui.add_space(4.0);
                // A column dragged from a table: dropped here, it goes on the map.
                if let Some(drag) =
                    egui::DragAndDrop::payload::<crate::app::ColumnDrag>(ui.ctx())
                {
                    let (_, dropped) = ui.dnd_drop_zone::<crate::app::ColumnDrag, ()>(
                        egui::Frame::default().inner_margin(8.0),
                        |ui| {
                            ui.label(format!("⤵ Drop here to show {} on the map", drag.column));
                        },
                    );
                    if let Some(d) = dropped {
                        actions.push(Action::ShowColumn(d.data, d.column.clone()));
                    }
                }
                ui.menu_button("➕ Add to map", |ui| {
                    for l in &project.layers {
                        if ui.button(format!("▣ {}", l.name)).clicked() {
                            actions.push(Action::AddToMap(l.id));
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("🎨 Colour areas by value…").clicked() {
                        actions.push(Action::Command(Command::ColorByValue));
                        ui.close();
                    }
                    for p in &project.plans {
                        if ui.button(format!("⬣ {}", p.name)).clicked() {
                            actions.push(Action::ShowPlan(p.id));
                            ui.close();
                        }
                    }
                });
            }

            // Data.
            section(ui, "DATA");
            for l in &project.layers {
                let selected = cx.selection == Selection::Data(DataRef::Layer(l.id));
                let kind = l.kind().noun(l.len());
                let r = ui
                    .selectable_label(selected, format!("🗺 {}  ", l.name))
                    .on_hover_text(format!(
                        "{} {kind} · key: {}",
                        format_value(l.len() as f64),
                        l.key_column.as_deref().unwrap_or("row number")
                    ));
                if r.clicked() {
                    actions.push(Action::Select(Selection::Data(DataRef::Layer(l.id))));
                }
                if r.double_clicked() {
                    actions.push(Action::OpenTab(Tab::Table(DataRef::Layer(l.id))));
                }
                let id = l.id;
                r.context_menu(|ui| {
                    if ui.button("Open table").clicked() {
                        actions.push(Action::OpenTab(Tab::Table(DataRef::Layer(id))));
                        ui.close();
                    }
                    if ui.button("Show on map").clicked() {
                        actions.push(Action::AddToMap(id));
                        ui.close();
                    }
                    if ui.button("Colour by value…").clicked() {
                        actions.push(Action::ColorBy(Some(id)));
                        ui.close();
                    }
                    if ui.button("Merge areas into a new layer…").clicked() {
                        actions.push(Action::Select(Selection::Data(DataRef::Layer(id))));
                        actions.push(Action::Command(Command::MergeAreas));
                        ui.close();
                    }
                    if ui
                        .button("Replace with a newer version…")
                        .on_hover_text("New boundaries from a file; data, colourings and territories carry over by key or by position")
                        .clicked()
                    {
                        actions.push(Action::Select(Selection::Data(DataRef::Layer(id))));
                        actions.push(Action::Command(Command::ReplaceLayer));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Remove…").clicked() {
                        actions.push(Action::RemoveLayer(id));
                        ui.close();
                    }
                });
                ui.label(
                    RichText::new(format!("    {} {kind}", format_value(l.len() as f64)))
                        .small()
                        .weak(),
                );
            }
            for t in &project.tables {
                let selected = cx.selection == Selection::Data(DataRef::Table(t.id));
                let r = ui.selectable_label(selected, format!("▦ {}", t.name));
                if r.clicked() {
                    actions.push(Action::Select(Selection::Data(DataRef::Table(t.id))));
                }
                if r.double_clicked() {
                    actions.push(Action::OpenTab(Tab::Table(DataRef::Table(t.id))));
                }
                let id = t.id;
                r.context_menu(|ui| {
                    if ui.button("Open table").clicked() {
                        actions.push(Action::OpenTab(Tab::Table(DataRef::Table(id))));
                        ui.close();
                    }
                    if ui.button("Link to map…").clicked() {
                        actions.push(Action::LinkTable(id));
                        ui.close();
                    }
                    if t.source.path.is_some()
                        && ui
                            .button("↻ Reload from file")
                            .on_hover_text("Read the file again (e.g. this month's figures); links and colourings stay")
                            .clicked()
                    {
                        actions.push(Action::ReloadTable(id));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Remove…").clicked() {
                        actions.push(Action::RemoveTable(id));
                        ui.close();
                    }
                });
                let links: Vec<String> = project
                    .links
                    .iter()
                    .filter(|l| l.table == t.id)
                    .filter_map(|l| project.layer(l.layer).map(|layer| layer.name.clone()))
                    .collect();
                let detail = if links.is_empty() {
                    format!(
                        "    {} rows · not linked",
                        format_value(t.table.rows() as f64)
                    )
                } else {
                    format!(
                        "    {} rows · → {}",
                        format_value(t.table.rows() as f64),
                        links.join(", ")
                    )
                };
                let r = ui.label(RichText::new(detail).small().weak());
                if links.is_empty() && r.interact(Sense::click()).clicked() {
                    actions.push(Action::LinkTable(t.id));
                }
            }
            ui.horizontal(|ui| {
                if ui.small_button("➕ Boundaries").clicked() {
                    actions.push(Action::Command(Command::AddBoundaries));
                }
                if ui.small_button("➕ Data").clicked() {
                    actions.push(Action::Command(Command::AddData));
                }
            });

            // Plans.
            section(ui, "TERRITORY PLANS");
            for p in &project.plans {
                let active = cx.active_plan == Some(p.id);
                let assigned = p.assignment.iter().filter(|a| a.is_some()).count();
                let r = ui
                    .selectable_label(active, format!("⬣ {}", p.name))
                    .on_hover_text(format!(
                        "{} territories · {} of {} areas assigned",
                        p.territories.len(),
                        format_value(assigned as f64),
                        format_value(p.assignment.len() as f64)
                    ));
                if r.clicked() {
                    actions.push(Action::ShowPlan(p.id));
                }
            }
            if ui.small_button("➕ New plan").clicked() {
                actions.push(Action::Command(Command::NewPlan));
            }
        });
    actions
}

fn move_layer(map: MapId, from: usize, to: usize) -> Action {
    Action::Edit(
        "Reorder layers".into(),
        Box::new(move |p| {
            if let Some(m) = p.map_mut(map)
                && from < m.layers.len()
                && to < m.layers.len()
            {
                let l = m.layers.remove(from);
                m.layers.insert(to, l);
            }
        }),
    )
}

/// The background map picker: none, a preset tile server or a custom URL.
fn background_ui(ui: &mut Ui, map: &territoria_model::Map, actions: &mut Vec<Action>) {
    use territoria_model::map::Background;
    let map_id = map.id;
    let set = |bg: Background| {
        Action::Edit(
            "Change background map".into(),
            Box::new(move |p: &mut territoria_model::Project| {
                if let Some(m) = p.map_mut(map_id) {
                    m.background = bg;
                }
            }),
        )
    };
    let current = match &map.background {
        Background::None => "None".to_owned(),
        Background::Tiles { url, .. } => crate::tiles::PRESETS
            .iter()
            .find(|p| p.url == url)
            .map_or("Custom".to_owned(), |p| p.name.to_owned()),
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Background").small());
        egui::ComboBox::from_id_salt("background")
            .width(150.0)
            .selected_text(&current)
            .show_ui(ui, |ui| {
                if ui.selectable_label(current == "None", "None").clicked() {
                    actions.push(set(Background::None));
                }
                for p in crate::tiles::PRESETS {
                    if ui
                        .selectable_label(current == p.name, p.name)
                        .on_hover_text(p.note)
                        .clicked()
                    {
                        actions.push(set(Background::Tiles {
                            url: p.url.to_owned(),
                            attribution: p.attribution.to_owned(),
                        }));
                    }
                }
                if ui
                    .selectable_label(current == "Custom", "Custom tile server…")
                    .on_hover_text(
                        "Any XYZ tile server in Web Mercator: a URL with {z}, {x} and {y}",
                    )
                    .clicked()
                    && current != "Custom"
                {
                    actions.push(set(Background::Tiles {
                        url: "https://example.org/tiles/{z}/{x}/{y}.png".into(),
                        attribution: String::new(),
                    }));
                }
            });
    });
    if current == "Custom"
        && let Background::Tiles { url, attribution } = &map.background
    {
        let (mut u, mut a) = (url.clone(), attribution.clone());
        let r1 = ui.add(
            egui::TextEdit::singleline(&mut u)
                .hint_text("https://…/{z}/{x}/{y}.png")
                .desired_width(f32::INFINITY),
        );
        let r2 = ui.add(
            egui::TextEdit::singleline(&mut a)
                .hint_text("Credit, e.g. © OpenStreetMap contributors")
                .desired_width(f32::INFINITY),
        );
        if (r1.changed() || r2.changed()) && !u.is_empty() {
            actions.push(set(Background::Tiles {
                url: u,
                attribution: a,
            }));
        }
    }
    if !matches!(map.background, Background::None) && !map.crs.is_web_mercator() {
        ui.label(
            RichText::new("Background maps need the Web Mercator projection.")
                .small()
                .weak(),
        );
    }
}

/// Lines of three widths with their values.
fn width_legend(ui: &mut Ui, style: &territoria_model::flows::FlowStyle, max: f64) {
    let big = nice_below(max);
    for v in [big, nice_below(big / 3.0), nice_below(big / 10.0)] {
        let w = (style.max_width * (v / max) as f32).max(0.6);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(36.0, 14.0), Sense::hover());
            ui.painter().line_segment(
                [rect.left_center(), rect.right_center()],
                egui::Stroke::new(w, color(style.color)),
            );
            ui.label(RichText::new(format_value(v)).small());
        });
    }
}

/// The colour grid of two values: the first rising to the right, the
/// second upward; hovering a cell tells its ranges and areas.
fn grid_legend(ui: &mut Ui, grid: &territoria_analysis::BivariateGrid, x: &str, y: &str) {
    let k = grid.size;
    let cell = 16.0;
    ui.label(RichText::new(format!("↑ {y}")).small());
    let (rect, response) =
        ui.allocate_exact_size(vec2(cell * k as f32, cell * k as f32), Sense::hover());
    let at = |row: usize, col: usize| {
        egui::Rect::from_min_size(
            egui::pos2(
                rect.left() + col as f32 * cell,
                rect.bottom() - (row + 1) as f32 * cell,
            ),
            vec2(cell, cell),
        )
    };
    for row in 0..k {
        for col in 0..k {
            ui.painter().rect_filled(
                at(row, col).shrink(0.5),
                0.0,
                color(grid.colors[row * k + col]),
            );
        }
    }
    let range = |edges: &[f64], i: usize| match (edges.get(i), edges.get(i + 1)) {
        (Some(a), Some(b)) => format!("{} – {}", format_value(*a), format_value(*b)),
        _ => "–".to_owned(),
    };
    if let Some(p) = response.hover_pos() {
        for row in 0..k {
            for col in 0..k {
                if at(row, col).contains(p) {
                    response.clone().on_hover_text(format!(
                        "{x}: {}\n{y}: {}\n{} areas",
                        range(&grid.x_edges, col),
                        range(&grid.y_edges, row),
                        grid.counts[row * k + col]
                    ));
                }
            }
        }
    }
    ui.label(RichText::new(format!("{x} →")).small());
}

/// The largest power-of-ten multiple of 1, 2 or 5 not above `x`.
fn nice_below(x: f64) -> f64 {
    let p = 10f64.powf(x.log10().floor());
    [5.0, 2.0, 1.0]
        .into_iter()
        .map(|m| m * p)
        .find(|v| *v <= x)
        .unwrap_or(p)
}

/// Nested circles for three values, with their numbers.
fn size_legend(ui: &mut Ui, symbol: &territoria_model::SymbolStyle, max: f64) {
    let big = nice_below(max);
    let steps = [big, nice_below(big / 4.0), nice_below(big / 16.0)];
    let r_max = symbol.radius(big, max).max(4.0);
    let (rect, _) =
        ui.allocate_exact_size(vec2(2.0 * r_max + 80.0, 2.0 * r_max + 20.0), Sense::hover());
    let p = ui.painter();
    let cx = rect.left() + r_max + 2.0;
    let bottom = rect.top() + 2.0 * r_max + 6.0;
    let mut last_label = f32::NEG_INFINITY;
    let fill = color(symbol.color);
    let line = ui.visuals().weak_text_color();
    for v in steps {
        let r = symbol.radius(v, max);
        if r < 1.0 {
            continue;
        }
        let c = egui::pos2(cx, bottom - r);
        let mut shapes = Vec::new();
        crate::map_view::symbol(
            &mut shapes,
            symbol.shape,
            c,
            r,
            fill.gamma_multiply(0.35),
            egui::Stroke::new(1.0, fill),
        );
        p.extend(shapes);
        let top = egui::pos2(cx, bottom - 2.0 * r);
        let label_x = cx + r_max + 8.0;
        // Labels keep apart even where small circles' tops are close.
        let y = top.y.max(last_label + 12.0);
        last_label = y;
        p.line_segment(
            [top, egui::pos2(label_x - 2.0, y)],
            egui::Stroke::new(0.5, line),
        );
        p.text(
            egui::pos2(label_x, y),
            egui::Align2::LEFT_CENTER,
            format_value(v),
            egui::FontId::proportional(10.0),
            ui.visuals().text_color(),
        );
    }
}

/// Views kept by name: go to one, keep the current one, rename or forget.
fn views_ui(ui: &mut Ui, map: &territoria_model::Map, actions: &mut Vec<Action>) {
    let map_id = map.id;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Views").small());
        ui.menu_button(
            if map.views.is_empty() {
                "None kept".to_owned()
            } else {
                format!("{} kept", map.views.len())
            },
            |ui| {
                for (i, v) in map.views.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui
                            .button(&v.name)
                            .on_hover_text("Go to this view")
                            .clicked()
                        {
                            actions.push(Action::GoToView(map_id, v.center, v.scale));
                            ui.close();
                        }
                        let mut name = v.name.clone();
                        if ui
                            .add(egui::TextEdit::singleline(&mut name).desired_width(100.0))
                            .changed()
                        {
                            actions.push(Action::Edit(
                                "Rename view".into(),
                                Box::new(move |p| {
                                    if let Some(v) =
                                        p.map_mut(map_id).and_then(|m| m.views.get_mut(i))
                                    {
                                        v.name = name;
                                    }
                                }),
                            ));
                        }
                        if ui
                            .small_button("✖")
                            .on_hover_text("Forget this view")
                            .clicked()
                        {
                            actions.push(Action::Edit(
                                "Forget view".into(),
                                Box::new(move |p| {
                                    if let Some(m) = p.map_mut(map_id)
                                        && i < m.views.len()
                                    {
                                        m.views.remove(i);
                                    }
                                }),
                            ));
                        }
                    });
                }
                if !map.views.is_empty() {
                    ui.separator();
                }
                if ui.button("➕ Keep this view").clicked() {
                    actions.push(Action::Command(Command::SaveView));
                    ui.close();
                }
            },
        );
    });
}

/// The projection the map is drawn in.
fn projection_ui(
    ui: &mut Ui,
    project: &territoria_model::Project,
    map: &territoria_model::Map,
    actions: &mut Vec<Action>,
) {
    use territoria_geo::Crs;
    let map_id = map.id;
    let local = map.crs.proj.contains("+proj=laea") && map.crs.epsg.is_none();
    let current = if map.crs.is_web_mercator() {
        "Web Mercator".to_owned()
    } else if local {
        "Equal area (local)".to_owned()
    } else {
        map.crs.label()
    };
    // The middle of all layers, for a local projection.
    let centre = project
        .layers
        .iter()
        .filter(|l| l.geometry.geographic)
        .filter_map(|l| l.geometry.bbox)
        .reduce(|a, b| {
            geo_types::Rect::new(
                geo_types::Coord {
                    x: a.min().x.min(b.min().x),
                    y: a.min().y.min(b.min().y),
                },
                geo_types::Coord {
                    x: a.max().x.max(b.max().x),
                    y: a.max().y.max(b.max().y),
                },
            )
        })
        .map(|b| b.center());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Projection").small());
        egui::ComboBox::from_id_salt("projection")
            .width(118.0)
            .selected_text(&current)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(map.crs.is_web_mercator(), "Web Mercator")
                    .on_hover_text("As web maps: needed for background maps; areas far from the equator look bigger")
                    .clicked()
                {
                    actions.push(Action::SetMapCrs(map_id, Crs::web_mercator()));
                }
                if let Some(c) = centre
                    && ui
                        .selectable_label(local, "Equal area (local)")
                        .on_hover_text("Lambert equal-area around your data: true sizes, for print and comparisons")
                        .clicked()
                    && let Ok(crs) = Crs::from_proj(&format!(
                        "+proj=laea +lat_0={:.4} +lon_0={:.4} +x_0=0 +y_0=0 +ellps=WGS84 +units=m +no_defs",
                        c.y, c.x
                    ))
                {
                    actions.push(Action::SetMapCrs(map_id, crs));
                }
            });
        let id = ui.id().with("epsg");
        let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
        let r = ui
            .add(egui::TextEdit::singleline(&mut text).hint_text("EPSG").desired_width(46.0))
            .on_hover_text("Any EPSG code, e.g. 2056 (Switzerland), 25832 (Germany), 2154 (France); Enter applies");
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !text.trim().is_empty() {
            match Crs::parse(&text) {
                Ok(crs) => actions.push(Action::SetMapCrs(map_id, crs)),
                Err(e) => actions.push(Action::Notify(format!("{text}: {e}"))),
            }
            text.clear();
        }
        ui.data_mut(|d| d.insert_temp(id, text));
    });
    if !map.crs.is_web_mercator()
        && matches!(
            map.background,
            territoria_model::map::Background::Tiles { .. }
        )
    {
        ui.label(
            RichText::new("Background maps show in Web Mercator only.")
                .small()
                .color(crate::theme::WARN),
        );
    }
}
