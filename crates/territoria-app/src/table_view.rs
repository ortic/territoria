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
}

/// What the row order depends on: revision, sort, filter, plan revisions,
/// selection.
type OrderKey = (u64, Option<(usize, bool)>, String, Vec<u64>, Option<u64>);

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
        ui.label(RichText::new(format!("{} rows", format_value(state.order.len() as f64))).weak());
        if selection.is_some() {
            ui.checkbox(&mut state.only_selected, "Only the selection")
                .on_hover_text("Rows of the areas selected on the map");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("Export", |ui| {
                if ui.button("Excel…").clicked() {
                    actions.push(Action::Export(Export::Data(data, true)));
                }
                if ui.button("CSV…").clicked() {
                    actions.push(Action::Export(Export::Data(data, false)));
                }
            })
            .response
            .on_hover_text("This table with each row's territories");
            if ui
                .button("ƒ Calculated column…")
                .on_hover_text("A new column computed from the others with a formula")
                .clicked()
            {
                actions.push(Action::Formula(data, None));
            }
        });
    });

    let plan_revs = plans.iter().map(|(p, _)| p.rev).collect();
    let only = state.only_selected && selection.is_some();
    let key = Some((
        rev,
        state.sort,
        state.filter.clone(),
        plan_revs,
        selection_key.filter(|_| only),
    ));
    if state.key != key {
        let needle = state.filter.to_lowercase();
        let mut order: Vec<usize> = (0..table.rows())
            .filter(|&r| !only || in_selection(r))
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
                                .frame(false),
                            )
                            .on_hover_text(match &c.formula {
                                Some(f) => format!(
                                    "{} · = {f}\nClick to sort, right-click to edit",
                                    c.kind.label()
                                ),
                                None => format!("{} · click to sort", c.kind.label()),
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
                    for c in &table.columns {
                        row.col(|ui| {
                            let text = c.display(r);
                            if c.kind.is_numeric() {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(match c.f64(r) {
                                            Some(v)
                                                if c.kind
                                                    == territoria_data::ColumnKind::Number =>
                                            {
                                                format_value(v)
                                            }
                                            _ => text,
                                        });
                                    },
                                );
                            } else {
                                ui.label(text);
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
                    if row.response().clicked() {
                        let add = ui_modifiers.shift || ui_modifiers.command;
                        actions.push(Action::SelectRows(data, vec![r], add));
                    }
                });
            });
    });
    ui.data_mut(|d| d.insert_temp(id, state));
    actions
}
