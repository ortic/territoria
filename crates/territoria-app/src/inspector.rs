//! The Properties panel: settings of what is selected.

use egui::{ComboBox, RichText, Sense, Ui, vec2};
use territoria_analysis::format::format_value;
use territoria_analysis::{ColorScale, Method, RAMPS, RampKind, RampSpec, ScaleMode};
use territoria_data::{ColumnKind, MatchMode};
use territoria_model::{
    BoundaryStyle, Cache, LabelMode, LayerId, MapId, MapLayer, MapLayerId, MapLayerKind, Project,
    ValueSource,
};

use crate::app::{Action, DataRef, Selection, Tab};
use crate::dialogs::{Colorable, colorables, match_report};
use crate::map_view::color;
use crate::meshes::AnalysisCache;

/// An undoable change of one map layer.
fn edit_layer(
    map: MapId,
    id: MapLayerId,
    label: &str,
    f: impl FnOnce(&mut MapLayer) + 'static,
) -> Action {
    Action::Edit(
        label.to_owned(),
        Box::new(move |p| {
            if let Some(l) = p.map_mut(map).and_then(|m| m.layer_mut(id)) {
                f(l);
            }
        }),
    )
}

fn rgba_button(ui: &mut Ui, c: territoria_analysis::Rgba) -> Option<territoria_analysis::Rgba> {
    let mut col = color(c);
    let before = col;
    egui::color_picker::color_edit_button_srgba(ui, &mut col, egui::color_picker::Alpha::OnlyBlend);
    (col != before).then(|| {
        let [r, g, b, a] = col.to_srgba_unmultiplied();
        territoria_analysis::Rgba([r, g, b, a])
    })
}

pub fn show(
    ui: &mut Ui,
    project: &Project,
    values: &mut Cache,
    analyses: &mut AnalysisCache,
    selection: Selection,
) -> Vec<Action> {
    let mut actions = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match selection {
            Selection::None => {
                ui.label(
                    RichText::new(
                        "Select a map layer or data in the Contents panel to see its settings.",
                    )
                    .weak(),
                );
            }
            Selection::MapLayer(map, id) => {
                if let Some(ml) = project.map(map).and_then(|m| m.layer(id)) {
                    map_layer(ui, project, values, analyses, map, ml, &mut actions);
                }
            }
            Selection::Data(DataRef::Layer(id)) => {
                if project.layer(id).is_some() {
                    layer(ui, project, id, &mut actions);
                }
            }
            Selection::Data(DataRef::Table(id)) => {
                if let Some(t) = project.table(id) {
                    ui.heading(&t.name);
                    ui.label(
                        RichText::new(format!(
                            "{} rows · {} columns",
                            format_value(t.table.rows() as f64),
                            t.table.columns.len()
                        ))
                        .weak(),
                    );
                    if let Some(p) = &t.source.path {
                        ui.label(RichText::new(p.display().to_string()).small().weak());
                    }
                    ui.add_space(6.0);
                    if ui.button("▦ Open table").clicked() {
                        actions.push(Action::OpenTab(Tab::Table(DataRef::Table(id))));
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("Linked to").strong());
                    let mut any = false;
                    for link in project.links.iter().filter(|l| l.table == id) {
                        any = true;
                        let Some(layer) = project.layer(link.layer) else {
                            continue;
                        };
                        ui.label(format!(
                            "{} by “{}” ({})",
                            layer.name,
                            link.column,
                            link.mode.label()
                        ));
                        if let Some(m) = values.matching(project, link.id) {
                            match_report(ui, &m.stats, "rows", "areas");
                        }
                        ui.add_space(4.0);
                    }
                    if !any {
                        ui.label(RichText::new("Not linked to any areas yet.").weak());
                    }
                    if ui.button("🔗 Link to map…").clicked() {
                        actions.push(Action::LinkTable(id));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Columns").strong());
                        if ui
                            .small_button("ƒ Calculated column…")
                            .on_hover_text("A new column computed from the others with a formula")
                            .clicked()
                        {
                            actions.push(Action::Formula(crate::app::DataRef::Table(id), None));
                        }
                    });
                    egui::Grid::new("cols").striped(true).show(ui, |ui| {
                        for c in &t.table.columns {
                            match &c.formula {
                                Some(f) => {
                                    if ui
                                        .link(format!("ƒ {}", c.name))
                                        .on_hover_text(format!("= {f}\nClick to edit the formula"))
                                        .clicked()
                                    {
                                        actions.push(Action::Formula(
                                            crate::app::DataRef::Table(id),
                                            Some(c.name.clone()),
                                        ));
                                    }
                                    ui.label(RichText::new(c.kind.label()).weak());
                                    ui.end_row();
                                    continue;
                                }
                                None => {
                                    ui.label(&c.name);
                                }
                            }
                            let mut kind = c.kind;
                            ComboBox::from_id_salt(("tk", &c.name))
                                .selected_text(kind.label())
                                .show_ui(ui, |ui| {
                                    for k in ColumnKind::ALL {
                                        ui.selectable_value(&mut kind, k, k.label());
                                    }
                                });
                            if kind != c.kind {
                                let name = c.name.clone();
                                actions.push(Action::Edit(
                                    "Change column type".into(),
                                    Box::new(move |p| {
                                        let rev = p.bump();
                                        if let Some(t) = p.table_mut(id) {
                                            let table = std::sync::Arc::make_mut(&mut t.table);
                                            if let Some(c) =
                                                table.columns.iter_mut().find(|c| c.name == name)
                                            {
                                                c.convert(kind);
                                            }
                                            territoria_expr::recalculate(table);
                                            t.rev = rev;
                                        }
                                    }),
                                ));
                            }
                            ui.end_row();
                        }
                    });
                }
            }
        });
    actions
}

fn layer(ui: &mut Ui, project: &Project, id: LayerId, actions: &mut Vec<Action>) {
    let Some(l) = project.layer(id) else { return };
    ui.heading(&l.name);
    ui.label(
        RichText::new(format!(
            "{} {}",
            format_value(l.len() as f64),
            l.kind().label()
        ))
        .weak(),
    );
    ui.add_space(6.0);
    egui::Grid::new("layer-props")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Key")
                .on_hover_text("Data is matched to areas by this column");
            let mut key = l.key_column.clone();
            ComboBox::from_id_salt("lkey")
                .selected_text(key.clone().unwrap_or("Row number".into()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut key, None, "Row number");
                    for c in &l.attributes.columns {
                        ui.selectable_value(&mut key, Some(c.name.clone()), &c.name);
                    }
                });
            if key != l.key_column {
                actions.push(Action::Edit(
                    "Change key column".into(),
                    Box::new(move |p| {
                        let rev = p.bump();
                        if let Some(l) = p.layer_mut(id) {
                            l.key_column = key;
                            l.rev = rev;
                        }
                    }),
                ));
            }
            ui.end_row();
            ui.label("Name");
            let mut name = l.name_column.clone();
            ComboBox::from_id_salt("lname")
                .selected_text(name.clone().unwrap_or("None".into()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut name, None, "None");
                    for c in l.attributes.columns.iter().filter(|c| c.kind.is_textual()) {
                        ui.selectable_value(&mut name, Some(c.name.clone()), &c.name);
                    }
                });
            if name != l.name_column {
                actions.push(Action::Edit(
                    "Change name column".into(),
                    Box::new(move |p| {
                        if let Some(l) = p.layer_mut(id) {
                            l.name_column = name;
                        }
                    }),
                ));
            }
            ui.end_row();
            // Long texts are cut (full text on hover): a grid doesn't wrap and
            // would widen the panel.
            let max = (ui.available_width() - 90.0).max(80.0);
            ui.label("Coordinates");
            ui.add_sized(
                [max, 16.0],
                egui::Label::new(RichText::new(&l.source.crs_note).small()).truncate(),
            )
            .on_hover_text(&l.source.crs_note);
            ui.end_row();
            if let Some(a) = &l.source.attribution {
                ui.label("Credit");
                ui.add_sized(
                    [max, 16.0],
                    egui::Label::new(RichText::new(a).small()).truncate(),
                )
                .on_hover_text(a);
                ui.end_row();
            }
            if let Some(p) = &l.source.path {
                ui.label("File");
                let path = p.display().to_string();
                ui.add_sized(
                    [max, 16.0],
                    egui::Label::new(RichText::new(&path).small()).truncate(),
                )
                .on_hover_text(path);
                ui.end_row();
            }
            if let Some(b) = l.geometry.bbox {
                let total: f64 = l.geometry.areas.iter().sum();
                if total > 0.0 {
                    ui.label("Area");
                    ui.label(format!("{} km²", format_value(total)));
                    ui.end_row();
                }
                let _ = b;
            }
        });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("▦ Open table").clicked() {
            actions.push(Action::OpenTab(Tab::Table(DataRef::Layer(id))));
        }
        if ui.button("🎨 Colour by value…").clicked() {
            actions.push(Action::ColorBy(Some(id)));
        }
    });
}

fn style_ui(
    ui: &mut Ui,
    style: &BoundaryStyle,
    layer: Option<&territoria_model::Layer>,
    fill: bool,
    map: MapId,
    id: MapLayerId,
    actions: &mut Vec<Action>,
) {
    egui::Grid::new("style")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            if fill {
                ui.label("Fill");
                if let Some(c) = rgba_button(ui, style.fill) {
                    actions.push(edit_layer(map, id, "Change fill", move |l| {
                        if let Some(s) = l.kind.style_mut() {
                            s.fill = c;
                        }
                    }));
                }
                ui.end_row();
            }
            ui.label("Borders");
            ui.horizontal(|ui| {
                if let Some(c) = rgba_button(ui, style.stroke) {
                    actions.push(edit_layer(map, id, "Change borders", move |l| {
                        if let Some(s) = l.kind.style_mut() {
                            s.stroke = c;
                        }
                    }));
                }
                let mut w = style.stroke_width;
                if ui
                    .add(
                        egui::DragValue::new(&mut w)
                            .range(0.0..=6.0)
                            .speed(0.05)
                            .suffix(" pt"),
                    )
                    .changed()
                {
                    actions.push(edit_layer(map, id, "Change border width", move |l| {
                        if let Some(s) = l.kind.style_mut() {
                            s.stroke_width = w;
                        }
                    }));
                }
            });
            ui.end_row();
            if let Some(layer) = layer {
                ui.label("Labels");
                let mut labels = style.labels.clone();
                let text = match &labels {
                    LabelMode::None => "None".to_owned(),
                    LabelMode::Key => "Key".to_owned(),
                    LabelMode::Name => "Name".to_owned(),
                    LabelMode::Column(c) => c.clone(),
                };
                ComboBox::from_id_salt("labels")
                    .selected_text(text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut labels, LabelMode::None, "None");
                        ui.selectable_value(&mut labels, LabelMode::Key, "Key");
                        if layer.name_column.is_some() {
                            ui.selectable_value(&mut labels, LabelMode::Name, "Name");
                        }
                        for c in &layer.attributes.columns {
                            ui.selectable_value(
                                &mut labels,
                                LabelMode::Column(c.name.clone()),
                                &c.name,
                            );
                        }
                    });
                if labels != style.labels {
                    actions.push(edit_layer(map, id, "Change labels", move |l| {
                        if let Some(s) = l.kind.style_mut() {
                            s.labels = labels;
                        }
                    }));
                }
                ui.end_row();
            }
        });
}

/// A row of ramp swatches to pick from.
fn ramp_picker(ui: &mut Ui, current: &RampSpec, kinds: &[RampKind]) -> Option<RampSpec> {
    let mut picked = None;
    let (name, reversed) = match current {
        RampSpec::Named { name, reversed } => (name.clone(), *reversed),
        RampSpec::Custom { .. } => (String::new(), false),
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for r in RAMPS.iter().filter(|r| kinds.contains(&r.kind)) {
            let (rect, resp) = ui.allocate_exact_size(vec2(46.0, 16.0), Sense::click());
            let colors = r.colors(5);
            let w = rect.width() / colors.len() as f32;
            for (i, c) in colors.iter().enumerate() {
                let rr = egui::Rect::from_min_size(
                    rect.min + vec2(i as f32 * w, 0.0),
                    vec2(w + 0.5, rect.height()),
                );
                ui.painter().rect_filled(rr, 0.0, color(*c));
            }
            if r.name == name {
                ui.painter().rect_stroke(
                    rect.expand(1.5),
                    2.0,
                    egui::Stroke::new(2.0, ui.visuals().text_color()),
                    egui::StrokeKind::Outside,
                );
            }
            let tip = if r.colorblind_safe {
                format!("{} · colour-blind safe", r.name)
            } else {
                r.name.to_owned()
            };
            if resp.on_hover_text(tip).clicked() {
                picked = Some(RampSpec::Named {
                    name: r.name.to_owned(),
                    reversed,
                });
            }
        }
    });
    let mut rev = reversed;
    if !name.is_empty() && ui.checkbox(&mut rev, "Reverse colours").changed() {
        picked = Some(RampSpec::Named {
            name,
            reversed: rev,
        });
    }
    picked
}

fn scale_ui(
    ui: &mut Ui,
    scale: &ColorScale,
    map: MapId,
    id: MapLayerId,
    actions: &mut Vec<Action>,
) {
    let edit = |label: &str, f: Box<dyn FnOnce(&mut ColorScale)>| {
        edit_layer(map, id, label, move |l| {
            if let MapLayerKind::Choropleth { scale, .. } = &mut l.kind {
                f(scale);
            }
        })
    };
    ui.horizontal(|ui| {
        let mut mode = scale.mode;
        ui.selectable_value(&mut mode, ScaleMode::Classified, "Classes");
        ui.selectable_value(&mut mode, ScaleMode::Continuous, "Smooth gradient");
        if mode != scale.mode {
            actions.push(edit("Change colouring", Box::new(move |s| s.mode = mode)));
        }
    });
    if scale.mode == ScaleMode::Classified {
        egui::Grid::new("classes")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("Method");
                let mut method = scale.method;
                ComboBox::from_id_salt("method")
                    .width(190.0)
                    .selected_text(method.label())
                    .show_ui(ui, |ui| {
                        for m in Method::ALL.into_iter().filter(|m| *m != Method::Manual) {
                            ui.selectable_value(&mut method, m, m.label())
                                .on_hover_text(m.help());
                        }
                    });
                if method != scale.method {
                    actions.push(edit(
                        "Change classification",
                        Box::new(move |s| s.method = method),
                    ));
                }
                ui.end_row();
                ui.label("Classes");
                let mut classes = scale.classes;
                if ui.add(egui::Slider::new(&mut classes, 2..=9)).changed() {
                    actions.push(edit(
                        "Change class count",
                        Box::new(move |s| s.classes = classes),
                    ));
                }
                ui.end_row();
                ui.label("");
                let mut round = scale.round;
                if ui.checkbox(&mut round, "Round class limits").changed() {
                    actions.push(edit(
                        "Round class limits",
                        Box::new(move |s| s.round = round),
                    ));
                }
                ui.end_row();
            });
        ui.label(RichText::new(scale.method.help()).small().weak());
    }
    ui.add_space(4.0);
    ui.label("Colours");
    if let Some(r) = ramp_picker(
        ui,
        &scale.ramp,
        &[RampKind::Sequential, RampKind::Diverging],
    ) {
        actions.push(edit("Change colours", Box::new(move |s| s.ramp = r)));
    }
}

fn map_layer(
    ui: &mut Ui,
    project: &Project,
    values: &mut Cache,
    analyses: &mut AnalysisCache,
    map: MapId,
    ml: &MapLayer,
    actions: &mut Vec<Action>,
) {
    let id = ml.id;
    let mut name = ml.name.clone();
    if ui
        .add(
            egui::TextEdit::singleline(&mut name)
                .font(egui::TextStyle::Heading)
                .desired_width(f32::INFINITY),
        )
        .changed()
    {
        actions.push(edit_layer(map, id, "Rename layer", move |l| l.name = name));
    }
    ui.label(RichText::new(ml.kind.type_label()).weak());
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Opacity");
        let mut o = ml.opacity;
        if ui
            .add(egui::Slider::new(&mut o, 0.0..=1.0).show_value(false))
            .changed()
        {
            actions.push(edit_layer(map, id, "Change opacity", move |l| {
                l.opacity = o
            }));
        }
    });
    ui.separator();
    match &ml.kind {
        MapLayerKind::Boundaries { layer, style } => {
            style_ui(ui, style, project.layer(*layer), true, map, id, actions);
        }
        MapLayerKind::Choropleth {
            layer,
            source,
            scale,
            style,
        } => {
            let options = colorables(project, *layer);
            ui.horizontal(|ui| {
                ui.label("Value");
                let current = options
                    .iter()
                    .find(|(_, c)| matches!(c, Colorable::Number(s) if same_source(s, source)))
                    .map_or_else(|| source.label(project), |(l, _)| l.clone());
                ComboBox::from_id_salt("source")
                    .width(220.0)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (label, c) in &options {
                            if let Colorable::Number(s) = c
                                && ui.selectable_label(same_source(s, source), label).clicked()
                            {
                                let mut s = s.clone();
                                // Keep the chosen aggregation when switching columns.
                                if let (
                                    ValueSource::Linked { agg, .. },
                                    ValueSource::Linked { agg: old, .. },
                                ) = (&mut s, source)
                                {
                                    *agg = *old;
                                }
                                let label = label.clone();
                                actions.push(edit_layer(map, id, "Change value", move |l| {
                                    if let MapLayerKind::Choropleth { source, .. } = &mut l.kind {
                                        *source = s;
                                    }
                                    l.name = label;
                                }));
                            }
                        }
                    });
            });
            if let ValueSource::Linked { agg, .. }
            | ValueSource::Points {
                column: Some(_),
                agg,
                ..
            } = source
            {
                ui.horizontal(|ui| {
                    ui.label("Per area");
                    let mut a = *agg;
                    ComboBox::from_id_salt("agg")
                        .selected_text(a.label())
                        .show_ui(ui, |ui| {
                            for x in territoria_data::Aggregate::ALL {
                                ui.selectable_value(&mut a, x, x.label());
                            }
                        });
                    if a != *agg {
                        actions.push(edit_layer(map, id, "Change aggregation", move |l| {
                            if let MapLayerKind::Choropleth {
                                source: ValueSource::Linked { agg, .. } | ValueSource::Points { agg, .. },
                                ..
                            } = &mut l.kind
                            {
                                *agg = a;
                            }
                        }));
                    }
                });
            }
            ui.add_space(4.0);
            scale_ui(ui, scale, map, id, actions);
            ui.separator();
            if let Some(scaled) = analyses.get(project, values, id, &ml.kind) {
                histogram(
                    ui,
                    project,
                    values,
                    *layer,
                    source,
                    &scaled.edges,
                    &scaled.legend,
                );
            }
            ui.separator();
            style_ui(ui, style, project.layer(*layer), false, map, id, actions);
        }
        MapLayerKind::Categories {
            layer,
            scale,
            style,
            ..
        } => {
            ui.label("Colours");
            if let Some(r) = ramp_picker(ui, &scale.ramp, &[RampKind::Qualitative]) {
                actions.push(edit_layer(map, id, "Change colours", move |l| {
                    if let MapLayerKind::Categories { scale, .. } = &mut l.kind {
                        scale.ramp = r;
                    }
                }));
            }
            ui.horizontal(|ui| {
                ui.label("Most frequent");
                let mut n = scale.max_categories;
                if ui.add(egui::Slider::new(&mut n, 2..=24)).changed() {
                    actions.push(edit_layer(map, id, "Change categories", move |l| {
                        if let MapLayerKind::Categories { scale, .. } = &mut l.kind {
                            scale.max_categories = n;
                        }
                    }));
                }
            });
            ui.separator();
            style_ui(ui, style, project.layer(*layer), false, map, id, actions);
        }
        MapLayerKind::Territories { style, plan } => {
            let edit =
                |label: &str, f: Box<dyn FnOnce(&mut territoria_model::map::TerritoryStyle)>| {
                    edit_layer(map, id, label, move |l| {
                        if let MapLayerKind::Territories { style, .. } = &mut l.kind {
                            f(style);
                        }
                    })
                };
            ui.horizontal(|ui| {
                ui.label("Fill");
                let mut o = style.fill_opacity;
                if ui
                    .add(egui::Slider::new(&mut o, 0.0..=1.0).show_value(false))
                    .changed()
                {
                    actions.push(edit("Change fill", Box::new(move |s| s.fill_opacity = o)));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Territory borders");
                let mut w = style.border_width;
                if ui
                    .add(
                        egui::DragValue::new(&mut w)
                            .range(0.0..=6.0)
                            .speed(0.05)
                            .suffix(" pt"),
                    )
                    .changed()
                {
                    actions.push(edit(
                        "Change borders",
                        Box::new(move |s| s.border_width = w),
                    ));
                }
            });
            let mut ub = style.unit_borders;
            if ui
                .checkbox(&mut ub, "Show area borders inside territories")
                .changed()
            {
                actions.push(edit("Area borders", Box::new(move |s| s.unit_borders = ub)));
            }
            let mut lb = style.labels;
            if ui.checkbox(&mut lb, "Territory names").changed() {
                actions.push(edit("Territory names", Box::new(move |s| s.labels = lb)));
            }
            ui.add_space(6.0);
            if ui.button("Open the Territories panel").clicked() {
                actions.push(Action::ShowPlan(*plan));
            }
        }
    }
    let _ = MatchMode::Exact;
}

fn same_source(a: &ValueSource, b: &ValueSource) -> bool {
    match (a, b) {
        (
            ValueSource::Linked {
                link: l1,
                column: c1,
                ..
            },
            ValueSource::Linked {
                link: l2,
                column: c2,
                ..
            },
        ) => l1 == l2 && c1 == c2,
        (
            ValueSource::Points {
                layer: l1,
                column: c1,
                ..
            },
            ValueSource::Points {
                layer: l2,
                column: c2,
                ..
            },
        ) => l1 == l2 && c1 == c2,
        _ => a == b,
    }
}

/// Distribution of the values with the class limits.
fn histogram(
    ui: &mut Ui,
    project: &Project,
    values: &mut Cache,
    layer: LayerId,
    source: &ValueSource,
    edges: &[f64],
    legend: &[territoria_analysis::LegendEntry],
) {
    let v: Vec<f64> = values
        .values(project, layer, source)
        .into_iter()
        .flatten()
        .filter(|x| x.is_finite())
        .collect();
    if v.is_empty() || edges.len() < 2 {
        ui.label(RichText::new("No values to show: check the link to the data.").weak());
        return;
    }
    let (min, max) = (edges[0], edges[edges.len() - 1]);
    let bins = 40;
    let mut counts = vec![0usize; bins];
    for x in &v {
        let i = (((x - min) / (max - min).max(1e-12)) * bins as f64).floor() as isize;
        counts[i.clamp(0, bins as isize - 1) as usize] += 1;
    }
    let peak = *counts.iter().max().unwrap_or(&1) as f32;
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().min(300.0), 64.0), Sense::hover());
    let p = ui.painter();
    let w = rect.width() / bins as f32;
    for (i, c) in counts.iter().enumerate() {
        let mid = min + (i as f64 + 0.5) / bins as f64 * (max - min);
        let class = territoria_analysis::classify::class_of(mid, edges).unwrap_or(0);
        let col = legend
            .get(class)
            .map_or(ui.visuals().weak_text_color(), |e| color(e.color));
        let h = (*c as f32 / peak) * rect.height();
        let r = egui::Rect::from_min_max(
            egui::pos2(rect.left() + i as f32 * w, rect.bottom() - h),
            egui::pos2(rect.left() + (i + 1) as f32 * w - 1.0, rect.bottom()),
        );
        p.rect_filled(r, 0.0, col);
    }
    for e in &edges[1..edges.len() - 1] {
        let x = rect.left() + ((e - min) / (max - min).max(1e-12)) as f32 * rect.width();
        p.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(1.0, ui.visuals().text_color()),
        );
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(format_value(min)).small());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format_value(max)).small())
        });
    });
    for e in legend {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
            ui.painter().rect_filled(r.shrink(1.0), 3.0, color(e.color));
            ui.label(&e.label);
            ui.label(RichText::new(format!("{} areas", e.count)).weak());
        });
    }
}
