//! What a map layer draws, worked out from the project and the caches:
//! per-feature colours, per-border strokes, labels and sites. The map view
//! and the exporters draw from this, so screen and file look the same.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, Stroke};
use territoria_geo::{Crs, OUTSIDE, Transformer};
use territoria_io::GeometryKind;
use territoria_model::{Cache, LabelMode, LayerId, MapLayer, MapLayerKind, Project};
use territoria_render::LayerMesh;

use crate::map_view::{TerritoryCache, color};
use crate::meshes::AnalysisCache;

pub enum Labels {
    None,
    /// Per feature, its label.
    Features(Arc<Vec<Option<String>>>),
    /// Territory names at a unit each: (unit, name).
    Territories(Vec<(u32, String)>),
}

pub struct Resolved {
    pub mesh: Arc<LayerMesh>,
    /// Per feature its fill (areas, points) or colour (lines).
    pub fills: Vec<Option<Color32>>,
    /// Areas: per topology arc its stroke.
    pub arcs: Vec<Option<Stroke>>,
    /// Lines: width; points: radius and outline.
    pub line_width: f32,
    pub point_radius: f32,
    pub point_stroke: Stroke,
    pub labels: Labels,
    /// Territory sites in display coordinates with their colour.
    pub sites: Vec<([f64; 2], Color32)>,
    /// Proportional circles: (feature, radius in points), largest first.
    pub symbols: Vec<(usize, f32)>,
    pub symbol_fill: Color32,
    pub symbol_stroke: Stroke,
}

pub fn with_opacity(c: Color32, opacity: f32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (a as f32 * opacity.clamp(0.0, 1.0)).round() as u8)
}

/// Border colour of territories for the theme.
pub fn territory_border(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xee, 0xee, 0xf2)
    } else {
        Color32::from_rgb(0x2a, 0x2d, 0x33)
    }
}

pub struct Context<'a> {
    pub project: &'a Project,
    pub values: &'a mut Cache,
    pub analyses: &'a mut AnalysisCache,
    pub territories: &'a mut TerritoryCache,
    pub dark: bool,
}

/// What `ml` draws: usually one layer; a plan on points draws their
/// Voronoi cells (from `cells`) under the points.
pub fn resolve(
    cx: &mut Context,
    meshes: &HashMap<LayerId, Arc<LayerMesh>>,
    cells: &HashMap<LayerId, Arc<LayerMesh>>,
    ml: &MapLayer,
    crs: &Crs,
) -> Vec<Resolved> {
    resolve_one(cx, meshes, cells, ml, crs).unwrap_or_default()
}

fn resolve_one(
    cx: &mut Context,
    meshes: &HashMap<LayerId, Arc<LayerMesh>>,
    cells: &HashMap<LayerId, Arc<LayerMesh>>,
    ml: &MapLayer,
    crs: &Crs,
) -> Option<Vec<Resolved>> {
    let project = cx.project;
    let opacity = ml.opacity;
    match &ml.kind {
        MapLayerKind::Boundaries { layer, style }
        | MapLayerKind::Choropleth { layer, style, .. }
        | MapLayerKind::Categories { layer, style, .. } => {
            let l = project.layer(*layer)?;
            let mesh = meshes.get(layer)?.clone();
            let n = l.len();
            let fills: Vec<Option<Color32>> = match &ml.kind {
                MapLayerKind::Boundaries { .. } => {
                    vec![Some(with_opacity(color(style.fill), opacity)); n]
                }
                _ => {
                    let scaled = cx.analyses.get(project, cx.values, ml.id, &ml.kind)?;
                    (0..n)
                        .map(|f| {
                            scaled
                                .colors
                                .get(f)
                                .copied()
                                .flatten()
                                .map(|c| with_opacity(color(c), opacity))
                        })
                        .collect()
                }
            };
            let stroke = Stroke::new(
                style.stroke_width,
                with_opacity(color(style.stroke), opacity),
            );
            let arcs = if l.kind() == GeometryKind::Area && style.stroke_width > 0.0 {
                l.geometry
                    .topology
                    .arcs
                    .iter()
                    .map(|a| (a.a != a.b).then_some(stroke))
                    .collect()
            } else {
                Vec::new()
            };
            let labels = match &style.labels {
                LabelMode::None => Labels::None,
                LabelMode::Template(t) => cx
                    .values
                    .template_texts(project, *layer, t)
                    .map_or(Labels::None, Labels::Features),
                mode => {
                    Labels::Features(Arc::new((0..n).map(|f| label_text(l, mode, f)).collect()))
                }
            };
            Some(vec![Resolved {
                mesh,
                fills,
                arcs,
                line_width: style.stroke_width.max(1.0),
                point_radius: style.point_size,
                point_stroke: stroke,
                labels,
                sites: Vec::new(),
                symbols: Vec::new(),
                symbol_fill: Color32::TRANSPARENT,
                symbol_stroke: Stroke::NONE,
            }])
        }
        MapLayerKind::Territories { plan, style } => {
            let p = project.plan(*plan)?;
            let points = project.layer(p.layer)?.kind() == GeometryKind::Point;
            // Points: the territories are drawn as their Voronoi cells.
            let mesh = if points {
                cells.get(&p.layer)?
            } else {
                meshes.get(&p.layer)?
            }
            .clone();
            let tr = cx.territories.get(project, *plan)?;
            let territory_fill: Vec<Color32> = p
                .territories
                .iter()
                .map(|t| with_opacity(color(t.color), style.fill_opacity * opacity))
                .collect();
            let unassigned = with_opacity(Color32::from_rgb(0xd8, 0xd8, 0xdc), 0.35 * opacity);
            let fills = tr
                .unit_territory
                .iter()
                .map(|t| Some(t.map_or(unassigned, |t| territory_fill[t as usize])))
                .collect();
            let border = territory_border(cx.dark);
            let inner = with_opacity(
                if cx.dark {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                },
                0.55,
            );
            let ut = &tr.unit_territory;
            let arcs = mesh
                .geometry
                .topology
                .arcs
                .iter()
                .map(|arc| {
                    let ta = ut.get(arc.a as usize).copied().flatten();
                    let tb = if arc.b == OUTSIDE {
                        None
                    } else {
                        ut.get(arc.b as usize).copied().flatten()
                    };
                    if arc.a == arc.b {
                        None
                    } else if ta != tb && (ta.is_some() || tb.is_some()) {
                        Some(Stroke::new(
                            style.border_width,
                            with_opacity(border, opacity),
                        ))
                    } else if style.unit_borders && !points {
                        Some(Stroke::new(0.5, with_opacity(inner, opacity)))
                    } else {
                        None
                    }
                })
                .collect();
            let labels = if style.labels {
                Labels::Territories(
                    tr.label_units
                        .iter()
                        .zip(&p.territories)
                        .filter_map(|(u, t)| u.map(|u| (u, t.name.clone())))
                        .collect(),
                )
            } else {
                Labels::None
            };
            let mut sites = Vec::new();
            if style.show_sites
                && let Ok(to_display) = Transformer::new(&Crs::wgs84(), crs)
            {
                for t in &p.territories {
                    if let Some([lon, lat]) = t.site
                        && let Some(d) = to_display.convert(geo_types::Coord { x: lon, y: lat })
                    {
                        sites.push(([d.x, d.y], color(t.color).to_opaque()));
                    }
                }
            }
            let area = Resolved {
                mesh,
                fills,
                arcs,
                line_width: 1.0,
                point_radius: 3.5,
                point_stroke: Stroke::new(1.0, border),
                labels,
                sites,
                symbols: Vec::new(),
                symbol_fill: Color32::TRANSPARENT,
                symbol_stroke: Stroke::NONE,
            };
            if !points {
                return Some(vec![area]);
            }
            // The customers themselves on top, in their territory's colour.
            let dots = meshes.get(&p.layer)?.clone();
            let dot_fills = tr
                .unit_territory
                .iter()
                .map(|t| {
                    Some(t.map_or(Color32::from_rgb(0x9a, 0x9a, 0xa2), |t| {
                        // A shade darker than the cell, so customers stand out.
                        with_opacity(
                            color(p.territories[t as usize].color.lighten(-0.18)).to_opaque(),
                            opacity,
                        )
                    }))
                })
                .collect();
            let dot_layer = Resolved {
                mesh: dots,
                fills: dot_fills,
                arcs: Vec::new(),
                line_width: 1.0,
                point_radius: 3.0,
                point_stroke: Stroke::new(0.8, with_opacity(border, 0.7 * opacity)),
                labels: Labels::None,
                sites: Vec::new(),
                symbols: Vec::new(),
                symbol_fill: Color32::TRANSPARENT,
                symbol_stroke: Stroke::NONE,
            };
            Some(vec![area, dot_layer])
        }
        MapLayerKind::Symbols {
            layer,
            source,
            normalize,
            symbol,
            style,
        } => {
            let l = project.layer(*layer)?;
            let mesh = meshes.get(layer)?.clone();
            let n = l.len();
            let v = cx.values.normalized(project, *layer, source, normalize);
            let max = v.iter().flatten().fold(0.0f64, |a, b| a.max(*b));
            let mut symbols: Vec<(usize, f32)> = v
                .iter()
                .enumerate()
                .filter_map(|(f, x)| {
                    let r = symbol.radius((*x)?, max);
                    // Tiny values stay visible as dots.
                    (r > 0.0).then_some((f, r.max(1.5)))
                })
                .collect();
            symbols.sort_by(|a, b| b.1.total_cmp(&a.1));
            let labels = match &style.labels {
                LabelMode::None => Labels::None,
                LabelMode::Template(t) => cx
                    .values
                    .template_texts(project, *layer, t)
                    .map_or(Labels::None, Labels::Features),
                mode => {
                    Labels::Features(Arc::new((0..n).map(|f| label_text(l, mode, f)).collect()))
                }
            };
            Some(vec![Resolved {
                mesh,
                fills: vec![None; n],
                arcs: Vec::new(),
                line_width: 1.0,
                point_radius: 0.0,
                point_stroke: Stroke::NONE,
                labels,
                sites: Vec::new(),
                symbols,
                symbol_fill: with_opacity(color(symbol.color), opacity),
                symbol_stroke: Stroke::new(0.8, with_opacity(color(symbol.outline), opacity)),
            }])
        }
    }
}

pub fn label_text(
    layer: &territoria_model::Layer,
    mode: &LabelMode,
    feature: usize,
) -> Option<String> {
    match mode {
        LabelMode::None => None,
        LabelMode::Key => Some(layer.key(feature)),
        LabelMode::Name => {
            let name = layer
                .name_column
                .as_deref()
                .and_then(|c| layer.attributes.column(c))
                .map(|c| c.display(feature));
            name.or_else(|| Some(layer.key(feature)))
        }
        LabelMode::Column(c) => layer.attributes.column(c).map(|c| c.display(feature)),
        // Templates need the caches: see `Cache::template_texts`.
        LabelMode::Template(_) => None,
    }
}
