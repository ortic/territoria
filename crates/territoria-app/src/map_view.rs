//! The map tab: draws a map's layers and handles navigation, hovering and
//! painting territories.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use territoria_analysis::Rgba;
use territoria_geo::{Crs, Transformer};
use territoria_io::GeometryKind;
use territoria_model::{Cache, LayerId, Map, MapLayerKind, PlanId, Project, TerritoryId};
use territoria_render::paint::{self, LabelStyle};

use crate::resolve::{Labels, Resolved};
use territoria_render::{LayerMesh, View};

use crate::jobs::Jobs;
use crate::meshes::{AnalysisCache, MeshCache};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Navigate,
    /// Drag over areas to assign them to the active territory.
    Paint,
    /// Click to place the active territory's site.
    Site,
    /// Click, drag a rectangle or Alt+drag a lasso to select areas.
    Select,
}

/// How a new selection combines with the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectMode {
    Replace,
    Add,
    /// Selected ones leave, others join.
    Toggle,
}

/// UI state of one map tab (not saved in the project).
#[derive(Default)]
pub struct MapState {
    pub view: Option<View>,
    pub hover: Option<Hover>,
    painting: bool,
    last_paint: Option<Pos2>,
    pub fit_requested: bool,
    /// Feature to zoom to on the next frame: (layer, features).
    pub zoom_to: Option<(LayerId, Vec<u32>)>,
    /// Screen points of the rectangle or lasso being dragged.
    selecting: Option<Vec<Pos2>>,
    lasso: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hover {
    pub layer: LayerId,
    pub feature: usize,
    /// Longitude/latitude under the pointer.
    pub lonlat: Option<[f64; 2]>,
}

pub enum MapEvent {
    /// Assign these units to the active territory (or unassign: `None`).
    Paint {
        plan: PlanId,
        units: Vec<u32>,
        to: Option<TerritoryId>,
    },
    /// The paint stroke ended.
    PaintEnd,
    /// Site tool: the active territory's site goes here (lon/lat).
    PlaceSite {
        plan: PlanId,
        lonlat: [f64; 2],
    },
    /// Shift+click in paint mode: make the territory under the pointer active.
    PickTerritory {
        plan: PlanId,
        territory: Option<TerritoryId>,
    },
    /// Clicked a territory's unit while navigating.
    ClickTerritory {
        plan: PlanId,
        territory: Option<TerritoryId>,
    },
    /// The view moved; remember it in the project.
    ViewChanged([f64; 2], f64),
    /// Select tool: these features of `layer`.
    Select {
        layer: LayerId,
        units: Vec<u32>,
        mode: SelectMode,
    },
    /// The selection goes to a territory (`None`: unassigned).
    AssignSelection {
        plan: PlanId,
        to: Option<TerritoryId>,
    },
    ZoomToSelection,
    ClearSelection,
}

pub struct MapContext<'a> {
    pub project: &'a Project,
    pub values: &'a mut Cache,
    pub meshes: &'a mut MeshCache,
    pub analyses: &'a mut AnalysisCache,
    pub territories: &'a mut TerritoryCache,
    pub jobs: &'a mut Jobs,
    pub tiles: &'a mut crate::tiles::Tiles,
    pub tool: Tool,
    pub active_plan: Option<PlanId>,
    pub active_territory: Option<TerritoryId>,
    pub dark: bool,
    /// Units to mark, e.g. the areas a scenario moves.
    pub highlight: Option<(LayerId, &'a [u32])>,
    /// Selected features.
    pub selection: Option<(LayerId, &'a [u32])>,
}

/// The rectangle (first and last point) or lasso of a drag, in screen points.
fn selection_shape(path: &[Pos2], lasso: bool) -> Vec<Pos2> {
    match (path.first(), path.last()) {
        (Some(_), Some(_)) if lasso => path.to_vec(),
        (Some(a), Some(b)) => {
            let r = Rect::from_two_pos(*a, *b);
            vec![
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
            ]
        }
        _ => Vec::new(),
    }
}

/// Even-odd point in polygon.
fn inside(poly: &[Pos2], p: Pos2) -> bool {
    let mut odd = false;
    let mut j = poly.len().wrapping_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            odd = !odd;
        }
        j = i;
    }
    odd
}

/// Per plan: unit → territory index and where territory labels go.
pub struct TerritoryRender {
    pub rev: u64,
    pub unit_territory: Vec<Option<u16>>,
    pub label_units: Vec<Option<u32>>,
}

#[derive(Default)]
pub struct TerritoryCache {
    entries: HashMap<PlanId, Arc<TerritoryRender>>,
}

impl TerritoryCache {
    pub fn get(&mut self, project: &Project, plan: PlanId) -> Option<Arc<TerritoryRender>> {
        let p = project.plan(plan)?;
        if let Some(r) = self.entries.get(&plan)
            && r.rev == p.rev
        {
            return Some(r.clone());
        }
        let layer = project.layer(p.layer)?;
        let index: HashMap<TerritoryId, u16> = p
            .territories
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, i as u16))
            .collect();
        let unit_territory: Vec<Option<u16>> = p
            .assignment
            .iter()
            .map(|t| t.and_then(|t| index.get(&t).copied()))
            .collect();
        // Label each territory at the unit nearest to its area-weighted centre.
        let g = &layer.geometry;
        let mut sums = vec![(0.0f64, 0.0f64, 0.0f64); p.territories.len()];
        for (u, t) in unit_territory.iter().enumerate() {
            if let (Some(t), Some(lp)) = (t, g.label_points.get(u).copied().flatten()) {
                let w = g.areas.get(u).copied().unwrap_or(1.0).max(1e-9);
                let s = &mut sums[*t as usize];
                s.0 += lp.x * w;
                s.1 += lp.y * w;
                s.2 += w;
            }
        }
        let mut label_units: Vec<Option<(u32, f64)>> = vec![None; p.territories.len()];
        for (u, t) in unit_territory.iter().enumerate() {
            let (Some(t), Some(lp)) = (t, g.label_points.get(u).copied().flatten()) else {
                continue;
            };
            let s = sums[*t as usize];
            let (cx, cy) = (s.0 / s.2, s.1 / s.2);
            // Prefer big units: labels need room.
            let d = ((lp.x - cx).powi(2) + (lp.y - cy).powi(2)).sqrt()
                / g.areas[u].max(1e-9).sqrt().max(1e-6);
            let slot = &mut label_units[*t as usize];
            if slot.is_none_or(|(_, best)| d < best) {
                *slot = Some((u as u32, d));
            }
        }
        let r = Arc::new(TerritoryRender {
            rev: p.rev,
            unit_territory,
            label_units: label_units.into_iter().map(|s| s.map(|(u, _)| u)).collect(),
        });
        self.entries.insert(plan, r.clone());
        Some(r)
    }
}

pub fn color(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), c.a())
}

/// Draws `map` and handles input. Returns what the app has to do.
pub fn show(ui: &mut Ui, map: &Map, state: &mut MapState, cx: &mut MapContext) -> Vec<MapEvent> {
    let mut events = Vec::new();
    let project: &Project = cx.project;
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let bg = if cx.dark {
        Color32::from_rgb(0x1c, 0x1f, 0x24)
    } else {
        Color32::from_rgb(0xf3, 0xf1, 0xec)
    };
    painter.rect_filled(rect, 0.0, bg);

    // Meshes of the visible layers.
    let mut meshes: HashMap<LayerId, Arc<LayerMesh>> = HashMap::new();
    let mut cells: HashMap<LayerId, Arc<LayerMesh>> = HashMap::new();
    let mut missing = false;
    for ml in map.layers.iter().filter(|l| l.visible) {
        let layer_id = match &ml.kind {
            MapLayerKind::Territories { plan, .. } => project.plan(*plan).map(|p| p.layer),
            k => k.layer(),
        };
        let Some(layer) = layer_id.and_then(|id| project.layer(id)) else {
            continue;
        };
        match cx.meshes.get(layer, &map.crs, cx.jobs) {
            Some(m) => {
                meshes.insert(layer.id, m);
            }
            None => missing = true,
        }
        // Plans on points are drawn as the points' Voronoi cells.
        if matches!(ml.kind, MapLayerKind::Territories { .. })
            && layer.kind() == GeometryKind::Point
        {
            match cx.meshes.get_cells(layer, &map.crs, cx.jobs) {
                Some(m) => {
                    cells.insert(layer.id, m);
                }
                None => missing = true,
            }
        }
    }
    if meshes.is_empty() {
        let text = if missing {
            "Preparing the map…"
        } else if map.layers.is_empty() {
            "This map is empty. Add boundaries from the Contents panel."
        } else {
            "All layers are hidden."
        };
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(15.0),
            ui.visuals().weak_text_color(),
        );
        if missing {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        return events;
    }

    // The view: restored, fitted, or kept.
    let mut view = match state.view {
        Some(mut v) => {
            v.rect = rect;
            v
        }
        None => match map.view {
            Some((center, scale)) => View {
                center,
                scale,
                rect,
            },
            None => {
                state.fit_requested = true;
                View {
                    center: [0.0, 0.0],
                    scale: 1.0,
                    rect,
                }
            }
        },
    };
    if state.fit_requested {
        if let Some((min, max)) = extent(meshes.values()) {
            view = View::fit(min, max, rect);
        }
        state.fit_requested = false;
    }
    if let Some((layer, features)) = state.zoom_to.take()
        && let Some(m) = meshes.get(&layer)
    {
        let mut b = territoria_render::Bounds::EMPTY;
        for &f in &features {
            if let Some(fb) = m.bounds.get(f as usize).filter(|b| !b.is_empty()) {
                b.add(fb.min);
                b.add(fb.max);
            }
        }
        if !b.is_empty() {
            let min = m.to_display(b.min);
            let max = m.to_display(b.max);
            let fitted = View::fit(min, max, rect);
            view.center = fitted.center;
            view.scale = fitted.scale * 1.25;
        }
    }

    // Navigation: wheel zoom, drag pan (or paint), double-click zoom.
    let ppp = ui.ctx().pixels_per_point();
    let pointer = response.hover_pos();
    let modifiers = ui.input(|i| i.modifiers);
    let painting_tool = cx.tool == Tool::Paint && cx.active_plan.is_some();
    let selecting_tool = cx.tool == Tool::Select;
    if let Some(p) = pointer {
        let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        let factor = zoom as f64 * (scroll.y as f64 / 300.0).exp();
        if (factor - 1.0).abs() > 1e-6 {
            view.zoom_at(p, factor);
        }
    }
    let pan_button = ui.input(|i| {
        i.pointer.button_down(egui::PointerButton::Middle)
            || i.pointer.button_down(egui::PointerButton::Secondary)
    });
    let space = ui.input(|i| i.key_down(egui::Key::Space));
    if response.dragged() && (!(painting_tool || selecting_tool) || pan_button || space) {
        view.pan(response.drag_delta());
    }
    if response.double_clicked()
        && !painting_tool
        && !selecting_tool
        && let Some(p) = pointer
    {
        view.zoom_at(p, 2.0);
    }
    if state.view.map(|v| (v.center, v.scale)) != Some((view.center, view.scale)) {
        events.push(MapEvent::ViewChanged(view.center, view.scale));
    }
    state.view = Some(view);

    // The background map, under everything.
    if let territoria_model::map::Background::Tiles { url, .. } = &map.background
        && map.crs.is_web_mercator()
    {
        let tint = if cx.dark {
            Color32::from_gray(200)
        } else {
            Color32::WHITE
        };
        cx.tiles.paint(&painter, &view, url, ppp, tint);
    }

    // Draw the layers bottom to top.
    let mut shapes: Vec<Shape> = Vec::new();
    let text_color = if cx.dark {
        Color32::from_rgb(0xe8, 0xea, 0xee)
    } else {
        Color32::from_rgb(0x20, 0x23, 0x28)
    };
    let halo = if cx.dark {
        Color32::from_black_alpha(170)
    } else {
        Color32::from_white_alpha(200)
    };
    let mut rcx = crate::resolve::Context {
        project,
        values: cx.values,
        analyses: cx.analyses,
        territories: cx.territories,
        dark: cx.dark,
    };
    let mut resolved = Vec::new();
    for ml in map.layers.iter().filter(|l| l.visible) {
        for r in crate::resolve::resolve(&mut rcx, &meshes, &cells, ml, &map.crs) {
            draw_resolved(&mut shapes, &r, &view, ppp);
            resolved.push(r);
        }
    }

    // Marked units: outlined and tinted.
    if let Some((layer, units)) = cx.highlight
        && let Some(mesh) = cells.get(&layer).or(meshes.get(&layer))
    {
        let level = paint::level(mesh, &view, ppp);
        let c = Color32::from_rgb(0xd9, 0x46, 0xef);
        paint::highlight(
            &mut shapes,
            mesh,
            level,
            &view,
            units,
            c.gamma_multiply(0.35),
        );
        for &u in units {
            paint::outline(
                &mut shapes,
                mesh,
                level,
                &view,
                u as usize,
                Stroke::new(2.0, c),
            );
        }
    }

    // The selection: tinted and outlined.
    if let Some((layer, units)) = cx.selection
        && let Some(mesh) = meshes.get(&layer)
    {
        let level = paint::level(mesh, &view, ppp);
        let c = crate::theme::ACCENT;
        paint::highlight(
            &mut shapes,
            mesh,
            level,
            &view,
            units,
            c.gamma_multiply(0.3),
        );
        for &u in units {
            paint::outline(
                &mut shapes,
                mesh,
                level,
                &view,
                u as usize,
                Stroke::new(1.5, c),
            );
        }
    }

    // Hover and painting.
    let interactive = interactive_layer(map, cx, painting_tool || selecting_tool);
    state.hover = None;
    if let (Some(p), Some((layer_id, plan))) = (pointer, interactive)
        && let Some(layer) = project.layer(layer_id)
        // Plans on points are worked on through the points' cells.
        && let Some(mesh) = cells.get(&layer_id).filter(|_| plan.is_some()).or(meshes.get(&layer_id))
        && let Ok(to_wgs) = Transformer::new(&map.crs, &Crs::wgs84())
    {
        let lookup = |s: Pos2| -> (Option<usize>, Option<[f64; 2]>) {
            let d = view.to_display(s);
            let Some(c) = (if layer.geometry.geographic {
                to_wgs.convert(geo_types::Coord { x: d[0], y: d[1] })
            } else {
                Some(geo_types::Coord { x: d[0], y: d[1] })
            }) else {
                return (None, None);
            };
            let tol = {
                // Points are small: a wider brush finds them.
                let brush = if layer.kind() == GeometryKind::Point {
                    10.0
                } else {
                    6.0
                };
                let d2 = view.to_display(s + vec2(brush, 0.0));
                let c2 = if layer.geometry.geographic {
                    to_wgs.convert(geo_types::Coord { x: d2[0], y: d2[1] })
                } else {
                    Some(geo_types::Coord { x: d2[0], y: d2[1] })
                };
                c2.map_or(0.0, |c2| (c2.x - c.x).abs())
            };
            (mesh.geometry.feature_at(c, tol), Some([c.x, c.y]))
        };
        let (feature, lonlat) = lookup(p);
        if let Some(f) = feature {
            state.hover = Some(Hover {
                layer: layer_id,
                feature: f,
                lonlat,
            });
            let level = paint::level(mesh, &view, ppp);
            let hl = if cx.dark {
                Color32::WHITE
            } else {
                Color32::from_rgb(0x10, 0x12, 0x16)
            };
            paint::outline(&mut shapes, mesh, level, &view, f, Stroke::new(2.0, hl));
        }
        if cx.tool == Tool::Site
            && let Some(plan) = cx.active_plan
        {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            if response.clicked()
                && let Some(lonlat) = lonlat
            {
                events.push(MapEvent::PlaceSite { plan, lonlat });
            }
        } else if let Some(plan) = plan.filter(|_| painting_tool) {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            let shift_click = response.clicked() && modifiers.shift;
            if shift_click {
                let t = feature.and_then(|f| {
                    project
                        .plan(plan)
                        .and_then(|p| p.assignment.get(f).copied().flatten())
                });
                events.push(MapEvent::PickTerritory { plan, territory: t });
            } else if (response.dragged_by(egui::PointerButton::Primary) || response.clicked())
                && !space
                && !modifiers.shift
            {
                // Sample along the pointer's path so fast strokes leave no gaps.
                let from = state.last_paint.unwrap_or(p);
                let steps = ((p - from).length() / 4.0).ceil().max(1.0) as usize;
                let mut units = Vec::new();
                for i in 0..=steps {
                    let s = from + (p - from) * (i as f32 / steps as f32);
                    if let (Some(f), _) = lookup(s) {
                        units.push(f as u32);
                    }
                }
                units.sort_unstable();
                units.dedup();
                let to = if modifiers.alt {
                    None
                } else {
                    cx.active_territory
                };
                if !units.is_empty() && (to.is_some() || modifiers.alt) {
                    events.push(MapEvent::Paint { plan, units, to });
                }
                state.painting = true;
                state.last_paint = Some(p);
            }
        } else if response.clicked()
            && let Some(plan) = plan
        {
            let t = feature.and_then(|f| {
                project
                    .plan(plan)
                    .and_then(|p| p.assignment.get(f).copied().flatten())
            });
            events.push(MapEvent::ClickTerritory { plan, territory: t });
        }
    }
    // Selecting: click, drag a rectangle, Alt+drag a lasso.
    if selecting_tool
        && let Some((layer_id, _)) = interactive
        && let Some(mesh) = meshes.get(&layer_id)
    {
        let add = modifiers.shift || modifiers.command;
        if response.drag_started_by(egui::PointerButton::Primary)
            && !space
            && let Some(p) = response.interact_pointer_pos()
        {
            state.selecting = Some(vec![p]);
            state.lasso = modifiers.alt;
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let (Some(path), Some(p)) = (&mut state.selecting, pointer)
            && path.last().is_none_or(|l| l.distance(p) > 2.0)
        {
            path.push(p);
        }
        if let Some(path) = &state.selecting {
            let poly = selection_shape(path, state.lasso);
            let c = crate::theme::ACCENT;
            if poly.len() > 2 {
                shapes.push(Shape::Path(egui::epaint::PathShape {
                    points: poly.clone(),
                    closed: true,
                    fill: if state.lasso {
                        Color32::TRANSPARENT
                    } else {
                        c.gamma_multiply(0.12)
                    },
                    stroke: egui::epaint::PathStroke::new(1.5, c),
                }));
            }
        }
        if response.drag_stopped()
            && let Some(path) = state.selecting.take()
        {
            let poly = selection_shape(&path, state.lasso);
            let units = mesh
                .label_points
                .iter()
                .enumerate()
                .filter_map(|(i, lp)| {
                    let s = view.to_screen(mesh.to_display((*lp)?));
                    inside(&poly, s).then_some(i as u32)
                })
                .collect();
            let mode = if add {
                SelectMode::Add
            } else {
                SelectMode::Replace
            };
            events.push(MapEvent::Select {
                layer: layer_id,
                units,
                mode,
            });
        } else if response.clicked() {
            let units = state
                .hover
                .as_ref()
                .filter(|h| h.layer == layer_id)
                .map(|h| vec![h.feature as u32])
                .unwrap_or_default();
            let mode = if add {
                SelectMode::Toggle
            } else {
                SelectMode::Replace
            };
            events.push(MapEvent::Select {
                layer: layer_id,
                units,
                mode,
            });
        }
        if state.selecting.is_none() {
            ui.ctx().set_cursor_icon(CursorIcon::Default);
        }
    }

    if state.painting && !response.dragged() && !ui.input(|i| i.pointer.primary_down()) {
        state.painting = false;
        state.last_paint = None;
        events.push(MapEvent::PaintEnd);
    }

    painter.extend(shapes);
    // A card with what's known about the area under the pointer.
    if cx.tool == Tool::Navigate
        && !response.dragged()
        && let Some(h) = state.hover.clone()
    {
        let rows = hover_rows(map, project, cx.values, &h);
        if !rows.is_empty() {
            response.clone().on_hover_ui_at_pointer(|ui| {
                ui.set_max_width(320.0);
                for (i, (label, value)) in rows.iter().enumerate() {
                    if i == 0 {
                        ui.label(egui::RichText::new(value).strong());
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(label).weak());
                            ui.label(value);
                        });
                    }
                }
            });
        }
    }
    let mut occupied = Vec::new();
    // Top layers label first: they win collisions.
    for r in resolved.iter().rev() {
        draw_labels(&painter, r, &view, text_color, halo, &mut occupied);
    }
    for r in &resolved {
        draw_sites(&painter, r, &view, cx.dark);
    }
    scale_bar(&painter, &view, &map.crs, cx.dark);
    attribution(&painter, &view, map, project, cx.dark);
    if let Some((layer, units)) = cx.selection
        && !units.is_empty()
    {
        selection_bar(ui, rect, map, cx, layer, units, &mut events);
    }
    events
}

/// A bar over the map for the selection: its size, what it adds up to,
/// and what can be done with it.
fn selection_bar(
    ui: &mut Ui,
    rect: Rect,
    map: &Map,
    cx: &mut MapContext,
    layer: LayerId,
    units: &[u32],
    events: &mut Vec<MapEvent>,
) {
    let project = cx.project;
    let plan = cx
        .active_plan
        .and_then(|p| project.plan(p))
        .filter(|p| p.layer == layer);
    let noun = match project.layer(layer).map(|l| l.kind()) {
        Some(GeometryKind::Point) => "points",
        Some(GeometryKind::Line) => "lines",
        _ => "areas",
    };
    // The plan's measure over the selection.
    let total = plan
        .and_then(|p| p.measures.first())
        .filter(|m| m.source != territoria_model::ValueSource::Units)
        .map(|m| {
            let values = cx.values.values(project, layer, &m.source);
            let sum: f64 = units
                .iter()
                .filter_map(|&u| values.get(u as usize).copied().flatten())
                .sum();
            (sum, m.source.label(project))
        });
    egui::Area::new(egui::Id::new(("selection-bar", map.id)))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(rect.center().x, rect.bottom() - 48.0))
        .pivot(Align2::CENTER_BOTTOM)
        .constrain_to(rect)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let count = territoria_analysis::format::format_value(units.len() as f64);
                    ui.label(egui::RichText::new(format!("{count} {noun} selected")).strong());
                    if let Some((sum, label)) = &total {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} {label}",
                                territoria_analysis::format::format_value(*sum)
                            ))
                            .weak(),
                        );
                    }
                    if let Some(p) = plan {
                        ui.separator();
                        if let Some(t) = cx.active_territory.and_then(|t| p.territory(t)) {
                            let r = ui.button(format!("Assign to {}", t.name)).on_hover_text(
                                "The selected areas join the active territory (locked ones stay)",
                            );
                            let swatch = Rect::from_center_size(
                                r.rect.left_center() + vec2(-6.0, 0.0),
                                vec2(8.0, 8.0),
                            );
                            ui.painter().rect_filled(swatch, 2.0, color(t.color));
                            if r.clicked() {
                                events.push(MapEvent::AssignSelection {
                                    plan: p.id,
                                    to: Some(t.id),
                                });
                            }
                        }
                        if ui.button("Unassign").clicked() {
                            events.push(MapEvent::AssignSelection {
                                plan: p.id,
                                to: None,
                            });
                        }
                    }
                    ui.separator();
                    if ui.button("Zoom to").clicked() {
                        events.push(MapEvent::ZoomToSelection);
                    }
                    if ui
                        .button("✖")
                        .on_hover_text("Clear the selection (Esc)")
                        .clicked()
                    {
                        events.push(MapEvent::ClearSelection);
                    }
                });
            });
        });
}

/// Title and (label, value) rows for the hover card: the area, then what
/// each visible map layer says about it.
fn hover_rows(
    map: &Map,
    project: &Project,
    values: &mut Cache,
    h: &Hover,
) -> Vec<(String, String)> {
    let Some(layer) = project.layer(h.layer) else {
        return Vec::new();
    };
    // A layer's own hover text replaces the name and the values.
    let custom = map
        .layers
        .iter()
        .rev()
        .filter(|l| l.visible && l.kind.layer() == Some(h.layer))
        .find_map(|l| l.kind.style().and_then(|s| s.tooltip.clone()));
    let mut rows = match &custom {
        Some(t) => {
            let text = values
                .template_texts(project, h.layer, t)
                .and_then(|texts| texts.get(h.feature).cloned().flatten())
                .unwrap_or_else(|| layer.display_name(h.feature));
            text.lines()
                .map(|l| (String::new(), l.to_owned()))
                .collect()
        }
        None => vec![(String::new(), layer.display_name(h.feature))],
    };
    for ml in map.layers.iter().rev().filter(|l| l.visible) {
        match &ml.kind {
            MapLayerKind::Choropleth { .. } | MapLayerKind::Categories { .. }
                if custom.is_some() => {}
            MapLayerKind::Choropleth {
                layer: l,
                source,
                normalize,
                ..
            } if *l == h.layer => {
                let v = values
                    .normalized(project, *l, source, normalize)
                    .get(h.feature)
                    .copied()
                    .flatten();
                let text = v.map_or(
                    "no data".to_owned(),
                    territoria_analysis::format::format_value,
                );
                rows.push((ml.name.clone(), text));
            }
            MapLayerKind::Symbols {
                layer: l,
                source,
                normalize,
                ..
            } if *l == h.layer && custom.is_none() => {
                let v = values
                    .normalized(project, *l, source, normalize)
                    .get(h.feature)
                    .copied()
                    .flatten();
                rows.push((
                    ml.name.clone(),
                    v.map_or(
                        "no data".to_owned(),
                        territoria_analysis::format::format_value,
                    ),
                ));
            }
            MapLayerKind::Categories {
                layer: l, source, ..
            } if *l == h.layer => {
                let v = values
                    .texts(project, *l, source)
                    .get(h.feature)
                    .cloned()
                    .flatten();
                rows.push((ml.name.clone(), v.unwrap_or_else(|| "no data".into())));
            }
            MapLayerKind::Territories { plan, .. } => {
                if let Some(p) = project.plan(*plan).filter(|p| p.layer == h.layer) {
                    let t = p
                        .assignment
                        .get(h.feature)
                        .copied()
                        .flatten()
                        .and_then(|t| p.territory(t));
                    rows.push((
                        p.name.clone(),
                        t.map_or("unassigned".into(), |t| t.name.clone()),
                    ));
                }
            }
            _ => {}
        }
    }
    rows
}

/// Draws a resolved layer's shapes (not its labels).
pub fn draw_resolved(shapes: &mut Vec<Shape>, r: &Resolved, view: &View, ppp: f32) {
    let mesh = &r.mesh;
    let level = paint::level(mesh, view, ppp);
    match mesh.kind {
        GeometryKind::Area => {
            paint::fills(shapes, mesh, level, view, |f| {
                r.fills.get(f).copied().flatten()
            });
            if !r.arcs.is_empty() {
                paint::arcs(shapes, mesh, level, view, |i| {
                    r.arcs.get(i).copied().flatten()
                });
            }
        }
        GeometryKind::Line => {
            paint::lines(shapes, mesh, level, view, |f| {
                r.fills
                    .get(f)
                    .copied()
                    .flatten()
                    .map(|c| Stroke::new(r.line_width, c))
            });
        }
        GeometryKind::Point => {
            paint::points(shapes, mesh, view, |f| {
                r.fills
                    .get(f)
                    .copied()
                    .flatten()
                    .map(|c| (r.point_radius, c, r.point_stroke))
            });
        }
    }
    // Proportional circles, largest first so small ones stay on top.
    for &(f, radius) in &r.symbols {
        let Some(lp) = mesh.label_points.get(f).copied().flatten() else {
            continue;
        };
        let p = view.to_screen(mesh.to_display(lp));
        if view.rect.expand(radius).contains(p) {
            shapes.push(Shape::circle_filled(p, radius, r.symbol_fill));
            shapes.push(Shape::circle_stroke(p, radius, r.symbol_stroke));
        }
    }
}

/// Site markers, drawn above labels.
fn draw_sites(painter: &egui::Painter, r: &Resolved, view: &View, dark: bool) {
    let border = crate::resolve::territory_border(dark);
    for (d, c) in &r.sites {
        let s = view.to_screen(*d);
        painter.circle_filled(s, 6.0, Color32::WHITE);
        painter.circle_filled(s, 4.5, *c);
        painter.circle_stroke(s, 6.0, Stroke::new(1.4, border));
    }
}

fn draw_labels(
    painter: &egui::Painter,
    r: &Resolved,
    view: &View,
    text: Color32,
    halo: Color32,
    occupied: &mut Vec<Rect>,
) {
    match &r.labels {
        Labels::None => {}
        Labels::Features(texts) => {
            let ls = LabelStyle {
                font: FontId::proportional(11.0),
                color: text,
                halo,
                min_width: 28.0,
            };
            paint::labels(painter, &r.mesh, view, &ls, occupied, |f| {
                texts.get(f).cloned().flatten()
            });
        }
        Labels::Territories(names) => {
            let font = FontId::proportional(13.0);
            for (unit, name) in names {
                let Some(lp) = r.mesh.label_points.get(*unit as usize).copied().flatten() else {
                    continue;
                };
                let at = view.to_screen(r.mesh.to_display(lp));
                if !view.rect.contains(at) {
                    continue;
                }
                let galley = painter.layout_no_wrap(name.clone(), font.clone(), text);
                let rect = Align2::CENTER_CENTER
                    .anchor_size(at, galley.size())
                    .expand(3.0);
                if occupied.iter().any(|o| o.intersects(rect)) {
                    continue;
                }
                occupied.push(rect);
                painter.rect_filled(rect, 4.0, halo);
                painter.galley(rect.min + vec2(3.0, 3.0), galley, text);
            }
        }
    }
}

/// The layer hover and painting act on: the active plan's layer when
/// painting, else the top visible layer.
fn interactive_layer(
    map: &Map,
    cx: &MapContext,
    painting: bool,
) -> Option<(LayerId, Option<PlanId>)> {
    if painting && let Some(plan) = cx.active_plan.and_then(|p| cx.project.plan(p)) {
        return Some((plan.layer, Some(plan.id)));
    }
    map.layers
        .iter()
        .rev()
        .filter(|l| l.visible)
        .find_map(|l| match &l.kind {
            MapLayerKind::Territories { plan, .. } => {
                cx.project.plan(*plan).map(|p| (p.layer, Some(*plan)))
            }
            k => k.layer().map(|id| (id, None)),
        })
}

fn extent<'a>(meshes: impl Iterator<Item = &'a Arc<LayerMesh>>) -> Option<([f64; 2], [f64; 2])> {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for m in meshes {
        if m.extent.is_empty() {
            continue;
        }
        let a = m.to_display(m.extent.min);
        let b = m.to_display(m.extent.max);
        min = [min[0].min(a[0]), min[1].min(a[1])];
        max = [max[0].max(b[0]), max[1].max(b[1])];
    }
    min[0].is_finite().then_some((min, max))
}

/// Credits of the visible layers' data, bottom left.
fn attribution(painter: &egui::Painter, view: &View, map: &Map, project: &Project, dark: bool) {
    let mut credits: Vec<String> = Vec::new();
    if let territoria_model::map::Background::Tiles { attribution, .. } = &map.background
        && !attribution.is_empty()
    {
        credits.push(attribution.clone());
    }
    for ml in map.layers.iter().filter(|l| l.visible) {
        let layer = match &ml.kind {
            MapLayerKind::Territories { plan, .. } => {
                project.plan(*plan).and_then(|p| project.layer(p.layer))
            }
            k => k.layer().and_then(|l| project.layer(l)),
        };
        if let Some(a) = layer.and_then(|l| l.source.attribution.clone())
            && !credits.contains(&a)
        {
            credits.push(a);
        }
    }
    if credits.is_empty() {
        return;
    }
    let fg = if dark {
        Color32::from_rgb(0xc8, 0xcb, 0xd0)
    } else {
        Color32::from_rgb(0x50, 0x54, 0x5a)
    };
    let galley = painter.layout(
        credits.join(" · "),
        FontId::proportional(10.0),
        fg,
        view.rect.width() * 0.6,
    );
    let pos = view.rect.left_bottom() + vec2(8.0, -8.0 - galley.size().y);
    let bg = Rect::from_min_size(pos - vec2(4.0, 2.0), galley.size() + vec2(8.0, 4.0));
    painter.rect_filled(
        bg,
        3.0,
        if dark {
            Color32::from_black_alpha(120)
        } else {
            Color32::from_white_alpha(170)
        },
    );
    painter.galley(pos, galley, fg);
}

/// A scale bar in the bottom right corner, true to ground distance at the
/// view's centre.
fn scale_bar(painter: &egui::Painter, view: &View, crs: &Crs, dark: bool) {
    let Ok(t) = Transformer::new(crs, &Crs::wgs84()) else {
        return;
    };
    let c = view.rect.center();
    let a = view.to_display(c);
    let b = view.to_display(c + vec2(100.0, 0.0));
    let (Some(a), Some(b)) = (
        t.convert(geo_types::Coord { x: a[0], y: a[1] }),
        t.convert(geo_types::Coord { x: b[0], y: b[1] }),
    ) else {
        return;
    };
    let metres_per_point = if crs.is_geographic() || crs.epsg.is_some() || !crs.proj.is_empty() {
        territoria_geo::haversine(a, b) / 100.0
    } else {
        return;
    };
    if !metres_per_point.is_finite() || metres_per_point <= 0.0 {
        return;
    }
    let target = metres_per_point * 110.0;
    let nice = territoria_analysis::classify::nice_step(target);
    let len = (nice / metres_per_point) as f32;
    let label = if nice >= 1000.0 {
        format!(
            "{} km",
            territoria_analysis::format::format_value(nice / 1000.0)
        )
    } else {
        format!("{} m", territoria_analysis::format::format_value(nice))
    };
    let fg = if dark {
        Color32::from_rgb(0xe0, 0xe2, 0xe6)
    } else {
        Color32::from_rgb(0x30, 0x33, 0x38)
    };
    let base = view.rect.right_bottom() + vec2(-16.0, -14.0);
    let left = pos2(base.x - len, base.y);
    let bg = view.rect.right_bottom() + vec2(-24.0 - len, -34.0);
    painter.rect_filled(
        Rect::from_min_max(bg, view.rect.right_bottom() + vec2(-8.0, -6.0)),
        4.0,
        if dark {
            Color32::from_black_alpha(120)
        } else {
            Color32::from_white_alpha(170)
        },
    );
    painter.line_segment([left, base], Stroke::new(2.0, fg));
    painter.line_segment([left, left - vec2(0.0, 5.0)], Stroke::new(2.0, fg));
    painter.line_segment([base, base - vec2(0.0, 5.0)], Stroke::new(2.0, fg));
    painter.text(
        pos2((left.x + base.x) / 2.0, base.y - 6.0),
        Align2::CENTER_BOTTOM,
        label,
        FontId::proportional(11.0),
        fg,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lasso_and_rectangle() {
        let rect = selection_shape(
            &[pos2(10.0, 10.0), pos2(5.0, 30.0), pos2(40.0, 20.0)],
            false,
        );
        assert_eq!(rect.len(), 4, "a rectangle from first to last point");
        assert!(inside(&rect, pos2(20.0, 15.0)));
        assert!(!inside(&rect, pos2(20.0, 25.0)));
        // An L-shaped lasso: the notch is outside.
        let lasso = [
            pos2(0.0, 0.0),
            pos2(10.0, 0.0),
            pos2(10.0, 4.0),
            pos2(4.0, 4.0),
            pos2(4.0, 10.0),
            pos2(0.0, 10.0),
        ];
        let l = selection_shape(&lasso, true);
        assert!(inside(&l, pos2(2.0, 8.0)));
        assert!(inside(&l, pos2(8.0, 2.0)));
        assert!(!inside(&l, pos2(8.0, 8.0)));
    }
}
