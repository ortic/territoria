//! The Territories panel: the active plan's territories with their figures,
//! editing, and balancing.

use std::collections::HashMap;

use egui::{Color32, RichText, Sense, Ui, vec2};
use territoria_analysis::format::{format_percent, format_value};
use territoria_model::{Cache, Measure, PlanId, PlanStats, Project, TerritoryId, ValueSource};
use territoria_optimize::{Mode, Options, Problem, Quality};

use crate::app::Action;
use crate::dialogs::{Colorable, colorables};
use crate::map_view::{Tool, color};
use crate::theme;

/// Plan figures, recomputed when the plan or its data change.
#[derive(Default)]
pub struct StatsCache {
    entries: HashMap<PlanId, (u64, std::sync::Arc<PlanStats>)>,
}

impl StatsCache {
    pub fn get(
        &mut self,
        project: &Project,
        values: &mut Cache,
        plan: PlanId,
    ) -> Option<std::sync::Arc<PlanStats>> {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let p = project.plan(plan)?;
        let mut h = DefaultHasher::new();
        p.rev.hash(&mut h);
        format!("{:?}{:?}", p.measures, p.territories).hash(&mut h);
        for l in &project.layers {
            l.rev.hash(&mut h);
        }
        for link in project.links_to(p.layer) {
            if let Some(t) = project.table(link.table) {
                t.rev.hash(&mut h);
            }
            format!("{link:?}").hash(&mut h);
        }
        let key = h.finish();
        if let Some((k, s)) = self.entries.get(&plan)
            && *k == key
        {
            return Some(s.clone());
        }
        let s = std::sync::Arc::new(p.stats(project, values));
        self.entries.insert(plan, (key, s.clone()));
        Some(s)
    }
}

/// A balancing result waiting for the user's decision.
pub struct Proposal {
    pub moves: Vec<(u32, TerritoryId)>,
    pub before: Quality,
    pub after: Quality,
    pub stopped_early: bool,
}

pub struct BalanceSettings {
    pub start_over: bool,
    pub compactness: f64,
    pub stability: f64,
}

impl Default for BalanceSettings {
    fn default() -> Self {
        BalanceSettings {
            start_over: false,
            compactness: 0.4,
            stability: 0.3,
        }
    }
}

#[derive(Default)]
pub struct TerritoriesPanel {
    pub proposal: Option<(PlanId, Proposal)>,
    pub settings: BalanceSettings,
    editing: Option<(TerritoryId, String)>,
}

pub struct PanelContext<'a> {
    pub project: &'a Project,
    pub values: &'a mut Cache,
    pub stats: &'a mut StatsCache,
    pub active_plan: Option<PlanId>,
    pub active_territory: Option<TerritoryId>,
    pub tool: Tool,
    pub busy: bool,
}

/// What the optimizer needs, gathered on the UI thread.
pub struct BalanceInput {
    problem: Problem,
    options: Options,
    territories: Vec<TerritoryId>,
}

pub fn balance_input(
    project: &Project,
    values: &mut Cache,
    plan_id: PlanId,
    settings: &BalanceSettings,
) -> Option<BalanceInput> {
    let plan = project.plan(plan_id)?;
    let layer = project.layer(plan.layer)?;
    if plan.territories.is_empty() || layer.is_empty() {
        return None;
    }
    let g = &layer.geometry;
    let index: HashMap<TerritoryId, u32> = plan
        .territories
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id, i as u32))
        .collect();
    // Positions in km around the layer's centre.
    let (lon0, lat0) = g.bbox.map_or((0.0, 0.0), |b| (b.center().x, b.center().y));
    let kx = if g.geographic {
        111.32 * lat0.to_radians().cos()
    } else {
        0.001
    };
    let ky = if g.geographic { 110.57 } else { 0.001 };
    let positions = g
        .label_points
        .iter()
        .map(|p| p.map_or([0.0, 0.0], |c| [(c.x - lon0) * kx, (c.y - lat0) * ky]))
        .collect();
    let measures = plan
        .measures
        .iter()
        .map(|m| territoria_optimize::Measure {
            values: values
                .values(project, plan.layer, &m.source)
                .into_iter()
                .map(|v| v.unwrap_or(0.0))
                .collect(),
            importance: 1.0,
            tolerance: m.tolerance,
        })
        .collect();
    let problem = Problem {
        neighbors: g.neighbors.clone(),
        positions,
        areas: g.areas.clone(),
        measures,
        capacity: plan.territories.iter().map(|t| t.weight).collect(),
        locked: plan.territories.iter().map(|t| t.locked).collect(),
        pinned: plan.pinned.to_vec(),
        initial: plan
            .assignment
            .iter()
            .map(|t| t.and_then(|t| index.get(&t).copied()))
            .collect(),
        anchors: vec![None; plan.territories.len()],
    };
    let options = Options {
        mode: if settings.start_over {
            Mode::Reorganize
        } else {
            Mode::Balance
        },
        compactness: settings.compactness,
        stability: settings.stability,
        ..Default::default()
    };
    Some(BalanceInput {
        problem,
        options,
        territories: plan.territories.iter().map(|t| t.id).collect(),
    })
}

/// Runs the optimizer; `keep_going(share_done)` reports progress and
/// returns `false` to stop early.
pub fn run_balance(
    input: BalanceInput,
    keep_going: &mut dyn FnMut(f32) -> bool,
) -> Result<Proposal, String> {
    let solution = territoria_optimize::optimize(&input.problem, &input.options, keep_going);
    let moves = solution
        .assignment
        .iter()
        .zip(&input.problem.initial)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .filter_map(|(u, (a, _))| a.map(|t| (u as u32, input.territories[t as usize])))
        .collect();
    Ok(Proposal {
        moves,
        before: solution.before,
        after: solution.after,
        stopped_early: solution.stopped_early,
    })
}

fn touch(p: &mut Project, plan: PlanId) {
    let rev = p.bump();
    if let Some(pl) = p.plan_mut(plan) {
        pl.rev = rev;
    }
}

/// A bar for a deviation: centre line = target, shaded band = tolerance.
fn deviation_bar(ui: &mut Ui, deviation: f64, tolerance: f64) {
    let (rect, _) = ui.allocate_exact_size(vec2(44.0, 12.0), Sense::hover());
    let p = ui.painter();
    let range = 0.5f64.max(tolerance * 2.5);
    let x = |d: f64| rect.center().x + (d.clamp(-range, range) / range) as f32 * rect.width() / 2.0;
    p.rect_filled(rect, 2.0, ui.visuals().faint_bg_color);
    let band = egui::Rect::from_x_y_ranges(x(-tolerance)..=x(tolerance), rect.y_range());
    p.rect_filled(band, 0.0, theme::SUCCESS.gamma_multiply(0.18));
    let inside = deviation.abs() <= tolerance + 1e-9;
    let c = if inside { theme::SUCCESS } else { theme::WARN };
    let bar = egui::Rect::from_x_y_ranges(
        x(0.0).min(x(deviation))..=x(0.0).max(x(deviation)),
        rect.shrink2(vec2(0.0, 2.0)).y_range(),
    );
    p.rect_filled(bar, 1.0, c);
    p.line_segment(
        [
            egui::pos2(x(0.0), rect.top()),
            egui::pos2(x(0.0), rect.bottom()),
        ],
        egui::Stroke::new(1.0, ui.visuals().text_color()),
    );
}

impl TerritoriesPanel {
    pub fn show(&mut self, ui: &mut Ui, cx: PanelContext) -> Vec<Action> {
        let mut actions = Vec::new();
        let project = cx.project;
        if project.plans.is_empty() {
            ui.add_space(8.0);
            ui.label("No territory plan yet.");
            ui.label(RichText::new("A plan groups areas into territories: sales regions, service areas, delivery districts.").weak());
            ui.add_space(6.0);
            if ui.button("⬣ New territory plan…").clicked() {
                actions.push(Action::Command(crate::commands::Command::NewPlan));
            }
            return actions;
        }
        let Some(plan) = cx
            .active_plan
            .and_then(|p| project.plan(p))
            .or_else(|| project.plans.first())
        else {
            return actions;
        };
        let plan_id = plan.id;
        if cx.active_plan != Some(plan_id) {
            actions.push(Action::ShowPlan(plan_id));
        }
        // Plan picker.
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("plan")
                .width(200.0)
                .selected_text(&plan.name)
                .show_ui(ui, |ui| {
                    for p in &project.plans {
                        if ui.selectable_label(p.id == plan_id, &p.name).clicked() {
                            actions.push(Action::ShowPlan(p.id));
                        }
                    }
                });
            if ui
                .button("➕")
                .on_hover_text("New territory plan")
                .clicked()
            {
                actions.push(Action::Command(crate::commands::Command::NewPlan));
            }
        });

        // Measure.
        let options = colorables(project, plan.layer);
        let numbers: Vec<(String, ValueSource)> =
            std::iter::once(("Number of areas".to_owned(), ValueSource::Units))
                .chain(options.into_iter().filter_map(|(label, c)| match c {
                    Colorable::Number(s) => Some((label, s)),
                    Colorable::Category(_) => None,
                }))
                .collect();
        let measure = plan.measures.first().cloned().unwrap_or(Measure {
            source: ValueSource::Units,
            tolerance: 0.15,
            target: None,
        });
        ui.horizontal(|ui| {
            ui.label("Balance by").on_hover_text("The figure territories should have about equally of (weighted by each territory's size).");
            let current = numbers.iter().find(|(_, s)| *s == measure.source).map_or_else(|| measure.source.label(project), |(l, _)| l.clone());
            egui::ComboBox::from_id_salt("measure").width(170.0).selected_text(current).show_ui(ui, |ui| {
                for (label, source) in &numbers {
                    if ui.selectable_label(*source == measure.source, label).clicked() {
                        let source = source.clone();
                        actions.push(Action::Edit(
                            "Change measure".into(),
                            Box::new(move |p| {
                                if let Some(pl) = p.plan_mut(plan_id) {
                                    if pl.measures.is_empty() {
                                        pl.measures.push(Measure { source, tolerance: 0.15, target: None });
                                    } else {
                                        pl.measures[0].source = source;
                                    }
                                }
                                touch(p, plan_id);
                            }),
                        ));
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Tolerance ±");
            let mut pct = (measure.tolerance * 100.0).round();
            if ui
                .add(
                    egui::DragValue::new(&mut pct)
                        .range(1.0..=100.0)
                        .suffix(" %"),
                )
                .changed()
            {
                let tol = pct / 100.0;
                actions.push(Action::Edit(
                    "Change tolerance".into(),
                    Box::new(move |p| {
                        if let Some(m) = p.plan_mut(plan_id).and_then(|pl| pl.measures.first_mut())
                        {
                            m.tolerance = tol;
                        }
                        touch(p, plan_id);
                    }),
                ));
            }
        });
        ui.add_space(4.0);

        let Some(stats) = cx.stats.get(project, cx.values, plan_id) else {
            return actions;
        };
        let members = plan.members();

        // Territory list.
        let row_h = 24.0;
        egui::ScrollArea::vertical()
            .id_salt("territory-list")
            .max_height((ui.available_height() - 210.0).max(120.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                // Rows never ask for more than there is (or the panel grows).
                let full = ui.available_width();
                for (i, t) in plan.territories.iter().enumerate() {
                    let s = &stats.territories[i];
                    let active = cx.active_territory == Some(t.id);
                    let frame = egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(4, 2))
                        .corner_radius(4)
                        .fill(if active { ui.visuals().selection.bg_fill } else { Color32::TRANSPARENT });
                    let resp = frame
                        .show(ui, |ui| {
                            ui.set_min_height(row_h);
                            ui.set_width(full - 8.0);
                            ui.horizontal(|ui| {
                                // Colour swatch with a picker.
                                let mut c = color(t.color);
                                let before = c;
                                egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque);
                                if c != before {
                                    let [r, g, b, _] = c.to_srgba_unmultiplied();
                                    let id = t.id;
                                    actions.push(Action::Edit(
                                        "Change colour".into(),
                                        Box::new(move |p| {
                                            if let Some(t) = p.plan_mut(plan_id).and_then(|pl| pl.territory_mut(id)) {
                                                t.color = territoria_analysis::Rgba::rgb(r, g, b);
                                            }
                                            touch(p, plan_id);
                                        }),
                                    ));
                                }
                                // The name gets what the figures on the right leave.
                                let name_w = (full - 8.0 - 40.0 - 190.0).max(48.0);
                                let editing = &mut self.editing;
                                ui.allocate_ui_with_layout(vec2(name_w, row_h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.set_width(name_w);
                                if let Some((_, text)) = editing.as_mut().filter(|(id, _)| *id == t.id) {
                                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(name_w - 8.0));
                                    r.request_focus();
                                    if r.lost_focus() {
                                        let name = text.trim().to_owned();
                                        let id = t.id;
                                        if !name.is_empty() && name != t.name {
                                            actions.push(Action::Edit(
                                                "Rename territory".into(),
                                                Box::new(move |p| {
                                                    if let Some(t) = p.plan_mut(plan_id).and_then(|pl| pl.territory_mut(id)) {
                                                        t.name = name;
                                                    }
                                                    touch(p, plan_id);
                                                }),
                                            ));
                                        }
                                        *editing = None;
                                    }
                                } else {
                                    let name = ui
                                        .add(egui::Label::new(RichText::new(&t.name).strong()).truncate().sense(Sense::click()))
                                        .on_hover_text(format!("{} · code {}\nClick: paint with it · double-click: rename · right-click: more", t.name, t.code));
                                    if name.double_clicked() {
                                        *editing = Some((t.id, t.name.clone()));
                                    } else if name.clicked() {
                                        actions.push(Action::ActivateTerritory(Some(t.id)));
                                    }
                                }
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if t.locked {
                                        ui.label("🔒").on_hover_text("Locked: keeps its areas when painting and balancing");
                                    }
                                    if let (Some(d), Some(_)) = (s.deviations.first(), s.values.first()) {
                                        deviation_bar(ui, *d, measure.tolerance);
                                        let txt = if d.abs() < 0.0005 { "±0 %".to_owned() } else { format!("{:+.1} %", d * 100.0) };
                                        let c = if d.abs() <= measure.tolerance { ui.visuals().text_color() } else { theme::WARN };
                                        ui.label(RichText::new(txt).color(c).monospace());
                                        ui.label(RichText::new(format_value(s.values[0])).monospace()).on_hover_text(format!(
                                            "{} areas · {} km²\ntarget {}",
                                            s.units,
                                            format_value(s.area),
                                            format_value(s.targets[0])
                                        ));
                                    }
                                    if s.parts > 1 {
                                        ui.label(RichText::new(format!("⚠{}", s.parts)).color(theme::WARN))
                                            .on_hover_text(format!("In {} separate pieces", s.parts));
                                    }
                                });
                            });
                        })
                        .response;
                    let resp = resp.interact(Sense::click());
                    if resp.clicked() {
                        actions.push(Action::ActivateTerritory(Some(t.id)));
                    }
                    if resp.double_clicked() {
                        actions.push(Action::ZoomTo(plan.layer, members[i].clone()));
                    }
                    let id = t.id;
                    let locked = t.locked;
                    resp.context_menu(|ui| {
                        if ui.button("Zoom to territory").clicked() {
                            actions.push(Action::ZoomTo(plan.layer, members[i].clone()));
                            ui.close();
                        }
                        if ui.button("Paint with it").clicked() {
                            actions.push(Action::ActivateTerritory(Some(id)));
                            actions.push(Action::SetTool(Tool::Paint));
                            ui.close();
                        }
                        if ui.button(if locked { "Unlock" } else { "Lock (keep its areas)" }).clicked() {
                            actions.push(Action::Edit(
                                if locked { "Unlock territory".into() } else { "Lock territory".into() },
                                Box::new(move |p| {
                                    if let Some(t) = p.plan_mut(plan_id).and_then(|pl| pl.territory_mut(id)) {
                                        t.locked = !t.locked;
                                    }
                                    touch(p, plan_id);
                                }),
                            ));
                            ui.close();
                        }
                        if ui.button("Rename").clicked() {
                            self.editing = Some((id, plan.territory(id).map(|t| t.name.clone()).unwrap_or_default()));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Delete").clicked() {
                            actions.push(Action::Edit(
                                "Delete territory".into(),
                                Box::new(move |p| {
                                    if let Some(pl) = p.plan_mut(plan_id) {
                                        pl.remove_territory(id);
                                    }
                                    touch(p, plan_id);
                                }),
                            ));
                            ui.close();
                        }
                    });
                }
            });

        ui.horizontal_wrapped(|ui| {
            if ui.button("➕ Territory").on_hover_text("Add a territory and paint it (Ctrl+Shift+T)").clicked() {
                actions.push(Action::Command(crate::commands::Command::NewTerritory));
            }
            if let Some(t) = cx.active_territory.filter(|t| plan.territory(*t).is_some())
                && ui.button("🗑").on_hover_text("Delete the active territory; its areas become unassigned").clicked()
            {
                actions.push(Action::Edit(
                    "Delete territory".into(),
                    Box::new(move |p| {
                        if let Some(pl) = p.plan_mut(plan_id) {
                            pl.remove_territory(t);
                        }
                        touch(p, plan_id);
                    }),
                ));
                actions.push(Action::ActivateTerritory(None));
            }
            if cx.tool != Tool::Paint && ui.button("✏ Paint").on_hover_text("Assign areas by dragging over them (B)").clicked() {
                actions.push(Action::SetTool(Tool::Paint));
            }
            if stats.unassigned > 0 && ui.button("Fill gaps").on_hover_text("Give unassigned areas to the neighbouring territory they share most border with").clicked() {
                actions.push(Action::Command(crate::commands::Command::FillGaps));
            }
            if ui.button("Recolour").on_hover_text("Different colours for neighbouring territories").clicked() {
                actions.push(Action::Command(crate::commands::Command::Recolor));
            }
        });

        // Summary.
        ui.separator();
        let outside = stats
            .territories
            .iter()
            .filter(|s| {
                s.deviations
                    .first()
                    .is_some_and(|d| d.abs() > measure.tolerance)
            })
            .count();
        let split = stats.territories.iter().filter(|s| s.parts > 1).count();
        ui.horizontal_wrapped(|ui| {
            if stats.unassigned > 0 {
                ui.label(
                    RichText::new(format!(
                        "{} areas unassigned",
                        format_value(stats.unassigned as f64)
                    ))
                    .color(theme::WARN),
                );
                ui.label("·");
            }
            let c = if outside == 0 {
                theme::SUCCESS
            } else {
                theme::WARN
            };
            ui.label(
                RichText::new(format!(
                    "{outside} of {} outside ±{:.0} %",
                    plan.territories.len(),
                    measure.tolerance * 100.0
                ))
                .color(c),
            );
            if split > 0 {
                ui.label("·");
                ui.label(RichText::new(format!("{split} in pieces")).color(theme::WARN));
            }
        });

        // Balancing.
        ui.add_space(4.0);
        egui::CollapsingHeader::new(RichText::new("Balance automatically").strong())
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.settings.start_over, false, "Improve current")
                        .on_hover_text("Move as few areas as needed");
                    ui.radio_value(&mut self.settings.start_over, true, "Start over")
                        .on_hover_text("New territories from scratch (locked ones stay)");
                });
                ui.horizontal(|ui| {
                    ui.label("Compact shapes");
                    ui.add(
                        egui::Slider::new(&mut self.settings.compactness, 0.0..=1.0)
                            .show_value(false),
                    );
                });
                if !self.settings.start_over {
                    ui.horizontal(|ui| {
                        ui.label("Few changes");
                        ui.add(
                            egui::Slider::new(&mut self.settings.stability, 0.0..=1.0)
                                .show_value(false),
                        );
                    });
                }
                ui.horizontal(|ui| {
                    if cx.busy {
                        ui.spinner();
                        ui.label("Balancing…");
                    } else if ui.button(RichText::new("⚖ Balance").strong()).clicked() {
                        actions.push(Action::Balance(plan_id));
                    }
                });
                if let Some((p, proposal)) = &self.proposal
                    && *p == plan_id
                {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("Proposal").strong());
                        let b = proposal
                            .before
                            .max_deviation
                            .first()
                            .copied()
                            .unwrap_or(0.0);
                        let a = proposal.after.max_deviation.first().copied().unwrap_or(0.0);
                        ui.label(format!(
                            "Largest deviation {} → {}",
                            format_percent(b),
                            format_percent(a)
                        ));
                        let ob = proposal
                            .before
                            .out_of_tolerance
                            .first()
                            .copied()
                            .unwrap_or(0);
                        let oa = proposal
                            .after
                            .out_of_tolerance
                            .first()
                            .copied()
                            .unwrap_or(0);
                        ui.label(format!("Outside tolerance {ob} → {oa}"));
                        ui.label(format!(
                            "Border length {} → {} km",
                            format_value(proposal.before.border_length),
                            format_value(proposal.after.border_length)
                        ));
                        ui.label(format!(
                            "{} areas change territory",
                            format_value(proposal.moves.len() as f64)
                        ));
                        if proposal.stopped_early {
                            ui.label(
                                RichText::new(
                                    "Stopped early: the result may improve with another run.",
                                )
                                .weak(),
                            );
                        }
                        ui.horizontal(|ui| {
                            if ui.button(RichText::new("Apply").strong()).clicked() {
                                actions.push(Action::AcceptProposal(plan_id));
                            }
                            if ui.button("Discard").clicked() {
                                actions.push(Action::DiscardProposal);
                            }
                        });
                    });
                }
            });
        actions
    }
}
