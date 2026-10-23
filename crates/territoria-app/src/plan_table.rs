//! A plan's territories as a table: edit codes, names, regions, sizes and
//! locks in place; sort by any figure.

use egui::{ComboBox, RichText, Sense, Ui, vec2};
use egui_extras::{Column as TableColumn, TableBuilder};
use territoria_analysis::format::{format_percent, format_value};
use territoria_model::{Cache, PlanId, Project, Territory, TerritoryId};

use crate::app::Action;
use crate::map_view::color;
use crate::territories::StatsCache;

fn edit(plan: PlanId, id: TerritoryId, label: &str, f: Box<dyn FnOnce(&mut Territory)>) -> Action {
    Action::Edit(
        label.to_owned(),
        Box::new(move |p| {
            let rev = p.bump();
            if let Some(pl) = p.plan_mut(plan) {
                if let Some(t) = pl.territory_mut(id) {
                    f(t);
                }
                pl.rev = rev;
            }
        }),
    )
}

pub fn show(
    ui: &mut Ui,
    project: &Project,
    values: &mut Cache,
    stats: &mut StatsCache,
    plan_id: PlanId,
) -> Vec<Action> {
    let mut actions = Vec::new();
    let Some(plan) = project.plan(plan_id) else {
        ui.label("This plan was removed.");
        return actions;
    };
    let Some(stats) = stats.get(project, values, plan_id) else {
        return actions;
    };
    let measure = plan
        .measures
        .first()
        .map_or("Value".to_owned(), |m| m.source.label(project));
    let tolerance = plan.measures.first().map_or(0.15, |m| m.tolerance);
    // Sorting, kept per tab.
    let state_id = ui.id().with(("plan-table", plan_id));
    let mut sort: Option<(usize, bool)> = ui.data_mut(|d| d.get_temp(state_id)).unwrap_or(None);
    let headers = [
        "Code",
        "Territory",
        "Region",
        "Size",
        "Locked",
        "Areas",
        "km²",
        measure.as_str(),
        "Target",
        "±",
        "Farthest km",
    ];
    let n = plan.territories.len().min(stats.territories.len());
    let mut order: Vec<usize> = (0..n).collect();
    if let Some((col, asc)) = sort {
        let key = |i: usize| -> f64 {
            let s = &stats.territories[i];
            match col {
                3 => plan.territories[i].weight,
                5 => s.units as f64,
                6 => s.area,
                7 => s.values.first().copied().unwrap_or(0.0),
                8 => s.targets.first().copied().unwrap_or(0.0),
                9 => s.deviations.first().copied().unwrap_or(0.0),
                10 => s.site_reach.unwrap_or(-1.0),
                _ => 0.0,
            }
        };
        let text = |i: usize| -> String {
            let t = &plan.territories[i];
            match col {
                0 => t.code.to_lowercase(),
                1 => t.name.to_lowercase(),
                2 => t
                    .region
                    .and_then(|r| plan.region(r))
                    .map_or(String::new(), |r| r.name.to_lowercase()),
                4 => (t.locked as u8).to_string(),
                _ => String::new(),
            }
        };
        if matches!(col, 0 | 1 | 2 | 4) {
            order.sort_by_key(|&i| text(i));
        } else {
            order.sort_by(|&a, &b| key(a).total_cmp(&key(b)));
        }
        if !asc {
            order.reverse();
        }
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(&plan.name).strong());
        ui.label(RichText::new(format!("{n} territories · click a heading to sort")).weak());
    });
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
    egui::ScrollArea::horizontal().show(ui, |ui| {
        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        builder = builder
            .column(TableColumn::exact(22.0))
            .column(TableColumn::initial(70.0).at_least(50.0))
            .column(TableColumn::initial(170.0).at_least(80.0))
            .column(TableColumn::initial(110.0).at_least(60.0));
        for _ in 3..headers.len() {
            builder = builder.column(TableColumn::auto().at_least(56.0));
        }
        builder
            .header(row_h, |mut header| {
                header.col(|_| {});
                for (i, h) in headers.iter().enumerate() {
                    header.col(|ui| {
                        let arrow = match sort {
                            Some((s, true)) if s == i => " ⏶",
                            Some((s, false)) if s == i => " ⏷",
                            _ => "",
                        };
                        if ui
                            .add(
                                egui::Button::new(RichText::new(format!("{h}{arrow}")).strong())
                                    .frame(false),
                            )
                            .clicked()
                        {
                            sort = match sort {
                                Some((s, true)) if s == i => Some((i, false)),
                                Some((s, false)) if s == i => None,
                                _ => Some((i, true)),
                            };
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(row_h, order.len(), |mut row| {
                    let i = order[row.index()];
                    let t = &plan.territories[i];
                    let s = &stats.territories[i];
                    let id = t.id;
                    row.col(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                        ui.painter().rect_filled(rect, 3.0, color(t.color));
                    });
                    row.col(|ui| {
                        let mut code = t.code.clone();
                        if ui
                            .add(egui::TextEdit::singleline(&mut code).desired_width(60.0))
                            .changed()
                        {
                            actions.push(edit(
                                plan_id,
                                id,
                                "Change code",
                                Box::new(move |t| t.code = code),
                            ));
                        }
                    });
                    row.col(|ui| {
                        let mut name = t.name.clone();
                        if ui
                            .add(egui::TextEdit::singleline(&mut name).desired_width(160.0))
                            .changed()
                            && !name.trim().is_empty()
                        {
                            actions.push(edit(
                                plan_id,
                                id,
                                "Rename territory",
                                Box::new(move |t| t.name = name),
                            ));
                        }
                    });
                    row.col(|ui| {
                        let current = t
                            .region
                            .and_then(|r| plan.region(r))
                            .map_or("—".to_owned(), |r| r.name.clone());
                        let mut region = t.region;
                        ComboBox::from_id_salt(("region", id))
                            .width(96.0)
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut region, None, "—");
                                for r in &plan.regions {
                                    ui.selectable_value(&mut region, Some(r.id), &r.name);
                                }
                            });
                        if region != t.region {
                            actions.push(edit(
                                plan_id,
                                id,
                                "Move to region",
                                Box::new(move |t| t.region = region),
                            ));
                        }
                    });
                    row.col(|ui| {
                        let mut w = t.weight;
                        if ui
                            .add(
                                egui::DragValue::new(&mut w)
                                    .range(0.0..=10.0)
                                    .speed(0.05)
                                    .fixed_decimals(2),
                            )
                            .changed()
                        {
                            actions.push(edit(
                                plan_id,
                                id,
                                "Change size",
                                Box::new(move |t| t.weight = w),
                            ));
                        }
                    });
                    row.col(|ui| {
                        let mut locked = t.locked;
                        if ui.checkbox(&mut locked, "").changed() {
                            actions.push(edit(
                                plan_id,
                                id,
                                if locked {
                                    "Lock territory"
                                } else {
                                    "Unlock territory"
                                },
                                Box::new(move |t| t.locked = locked),
                            ));
                        }
                    });
                    let num = |ui: &mut Ui, text: String| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(text);
                        });
                    };
                    row.col(|ui| num(ui, format_value(s.units as f64)));
                    row.col(|ui| num(ui, format_value(s.area)));
                    row.col(|ui| {
                        num(
                            ui,
                            s.values.first().map_or("–".into(), |v| format_value(*v)),
                        )
                    });
                    row.col(|ui| {
                        num(
                            ui,
                            s.targets.first().map_or("–".into(), |v| format_value(*v)),
                        )
                    });
                    row.col(|ui| {
                        if let Some(d) = s.deviations.first() {
                            let c = if d.abs() > tolerance {
                                crate::theme::WARN
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{}{}",
                                            if *d > 0.0 { "+" } else { "" },
                                            format_percent(*d)
                                        ))
                                        .color(c),
                                    );
                                },
                            );
                        }
                    });
                    row.col(|ui| num(ui, s.site_reach.map_or("–".into(), format_value)));
                });
            });
    });
    ui.data_mut(|d| d.insert_temp(state_id, sort));
    actions
}
