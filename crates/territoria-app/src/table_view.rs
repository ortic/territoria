//! A table tab: a layer's attributes or a dataset, sortable and searchable.

use egui::{RichText, Ui};
use egui_extras::{Column as TableColumn, TableBuilder};
use territoria_analysis::format::format_value;
use territoria_data::Table;
use territoria_model::Project;

use crate::app::{Action, DataRef};

#[derive(Clone, Default)]
struct TableState {
    sort: Option<(usize, bool)>,
    filter: String,
    /// Row order for `key`.
    order: Vec<usize>,
    key: Option<OrderKey>,
}

/// What the row order depends on: revision, sort, filter.
type OrderKey = (u64, Option<(usize, bool)>, String);

pub fn show(ui: &mut Ui, project: &Project, data: DataRef) -> Vec<Action> {
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
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("ƒ Calculated column…")
                .on_hover_text("A new column computed from the others with a formula")
                .clicked()
            {
                actions.push(Action::Formula(data, None));
            }
        });
    });

    let key = Some((rev, state.sort, state.filter.clone()));
    if state.key != key {
        let needle = state.filter.to_lowercase();
        let mut order: Vec<usize> = (0..table.rows())
            .filter(|&r| {
                needle.is_empty()
                    || table
                        .columns
                        .iter()
                        .any(|c| c.display(r).to_lowercase().contains(&needle))
            })
            .collect();
        if let Some((col, asc)) = state.sort
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
    egui::ScrollArea::horizontal().show(ui, |ui| {
        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        for _ in &table.columns {
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
            })
            .body(|body| {
                body.rows(text_height, state.order.len(), |mut row| {
                    let r = state.order[row.index()];
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
                });
            });
    });
    ui.data_mut(|d| d.insert_temp(id, state));
    actions
}
