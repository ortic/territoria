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
    /// Per feature, its label and font size in points.
    Sized(Arc<Vec<Option<String>>>, Vec<f32>),
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
    /// Per feature its circle's colour, when coloured by a category.
    pub symbol_colors: Vec<Option<Color32>>,
    pub symbol_stroke: Stroke,
    pub symbol_shape: territoria_model::SymbolShape,
    /// Charts per feature, largest first.
    pub charts: Vec<Chart>,
    /// Flow lines, widest last.
    pub flows: Vec<Flow>,
    /// Closed lines in display coordinates (catchment rings) with a label.
    pub rings: Vec<Ring>,
}

/// A line from one place to another (display coordinates).
pub struct Flow {
    pub from: [f64; 2],
    pub to: [f64; 2],
    pub width: f32,
    pub color: Color32,
    pub curved: bool,
    pub arrow: bool,
}

/// A chart on a feature's label point.
pub struct Chart {
    pub feature: usize,
    pub shape: territoria_model::ChartShape,
    /// Pie radius, or the height of the tallest bar, in points.
    pub size: f32,
    /// Pies: shares adding up to 1; bars: heights as fractions of `size`.
    pub parts: Vec<(f32, Color32)>,
    pub outline: Stroke,
}

pub struct Ring {
    pub points: Vec<[f64; 2]>,
    pub stroke: Stroke,
    pub fill: Color32,
    pub label: String,
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
        // Drawn as an image of its own (see `heat`).
        MapLayerKind::Heatmap { .. } => Some(Vec::new()),
        MapLayerKind::Flows { style, .. } => {
            let to_display = Transformer::new(&Crs::wgs84(), crs).ok()?;
            let lines = cx.analyses.flows(project, ml.id, &ml.kind)?;
            let max = lines.iter().map(|l| l.value).fold(0.0f64, f64::max);
            let c = with_opacity(color(style.color), opacity);
            let flows = lines
                .iter()
                .filter_map(|l| {
                    let (a, b) = (to_display.convert(l.from)?, to_display.convert(l.to)?);
                    Some(Flow {
                        from: [a.x, a.y],
                        to: [b.x, b.y],
                        width: (style.max_width * (l.value / max) as f32).max(0.6),
                        color: c,
                        curved: style.curved,
                        arrow: style.arrows,
                    })
                })
                .collect();
            Some(vec![Resolved {
                mesh: Arc::new(LayerMesh::empty()),
                fills: Vec::new(),
                arcs: Vec::new(),
                line_width: 1.0,
                point_radius: 0.0,
                point_stroke: Stroke::NONE,
                labels: Labels::None,
                sites: Vec::new(),
                symbols: Vec::new(),
                symbol_fill: Color32::TRANSPARENT,
                symbol_stroke: Stroke::NONE,
                symbol_shape: Default::default(),
                symbol_colors: Vec::new(),
                charts: Vec::new(),
                flows,
                rings: Vec::new(),
            }])
        }
        MapLayerKind::Rings {
            centres,
            radii_km,
            color: c,
        } => {
            let to_display = Transformer::new(&Crs::wgs84(), crs).ok()?;
            let base = color(*c);
            let mut rings = Vec::new();
            let mut sites = Vec::new();
            let list = centres.list(project);
            for (_, centre) in &list {
                // Largest first: the smaller ones' fills lie on top.
                for r in radii_km.iter().rev() {
                    let points = territoria_model::catchment::ring(*centre, *r, 96)
                        .into_iter()
                        .filter_map(|p| to_display.convert(p))
                        .map(|d| [d.x, d.y])
                        .collect();
                    rings.push(Ring {
                        points,
                        stroke: Stroke::new(1.2, with_opacity(base, opacity)),
                        fill: with_opacity(base, 0.07 * opacity),
                        label: format!("{} km", territoria_data::table::format_number(*r)),
                    });
                }
                if let Some(d) = to_display.convert(*centre) {
                    sites.push(([d.x, d.y], base.to_opaque()));
                }
            }
            Some(vec![Resolved {
                mesh: Arc::new(LayerMesh::empty()),
                fills: Vec::new(),
                arcs: Vec::new(),
                line_width: 1.0,
                point_radius: 0.0,
                point_stroke: Stroke::NONE,
                labels: Labels::None,
                sites,
                symbols: Vec::new(),
                symbol_fill: Color32::TRANSPARENT,
                symbol_stroke: Stroke::NONE,
                symbol_shape: Default::default(),
                symbol_colors: Vec::new(),
                charts: Vec::new(),
                flows: Vec::new(),
                rings,
            }])
        }
        MapLayerKind::Boundaries { layer, style }
        | MapLayerKind::Choropleth { layer, style, .. }
        | MapLayerKind::Bivariate { layer, style, .. }
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
            let labels = feature_labels(cx, l, style);
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
                symbol_shape: Default::default(),
                symbol_colors: Vec::new(),
                charts: Vec::new(),
                flows: Vec::new(),
                rings: Vec::new(),
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
            // Per territory index its region: region borders are drawn wider.
            let region_of: Vec<Option<territoria_model::RegionId>> =
                p.territories.iter().map(|t| t.region).collect();
            let region =
                |t: Option<u16>| t.and_then(|t| region_of.get(t as usize).copied().flatten());
            let region_width = style.region_border_width.max(style.border_width);
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
                        let (ra, rb) = (region(ta), region(tb));
                        let width = if ra != rb && (ra.is_some() || rb.is_some()) {
                            region_width
                        } else {
                            style.border_width
                        };
                        Some(Stroke::new(width, with_opacity(border, opacity)))
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
                symbol_shape: Default::default(),
                symbol_colors: Vec::new(),
                charts: Vec::new(),
                flows: Vec::new(),
                rings: Vec::new(),
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
                symbol_shape: Default::default(),
                symbol_colors: Vec::new(),
                charts: Vec::new(),
                flows: Vec::new(),
                rings: Vec::new(),
            };
            Some(vec![area, dot_layer])
        }
        MapLayerKind::Charts {
            layer,
            parts,
            chart,
            style,
        } => {
            let l = project.layer(*layer)?;
            let mesh = meshes.get(layer)?.clone();
            let n = l.len();
            let labels = feature_labels(cx, l, style);
            Some(vec![Resolved {
                mesh,
                fills: vec![None; n],
                arcs: Vec::new(),
                line_width: 1.0,
                point_radius: 0.0,
                point_stroke: Stroke::NONE,
                labels,
                sites: Vec::new(),
                symbols: Vec::new(),
                symbol_fill: Color32::TRANSPARENT,
                symbol_stroke: Stroke::NONE,
                symbol_shape: Default::default(),
                symbol_colors: Vec::new(),
                charts: charts(cx, *layer, parts, chart, opacity),
                flows: Vec::new(),
                rings: Vec::new(),
            }])
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
            // Coloured by a category: features without one in grey.
            let symbol_colors = if symbol.color_by.is_some() {
                let scaled = cx.analyses.get(project, cx.values, ml.id, &ml.kind)?;
                let none = with_opacity(Color32::from_rgb(0xc4, 0xc4, 0xca), opacity);
                (0..n)
                    .map(|f| {
                        Some(scaled.colors.get(f).copied().flatten().map_or(none, |c| {
                            with_opacity(color(c).gamma_multiply(0.9), opacity)
                        }))
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let labels = feature_labels(cx, l, style);
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
                symbol_shape: symbol.shape,
                symbol_colors,
                charts: Vec::new(),
                flows: Vec::new(),
                rings: Vec::new(),
            }])
        }
    }
}

/// Per feature its chart: values of each part, sized and scaled as the
/// style says.
fn charts(
    cx: &mut Context,
    layer: LayerId,
    parts: &[territoria_model::ChartPart],
    chart: &territoria_model::ChartStyle,
    opacity: f32,
) -> Vec<Chart> {
    let n = cx.project.layer(layer).map_or(0, |l| l.len());
    let values: Vec<Vec<Option<f64>>> = parts
        .iter()
        .map(|p| cx.values.values(cx.project, layer, &p.source))
        .collect();
    let colors: Vec<Color32> = parts
        .iter()
        .map(|p| with_opacity(color(p.color), opacity))
        .collect();
    let outline = Stroke::new(0.8, with_opacity(color(chart.outline), opacity));
    let at = |f: usize| -> Vec<f64> {
        values
            .iter()
            .map(|v| v.get(f).copied().flatten().unwrap_or(0.0).max(0.0))
            .collect()
    };
    let mut out = Vec::new();
    if chart.shape == territoria_model::ChartShape::Stacked {
        // One scale for the totals: the tallest stack gets the full height.
        let totals: Vec<f64> = (0..n).map(|f| at(f).iter().sum()).collect();
        let max = totals.iter().copied().fold(0.0f64, f64::max);
        if max <= 0.0 {
            return out;
        }
        for (f, total) in totals.iter().enumerate() {
            if *total <= 0.0 {
                continue;
            }
            out.push(Chart {
                feature: f,
                shape: chart.shape,
                size: chart.max_size,
                parts: at(f)
                    .iter()
                    .zip(&colors)
                    .map(|(x, c)| ((x / max) as f32, *c))
                    .collect(),
                outline,
            });
        }
    } else if chart.shape == territoria_model::ChartShape::Bars {
        let max = (0..n).flat_map(&at).fold(0.0f64, f64::max);
        if max <= 0.0 {
            return out;
        }
        for f in 0..n {
            let v = at(f);
            if v.iter().all(|x| *x <= 0.0) {
                continue;
            }
            out.push(Chart {
                feature: f,
                shape: chart.shape,
                size: chart.max_size,
                parts: v
                    .iter()
                    .zip(&colors)
                    .map(|(x, c)| ((x / max) as f32, *c))
                    .collect(),
                outline,
            });
        }
    } else {
        let totals: Vec<f64> = (0..n).map(|f| at(f).iter().sum()).collect();
        let max = totals.iter().copied().fold(0.0f64, f64::max);
        for (f, total) in totals.iter().enumerate() {
            let size = chart.radius(*total, max);
            if size <= 0.0 {
                continue;
            }
            out.push(Chart {
                feature: f,
                shape: chart.shape,
                size,
                parts: at(f)
                    .iter()
                    .zip(&colors)
                    .map(|(x, c)| ((x / total) as f32, *c))
                    .collect(),
                outline,
            });
        }
        out.sort_by(|a, b| b.size.total_cmp(&a.size));
    }
    out
}

/// A layer's labels as its style says: texts, sized by a number if so.
fn feature_labels(
    cx: &mut Context,
    l: &territoria_model::Layer,
    style: &territoria_model::BoundaryStyle,
) -> Labels {
    let texts = match &style.labels {
        LabelMode::None => return Labels::None,
        LabelMode::Template(t) => match cx.values.template_texts(cx.project, l.id, t) {
            Some(t) => t,
            None => return Labels::None,
        },
        mode => Arc::new((0..l.len()).map(|f| label_text(l, mode, f)).collect()),
    };
    match &style.label_size {
        Some(size) => {
            let v = cx.values.values(cx.project, l.id, &size.source);
            Labels::Sized(texts, size.sizes(&v))
        }
        None => Labels::Features(texts),
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
