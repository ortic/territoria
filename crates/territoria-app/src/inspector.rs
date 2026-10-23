//! The Properties panel: settings of what is selected.

use crate::i18n::tr;
use egui::{ComboBox, RichText, Sense, Ui, vec2};
use territoria_analysis::format::format_value;
use territoria_analysis::{ColorScale, Method, RAMPS, RampKind, RampSpec, ScaleMode};
use territoria_data::{ColumnKind, MatchMode};
use territoria_model::{
    BoundaryStyle, Cache, LabelMode, LayerId, MapId, MapLayer, MapLayerId, MapLayerKind, Normalize,
    Project, TextSource, ValueSource,
};

use crate::app::{Action, DataRef, Selection, Tab};
use crate::dialogs::{Colorable, colorables, match_report};
use crate::map_view::color;
use crate::meshes::AnalysisCache;

/// A change of a flow layer's ends, value and style.
type FlowEdit = Box<
    dyn FnOnce(
        &mut territoria_model::flows::FlowEnd,
        &mut territoria_model::flows::FlowEnd,
        &mut Option<String>,
        &mut territoria_model::flows::FlowStyle,
    ),
>;

/// A change of a chart layer's values and style.
type ChartEdit =
    Box<dyn FnOnce(&mut Vec<territoria_model::ChartPart>, &mut territoria_model::ChartStyle)>;

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
    view: Option<(f64, f64)>,
    class_sets: &[territoria_config::ClassSet],
) -> Vec<Action> {
    let mut actions = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match selection {
            Selection::None => {
                ui.label(
                    RichText::new(tr(
                        "Select a map layer or data in the Contents panel to see its settings.",
                    ))
                    .weak(),
                );
            }
            Selection::MapLayer(map, id) => {
                if let Some(ml) = project.map(map).and_then(|m| m.layer(id)) {
                    map_layer(
                        ui,
                        project,
                        values,
                        analyses,
                        map,
                        ml,
                        view,
                        class_sets,
                        &mut actions,
                    );
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
                    if ui.button(tr("▦ Open table")).clicked() {
                        actions.push(Action::OpenTab(Tab::Table(DataRef::Table(id))));
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr("Linked to")).strong());
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
                        ui.label(RichText::new(tr("Not linked to any areas yet.")).weak());
                    }
                    ui.horizontal(|ui| {
                        if ui.button(tr("🔗 Link to map…")).clicked() {
                            actions.push(Action::LinkTable(id));
                        }
                        if t.source.path.is_some()
                            && ui
                                .button(tr("↻ Reload"))
                                .on_hover_text(tr(
                                    "Read the file again; links, types and calculated columns stay",
                                ))
                                .clicked()
                        {
                            actions.push(Action::ReloadTable(id));
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr("Columns")).strong());
                        if ui
                            .small_button(tr("ƒ Calculated column…"))
                            .on_hover_text(tr(
                                "A new column computed from the others with a formula",
                            ))
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
            l.kind().noun(l.len())
        ))
        .weak(),
    );
    ui.add_space(6.0);
    egui::Grid::new("layer-props")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label(tr("Key"))
                .on_hover_text(tr("Data is matched to areas by this column"));
            let mut key = l.key_column.clone();
            ComboBox::from_id_salt("lkey")
                .selected_text(key.clone().unwrap_or("Row number".into()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut key, None, tr("Row number"));
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
            ui.label(tr("Name"));
            let mut name = l.name_column.clone();
            ComboBox::from_id_salt("lname")
                .selected_text(name.clone().unwrap_or("None".into()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut name, None, tr("None"));
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
            ui.label(tr("Coordinates"));
            ui.add_sized(
                [max, 16.0],
                egui::Label::new(RichText::new(&l.source.crs_note).small()).truncate(),
            )
            .on_hover_text(&l.source.crs_note);
            ui.end_row();
            if let Some(a) = &l.source.attribution {
                ui.label(tr("Credit"));
                ui.add_sized(
                    [max, 16.0],
                    egui::Label::new(RichText::new(a).small()).truncate(),
                )
                .on_hover_text(a);
                ui.end_row();
            }
            if let Some(p) = &l.source.path {
                ui.label(tr("File"));
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
                    ui.label(tr("Area"));
                    ui.label(format!("{} km²", format_value(total)));
                    ui.end_row();
                }
                let _ = b;
            }
        });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button(tr("▦ Open table")).clicked() {
            actions.push(Action::OpenTab(Tab::Table(DataRef::Layer(id))));
        }
        if ui.button(tr("🎨 Colour by value…")).clicked() {
            actions.push(Action::ColorBy(Some(id)));
        }
    });
}

/// Edits a text template for a layer's features: shows mistakes and
/// offers the columns to insert. Returns the new text when it changed.
fn template_editor(
    ui: &mut Ui,
    project: &Project,
    values: &mut Cache,
    layer: LayerId,
    text: &str,
    salt: &str,
) -> Option<String> {
    let mut edited = text.to_owned();
    let mut changed = ui
        .add(
            egui::TextEdit::multiline(&mut edited)
                .id_salt(salt)
                .font(egui::TextStyle::Monospace)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        )
        .changed();
    let table = values.feature_table(project, layer);
    ui.horizontal(|ui| {
        if let Some(table) = &table {
            ui.menu_button(tr("➕ Insert"), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for c in &table.columns {
                            if ui.button(&c.name).clicked() {
                                let simple =
                                    c.name.chars().all(|ch| ch.is_alphanumeric() || ch == '_');
                                let name = if simple {
                                    c.name.clone()
                                } else {
                                    format!("[{}]", c.name.replace(']', "]]"))
                                };
                                if !edited.is_empty() && !edited.ends_with([' ', '\n']) {
                                    edited.push(' ');
                                }
                                edited.push_str(&format!("{{{name}}}"));
                                changed = true;
                                ui.close();
                            }
                        }
                    });
            })
            .response
            .on_hover_text("Columns of the layer, of linked tables and of points inside");
            match territoria_expr::Template::compile(&edited, table) {
                Ok(_) => {
                    ui.label(
                        RichText::new("Formulas in {braces}, e.g. {format([Revenue] / 1000, 0)}k")
                            .weak()
                            .small(),
                    );
                }
                Err(e) => {
                    ui.label(
                        RichText::new(format!("⚠ {}", e.message))
                            .color(crate::theme::DANGER)
                            .small(),
                    );
                }
            }
        }
    });
    changed.then_some(edited)
}

#[allow(clippy::too_many_arguments)]
fn style_ui(
    ui: &mut Ui,
    project: &Project,
    values: &mut Cache,
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
                ui.label(tr("Fill"));
                if let Some(c) = rgba_button(ui, style.fill) {
                    actions.push(edit_layer(map, id, "Change fill", move |l| {
                        if let Some(s) = l.kind.style_mut() {
                            s.fill = c;
                        }
                    }));
                }
                ui.end_row();
            }
            ui.label(tr("Borders"));
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
                ui.label(tr("Labels"));
                let mut labels = style.labels.clone();
                let text = match &labels {
                    LabelMode::None => tr("None").to_owned(),
                    LabelMode::Key => "Key".to_owned(),
                    LabelMode::Name => "Name".to_owned(),
                    LabelMode::Column(c) => c.clone(),
                    LabelMode::Template(_) => "Template…".to_owned(),
                };
                let first = if layer.name_column.is_some() {
                    "{Name}"
                } else {
                    "{Key}"
                };
                let default_template = layer
                    .name_column
                    .clone()
                    .or_else(|| layer.key_column.clone())
                    .map_or(first.to_owned(), |c| {
                        if c.chars().all(|ch| ch.is_alphanumeric() || ch == '_') {
                            format!("{{{c}}}")
                        } else {
                            format!("{{[{c}]}}")
                        }
                    });
                ComboBox::from_id_salt("labels")
                    .selected_text(text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut labels, LabelMode::None, tr("None"));
                        ui.selectable_value(&mut labels, LabelMode::Key, tr("Key"));
                        if layer.name_column.is_some() {
                            ui.selectable_value(&mut labels, LabelMode::Name, tr("Name"));
                        }
                        for c in &layer.attributes.columns {
                            ui.selectable_value(
                                &mut labels,
                                LabelMode::Column(c.name.clone()),
                                &c.name,
                            );
                        }
                        ui.separator();
                        if ui
                            .selectable_label(matches!(labels, LabelMode::Template(_)), "Template…")
                            .on_hover_text(tr(
                                "Your own text with values, e.g. the name and the revenue below it",
                            ))
                            .clicked()
                            && !matches!(labels, LabelMode::Template(_))
                        {
                            labels = LabelMode::Template(default_template.clone());
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
    let Some(layer) = layer else { return };
    // Lines as wide as a number (traffic on roads, deliveries on routes).
    if layer.kind() == territoria_io::GeometryKind::Line {
        let numbers: Vec<(String, ValueSource)> = colorables(project, layer.id)
            .into_iter()
            .filter_map(|(label, c)| match c {
                Colorable::Number(s) => Some((label, s)),
                _ => None,
            })
            .collect();
        ui.horizontal(|ui| {
            ui.label(tr("Line width"));
            let current = style.width_by.as_ref().map(|s| s.source.clone());
            let mut chosen = current.clone();
            let text = current.as_ref().map_or("All alike".to_owned(), |s| {
                format!("By {}", s.label(project))
            });
            ComboBox::from_id_salt("width-by")
                .width(170.0)
                .selected_text(text)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut chosen, None, tr("All alike"));
                    for (label, s) in &numbers {
                        ui.selectable_value(&mut chosen, Some(s.clone()), format!("By {label}"));
                    }
                });
            if chosen != current {
                actions.push(edit_layer(map, id, "Line widths", move |l| {
                    if let Some(s) = l.kind.style_mut() {
                        s.width_by = chosen.map(|source| territoria_model::map::LabelSize {
                            source,
                            min: 0.5,
                            max: 8.0,
                        });
                    }
                }));
            }
            if let Some(w) = &style.width_by {
                let (mut lo, mut hi) = (w.min, w.max);
                let a = ui.add(
                    egui::DragValue::new(&mut lo)
                        .range(0.1..=20.0)
                        .speed(0.1)
                        .suffix(" pt"),
                );
                ui.label(tr("to"));
                let b = ui.add(
                    egui::DragValue::new(&mut hi)
                        .range(0.1..=40.0)
                        .speed(0.1)
                        .suffix(" pt"),
                );
                if a.changed() || b.changed() {
                    actions.push(edit_layer(map, id, "Line widths", move |l| {
                        if let Some(s) = l.kind.style_mut()
                            && let Some(w) = &mut s.width_by
                        {
                            w.min = lo;
                            w.max = hi.max(lo);
                        }
                    }));
                }
            }
        });
    }
    // Label sizes from a number: bigger values, bigger labels.
    if style.labels != LabelMode::None {
        let numbers: Vec<(String, ValueSource)> = colorables(project, layer.id)
            .into_iter()
            .filter_map(|(label, c)| match c {
                Colorable::Number(s) => Some((label, s)),
                _ => None,
            })
            .collect();
        ui.horizontal(|ui| {
            ui.label(tr("Label size"));
            let current = style.label_size.as_ref().map(|s| s.source.clone());
            let mut chosen = current.clone();
            let text = current.as_ref().map_or("All alike".to_owned(), |s| {
                format!("By {}", s.label(project))
            });
            ComboBox::from_id_salt("label-size")
                .width(170.0)
                .selected_text(text)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut chosen, None, tr("All alike"));
                    for (label, s) in &numbers {
                        ui.selectable_value(&mut chosen, Some(s.clone()), format!("By {label}"));
                    }
                });
            if chosen != current {
                actions.push(edit_layer(map, id, "Label sizes", move |l| {
                    if let Some(s) = l.kind.style_mut() {
                        s.label_size = chosen.map(|source| territoria_model::map::LabelSize {
                            source,
                            min: 8.0,
                            max: 18.0,
                        });
                    }
                }));
            }
            if let Some(size) = &style.label_size {
                let (mut lo, mut hi) = (size.min, size.max);
                let a = ui.add(
                    egui::DragValue::new(&mut lo)
                        .range(5.0..=40.0)
                        .suffix(" pt"),
                );
                ui.label(tr("to"));
                let b = ui.add(
                    egui::DragValue::new(&mut hi)
                        .range(5.0..=60.0)
                        .suffix(" pt"),
                );
                if a.changed() || b.changed() {
                    actions.push(edit_layer(map, id, "Label sizes", move |l| {
                        if let Some(s) = l.kind.style_mut()
                            && let Some(size) = &mut s.label_size
                        {
                            size.min = lo;
                            size.max = hi.max(lo);
                        }
                    }));
                }
            }
        });
    }
    if let LabelMode::Template(text) = &style.labels {
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Label template")).strong());
        if let Some(new) = template_editor(ui, project, values, layer.id, text, "label-template") {
            actions.push(edit_layer(map, id, "Change label template", move |l| {
                if let Some(s) = l.kind.style_mut() {
                    s.labels = LabelMode::Template(new);
                }
            }));
        }
    }
    ui.add_space(4.0);
    let mut custom = style.tooltip.is_some();
    if ui
        .checkbox(&mut custom, tr("Own text in the hover card"))
        .on_hover_text(tr("What the card under the mouse shows for this layer"))
        .changed()
    {
        let text = custom.then(|| match &style.labels {
            LabelMode::Template(t) => t.clone(),
            _ => "{Name}".to_owned(),
        });
        actions.push(edit_layer(map, id, "Change hover text", move |l| {
            if let Some(s) = l.kind.style_mut() {
                s.tooltip = text;
            }
        }));
    }
    if let Some(text) = &style.tooltip
        && let Some(new) = template_editor(ui, project, values, layer.id, text, "tooltip-template")
    {
        actions.push(edit_layer(map, id, "Change hover text", move |l| {
            if let Some(s) = l.kind.style_mut() {
                s.tooltip = Some(new);
            }
        }));
    }
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
    if !name.is_empty() && ui.checkbox(&mut rev, tr("Reverse colours")).changed() {
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
    edges: &[f64],
    class_sets: &[territoria_config::ClassSet],
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
        ui.selectable_value(&mut mode, ScaleMode::Classified, tr("Classes"));
        ui.selectable_value(&mut mode, ScaleMode::Continuous, tr("Smooth gradient"));
        if mode != scale.mode {
            actions.push(edit("Change colouring", Box::new(move |s| s.mode = mode)));
        }
        // Classes kept by name, for other maps and projects.
        ui.menu_button(tr("Class sets"), |ui| {
            for set in class_sets {
                let s = set.scale.clone();
                if ui
                    .button(&set.name)
                    .on_hover_text(tr("Use these classes, colours and labels here"))
                    .clicked()
                {
                    actions.push(edit("Use a class set", Box::new(move |scale| *scale = s)));
                    ui.close();
                }
            }
            if !class_sets.is_empty() {
                ui.separator();
            }
            let name_id = ui.id().with("class-set-name");
            let mut name: String = ui.data_mut(|d| d.get_temp(name_id).unwrap_or_default());
            ui.label(tr("Keep these classes as"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .hint_text(tr("Name, e.g. Revenue bands"))
                        .desired_width(160.0),
                );
                if ui
                    .add_enabled(!name.trim().is_empty(), egui::Button::new(tr("Keep")))
                    .clicked()
                {
                    // Manual breaks keep the limits as they are now.
                    let mut kept = scale.clone();
                    if kept.method != Method::Manual && edges.len() > 2 {
                        kept.method = Method::Manual;
                        kept.manual = edges[1..edges.len() - 1].to_vec();
                    }
                    actions.push(Action::SaveClassSet(name.trim().to_owned(), kept));
                    name.clear();
                    ui.close();
                }
            });
            ui.data_mut(|d| d.insert_temp(name_id, name));
        })
        .response
        .on_hover_text(
            "Classes and colours kept by name, to use on other maps and in other projects",
        );
    });
    if scale.mode == ScaleMode::Classified {
        egui::Grid::new("classes")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label(tr("Method"));
                let mut method = scale.method;
                ComboBox::from_id_salt("method")
                    .width(190.0)
                    .selected_text(method.label())
                    .show_ui(ui, |ui| {
                        for m in Method::ALL {
                            ui.selectable_value(&mut method, m, m.label())
                                .on_hover_text(m.help());
                        }
                    });
                if method != scale.method {
                    // Manual starts from the limits there are now.
                    let inner: Vec<f64> = edges
                        .get(1..edges.len().saturating_sub(1))
                        .unwrap_or_default()
                        .to_vec();
                    actions.push(edit(
                        "Change classification",
                        Box::new(move |s| {
                            if method == Method::Manual {
                                s.manual = inner;
                            }
                            s.method = method;
                        }),
                    ));
                }
                ui.end_row();
                if scale.method == Method::Manual {
                    ui.label(tr("Limits"));
                    let mut text = scale
                        .manual
                        .iter()
                        .map(|v| territoria_data::table::format_number(*v))
                        .collect::<Vec<_>>()
                        .join(", ");
                    if ui
                        .add(egui::TextEdit::singleline(&mut text).desired_width(190.0))
                        .on_hover_text("Between the classes, e.g. 1000, 5000, 20000; or drag them in the histogram")
                        .changed()
                    {
                        let mut limits: Vec<f64> = text
                            .split([',', ';', ' '])
                            .filter_map(|t| t.trim().parse::<f64>().ok())
                            .collect();
                        limits.sort_by(f64::total_cmp);
                        limits.dedup();
                        actions.push(edit("Change class limits", Box::new(move |s| s.manual = limits)));
                    }
                } else {
                    ui.label(tr("Classes"));
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
                    if ui.checkbox(&mut round, tr("Round class limits")).changed() {
                        actions.push(edit(
                            "Round class limits",
                            Box::new(move |s| s.round = round),
                        ));
                    }
                }
                ui.end_row();
            });
        ui.label(RichText::new(scale.method.help()).small().weak());
    }
    ui.add_space(4.0);
    ui.label(tr("Colours"));
    if let Some(r) = ramp_picker(
        ui,
        &scale.ramp,
        &[RampKind::Sequential, RampKind::Diverging],
    ) {
        actions.push(edit("Change colours", Box::new(move |s| s.ramp = r)));
    }
    ui.horizontal(|ui| {
        let mut shown = scale.no_data.is_some();
        if ui
            .checkbox(&mut shown, tr("Areas without data"))
            .on_hover_text(
                "Fill areas that have no data in a colour of their own (otherwise they stay empty)",
            )
            .changed()
        {
            let c = shown.then_some(territoria_analysis::Rgba::rgb(0xee, 0xee, 0xee));
            actions.push(edit("Areas without data", Box::new(move |s| s.no_data = c)));
        }
        if let Some(c) = scale.no_data
            && let Some(nc) = rgba_button(ui, c)
        {
            actions.push(edit(
                "Areas without data",
                Box::new(move |s| s.no_data = Some(nc)),
            ));
        }
    });
}

/// The ways a value can be shown, for switching between them.
#[derive(Clone, Copy, PartialEq)]
enum Shown {
    Colours,
    Circles,
    Heat,
}

/// `kind` shown another way, keeping its layer and value.
fn shown_as(project: &Project, kind: &MapLayerKind, to: Shown) -> Option<MapLayerKind> {
    let (layer, source, normalize, style) = match kind {
        MapLayerKind::Choropleth {
            layer,
            source,
            normalize,
            style,
            ..
        }
        | MapLayerKind::Symbols {
            layer,
            source,
            normalize,
            style,
            ..
        } => (
            *layer,
            Some(source.clone()),
            normalize.clone(),
            style.clone(),
        ),
        MapLayerKind::Heatmap { layer, weight, .. } => (
            *layer,
            weight
                .clone()
                .map(|column| ValueSource::Attribute { column }),
            Normalize::None,
            BoundaryStyle::default(),
        ),
        _ => return None,
    };
    let l = project.layer(layer)?;
    Some(match to {
        Shown::Colours => MapLayerKind::Choropleth {
            layer,
            source: source?,
            normalize,
            scale: ColorScale::default(),
            style: BoundaryStyle {
                stroke: territoria_analysis::Rgba([0xff, 0xff, 0xff, 140]),
                stroke_width: 0.5,
                ..style
            },
        },
        Shown::Circles => MapLayerKind::Symbols {
            layer,
            source: source?,
            normalize,
            symbol: territoria_model::SymbolStyle::for_count(l.len()),
            style,
        },
        Shown::Heat => {
            if l.kind() != territoria_io::GeometryKind::Point {
                return None;
            }
            let radius_km = l.geometry.bbox.map_or(5.0, |b| {
                (territoria_geo::haversine(b.min(), b.max()) / 1000.0 / 40.0).max(0.1)
            });
            MapLayerKind::Heatmap {
                layer,
                weight: match source {
                    Some(ValueSource::Attribute { column }) => Some(column),
                    _ => None,
                },
                radius_km: (radius_km * 10.0).round() / 10.0,
                ramp: crate::heat::default_ramp(),
            }
        }
    })
}

/// Buttons to show the layer's value another way: colours, circles, heat.
fn gallery_ui(
    ui: &mut Ui,
    project: &Project,
    ml: &MapLayer,
    map: MapId,
    actions: &mut Vec<Action>,
) {
    let current = match ml.kind {
        MapLayerKind::Choropleth { .. } => Shown::Colours,
        MapLayerKind::Symbols { .. } => Shown::Circles,
        MapLayerKind::Heatmap { .. } => Shown::Heat,
        _ => return,
    };
    let id = ml.id;
    ui.horizontal(|ui| {
        ui.label(tr("Show as"));
        for (to, label, tip) in [
            (Shown::Colours, "Colours", "Each area coloured by its class"),
            (Shown::Circles, "Circles", "A circle sized by the value"),
            (
                Shown::Heat,
                "Heat map",
                "Where points are dense, weighted by the value",
            ),
        ] {
            let other = (to != current)
                .then(|| shown_as(project, &ml.kind, to))
                .flatten();
            if to != current && other.is_none() {
                continue;
            }
            if ui
                .selectable_label(to == current, label)
                .on_hover_text(tip)
                .clicked()
                && let Some(kind) = other
            {
                actions.push(edit_layer(map, id, "Show another way", move |l| {
                    l.opacity = if to == Shown::Heat { 0.85 } else { 1.0 };
                    l.kind = kind;
                }));
            }
        }
    });
}

/// At which zoom levels the layer shows: limits taken from the view.
fn zoom_ui(
    ui: &mut Ui,
    ml: &MapLayer,
    view: Option<(f64, f64)>,
    map: MapId,
    actions: &mut Vec<Action>,
) {
    let id = ml.id;
    let z = ml.zoom;
    let text = |s: f64| view.map_or("set".to_owned(), |v| crate::map_view::scale_text(s, &v));
    let summary = match (z.closest, z.farthest) {
        (None, None) => "At all zoom levels".to_owned(),
        (None, Some(f)) => format!("Closer than {}", text(f)),
        (Some(c), None) => format!("Farther than {}", text(c)),
        (Some(c), Some(f)) => format!("{} to {}", text(c), text(f)),
    };
    ui.horizontal(|ui| {
        ui.label(tr("Shown"));
        ui.menu_button(summary, |ui| {
            let Some(now) = view else {
                ui.label(RichText::new(tr("Open the map to set zoom levels.")).weak());
                return;
            };
            let here = crate::map_view::scale_text(now.0, &now);
            if ui
                .button(format!("Only when zoomed in closer than now ({here})"))
                .on_hover_text(
                    "Hide it when zoomed out farther, e.g. postcode labels on a country map",
                )
                .clicked()
            {
                let s = now.0;
                actions.push(edit_layer(map, id, "Show when zoomed in", move |l| {
                    l.zoom.farthest = Some(s);
                    if l.zoom.closest.is_some_and(|c| c >= s) {
                        l.zoom.closest = None;
                    }
                }));
                ui.close();
            }
            if ui
                .button(format!("Only when zoomed out farther than now ({here})"))
                .on_hover_text(
                    "Hide it when zoomed in closer, e.g. a summary layer that gives way to details",
                )
                .clicked()
            {
                let s = now.0;
                actions.push(edit_layer(map, id, "Show when zoomed out", move |l| {
                    l.zoom.closest = Some(s);
                    if l.zoom.farthest.is_some_and(|f| f <= s) {
                        l.zoom.farthest = None;
                    }
                }));
                ui.close();
            }
            if z.is_limited() && ui.button(tr("At all zoom levels")).clicked() {
                actions.push(edit_layer(map, id, "Show at all zoom levels", |l| {
                    l.zoom = Default::default();
                }));
                ui.close();
            }
        });
        if ml.visible && view.is_some_and(|v| !z.contains(v.0)) {
            ui.label(RichText::new(tr("hidden now")).weak());
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn map_layer(
    ui: &mut Ui,
    project: &Project,
    values: &mut Cache,
    analyses: &mut AnalysisCache,
    map: MapId,
    ml: &MapLayer,
    view: Option<(f64, f64)>,
    class_sets: &[territoria_config::ClassSet],
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
    gallery_ui(ui, project, ml, map, actions);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(tr("Opacity"));
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
    zoom_ui(ui, ml, view, map, actions);
    ui.separator();
    match &ml.kind {
        MapLayerKind::Flows {
            rows,
            from,
            to,
            value,
            style,
        } => {
            use territoria_model::flows::FlowEnd;
            let edit_flow = |label: &str, f: FlowEdit| {
                edit_layer(map, id, label, move |l| {
                    if let MapLayerKind::Flows {
                        from,
                        to,
                        value,
                        style,
                        ..
                    } = &mut l.kind
                    {
                        f(from, to, value, style);
                    }
                })
            };
            let table = rows.table(project);
            let columns: Vec<String> = table
                .map(|t| t.columns.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default();
            let lines = analyses.flows(project, id, &ml.kind);
            ui.label(
                RichText::new(format!(
                    "Rows of {} · {} lines",
                    rows.name(project),
                    lines.as_ref().map_or(0, |l| l.len())
                ))
                .weak(),
            );
            egui::Grid::new("flows")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    for (which, end) in [(0, from), (1, to)] {
                        ui.label(if which == 0 { tr("From") } else { "To" });
                        let mut chosen = end.clone();
                        ui.horizontal(|ui| {
                            let text = match end {
                                FlowEnd::Own => "The row's own place".to_owned(),
                                FlowEnd::Column { column, .. } => column.clone(),
                            };
                            ComboBox::from_id_salt(("flow-end", which))
                                .width(130.0)
                                .selected_text(text)
                                .show_ui(ui, |ui| {
                                    if matches!(rows, territoria_model::flows::FlowRows::Layer(_)) {
                                        ui.selectable_value(
                                            &mut chosen,
                                            FlowEnd::Own,
                                            tr("The row's own place"),
                                        );
                                    }
                                    let layer = match end {
                                        FlowEnd::Column { layer, .. } => Some(*layer),
                                        FlowEnd::Own => project.layers.first().map(|l| l.id),
                                    };
                                    if let Some(layer) = layer {
                                        for c in &columns {
                                            ui.selectable_value(
                                                &mut chosen,
                                                FlowEnd::Column {
                                                    column: c.clone(),
                                                    layer,
                                                },
                                                c,
                                            );
                                        }
                                    }
                                });
                            if let FlowEnd::Column { layer, .. } = end {
                                ui.label(tr("in"));
                                ComboBox::from_id_salt(("flow-layer", which))
                                    .width(110.0)
                                    .selected_text(
                                        project.layer(*layer).map_or("", |l| l.name.as_str()),
                                    )
                                    .show_ui(ui, |ui| {
                                        for l in &project.layers {
                                            if let FlowEnd::Column { layer, .. } = &mut chosen {
                                                ui.selectable_value(layer, l.id, &l.name);
                                            }
                                        }
                                    });
                            }
                        });
                        if &chosen != end {
                            actions.push(edit_flow(
                                "Change flow ends",
                                Box::new(move |from, to, _, _| {
                                    *if which == 0 { from } else { to } = chosen;
                                }),
                            ));
                        }
                        ui.end_row();
                    }
                    ui.label(tr("Widths follow"));
                    let mut v = value.clone();
                    ComboBox::from_id_salt("flow-value")
                        .width(180.0)
                        .selected_text(v.clone().unwrap_or_else(|| "Number of rows".into()))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut v, None, tr("Number of rows"));
                            if let Some(t) = table {
                                for c in t.columns.iter().filter(|c| c.kind.is_numeric()) {
                                    ui.selectable_value(&mut v, Some(c.name.clone()), &c.name);
                                }
                            }
                        });
                    if &v != value {
                        actions.push(edit_flow(
                            "Change flow widths",
                            Box::new(move |_, _, value, _| *value = v),
                        ));
                    }
                    ui.end_row();
                    ui.label(tr("Colour"));
                    if let Some(c) = rgba_button(ui, style.color) {
                        actions.push(edit_flow(
                            "Change flow colour",
                            Box::new(move |_, _, _, s| s.color = c),
                        ));
                    }
                    ui.end_row();
                    ui.label(tr("Widest"));
                    let mut w = style.max_width;
                    if ui
                        .add(egui::Slider::new(&mut w, 1.0..=40.0).suffix(" pt"))
                        .changed()
                    {
                        actions.push(edit_flow(
                            "Change flow widths",
                            Box::new(move |_, _, _, s| s.max_width = w),
                        ));
                    }
                    ui.end_row();
                    ui.label("");
                    ui.horizontal(|ui| {
                        let mut curved = style.curved;
                        if ui.checkbox(&mut curved, tr("Curved")).changed() {
                            actions.push(edit_flow(
                                "Curve flows",
                                Box::new(move |_, _, _, s| s.curved = curved),
                            ));
                        }
                        let mut arrows = style.arrows;
                        if ui.checkbox(&mut arrows, tr("Arrows")).changed() {
                            actions.push(edit_flow(
                                "Flow arrows",
                                Box::new(move |_, _, _, s| s.arrows = arrows),
                            ));
                        }
                    });
                    ui.end_row();
                });
        }
        MapLayerKind::Boundaries { layer, style } => {
            style_ui(
                ui,
                project,
                values,
                style,
                project.layer(*layer),
                true,
                map,
                id,
                actions,
            );
        }
        MapLayerKind::Bivariate {
            layer,
            x,
            y,
            scale,
            style,
        } => {
            let numbers: Vec<(String, ValueSource)> = colorables(project, *layer)
                .into_iter()
                .filter_map(|(label, c)| match c {
                    Colorable::Number(s) => Some((label, s)),
                    _ => None,
                })
                .collect();
            egui::Grid::new("bivariate")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    for (axis, current, label) in [(0, x, "Across →"), (1, y, "Up ↑")] {
                        ui.label(label).on_hover_text(if axis == 0 {
                            "Rises from left to right in the legend"
                        } else {
                            "Rises from bottom to top in the legend"
                        });
                        let mut chosen = current.clone();
                        ComboBox::from_id_salt(("bivariate-axis", axis))
                            .width(180.0)
                            .selected_text(current.label(project))
                            .show_ui(ui, |ui| {
                                for (label, s) in &numbers {
                                    ui.selectable_value(&mut chosen, s.clone(), label);
                                }
                            });
                        if &chosen != current {
                            actions.push(edit_layer(map, id, "Change value", move |l| {
                                if let MapLayerKind::Bivariate { x, y, .. } = &mut l.kind {
                                    *if axis == 0 { x } else { y } = chosen;
                                }
                            }));
                        }
                        ui.end_row();
                    }
                    ui.label("");
                    if ui
                        .button(tr("⇄ Swap"))
                        .on_hover_text(tr("Swap the two values"))
                        .clicked()
                    {
                        actions.push(edit_layer(map, id, "Swap values", |l| {
                            if let MapLayerKind::Bivariate { x, y, .. } = &mut l.kind {
                                std::mem::swap(x, y);
                            }
                        }));
                    }
                    ui.end_row();
                    ui.label(tr("Classes"));
                    ui.horizontal(|ui| {
                        for k in [2usize, 3] {
                            if ui
                                .selectable_label(scale.classes == k, format!("{k} × {k}"))
                                .on_hover_text("Each value in equal-count classes (quantiles)")
                                .clicked()
                                && scale.classes != k
                            {
                                actions.push(edit_layer(map, id, "Change classes", move |l| {
                                    if let MapLayerKind::Bivariate { scale, .. } = &mut l.kind {
                                        scale.classes = k;
                                    }
                                }));
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Colours"));
                    ui.horizontal(|ui| {
                        for p in territoria_analysis::BIVARIATE {
                            let preview = territoria_analysis::BivariateScale {
                                palette: p.name.to_owned(),
                                ..scale.clone()
                            };
                            let k = scale.classes.clamp(2, 3);
                            let cell = 18.0 / k as f32;
                            let (rect, r) =
                                ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                            let colors = preview.colors();
                            for row in 0..k {
                                for col in 0..k {
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_size(
                                            egui::pos2(
                                                rect.left() + 2.0 + col as f32 * cell,
                                                rect.bottom() - 2.0 - (row + 1) as f32 * cell,
                                            ),
                                            vec2(cell, cell),
                                        ),
                                        0.0,
                                        color(colors[row * k + col]),
                                    );
                                }
                            }
                            if scale.palette == p.name {
                                ui.painter().rect_stroke(
                                    rect,
                                    2.0,
                                    egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            if r.on_hover_text(p.name).clicked() {
                                let name = p.name.to_owned();
                                actions.push(edit_layer(map, id, "Change colours", move |l| {
                                    if let MapLayerKind::Bivariate { scale, .. } = &mut l.kind {
                                        scale.palette = name;
                                    }
                                }));
                            }
                        }
                    });
                    ui.end_row();
                });
            ui.separator();
            style_ui(
                ui,
                project,
                values,
                style,
                project.layer(*layer),
                false,
                map,
                id,
                actions,
            );
        }
        MapLayerKind::Choropleth {
            layer,
            source,
            normalize,
            scale,
            style,
        } => {
            value_ui(ui, project, *layer, source, normalize, map, id, actions);
            ui.add_space(4.0);
            let scaled = analyses.get(project, values, id, &ml.kind);
            let edges = scaled.as_ref().map_or_else(Vec::new, |s| s.edges.clone());
            scale_ui(ui, scale, &edges, class_sets, map, id, actions);
            ui.separator();
            if let Some(scaled) = scaled {
                let v = values.normalized(project, *layer, source, normalize);
                histogram(ui, v, &scaled, scale, map, id, actions);
            }
            ui.separator();
            style_ui(
                ui,
                project,
                values,
                style,
                project.layer(*layer),
                false,
                map,
                id,
                actions,
            );
        }
        MapLayerKind::Heatmap {
            layer,
            weight,
            radius_km,
            ramp,
        } => {
            let l = project.layer(*layer);
            egui::Grid::new("heat")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label(tr("Weight"));
                    let text = weight
                        .clone()
                        .unwrap_or_else(|| "Each point counts 1".into());
                    let mut w = weight.clone();
                    ComboBox::from_id_salt("heat-weight")
                        .selected_text(text)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut w, None, tr("Each point counts 1"));
                            if let Some(l) = l {
                                for c in l.attributes.columns.iter().filter(|c| c.kind.is_numeric())
                                {
                                    ui.selectable_value(&mut w, Some(c.name.clone()), &c.name);
                                }
                            }
                        });
                    if &w != weight {
                        actions.push(edit_layer(map, id, "Change heat weight", move |l| {
                            if let MapLayerKind::Heatmap { weight, .. } = &mut l.kind {
                                *weight = w;
                            }
                        }));
                    }
                    ui.end_row();
                    ui.label(tr("Spread"))
                        .on_hover_text(tr("How far each point reaches"));
                    let mut r = *radius_km;
                    if ui
                        .add(
                            egui::DragValue::new(&mut r)
                                .range(0.05..=500.0)
                                .speed(0.1)
                                .suffix(" km"),
                        )
                        .changed()
                    {
                        actions.push(edit_layer(map, id, "Change heat spread", move |l| {
                            if let MapLayerKind::Heatmap { radius_km, .. } = &mut l.kind {
                                *radius_km = r;
                            }
                        }));
                    }
                    ui.end_row();
                });
            ui.label(tr("Colours"));
            if let Some(r) = ramp_picker(ui, ramp, &[RampKind::Sequential]) {
                actions.push(edit_layer(map, id, "Change heat colours", move |l| {
                    if let MapLayerKind::Heatmap { ramp, .. } = &mut l.kind {
                        *ramp = r;
                    }
                }));
            }
        }
        MapLayerKind::Charts {
            layer,
            parts,
            chart,
            style,
        } => {
            let edit_chart = |label: &str, f: ChartEdit| {
                edit_layer(map, id, label, move |l| {
                    if let MapLayerKind::Charts { parts, chart, .. } = &mut l.kind {
                        f(parts, chart);
                    }
                })
            };
            ui.label(RichText::new(tr("Values")).strong());
            for (i, p) in parts.iter().enumerate() {
                ui.horizontal(|ui| {
                    if let Some(c) = rgba_button(ui, p.color) {
                        actions.push(edit_chart(
                            "Change chart colour",
                            Box::new(move |parts, _| {
                                if let Some(p) = parts.get_mut(i) {
                                    p.color = c;
                                }
                            }),
                        ));
                    }
                    let mut label = p.label.clone();
                    if ui
                        .add(egui::TextEdit::singleline(&mut label).desired_width(150.0))
                        .on_hover_text(p.source.label(project))
                        .changed()
                    {
                        actions.push(edit_chart(
                            "Rename chart value",
                            Box::new(move |parts, _| {
                                if let Some(p) = parts.get_mut(i) {
                                    p.label = label;
                                }
                            }),
                        ));
                    }
                    if parts.len() > 1
                        && ui
                            .small_button("🗑")
                            .on_hover_text(tr("Remove this value"))
                            .clicked()
                    {
                        actions.push(edit_chart(
                            "Remove chart value",
                            Box::new(move |parts, _| {
                                if i < parts.len() {
                                    parts.remove(i);
                                }
                            }),
                        ));
                    }
                });
            }
            let options: Vec<(String, ValueSource)> = colorables(project, *layer)
                .into_iter()
                .filter_map(|(label, c)| match c {
                    Colorable::Number(s) if !parts.iter().any(|p| p.source == s) => {
                        Some((label, s))
                    }
                    _ => None,
                })
                .collect();
            if !options.is_empty() {
                ui.menu_button(tr("➕ Add a value"), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for (label, source) in options {
                                if ui.button(&label).clicked() {
                                    let n = parts.len();
                                    actions.push(edit_chart(
                                        "Add chart value",
                                        Box::new(move |parts, _| {
                                            parts.push(territoria_model::ChartPart {
                                                label,
                                                source,
                                                color: territoria_model::ChartPart::color_for(n),
                                            });
                                        }),
                                    ));
                                    ui.close();
                                }
                            }
                        });
                });
            }
            ui.add_space(4.0);
            egui::Grid::new("chart")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label(tr("Chart"));
                    ui.horizontal(|ui| {
                        for shape in territoria_model::ChartShape::ALL {
                            if ui
                                .selectable_label(chart.shape == shape, shape.label())
                                .clicked()
                                && chart.shape != shape
                            {
                                actions.push(edit_chart("Change chart type", Box::new(move |_, c| {
                                    c.shape = shape;
                                })));
                            }
                        }
                    });
                    ui.end_row();
                    let bars = chart.shape.is_bars();
                    ui.label(if bars { tr("Tallest") } else { "Largest" }).on_hover_text(if bars {
                        "Height of the bar for the largest value"
                    } else {
                        "Radius of the largest pie"
                    });
                    let mut r = chart.max_size;
                    if ui
                        .add(egui::Slider::new(&mut r, 4.0..=80.0).suffix(" pt"))
                        .changed()
                    {
                        actions.push(edit_chart("Change chart size", Box::new(move |_, c| {
                            c.max_size = r;
                        })));
                    }
                    ui.end_row();
                    if !bars {
                        ui.label("");
                        let mut sized = chart.sized;
                        if ui
                            .checkbox(&mut sized, tr("Sized by the total"))
                            .on_hover_text("Pie areas grow with the sum of the values; else all pies are alike")
                            .changed()
                        {
                            actions.push(edit_chart("Change chart sizes", Box::new(move |_, c| {
                                c.sized = sized;
                            })));
                        }
                        ui.end_row();
                    }
                    ui.label(tr("Outline"));
                    if let Some(o) = rgba_button(ui, chart.outline) {
                        actions.push(edit_chart("Change chart outline", Box::new(move |_, c| {
                            c.outline = o;
                        })));
                    }
                    ui.end_row();
                });
            ui.separator();
            style_ui(
                ui,
                project,
                values,
                style,
                project.layer(*layer),
                false,
                map,
                id,
                actions,
            );
        }
        MapLayerKind::Rings {
            centres,
            radii_km,
            color: c,
        } => {
            ui.label(RichText::new(format!("Around the {}", centres.label(project))).weak());
            egui::Grid::new("rings")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label(tr("Radii"));
                    let text: Vec<String> = radii_km
                        .iter()
                        .map(|r| territoria_data::table::format_number(*r))
                        .collect();
                    let mut t = text.join(", ");
                    if ui
                        .add(egui::TextEdit::singleline(&mut t).desired_width(160.0))
                        .on_hover_text("Kilometres, e.g. 5, 10, 20")
                        .changed()
                    {
                        let radii = territoria_model::catchment::parse_radii(&t);
                        if !radii.is_empty() {
                            actions.push(edit_layer(map, id, "Change radii", move |l| {
                                if let MapLayerKind::Rings { radii_km, .. } = &mut l.kind {
                                    *radii_km = radii;
                                }
                            }));
                        }
                    }
                    ui.label("km");
                    ui.end_row();
                    ui.label(tr("Colour"));
                    if let Some(nc) = rgba_button(ui, *c) {
                        actions.push(edit_layer(map, id, "Change ring colour", move |l| {
                            if let MapLayerKind::Rings { color, .. } = &mut l.kind {
                                *color = nc;
                            }
                        }));
                    }
                    ui.end_row();
                });
            if ui.button(tr("Figures within the rings…")).clicked() {
                actions.push(Action::Command(crate::commands::Command::Catchment));
            }
        }
        MapLayerKind::Symbols {
            layer,
            source,
            normalize,
            symbol,
            style,
        } => {
            value_ui(ui, project, *layer, source, normalize, map, id, actions);
            ui.add_space(4.0);
            egui::Grid::new("symbol")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label(tr("Colour"));
                    ui.horizontal(|ui| {
                        if symbol.color_by.is_none()
                            && let Some(c) = rgba_button(ui, symbol.color)
                        {
                            actions.push(edit_layer(map, id, "Change circle colour", move |l| {
                                if let MapLayerKind::Symbols { symbol, .. } = &mut l.kind {
                                    symbol.color = c;
                                }
                            }));
                        }
                        // Or a colour per category of a text column.
                        let categories: Vec<(String, TextSource)> = colorables(project, *layer)
                            .into_iter()
                            .filter_map(|(label, c)| match c {
                                Colorable::Category(s) => Some((label, s)),
                                _ => None,
                            })
                            .collect();
                        let current = symbol.color_by.as_ref().map(|(s, _)| s.clone());
                        let mut chosen = current.clone();
                        let text = current
                            .as_ref()
                            .and_then(|s| categories.iter().find(|(_, c)| c == s))
                            .map_or("One colour".to_owned(), |(l, _)| format!("By {l}"));
                        ComboBox::from_id_salt("symbol-color-by")
                            .width(170.0)
                            .selected_text(text)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut chosen, None, tr("One colour"));
                                for (label, s) in &categories {
                                    ui.selectable_value(&mut chosen, Some(s.clone()), format!("By {label}"));
                                }
                            });
                        if chosen != current {
                            actions.push(edit_layer(map, id, "Colour circles by category", move |l| {
                                if let MapLayerKind::Symbols { symbol, .. } = &mut l.kind {
                                    symbol.color_by =
                                        chosen.map(|s| (s, territoria_analysis::CategoryScale::default()));
                                }
                            }));
                        }
                    });
                    ui.end_row();
                    if let Some((_, scale)) = &symbol.color_by {
                        ui.label(tr("Colours"));
                        if let Some(r) = ramp_picker(ui, &scale.ramp, &[RampKind::Qualitative]) {
                            actions.push(edit_layer(map, id, "Change colours", move |l| {
                                if let MapLayerKind::Symbols { symbol, .. } = &mut l.kind
                                    && let Some((_, scale)) = &mut symbol.color_by
                                {
                                    scale.ramp = r;
                                }
                            }));
                        }
                        ui.end_row();
                    }
                    ui.label(tr("Outline"));
                    if let Some(c) = rgba_button(ui, symbol.outline) {
                        actions.push(edit_layer(map, id, "Change circle outline", move |l| {
                            if let MapLayerKind::Symbols { symbol, .. } = &mut l.kind {
                                symbol.outline = c;
                            }
                        }));
                    }
                    ui.end_row();
                    ui.label(tr("Largest"))
                        .on_hover_text("Radius of the circle for the largest value; circle areas grow with the value");
                    let mut r = symbol.max_radius;
                    if ui
                        .add(egui::Slider::new(&mut r, 4.0..=80.0).suffix(" pt"))
                        .changed()
                    {
                        actions.push(edit_layer(map, id, "Change circle size", move |l| {
                            if let MapLayerKind::Symbols { symbol, .. } = &mut l.kind {
                                symbol.max_radius = r;
                            }
                        }));
                    }
                    ui.end_row();
                    ui.label(tr("Shape"));
                    ui.horizontal(|ui| {
                        for shape in territoria_model::SymbolShape::ALL {
                            if ui
                                .selectable_label(symbol.shape == shape, shape.label())
                                .clicked()
                                && symbol.shape != shape
                            {
                                actions.push(edit_layer(map, id, "Change symbol shape", move |l| {
                                    if let MapLayerKind::Symbols { symbol, .. } = &mut l.kind {
                                        symbol.shape = shape;
                                    }
                                }));
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Sizes"));
                    let mut scaling = symbol.scaling;
                    ComboBox::from_id_salt("symbol-scaling")
                        .width(190.0)
                        .selected_text(scaling.label())
                        .show_ui(ui, |ui| {
                            for s in territoria_model::SymbolScaling::ALL {
                                ui.selectable_value(&mut scaling, s, s.label());
                            }
                        })
                        .response
                        .on_hover_text("By area is true to the data; by width makes differences look bigger; logarithmic suits values over many magnitudes");
                    if scaling != symbol.scaling {
                        actions.push(edit_layer(map, id, "Change symbol sizes", move |l| {
                            if let MapLayerKind::Symbols { symbol, .. } = &mut l.kind {
                                symbol.scaling = scaling;
                            }
                        }));
                    }
                    ui.end_row();
                });
            ui.separator();
            style_ui(
                ui,
                project,
                values,
                style,
                project.layer(*layer),
                false,
                map,
                id,
                actions,
            );
        }
        MapLayerKind::Categories {
            layer,
            scale,
            style,
            ..
        } => {
            ui.label(tr("Colours"));
            if let Some(r) = ramp_picker(ui, &scale.ramp, &[RampKind::Qualitative]) {
                actions.push(edit_layer(map, id, "Change colours", move |l| {
                    if let MapLayerKind::Categories { scale, .. } = &mut l.kind {
                        scale.ramp = r;
                    }
                }));
            }
            ui.horizontal(|ui| {
                ui.label(tr("Most frequent"));
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
            style_ui(
                ui,
                project,
                values,
                style,
                project.layer(*layer),
                false,
                map,
                id,
                actions,
            );
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
                ui.label(tr("Fill"));
                let mut o = style.fill_opacity;
                if ui
                    .add(egui::Slider::new(&mut o, 0.0..=1.0).show_value(false))
                    .changed()
                {
                    actions.push(edit("Change fill", Box::new(move |s| s.fill_opacity = o)));
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr("Territory borders"));
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
            if project.plan(*plan).is_some_and(|p| !p.regions.is_empty()) {
                ui.horizontal(|ui| {
                    ui.label(tr("Region borders"));
                    let mut w = style.region_border_width;
                    if ui
                        .add(
                            egui::DragValue::new(&mut w)
                                .range(0.0..=10.0)
                                .speed(0.05)
                                .suffix(" pt"),
                        )
                        .changed()
                    {
                        actions.push(edit(
                            "Change region borders",
                            Box::new(move |s| s.region_border_width = w),
                        ));
                    }
                });
            }
            let mut ub = style.unit_borders;
            if ui
                .checkbox(&mut ub, tr("Show area borders inside territories"))
                .changed()
            {
                actions.push(edit("Area borders", Box::new(move |s| s.unit_borders = ub)));
            }
            let mut lb = style.labels;
            if ui.checkbox(&mut lb, tr("Territory names")).changed() {
                actions.push(edit("Territory names", Box::new(move |s| s.labels = lb)));
            }
            ui.add_space(6.0);
            if ui.button(tr("Open the Territories panel")).clicked() {
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
    values: Vec<Option<f64>>,
    scaled: &territoria_analysis::Scaled,
    scale: &ColorScale,
    map: MapId,
    id: MapLayerId,
    actions: &mut Vec<Action>,
) {
    let (edges, legend) = (&scaled.edges, &scaled.legend);
    let v: Vec<f64> = values
        .into_iter()
        .flatten()
        .filter(|x| x.is_finite())
        .collect();
    if v.is_empty() || edges.len() < 2 {
        ui.label(RichText::new(tr("No values to show: check the link to the data.")).weak());
        return;
    }
    let edit = |label: &str, f: Box<dyn FnOnce(&mut ColorScale)>| {
        edit_layer(map, id, label, move |l| {
            if let MapLayerKind::Choropleth { scale, .. } = &mut l.kind {
                f(scale);
            }
        })
    };
    let (min, max) = (edges[0], edges[edges.len() - 1]);
    let span = (max - min).max(1e-12);
    let bins = 40;
    let mut counts = vec![0usize; bins];
    for x in &v {
        let i = (((x - min) / span) * bins as f64).floor() as isize;
        counts[i.clamp(0, bins as isize - 1) as usize] += 1;
    }
    let peak = *counts.iter().max().unwrap_or(&1) as f32;
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().min(300.0), 64.0), Sense::hover());
    let w = rect.width() / bins as f32;
    for (i, c) in counts.iter().enumerate() {
        let mid = min + (i as f64 + 0.5) / bins as f64 * span;
        let class = territoria_analysis::classify::class_of(mid, edges).unwrap_or(0);
        let col = legend
            .get(class)
            .map_or(ui.visuals().weak_text_color(), |e| color(e.color));
        let h = (*c as f32 / peak) * rect.height();
        let r = egui::Rect::from_min_max(
            egui::pos2(rect.left() + i as f32 * w, rect.bottom() - h),
            egui::pos2(rect.left() + (i + 1) as f32 * w - 1.0, rect.bottom()),
        );
        ui.painter().rect_filled(r, 0.0, col);
    }
    // Class limits: drag them to set them by hand.
    let inner: Vec<f64> = edges[1..edges.len() - 1].to_vec();
    if scale.mode == territoria_analysis::ScaleMode::Classified {
        for (k, e) in inner.iter().enumerate() {
            let x = rect.left() + ((e - min) / span) as f32 * rect.width();
            let handle = egui::Rect::from_center_size(
                egui::pos2(x, rect.center().y),
                vec2(9.0, rect.height()),
            );
            let r = ui.interact(handle, ui.id().with(("class-limit", k)), Sense::drag());
            let hot = r.hovered() || r.dragged();
            if hot {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            ui.painter().line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(if hot { 2.5 } else { 1.0 }, ui.visuals().text_color()),
            );
            let r = r.on_hover_text(format!("{} · drag to move", format_value(*e)));
            if r.dragged()
                && let Some(p) = r.interact_pointer_pos()
            {
                let raw = min + ((p.x - rect.left()) / rect.width()) as f64 * span;
                // Round to a step of about a 200th of the range.
                let step = 10f64.powf((span / 200.0).log10().floor());
                let lo = if k == 0 { min } else { inner[k - 1] };
                let hi = inner.get(k + 1).copied().unwrap_or(max);
                let value = ((raw / step).round() * step).clamp(lo + step, hi - step);
                let mut manual = inner.clone();
                manual[k] = value;
                actions.push(edit(
                    "Move class limit",
                    Box::new(move |s| {
                        s.method = territoria_analysis::classify::Method::Manual;
                        s.manual = manual;
                    }),
                ));
            }
        }
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(format_value(min)).small());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format_value(max)).small())
        });
    });
    let classes = edges.len() - 1;
    for (c, e) in legend.iter().enumerate() {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
            ui.painter().rect_filled(r.shrink(1.0), 3.0, color(e.color));
            if c < classes && scale.mode == territoria_analysis::ScaleMode::Classified {
                // Own labels; empty shows the limits.
                let auto = format!(
                    "{} – {}",
                    format_value(edges[c]),
                    format_value(edges[c + 1])
                );
                let mut text = scale.labels.get(c).cloned().unwrap_or_default();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut text)
                            .hint_text(auto)
                            .desired_width(150.0),
                    )
                    .on_hover_text("A label of your own for the legend; empty shows the limits")
                    .changed()
                {
                    actions.push(edit(
                        "Change class label",
                        Box::new(move |s| {
                            if s.labels.len() < classes {
                                s.labels.resize(classes, String::new());
                            }
                            s.labels[c] = text;
                        }),
                    ));
                }
            } else {
                ui.label(&e.label);
            }
            ui.label(RichText::new(format!("{} areas", e.count)).weak());
        });
    }
}

/// The value of an analysis: which number, how per area, relative to what.
/// The data a value source adds up rows of.
fn rows_data(project: &Project, source: &ValueSource) -> Option<DataRef> {
    match source {
        ValueSource::Linked { link, .. } | ValueSource::RowCount { link } => {
            project.link(*link).map(|l| DataRef::Table(l.table))
        }
        ValueSource::Points { layer, .. } => Some(DataRef::Layer(*layer)),
        ValueSource::Filtered { source, .. } => rows_data(project, source),
        _ => None,
    }
}

/// Segments of the rows (their yes/no columns) to filter by, and saving
/// the typed filter as one.
fn segments_menu(
    ui: &mut Ui,
    project: &Project,
    source: &ValueSource,
    table: &territoria_data::Table,
    text: &str,
    set_filter: &dyn Fn(String) -> Action,
    actions: &mut Vec<Action>,
) {
    let segments: Vec<&str> = table
        .columns
        .iter()
        .filter(|c| c.kind == ColumnKind::Bool)
        .map(|c| c.name.as_str())
        .collect();
    // A filter worth keeping: a valid yes/no formula, not just a segment.
    let savable = !text.trim().is_empty()
        && !segments.iter().any(|s| text.trim() == format!("[{s}]"))
        && territoria_expr::Formula::compile(text, table)
            .is_ok_and(|f| f.ty() == territoria_expr::Type::Bool);
    let data = rows_data(project, source);
    ui.menu_button(tr("Segments"), |ui| {
        for s in &segments {
            if ui.button(*s).clicked() {
                actions.push(set_filter(format!("[{s}]")));
                ui.close();
            }
        }
        if segments.is_empty() {
            ui.label(
                RichText::new(
                    "Segments are yes/no columns of the data; type a filter and keep it as one.",
                )
                .weak(),
            );
        }
        if savable && let Some(data) = data {
            ui.separator();
            let name_id = ui.id().with("segment-name");
            let mut name: String = ui.data_mut(|d| d.get_temp(name_id).unwrap_or_default());
            ui.label(tr("Keep this filter as a segment"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .hint_text(tr("Name, e.g. Key accounts"))
                        .desired_width(160.0),
                );
                let ok = !name.trim().is_empty() && table.column(name.trim()).is_none();
                if ui.add_enabled(ok, egui::Button::new(tr("Keep"))).clicked() {
                    actions.push(Action::SaveSegment {
                        data,
                        name: name.trim().to_owned(),
                        formula: text.trim().to_owned(),
                    });
                    actions.push(set_filter(format!("[{}]", name.trim())));
                    name.clear();
                    ui.close();
                }
            });
            ui.data_mut(|d| d.insert_temp(name_id, name));
        }
    })
    .response
    .on_hover_text("Named filters (yes/no columns) to reuse in tables, analyses and exports");
}

#[allow(clippy::too_many_arguments)]
fn value_ui(
    ui: &mut Ui,
    project: &Project,
    layer: LayerId,
    source: &ValueSource,
    normalize: &Normalize,
    map: MapId,
    id: MapLayerId,
    actions: &mut Vec<Action>,
) {
    let options = colorables(project, layer);
    // The filter is kept apart: values, aggregation and relative-to look
    // at the source without it.
    let (source, filter) = source.split();
    ui.horizontal(|ui| {
        ui.label(tr("Value"));
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
                        // And the filter, when the rows are the same.
                        if let Some(f) = filter
                            && let (Some(a), Some(b)) =
                                (s.rows_table(project), source.rows_table(project))
                            && std::ptr::eq(a, b)
                        {
                            s = ValueSource::filtered(s, f);
                        }
                        let label = format!("{label}{}", normalize.suffix(project));
                        actions.push(edit_layer(map, id, "Change value", move |l| {
                            if let MapLayerKind::Choropleth { source, .. }
                            | MapLayerKind::Symbols { source, .. } = &mut l.kind
                            {
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
            ui.label(tr("Per area"));
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
                    if let MapLayerKind::Choropleth { source, .. }
                    | MapLayerKind::Symbols { source, .. } = &mut l.kind
                        && let Some(agg) = source.agg_mut()
                    {
                        *agg = a;
                    }
                }));
            }
        });
    }
    // A segment: only the rows a formula holds for.
    if let Some(table) = source.rows_table(project) {
        let mut text = filter.unwrap_or_default().to_owned();
        // The filter as an undoable change that also renames the layer.
        let set_filter = |t: String| {
            let inner = source.clone();
            let base = options
                .iter()
                .find(|(_, c)| matches!(c, Colorable::Number(s) if same_source(s, &inner)))
                .map_or_else(|| inner.label(project), |(l, _)| l.clone());
            let label = if t.trim().is_empty() {
                format!("{base}{}", normalize.suffix(project))
            } else {
                format!("{base} where {}{}", t.trim(), normalize.suffix(project))
            };
            edit_layer(map, id, "Change filter", move |l| {
                if let MapLayerKind::Choropleth { source, .. }
                | MapLayerKind::Symbols { source, .. } = &mut l.kind
                {
                    *source = ValueSource::filtered(inner, &t);
                }
                l.name = label;
            })
        };
        ui.horizontal(|ui| {
            ui.label(tr("Only where")).on_hover_text(
                "Count or add up only some rows, e.g. [Segment] = \"Retail\" or [Orders] > 10",
            );
            let changed = ui
                .add(
                    egui::TextEdit::singleline(&mut text)
                        .hint_text(tr("all rows"))
                        .font(egui::TextStyle::Monospace)
                        .desired_width(200.0),
                )
                .changed();
            if changed {
                actions.push(set_filter(text.clone()));
            }
            segments_menu(ui, project, source, table, &text, &set_filter, actions);
        });
        if !text.trim().is_empty() {
            match territoria_expr::Formula::compile(&text, table) {
                Ok(f) if f.ty() == territoria_expr::Type::Bool => {
                    let n = f
                        .evaluate(table)
                        .iter()
                        .filter(|v| matches!(v, territoria_expr::Val::Bool(true)))
                        .count();
                    ui.label(
                        RichText::new(format!(
                            "{} of {} rows",
                            format_value(n as f64),
                            format_value(table.rows() as f64)
                        ))
                        .weak()
                        .small(),
                    );
                }
                Ok(_) => {
                    ui.label(
                        RichText::new("⚠ A filter is a yes/no question, e.g. [Sales] > 1000")
                            .color(crate::theme::WARN)
                            .small(),
                    );
                }
                Err(e) => {
                    ui.label(
                        RichText::new(format!("⚠ {}", e.message))
                            .color(crate::theme::WARN)
                            .small(),
                    );
                }
            }
        }
    }

    // Relative to: area, another value, the total.
    ui.horizontal(|ui| {
            ui.label(tr("Relative to")).on_hover_text("Sums grow with the size of areas: per km² or per another value (customers, population) makes areas comparable");
            let base = options
                .iter()
                .find(|(_, c)| matches!(c, Colorable::Number(s) if same_source(s, source)))
                .map_or_else(|| source.label(project), |(l, _)| l.clone());
            let text = match normalize {
                Normalize::None => "—".to_owned(),
                Normalize::PerArea => "per km²".to_owned(),
                Normalize::Percent => "% of the total".to_owned(),
                Normalize::By(s) => format!("per {}", s.label(project)),
            };
            let mut choice: Option<Normalize> = None;
            ComboBox::from_id_salt("normalize")
                .width(200.0)
                .selected_text(text)
                .show_ui(ui, |ui| {
                    for (n, label) in [
                        (Normalize::None, "— (as they are)".to_owned()),
                        (Normalize::PerArea, "per km²".to_owned()),
                        (Normalize::Percent, "% of the total".to_owned()),
                    ] {
                        if ui.selectable_label(*normalize == n, label).clicked() {
                            choice = Some(n);
                        }
                    }
                    ui.separator();
                    for (label, c) in &options {
                        if let Colorable::Number(s) = c
                            && !same_source(s, source)
                            && *s != ValueSource::Area
                        {
                            let n = Normalize::By(s.clone());
                            if ui
                                .selectable_label(*normalize == n, format!("per {label}"))
                                .clicked()
                            {
                                choice = Some(n);
                            }
                        }
                    }
                });
            if let Some(n) = choice.filter(|n| n != normalize) {
                let name = format!("{base}{}", n.suffix(project));
                actions.push(edit_layer(map, id, "Change relative to", move |l| {
                    if let MapLayerKind::Choropleth { normalize, .. } | MapLayerKind::Symbols { normalize, .. } = &mut l.kind {
                        *normalize = n;
                    }
                    l.name = name;
                }));
            }
        });
}
