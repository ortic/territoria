//! The Contents panel: what the map shows (with legends), the data in the
//! project and the territory plans.

use egui::{Color32, RichText, Sense, Ui, vec2};
use territoria_analysis::format::format_value;
use territoria_io::GeometryKind;
use territoria_model::{Cache, MapId, MapLayerKind, PlanId, Project};

use crate::app::{Action, DataRef, Selection, Tab};
use crate::commands::Command;
use crate::map_view::color;
use crate::meshes::AnalysisCache;

pub struct ContentsContext<'a> {
    pub project: &'a Project,
    pub values: &'a mut Cache,
    pub analyses: &'a mut AnalysisCache,
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
                            MapLayerKind::Categories { .. } => "◧",
                            MapLayerKind::Territories { .. } => "⬣",
                            MapLayerKind::Symbols { .. } => "●",
                        };
                        let text = RichText::new(format!("{icon} {}", ml.name));
                        let text = if ml.visible { text } else { text.weak() };
                        let r = ui
                            .selectable_label(selected, text)
                            .on_hover_text(ml.kind.type_label());
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
                                if max > 0.0 {
                                    ui.indent(("legend", ml.id.0), |ui| {
                                        size_legend(ui, symbol, max);
                                    });
                                }
                            }
                            MapLayerKind::Boundaries { .. } => {}
                        }
                    }
                }
                ui.add_space(4.0);
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
                let kind = match l.kind() {
                    GeometryKind::Area => "areas",
                    GeometryKind::Line => "lines",
                    GeometryKind::Point => "points",
                };
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
        p.circle_filled(c, r, fill.gamma_multiply(0.35));
        p.circle_stroke(c, r, egui::Stroke::new(1.0, fill));
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
