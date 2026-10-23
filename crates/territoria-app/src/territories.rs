//! The Territories panel: the active plan's territories with their figures,
//! editing, and balancing.

use std::collections::HashMap;

use egui::{Color32, ComboBox, RichText, Sense, Ui, vec2};
use territoria_analysis::format::{format_percent, format_value};
use territoria_model::{
    Cache, Comparison, LayerId, Measure, PlanId, PlanStats, Project, TerritoryId, ValueSource,
};
use territoria_optimize::{Mode, Options, Problem, Quality};

use crate::app::{Action, Export};
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
        let p = project.plan(plan)?;
        let key = plan_key(project, plan)?;
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

/// Changes when the plan's figures may change: its assignment, measures,
/// territories, or the data behind them.
fn plan_key(project: &Project, plan: PlanId) -> Option<u64> {
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
    Some(h.finish())
}

/// The active plan compared with another one.
struct Compared {
    key: (u64, u64),
    layer: LayerId,
    base: PlanId,
    plan: PlanId,
    result: Comparison,
    /// The units that change territory.
    units: Vec<u32>,
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
    /// The new name while renaming the plan.
    renaming: Option<String>,
    /// The plan the active one is compared with.
    pub compare: Option<PlanId>,
    /// Mark the areas that change territory on the map.
    pub mark_changes: bool,
    compared: Option<Compared>,
    /// What's wrong with the active plan's data, by plan key.
    diagnostics: Option<(PlanId, u64, Vec<territoria_model::Diagnostic>)>,
    /// How many regions "Group into regions" makes.
    region_count: usize,
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

/// Groups a plan's territories into `k` contiguous regions, balanced by
/// the plan's measure: per territory the index of its region.
pub fn group_regions(
    project: &Project,
    stats: &PlanStats,
    plan: PlanId,
    k: usize,
) -> Option<Vec<Option<usize>>> {
    let p = project.plan(plan)?;
    let g = &project.layer(p.layer)?.geometry;
    let n = p.territories.len();
    if k == 0 || n == 0 {
        return None;
    }
    let index: HashMap<TerritoryId, usize> = p
        .territories
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id, i))
        .collect();
    // Shared border length between territories, from their units'.
    let mut border: Vec<HashMap<u32, f64>> = vec![HashMap::new(); n];
    for (u, t) in p.assignment.iter().enumerate() {
        let Some(&a) = t.and_then(|t| index.get(&t)) else {
            continue;
        };
        for &(nb, len) in g.neighbors.get(u).map(Vec::as_slice).unwrap_or_default() {
            if let Some(&b) = p
                .assignment
                .get(nb as usize)
                .copied()
                .flatten()
                .and_then(|t| index.get(&t))
                && a != b
            {
                *border[a].entry(b as u32).or_insert(0.0) += len.max(1e-3);
            }
        }
    }
    let neighbors: Vec<Vec<(u32, f64)>> = border
        .into_iter()
        .map(|m| m.into_iter().collect())
        .collect();
    // Positions in km around the middle latitude.
    let centres = p.centres(g, None);
    let lat0 = centres.iter().flatten().map(|c| c.y).sum::<f64>()
        / centres.iter().flatten().count().max(1) as f64;
    let positions: Vec<[f64; 2]> = centres
        .iter()
        .map(|c| {
            c.map_or([0.0, 0.0], |c| {
                [c.x * 111.32 * lat0.to_radians().cos(), c.y * 110.57]
            })
        })
        .collect();
    let measure_values: Vec<f64> = stats
        .territories
        .iter()
        .map(|s| s.values.first().copied().unwrap_or(s.units as f64))
        .collect();
    let problem = Problem {
        neighbors,
        positions,
        areas: stats.territories.iter().map(|s| s.area).collect(),
        measures: vec![territoria_optimize::Measure {
            values: measure_values,
            importance: 1.0,
            tolerance: 0.15,
        }],
        capacity: vec![1.0; k],
        locked: vec![false; k],
        pinned: vec![false; n],
        initial: vec![None; n],
        anchors: vec![None; k],
        sites: vec![None; k],
    };
    let options = Options {
        mode: Mode::Reorganize,
        compactness: 0.5,
        contiguous: true,
        assign_unassigned: true,
        max_seconds: 2.0,
        ..Default::default()
    };
    let solution = territoria_optimize::optimize(&problem, &options, &mut |_| true);
    Some(
        solution
            .assignment
            .iter()
            .map(|r| r.map(|r| r as usize))
            .collect(),
    )
}

/// Splits a territory's units into `k` contiguous parts of about equal
/// measure: per unit (of the whole layer) the part it goes to.
pub fn split_territory(
    project: &Project,
    values: &mut Cache,
    plan: PlanId,
    territory: TerritoryId,
    k: usize,
) -> Option<Vec<(u32, usize)>> {
    let p = project.plan(plan)?;
    let g = &project.layer(p.layer)?.geometry;
    let units: Vec<u32> = (0..p.assignment.len() as u32)
        .filter(|&u| p.assignment[u as usize] == Some(territory))
        .collect();
    if units.len() < k || k < 2 {
        return None;
    }
    let local: HashMap<u32, u32> = units
        .iter()
        .enumerate()
        .map(|(i, u)| (*u, i as u32))
        .collect();
    let neighbors = units
        .iter()
        .map(|&u| {
            g.neighbors
                .get(u as usize)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|(n, len)| local.get(n).map(|l| (*l, *len)))
                .collect()
        })
        .collect();
    let (lon0, lat0) = g.bbox.map_or((0.0, 0.0), |b| (b.center().x, b.center().y));
    let (kx, ky) = if g.geographic {
        (111.32 * lat0.to_radians().cos(), 110.57)
    } else {
        (0.001, 0.001)
    };
    let positions = units
        .iter()
        .map(|&u| {
            g.label_points
                .get(u as usize)
                .copied()
                .flatten()
                .map_or([0.0, 0.0], |c| [(c.x - lon0) * kx, (c.y - lat0) * ky])
        })
        .collect();
    let source = p
        .measures
        .first()
        .map_or(ValueSource::Units, |m| m.source.clone());
    let all = values.values(project, p.layer, &source);
    let problem = Problem {
        neighbors,
        positions,
        areas: units
            .iter()
            .map(|&u| g.areas.get(u as usize).copied().unwrap_or(0.0))
            .collect(),
        measures: vec![territoria_optimize::Measure {
            values: units
                .iter()
                .map(|&u| all.get(u as usize).copied().flatten().unwrap_or(0.0))
                .collect(),
            importance: 1.0,
            tolerance: 0.1,
        }],
        capacity: vec![1.0; k],
        locked: vec![false; k],
        pinned: vec![false; units.len()],
        initial: vec![None; units.len()],
        anchors: vec![None; k],
        sites: vec![None; k],
    };
    let options = Options {
        mode: Mode::Reorganize,
        compactness: 0.5,
        contiguous: true,
        assign_unassigned: true,
        max_seconds: 3.0,
        ..Default::default()
    };
    let solution = territoria_optimize::optimize(&problem, &options, &mut |_| true);
    Some(
        units
            .iter()
            .zip(&solution.assignment)
            .map(|(u, part)| (*u, part.unwrap_or(0) as usize))
            .collect(),
    )
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
    // Sites: their units stay with the territory, distance to them counts.
    let site_units = plan.site_units(g);
    let sites = plan
        .territories
        .iter()
        .map(|t| t.site.map(|[x, y]| [(x - lon0) * kx, (y - lat0) * ky]))
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
        anchors: site_units,
        sites,
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
    /// The units to mark on the map while comparing `plan` with another.
    pub fn highlight(&self, plan: Option<PlanId>) -> Option<(LayerId, &[u32])> {
        let c = self.compared.as_ref()?;
        (self.mark_changes && Some(c.plan) == plan && self.compare == Some(c.base))
            .then_some((c.layer, c.units.as_slice()))
    }

    /// Compares the active plan with another and marks the changes.
    pub fn compare_with(&mut self, base: PlanId) {
        self.compare = Some(base);
        self.mark_changes = true;
    }

    fn comparison(
        &mut self,
        project: &Project,
        values: &mut Cache,
        base: PlanId,
        plan: PlanId,
    ) -> Option<&Compared> {
        let key = (plan_key(project, base)?, plan_key(project, plan)?);
        let fresh = self
            .compared
            .as_ref()
            .is_some_and(|c| c.key == key && c.base == base && c.plan == plan);
        if !fresh {
            let result = project.compare_plans(base, plan, values)?;
            let units = result.moves.iter().map(|m| m.unit).collect();
            self.compared = Some(Compared {
                key,
                layer: project.plan(plan)?.layer,
                base,
                plan,
                result,
                units,
            });
        }
        self.compared.as_ref()
    }

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
        // Plan picker, or the plan's name while renaming it.
        ui.horizontal(|ui| {
            if let Some(mut name) = self.renaming.take() {
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(200.0));
                r.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    // Cancelled.
                } else if r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let name = name.trim().to_owned();
                    if !name.is_empty() && name != plan.name {
                        let old = plan.name.clone();
                        actions.push(Action::Edit(
                            "Rename plan".into(),
                            Box::new(move |p| {
                                for map in &mut p.maps {
                                    for ml in &mut map.layers {
                                        if ml.kind.plan() == Some(plan_id) && ml.name == old {
                                            ml.name = name.clone();
                                        }
                                    }
                                }
                                if let Some(pl) = p.plan_mut(plan_id) {
                                    pl.name = name;
                                }
                            }),
                        ));
                    }
                } else {
                    self.renaming = Some(name);
                }
                return;
            }
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
            ui.menu_button("⋯", |ui| {
                if ui
                    .button("⧉ Duplicate as scenario")
                    .on_hover_text("A copy to try changes on. The copy is compared with this plan.")
                    .clicked()
                {
                    actions.push(Action::DuplicatePlan(plan_id));
                }
                let others: Vec<_> = project
                    .plans
                    .iter()
                    .filter(|p| p.id != plan_id && p.layer == plan.layer)
                    .collect();
                ui.add_enabled_ui(!others.is_empty(), |ui| {
                    ui.menu_button("⇄ Compare with", |ui| {
                        for p in others {
                            if ui.button(&p.name).clicked() {
                                self.compare_with(p.id);
                            }
                        }
                    })
                    .response
                    .on_disabled_hover_text(
                        "No other plan on the same areas. Duplicate this one to try changes.",
                    );
                });
                if ui.button("✏ Rename").clicked() {
                    self.renaming = Some(plan.name.clone());
                }
                if ui.button("Export (Excel)…").clicked() {
                    actions.push(Action::Export(Export::PlanExcel(plan_id)));
                }
                if ui
                    .button("Report (PDF)…")
                    .on_hover_text("An overview and a page per territory")
                    .clicked()
                {
                    actions.push(Action::Export(Export::Report(plan_id)));
                }
                ui.separator();
                if ui.button("🗑 Delete plan…").clicked() {
                    actions.push(Action::RemovePlan(plan_id));
                }
            });
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
        let site_units = project
            .layer(plan.layer)
            .map(|l| plan.site_units(&l.geometry))
            .unwrap_or_default();

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
                                        let reach = s.site_reach.map_or(String::new(), |r| format!("\nfarthest area {} km from the site", format_value(r)));
                                        ui.label(RichText::new(format_value(s.values[0])).monospace()).on_hover_text(format!(
                                            "{} areas · {} km²\ntarget {}{reach}",
                                            s.units,
                                            format_value(s.area),
                                            format_value(s.targets[0])
                                        ));
                                    }
                                    if site_units.get(i).copied().flatten().is_some_and(|u| plan.assignment.get(u as usize).copied().flatten() != Some(t.id)) {
                                        ui.label(RichText::new("📍").color(theme::WARN))
                                            .on_hover_text("The site lies outside the territory");
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
                    let has_site = t.site.is_some();
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
                        if ui.button("Place site on the map").clicked() {
                            actions.push(Action::ActivateTerritory(Some(id)));
                            actions.push(Action::SetTool(Tool::Site));
                            ui.close();
                        }
                        if has_site && ui.button("Remove site").clicked() {
                            actions.push(Action::Edit(
                                "Remove site".into(),
                                Box::new(move |p| {
                                    if let Some(t) = p.plan_mut(plan_id).and_then(|pl| pl.territory_mut(id)) {
                                        t.site = None;
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
                        ui.menu_button("Split into", |ui| {
                            for k in 2..=5 {
                                if ui
                                    .button(format!("{k} territories"))
                                    .on_hover_text("Its areas become contiguous parts of about equal size by the measure")
                                    .clicked()
                                {
                                    if let Some(parts) = split_territory(project, cx.values, plan_id, id, k) {
                                        let base = plan.territory(id).map(|t| t.name.clone()).unwrap_or_default();
                                        actions.push(Action::Edit(
                                            "Split territory".into(),
                                            Box::new(move |p| {
                                                if let Some(pl) = p.plan_mut(plan_id) {
                                                    let new: Vec<TerritoryId> = (2..=k)
                                                        .map(|i| pl.add_territory(None, Some(format!("{base} {i}"))))
                                                        .collect();
                                                    for (u, part) in &parts {
                                                        if *part > 0 {
                                                            pl.assign(&[*u], Some(new[part - 1]));
                                                        }
                                                    }
                                                }
                                                touch(p, plan_id);
                                            }),
                                        ));
                                    }
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("Merge into", |ui| {
                            for other in plan.territories.iter().filter(|o| o.id != id) {
                                if ui.button(&other.name).clicked() {
                                    let into = other.id;
                                    actions.push(Action::Edit(
                                        "Merge territories".into(),
                                        Box::new(move |p| {
                                            if let Some(pl) = p.plan_mut(plan_id) {
                                                let units: Vec<u32> = (0..pl.assignment.len() as u32)
                                                    .filter(|&u| pl.assignment[u as usize] == Some(id))
                                                    .collect();
                                                pl.assign(&units, Some(into));
                                                pl.remove_territory(id);
                                            }
                                            touch(p, plan_id);
                                        }),
                                    ));
                                    ui.close();
                                }
                            }
                        });
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
            ui.menu_button("Names", |ui| {
                if ui
                    .button("Name after the largest place")
                    .on_hover_text("Each territory takes the name of its largest area by the measure (or by size). Locked territories keep theirs.")
                    .clicked()
                {
                    let weights = (measure.source != ValueSource::Units)
                        .then(|| cx.values.values(project, plan.layer, &measure.source));
                    actions.push(Action::Edit(
                        "Name territories".into(),
                        Box::new(move |p| {
                            let Some(l) = p.plan(plan_id).and_then(|pl| p.layer(pl.layer)).cloned() else { return };
                            if let Some(pl) = p.plan_mut(plan_id) {
                                pl.name_after_largest(&l, weights.as_deref());
                            }
                            touch(p, plan_id);
                        }),
                    ));
                    ui.close();
                }
                if ui
                    .button("Renumber north to south")
                    .on_hover_text("Codes 01, 02, … in reading order on the map; default names follow")
                    .clicked()
                {
                    actions.push(Action::Edit(
                        "Renumber territories".into(),
                        Box::new(move |p| {
                            let Some(g) = p.plan(plan_id).and_then(|pl| p.layer(pl.layer)).map(|l| l.geometry.clone()) else { return };
                            if let Some(pl) = p.plan_mut(plan_id) {
                                pl.renumber_by_position(&g);
                            }
                            touch(p, plan_id);
                        }),
                    ));
                    ui.close();
                }
            });
            let missing = plan.territories.iter().filter(|t| t.site.is_none()).count();
            let any_site = missing < plan.territories.len();
            // Centres weighted by the measure: where the revenue is.
            let by_measure = measure.source != ValueSource::Units;
            let what = if by_measure { measure.source.label(project) } else { "area".to_owned() };
            let sites_at = |replace: bool, values: &mut Cache| {
                let weights = by_measure.then(|| values.values(project, plan.layer, &measure.source));
                Action::Edit(
                    if replace { "Move sites to centres".into() } else { "Sites at centres".into() },
                    Box::new(move |p| {
                        let Some(g) = p.plan(plan_id).and_then(|pl| p.layer(pl.layer)).map(|l| l.geometry.clone()) else { return };
                        if let Some(pl) = p.plan_mut(plan_id) {
                            pl.sites_at_centres(&g, replace, weights.as_deref());
                        }
                        touch(p, plan_id);
                    }),
                )
            };
            ui.menu_button("📍 Sites", |ui| {
                if ui
                    .add_enabled(missing > 0, egui::Button::new("Sites at centres"))
                    .on_hover_text(format!("A site (rep home, branch) for each territory without one, in the area nearest the centre of its {what}. Start over then builds territories around the sites."))
                    .clicked()
                {
                    actions.push(sites_at(false, cx.values));
                    ui.close();
                }
                if ui
                    .add_enabled(any_site, egui::Button::new("Move all sites to the centres"))
                    .on_hover_text(format!("Every site goes to the area nearest the centre of its territory's {what}"))
                    .clicked()
                {
                    actions.push(sites_at(true, cx.values));
                    ui.close();
                }
                let has_points = project.layers.iter().any(|l| l.kind() == territoria_io::GeometryKind::Point && l.id != plan.layer);
                if ui
                    .add_enabled(has_points, egui::Button::new("Sites from points…"))
                    .on_hover_text("Rep homes or branches from a point layer, matched to the territories; or the best of many possible locations")
                    .on_disabled_hover_text("Add points first (a table with coordinates or addresses)")
                    .clicked()
                {
                    actions.push(Action::SitesFromPoints(plan_id));
                    ui.close();
                }
                if ui.add_enabled(any_site, egui::Button::new("Remove all sites")).clicked() {
                    actions.push(Action::Edit(
                        "Remove sites".into(),
                        Box::new(move |p| {
                            if let Some(pl) = p.plan_mut(plan_id) {
                                for t in &mut pl.territories {
                                    t.site = None;
                                }
                            }
                            touch(p, plan_id);
                        }),
                    ));
                    ui.close();
                }
            });
        });

        // The active territory's details.
        if let Some(t) = cx.active_territory.and_then(|t| plan.territory(t)) {
            self.details_ui(ui, plan, t, &mut actions);
        }

        // Regions above the territories.
        self.regions_ui(ui, project, plan, &stats, measure.tolerance, &mut actions);

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

        // Things in the data that keep territories from balancing.
        if let Some(key) = plan_key(project, plan_id) {
            if self
                .diagnostics
                .as_ref()
                .is_none_or(|(p, k, _)| *p != plan_id || *k != key)
            {
                self.diagnostics = Some((plan_id, key, plan.diagnose(project, cx.values)));
            }
            if let Some((_, _, list)) = &self.diagnostics
                && !list.is_empty()
            {
                ui.add_space(2.0);
                for d in list {
                    ui.horizontal(|ui| {
                        let (icon, color) = if d.serious {
                            ("⚠", theme::WARN)
                        } else {
                            ("ℹ", ui.visuals().weak_text_color())
                        };
                        ui.label(RichText::new(icon).color(color));
                        if !d.units.is_empty()
                            && ui
                                .small_button("Show")
                                .on_hover_text("Select these areas on the map")
                                .clicked()
                        {
                            actions.push(Action::SelectUnits(plan.layer, d.units.clone()));
                        }
                        ui.add(
                            egui::Label::new(RichText::new(&d.text).color(color).small()).wrap(),
                        );
                    });
                }
            }
        }

        // Comparison with another plan (a scenario with its original).
        if let Some(base) = self.compare.filter(|b| *b != plan_id) {
            self.compare_ui(ui, project, cx.values, base, plan_id, &mut actions);
        }

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

    fn regions_ui(
        &mut self,
        ui: &mut Ui,
        project: &Project,
        plan: &territoria_model::Plan,
        stats: &PlanStats,
        tolerance: f64,
        actions: &mut Vec<Action>,
    ) {
        let plan_id = plan.id;
        let n = plan.territories.len();
        if self.region_count == 0 {
            self.region_count = ((n as f64).sqrt().round() as usize).clamp(2, 12);
        }
        let header = if plan.regions.is_empty() {
            "Regions".to_owned()
        } else {
            format!("Regions ({})", plan.regions.len())
        };
        egui::CollapsingHeader::new(RichText::new(header).strong())
            .id_salt("regions")
            .default_open(!plan.regions.is_empty())
            .show(ui, |ui| {
                if plan.regions.is_empty() {
                    ui.label(RichText::new("Group territories into regions: sales regions above the territories, with their own borders and figures.").weak());
                }
                let region_stats = plan.region_stats(stats);
                for (r, rs) in plan.regions.iter().zip(&region_stats) {
                    let id = r.id;
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                        ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(2.0, color(r.color)), egui::StrokeKind::Inside);
                        let mut name = r.name.clone();
                        if ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0)).changed() {
                            actions.push(Action::Edit(
                                "Rename region".into(),
                                Box::new(move |p| {
                                    if let Some(r) = p.plan_mut(plan_id).and_then(|pl| pl.regions.iter_mut().find(|r| r.id == id)) {
                                        r.name = name;
                                    }
                                    touch(p, plan_id);
                                }),
                            ));
                        }
                        ui.label(RichText::new(format!("{} territories", rs.territories)).weak().small());
                        if let (Some(v), Some(d)) = (rs.values.first(), rs.deviations.first()) {
                            ui.label(RichText::new(format_value(*v)).monospace());
                            deviation_bar(ui, *d, tolerance);
                        }
                        if ui.small_button("✖").on_hover_text("Remove the region (its territories stay)").clicked() {
                            actions.push(Action::Edit(
                                "Remove region".into(),
                                Box::new(move |p| {
                                    if let Some(pl) = p.plan_mut(plan_id) {
                                        pl.remove_region(id);
                                    }
                                    touch(p, plan_id);
                                }),
                            ));
                        }
                    });
                }
                let unassigned = plan.territories.iter().filter(|t| t.region.is_none()).count();
                if !plan.regions.is_empty() && unassigned > 0 {
                    ui.label(RichText::new(format!("{unassigned} territories in no region")).color(theme::WARN).small());
                }
                ui.horizontal(|ui| {
                    if ui.button("➕ Region").clicked() {
                        actions.push(Action::Edit(
                            "Add region".into(),
                            Box::new(move |p| {
                                if let Some(pl) = p.plan_mut(plan_id) {
                                    pl.add_region(None);
                                }
                                touch(p, plan_id);
                            }),
                        ));
                    }
                    ui.label("Group into");
                    ui.add(egui::DragValue::new(&mut self.region_count).range(1..=n.max(1)));
                    if ui
                        .add_enabled(n > 1, egui::Button::new("regions"))
                        .on_hover_text("Neighbouring territories become regions of about equal size by the measure; existing regions are replaced")
                        .clicked()
                        && let Some(groups) = group_regions(project, stats, plan_id, self.region_count)
                    {
                        let k = self.region_count;
                        actions.push(Action::Edit(
                            "Group into regions".into(),
                            Box::new(move |p| {
                                if let Some(pl) = p.plan_mut(plan_id) {
                                    pl.regions.clear();
                                    let ids: Vec<_> = (0..k).map(|_| pl.add_region(None)).collect();
                                    for (t, g) in pl.territories.iter_mut().zip(&groups) {
                                        t.region = g.and_then(|g| ids.get(g).copied());
                                    }
                                }
                                touch(p, plan_id);
                            }),
                        ));
                    }
                });
            });
    }

    fn details_ui(
        &mut self,
        ui: &mut Ui,
        plan: &territoria_model::Plan,
        t: &territoria_model::Territory,
        actions: &mut Vec<Action>,
    ) {
        let plan_id = plan.id;
        let id = t.id;
        let edit = |label: &str, change: Box<dyn FnOnce(&mut territoria_model::Territory)>| {
            Action::Edit(
                label.to_owned(),
                Box::new(move |p| {
                    if let Some(t) = p.plan_mut(plan_id).and_then(|pl| pl.territory_mut(id)) {
                        change(t);
                    }
                    touch(p, plan_id);
                }),
            )
        };
        egui::CollapsingHeader::new(RichText::new(format!("Details of {}", t.name)).strong())
            .id_salt("territory-details")
            .show(ui, |ui| {
                egui::Grid::new("territory-details-grid")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Name");
                        let mut name = t.name.clone();
                        if ui.add(egui::TextEdit::singleline(&mut name).desired_width(200.0)).changed()
                            && !name.trim().is_empty()
                        {
                            actions.push(edit("Rename territory", Box::new(move |t| t.name = name)));
                        }
                        ui.end_row();
                        ui.label("Code").on_hover_text("What people call it: T01, a rep number, a region code");
                        let mut code = t.code.clone();
                        if ui.add(egui::TextEdit::singleline(&mut code).desired_width(120.0)).changed() {
                            actions.push(edit("Change code", Box::new(move |t| t.code = code)));
                        }
                        ui.end_row();
                        ui.label("Size").on_hover_text("Its share of the total: 1 is a normal territory, 0.5 one for a part-time rep, 2 a double one");
                        let mut weight = t.weight;
                        if ui
                            .add(egui::DragValue::new(&mut weight).range(0.0..=10.0).speed(0.05).fixed_decimals(2))
                            .changed()
                        {
                            actions.push(edit("Change size", Box::new(move |t| t.weight = weight)));
                        }
                        ui.end_row();
                        if !plan.regions.is_empty() {
                            ui.label("Region");
                            let current = t.region.and_then(|r| plan.region(r)).map_or("None".to_owned(), |r| r.name.clone());
                            let mut region = t.region;
                            ComboBox::from_id_salt("territory-region").selected_text(current).show_ui(ui, |ui| {
                                ui.selectable_value(&mut region, None, "None");
                                for r in &plan.regions {
                                    ui.selectable_value(&mut region, Some(r.id), &r.name);
                                }
                            });
                            if region != t.region {
                                actions.push(edit("Move to region", Box::new(move |t| t.region = region)));
                            }
                            ui.end_row();
                        }
                        ui.label("Locked");
                        let mut locked = t.locked;
                        if ui
                            .checkbox(&mut locked, "keeps its areas")
                            .on_hover_text("Painting and balancing leave it as it is")
                            .changed()
                        {
                            actions.push(edit(
                                if locked { "Lock territory" } else { "Unlock territory" },
                                Box::new(move |t| t.locked = locked),
                            ));
                        }
                        ui.end_row();
                        ui.label("Site");
                        ui.horizontal(|ui| {
                            match t.site {
                                Some([lon, lat]) => {
                                    ui.label(RichText::new(format!("{lat:.5}°, {lon:.5}°")).monospace());
                                    if ui.small_button("Remove").clicked() {
                                        actions.push(edit("Remove site", Box::new(|t| t.site = None)));
                                    }
                                }
                                None => {
                                    ui.label(RichText::new("none").weak());
                                }
                            }
                            if ui.small_button("📍 Place on the map").clicked() {
                                actions.push(Action::SetTool(Tool::Site));
                            }
                        });
                        ui.end_row();
                        ui.label("Notes");
                        let mut notes = t.notes.clone();
                        if ui
                            .add(egui::TextEdit::multiline(&mut notes).desired_rows(2).desired_width(f32::INFINITY))
                            .changed()
                        {
                            actions.push(edit("Change notes", Box::new(move |t| t.notes = notes)));
                        }
                        ui.end_row();
                    });
            });
    }

    fn compare_ui(
        &mut self,
        ui: &mut Ui,
        project: &Project,
        values: &mut Cache,
        base: PlanId,
        plan: PlanId,
        actions: &mut Vec<Action>,
    ) {
        let Some(base_plan) = project.plan(base) else {
            self.compare = None;
            return;
        };
        let mut close = false;
        let mut mark = self.mark_changes;
        ui.add_space(4.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Compared with").strong());
                ui.label(&base_plan.name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("✖")
                        .on_hover_text("End the comparison")
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            let Some(c) = self.comparison(project, values, base, plan) else {
                ui.label(
                    RichText::new("The plans are on different areas and can't be compared.").weak(),
                );
                return;
            };
            let r = &c.result;
            if r.moves.is_empty() {
                ui.label(
                    RichText::new("No changes yet. Paint or balance this plan to try changes.")
                        .weak(),
                );
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} areas change territory",
                    format_value(r.moves.len() as f64)
                ));
                for (layer, n) in &r.points {
                    if let Some(l) = project.layer(*layer) {
                        ui.label("·");
                        ui.label(format!("{} {}", format_value(*n as f64), l.name))
                            .on_hover_text("Points in the areas that change territory");
                    }
                }
            });
            ui.checkbox(&mut mark, "Mark them on the map");
            let changed: Vec<_> = r
                .territories
                .iter()
                .filter(|t| t.gained + t.lost > 0)
                .collect();
            egui::Grid::new("compare-grid")
                .num_columns(4)
                .spacing([10.0, 3.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(RichText::new("Territory").weak());
                    ui.label(RichText::new("Areas").weak());
                    ui.label(RichText::new(&r.measure).weak());
                    ui.label("");
                    ui.end_row();
                    for t in &changed {
                        ui.horizontal(|ui| {
                            ui.set_min_width(110.0);
                            ui.set_max_width(170.0);
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                            ui.painter().rect_filled(rect, 2.0, color(t.color));
                            ui.add(egui::Label::new(&t.name).truncate());
                        });
                        let mut areas = Vec::new();
                        if t.gained > 0 {
                            areas.push(format!("+{}", t.gained));
                        }
                        if t.lost > 0 {
                            areas.push(format!("−{}", t.lost));
                        }
                        ui.label(areas.join(" "));
                        ui.label(format!(
                            "{} → {}",
                            format_value(t.values[0]),
                            format_value(t.values[1])
                        ));
                        let change = if t.values[0] != 0.0 {
                            t.values[1] / t.values[0] - 1.0
                        } else {
                            0.0
                        };
                        ui.label(
                            RichText::new(format!(
                                "{}{}",
                                if change > 0.0 { "+" } else { "" },
                                format_percent(change)
                            ))
                            .weak(),
                        );
                        ui.end_row();
                    }
                });
            let same = r.territories.len() - changed.len();
            if same > 0 {
                ui.label(RichText::new(format!("{same} territories unchanged")).weak());
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(format!("Adopt in “{}”", base_plan.name))
                    .on_hover_text("The original plan takes over this scenario's territories")
                    .clicked()
                {
                    actions.push(Action::Edit(
                        "Adopt scenario".into(),
                        Box::new(move |p| {
                            p.adopt_plan(plan, base);
                        }),
                    ));
                }
                if ui.button("Export changes (Excel)…").clicked() {
                    actions.push(Action::Export(Export::Changes(base, plan)));
                }
            });
        });
        self.mark_changes = mark;
        if close {
            self.compare = None;
        }
    }
}
