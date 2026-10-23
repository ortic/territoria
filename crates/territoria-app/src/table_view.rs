//! A table tab: a layer's attributes or a dataset, sortable and searchable,
//! with each row's territory in the plans it relates to.

use egui::{RichText, Sense, Ui, vec2};
use egui_extras::{Column as TableColumn, TableBuilder};
use territoria_analysis::format::format_value;
use territoria_data::Table;
use territoria_model::{Cache, LayerId, Plan, Project, RowUnits};

use crate::app::{Action, DataRef, Export};

#[derive(Clone, Default)]
struct TableState {
    sort: Option<(usize, bool)>,
    filter: String,
    /// Row order for `key`.
    order: Vec<usize>,
    key: Option<OrderKey>,
    /// Show only the rows of the map's selection.
    only_selected: bool,
    /// Show only the rows of a segment (a yes/no column).
    segment: Option<String>,
    /// The cell being edited: row, column, text.
    editing: Option<(usize, usize, String)>,
}

/// What the row order depends on: revision, sort, filter, plan revisions,
/// selection, segment.
type OrderKey = (
    u64,
    Option<(usize, bool)>,
    String,
    Vec<u64>,
    Option<u64>,
    Option<String>,
);

pub fn show(
    ui: &mut Ui,
    project: &Project,
    values: &mut Cache,
    data: DataRef,
    selected: Option<(LayerId, &[u32])>,
) -> Vec<Action> {
    let mut actions = Vec::new();
    let (table, rev, keys): (&Table, u64, Option<&str>) = match data {
        DataRef::Layer(id) => match project.layer(id) {
            Some(l) => (&l.attributes, l.rev, l.key_column.as_deref()),
            None => {
                ui.label("This layer was removed.");
                return actions;
            }
        },
        DataRef::Table(id) => match project.table(id) {
            Some(t) => (&t.table, t.rev, None),
            None => {
                ui.label("This table was removed.");
                return actions;
            }
        },
    };
    // Plans whose territories the rows belong to: shown as live columns.
    let plans: Vec<(&Plan, RowUnits)> = project
        .plans
        .iter()
        .filter_map(|p| Some((p, crate::app::plan_rows(project, values, data, p)?)))
        .collect();
    let territory = |i: usize, r: usize| plans[i].0.row_territory(&plans[i].1, r);
    // Rows that belong to the features selected on the map.
    let selection: Option<(RowUnits, std::collections::HashSet<u32>)> =
        selected.and_then(|(layer, units)| {
            let rows = match data {
                DataRef::Layer(l) => project.layer_rows(values, l, layer),
                DataRef::Table(t) => project.table_rows(values, t, layer),
            }?;
            Some((rows, units.iter().copied().collect()))
        });
    let in_selection = |r: usize| {
        selection
            .as_ref()
            .is_some_and(|(rows, set)| rows.unit(r).is_some_and(|u| set.contains(&u)))
    };
    let selection_key = selection.as_ref().map(|(_, set)| {
        use std::hash::{Hash, Hasher};
        let mut h = std::hash::DefaultHasher::new();
        let mut units: Vec<&u32> = set.iter().collect();
        units.sort_unstable();
        units.hash(&mut h);
        h.finish()
    });
    let id = ui.id().with(("table-state", data));
    let mut state: TableState = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());

    ui.horizontal(|ui| {
        ui.label("🔍");
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .hint_text("Find in all columns")
                .desired_width(220.0),
        );
        // Segments: the table's yes/no columns, e.g. a calculated [Key account].
        let segments: Vec<&str> = table
            .columns
            .iter()
            .filter(|c| c.kind == territoria_data::ColumnKind::Bool)
            .map(|c| c.name.as_str())
            .collect();
        if state.segment.as_deref().is_some_and(|s| !segments.contains(&s)) {
            state.segment = None;
        }
        egui::ComboBox::from_id_salt(("table-segment", data))
            .width(130.0)
            .selected_text(state.segment.clone().unwrap_or_else(|| "All rows".into()))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.segment, None, "All rows");
                for s in &segments {
                    ui.selectable_value(&mut state.segment, Some(s.to_string()), *s);
                }
                ui.separator();
                if ui
                    .button("New segment…")
                    .on_hover_text("A yes/no calculated column, e.g. [Sales] > 10000: it filters tables and analyses")
                    .clicked()
                {
                    actions.push(Action::Formula(data, None));
                }
            })
            .response
            .on_hover_text("Only the rows of a segment: a yes/no column");
        ui.label(RichText::new(format!("{} rows", format_value(state.order.len() as f64))).weak());
        if selection.is_some() {
            ui.checkbox(&mut state.only_selected, "Only the selection")
                .on_hover_text("Rows of the areas selected on the map");
        }
    });
    // What can be done with the table, on a row of its own.
    ui.horizontal_wrapped(|ui| {
            if ui
                .button("ƒ Calculated column…")
                .on_hover_text("A new column computed from the others with a formula")
                .clicked()
            {
                actions.push(Action::Formula(data, None));
            }
            if ui
                .button("⊕ Columns from…")
                .on_hover_text("Values from the map (the area a point lies in, points in an area, the nearest point) or another table by key")
                .clicked()
            {
                actions.push(Action::Join(data));
            }
            if ui
                .button("Σ Summarise…")
                .on_hover_text("A new table with one row per group, e.g. sales per rep")
                .clicked()
            {
                actions.push(Action::Summarise(data));
            }
            if ui
                .button("⚖ Score…")
                .on_hover_text("A weighted mix of several figures, as a calculated column")
                .clicked()
            {
                actions.push(Action::Score(data));
            }
            if matches!(data, DataRef::Table(_))
                && ui.button("➕ Row").on_hover_text("An empty row at the end; double-click cells to fill them").clicked()
            {
                actions.push(Action::AddRow(data));
            }
            ui.menu_button("Export", |ui| {
                if ui.button("Excel…").clicked() {
                    actions.push(Action::Export(Export::Data(data, true, state.segment.clone())));
                }
                if ui.button("CSV…").clicked() {
                    actions.push(Action::Export(Export::Data(data, false, state.segment.clone())));
                }
            })
            .response
            .on_hover_text("This table with each row's territories (only the segment's rows when one is chosen)");
    });

    let plan_revs = plans.iter().map(|(p, _)| p.rev).collect();
    let only = state.only_selected && selection.is_some();
    let key = Some((
        rev,
        state.sort,
        state.filter.clone(),
        plan_revs,
        selection_key.filter(|_| only),
        state.segment.clone(),
    ));
    if state.key != key {
        let needle = state.filter.to_lowercase();
        let segment = state.segment.as_deref().and_then(|s| table.column(s));
        let mut order: Vec<usize> = (0..table.rows())
            .filter(|&r| !only || in_selection(r))
            .filter(|&r| {
                segment
                    .is_none_or(|c| matches!(c.get(r), territoria_data::table::Value::Bool(true)))
            })
            .filter(|&r| {
                needle.is_empty()
                    || table
                        .columns
                        .iter()
                        .any(|c| c.display(r).to_lowercase().contains(&needle))
                    || (0..plans.len()).any(|i| {
                        territory(i, r).is_some_and(|t| t.name.to_lowercase().contains(&needle))
                    })
            })
            .collect();
        let columns = table.columns.len();
        if let Some((col, asc)) = state.sort
            && col >= columns
            && col - columns < plans.len()
        {
            order.sort_by_key(|&r| territory(col - columns, r).map(|t| t.name.to_lowercase()));
            if !asc {
                order.reverse();
            }
        } else if let Some((col, asc)) = state.sort
            && let Some(c) = table.columns.get(col)
        {
            if c.kind.is_numeric() {
                order.sort_by(|&a, &b| {
                    let (x, y) = (c.f64(a), c.f64(b));
                    x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
                });
            } else {
                order.sort_by_key(|&r| c.display(r).to_lowercase());
            }
            if !asc {
                order.reverse();
            }
        }
        state.order = order;
        state.key = key;
    }

    let text_height = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
    let ui_modifiers = ui.input(|i| i.modifiers);
    egui::ScrollArea::horizontal().show(ui, |ui| {
        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .sense(Sense::click())
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        for _ in 0..table.columns.len() + plans.len() {
            builder = builder.column(TableColumn::auto().at_least(60.0).clip(true));
        }
        builder
            .header(text_height + 6.0, |mut header| {
                for (i, c) in table.columns.iter().enumerate() {
                    header.col(|ui| {
                        let arrow = match state.sort {
                            Some((s, true)) if s == i => " ⏶",
                            Some((s, false)) if s == i => " ⏷",
                            _ => "",
                        };
                        let key_mark = if keys == Some(c.name.as_str()) {
                            "🔑 "
                        } else if c.formula.is_some() {
                            "ƒ "
                        } else {
                            ""
                        };
                        let r = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("{key_mark}{}{arrow}", c.name)).strong(),
                                )
                                .frame(false)
                                .sense(Sense::click_and_drag()),
                            )
                            .on_hover_text(match &c.formula {
                                Some(f) => format!(
                                    "{} · = {f}\nClick to sort, right-click to edit",
                                    c.kind.label()
                                ),
                                None => format!(
                                    "{} · click to sort, drag onto the map to show it",
                                    c.kind.label()
                                ),
                            });
                        r.dnd_set_drag_payload(crate::app::ColumnDrag {
                            data,
                            column: c.name.clone(),
                        });
                        if c.formula.is_some() {
                            r.context_menu(|ui| {
                                if ui.button("✏ Edit formula…").clicked() {
                                    actions.push(Action::Formula(data, Some(c.name.clone())));
                                }
                                if ui.button("🗑 Delete column").clicked() {
                                    actions.push(Action::DeleteColumn(data, c.name.clone()));
                                }
                            });
                        }
                        if r.clicked() {
                            state.sort = match state.sort {
                                Some((s, true)) if s == i => Some((i, false)),
                                Some((s, false)) if s == i => None,
                                _ => Some((i, true)),
                            };
                        }
                    });
                }
                for (i, (plan, _)) in plans.iter().enumerate() {
                    let col = table.columns.len() + i;
                    header.col(|ui| {
                        let arrow = match state.sort {
                            Some((s, true)) if s == col => " ⏶",
                            Some((s, false)) if s == col => " ⏷",
                            _ => "",
                        };
                        let r = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("⬣ {}{arrow}", plan.name)).strong(),
                                )
                                .frame(false),
                            )
                            .on_hover_text(format!(
                                "Territory in “{}”: follows the plan as you change it · click to sort",
                                plan.name
                            ));
                        if r.clicked() {
                            state.sort = match state.sort {
                                Some((s, true)) if s == col => Some((col, false)),
                                Some((s, false)) if s == col => None,
                                _ => Some((col, true)),
                            };
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(text_height, state.order.len(), |mut row| {
                    let r = state.order[row.index()];
                    row.set_selected(in_selection(r));
                    for (ci, c) in table.columns.iter().enumerate() {
                        row.col(|ui| {
                            // Editing this cell: Enter keeps, Esc drops.
                            if let Some((er, ec, text)) = &mut state.editing
                                && *er == r
                                && *ec == ci
                            {
                                let resp = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                                resp.request_focus();
                                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    state.editing = None;
                                } else if resp.lost_focus() {
                                    actions.push(Action::EditCell(data, r, c.name.clone(), text.clone()));
                                    state.editing = None;
                                }
                                return;
                            }
                            let text = c.display(r);
                            let shown = match c.f64(r) {
                                Some(v) if c.kind == territoria_data::ColumnKind::Number => format_value(v),
                                _ => text.clone(),
                            };
                            let label = egui::Label::new(shown).sense(Sense::click());
                            let resp = if c.kind.is_numeric() {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.add(label))
                                    .inner
                            } else {
                                ui.add(label)
                            };
                            if resp.double_clicked() && c.formula.is_none() {
                                state.editing = Some((r, ci, text));
                            }
                        });
                    }
                    for i in 0..plans.len() {
                        row.col(|ui| {
                            if let Some(t) = territory(i, r) {
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                                ui.painter().rect_filled(
                                    rect,
                                    2.0,
                                    crate::map_view::color(t.color),
                                );
                                ui.label(&t.name);
                            }
                        });
                    }
                    let resp = row.response();
                    if resp.clicked() {
                        let add = ui_modifiers.shift || ui_modifiers.command;
                        actions.push(Action::SelectRows(data, vec![r], add));
                    }
                    if matches!(data, DataRef::Table(_)) {
                        resp.context_menu(|ui| {
                            if ui.button("🗑 Delete row").clicked() {
                                actions.push(Action::DeleteRows(data, vec![r]));
                                ui.close();
                            }
                        });
                    }
                });
            });
    });
    ui.data_mut(|d| d.insert_temp(id, state));
    actions
}
