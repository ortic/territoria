//! Maps: what is shown, in which order and style.

use serde::{Deserialize, Serialize};
use territoria_analysis::{CategoryScale, ColorScale, Rgba};
use territoria_geo::Crs;

use crate::values::{TextSource, ValueSource};
use crate::{LayerId, LinkId, MapId, MapLayerId, PlanId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Map {
    pub id: MapId,
    pub name: String,
    /// Bottom first.
    pub layers: Vec<MapLayer>,
    /// The projection maps are drawn in.
    pub crs: Crs,
    /// Last view: centre in display coordinates and units per screen point.
    pub view: Option<([f64; 2], f64)>,
    pub background: Background,
    /// Views kept by name, to come back to.
    #[serde(default)]
    pub views: Vec<NamedView>,
}

/// A view kept by name: centre in display coordinates and units per point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedView {
    pub name: String,
    pub center: [f64; 2],
    pub scale: f64,
}

impl Map {
    pub fn new(id: MapId, name: &str) -> Map {
        Map {
            id,
            name: name.to_owned(),
            layers: Vec::new(),
            crs: Crs::web_mercator(),
            view: None,
            background: Background::default(),
            views: Vec::new(),
        }
    }

    pub fn layer(&self, id: MapLayerId) -> Option<&MapLayer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: MapLayerId) -> Option<&mut MapLayer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Background {
    #[default]
    None,
    /// XYZ raster tiles (`{z}/{x}/{y}` in the URL).
    Tiles { url: String, attribution: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapLayer {
    pub id: MapLayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    /// The zoom levels it shows at.
    #[serde(default)]
    pub zoom: ZoomRange,
    pub kind: MapLayerKind,
}

impl MapLayer {
    /// Whether it shows in a view of `scale` (display units per point).
    pub fn shown_at(&self, scale: f64) -> bool {
        self.visible && self.zoom.contains(scale)
    }
}

/// The view scales (display units per screen point) a layer shows at;
/// `None` is no limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ZoomRange {
    /// Hidden when zoomed in closer than this.
    pub closest: Option<f64>,
    /// Hidden when zoomed out farther than this.
    pub farthest: Option<f64>,
}

impl ZoomRange {
    pub fn contains(&self, scale: f64) -> bool {
        self.closest.is_none_or(|c| scale >= c) && self.farthest.is_none_or(|f| scale <= f)
    }

    pub fn is_limited(&self) -> bool {
        self.closest.is_some() || self.farthest.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MapLayerKind {
    /// The layer as it is: one fill, borders, labels.
    Boundaries {
        layer: LayerId,
        style: BoundaryStyle,
    },
    /// Areas coloured by a number.
    Choropleth {
        layer: LayerId,
        source: ValueSource,
        /// Per km², per another value, or as a share.
        #[serde(default)]
        normalize: crate::values::Normalize,
        scale: ColorScale,
        style: BoundaryStyle,
    },
    /// Areas coloured by two numbers at once (a grid of 2×2 or 3×3 colours).
    Bivariate {
        layer: LayerId,
        x: ValueSource,
        y: ValueSource,
        scale: territoria_analysis::BivariateScale,
        style: BoundaryStyle,
    },
    /// Areas coloured by a text value.
    Categories {
        layer: LayerId,
        source: TextSource,
        scale: CategoryScale,
        style: BoundaryStyle,
    },
    /// Territories of a plan: fills, borders, labels.
    Territories { plan: PlanId, style: TerritoryStyle },
    /// Density of points as a smooth colour field.
    Heatmap {
        layer: LayerId,
        /// Points weigh by this number (each counts 1 without).
        weight: Option<String>,
        /// How far each point spreads, in km.
        radius_km: f64,
        ramp: territoria_analysis::RampSpec,
    },
    /// Lines from where rows start to where they go, as wide as their value.
    Flows {
        rows: crate::flows::FlowRows,
        from: crate::flows::FlowEnd,
        to: crate::flows::FlowEnd,
        /// Number column the widths follow (each row counts 1 without).
        value: Option<String>,
        style: crate::flows::FlowStyle,
    },
    /// Rings of given radii around centres (catchment).
    Rings {
        centres: crate::catchment::Centres,
        radii_km: Vec<f64>,
        color: Rgba,
    },
    /// Circles sized by a number: at each area's centre, or each point.
    Symbols {
        layer: LayerId,
        source: ValueSource,
        #[serde(default)]
        normalize: crate::values::Normalize,
        symbol: SymbolStyle,
        style: BoundaryStyle,
    },
    /// A small chart on each area or point: a slice or bar per value.
    Charts {
        layer: LayerId,
        parts: Vec<ChartPart>,
        chart: ChartStyle,
        style: BoundaryStyle,
    },
}

/// One value of a chart with its colour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartPart {
    pub label: String,
    pub source: ValueSource,
    pub color: Rgba,
}

impl ChartPart {
    /// The colour of a chart's `i`th part, distinct from the others.
    pub fn color_for(i: usize) -> Rgba {
        let colors = territoria_analysis::RampSpec::named("Tableau").colors(i + 1);
        colors[i % colors.len()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChartShape {
    #[default]
    Pie,
    Donut,
    Bars,
    /// One bar per feature, its values stacked.
    Stacked,
}

impl ChartShape {
    pub const ALL: [ChartShape; 4] = [
        ChartShape::Pie,
        ChartShape::Donut,
        ChartShape::Bars,
        ChartShape::Stacked,
    ];

    /// Bars of either kind (heights on a scale, not pie slices).
    pub fn is_bars(self) -> bool {
        matches!(self, ChartShape::Bars | ChartShape::Stacked)
    }

    pub fn label(self) -> &'static str {
        match self {
            ChartShape::Pie => "Pie",
            ChartShape::Donut => "Donut",
            ChartShape::Bars => "Bars",
            ChartShape::Stacked => "Stacked",
        }
    }
}

/// How charts look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartStyle {
    pub shape: ChartShape,
    /// The largest chart in screen points: pie radius or tallest bar.
    pub max_size: f32,
    /// Pies sized by their total (areas grow with it), else all alike.
    pub sized: bool,
    pub outline: Rgba,
}

impl ChartStyle {
    /// A style whose charts suit `n` features (see [`SymbolStyle::for_count`]).
    pub fn for_count(shape: ChartShape, n: usize) -> ChartStyle {
        ChartStyle {
            shape,
            max_size: SymbolStyle::for_count(n).max_radius.max(10.0),
            sized: true,
            outline: Rgba([0xff, 0xff, 0xff, 230]),
        }
    }

    /// Pie radius for a total when `max` gets the full size.
    pub fn radius(&self, total: f64, max: f64) -> f32 {
        if total <= 0.0 || max <= 0.0 {
            return 0.0;
        }
        if self.sized {
            (self.max_size * (total / max).min(1.0).sqrt() as f32).max(3.0)
        } else {
            self.max_size
        }
    }
}

/// How proportional circles look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolStyle {
    pub color: Rgba,
    pub outline: Rgba,
    /// Radius of the largest value, in screen points.
    pub max_radius: f32,
    /// Colours by a category instead of one colour.
    #[serde(default)]
    pub color_by: Option<(TextSource, CategoryScale)>,
}

impl Default for SymbolStyle {
    fn default() -> Self {
        SymbolStyle {
            color: Rgba([0xe0, 0x6c, 0x3c, 190]),
            outline: Rgba([0xff, 0xff, 0xff, 230]),
            max_radius: 22.0,
            color_by: None,
        }
    }
}

impl SymbolStyle {
    /// A style whose largest circle suits `n` features: big for a few
    /// regions, small for thousands of postcodes.
    pub fn for_count(n: usize) -> SymbolStyle {
        let r = 28.0 * (50.0 / n.max(1) as f32).powf(0.3);
        SymbolStyle {
            max_radius: r.clamp(8.0, 28.0).round(),
            ..Default::default()
        }
    }

    /// The radius for `value` when `max` gets the full size: circle areas
    /// grow with the value.
    pub fn radius(&self, value: f64, max: f64) -> f32 {
        if value <= 0.0 || max <= 0.0 {
            return 0.0;
        }
        self.max_radius * (value / max).min(1.0).sqrt() as f32
    }
}

impl MapLayerKind {
    pub fn layer(&self) -> Option<LayerId> {
        match self {
            MapLayerKind::Boundaries { layer, .. }
            | MapLayerKind::Choropleth { layer, .. }
            | MapLayerKind::Bivariate { layer, .. }
            | MapLayerKind::Categories { layer, .. }
            | MapLayerKind::Symbols { layer, .. }
            | MapLayerKind::Charts { layer, .. }
            | MapLayerKind::Heatmap { layer, .. } => Some(*layer),
            MapLayerKind::Rings {
                centres: crate::catchment::Centres::Points(layer),
                ..
            } => Some(*layer),
            MapLayerKind::Territories { .. }
            | MapLayerKind::Rings { .. }
            | MapLayerKind::Flows { .. } => None,
        }
    }

    pub fn plan(&self) -> Option<PlanId> {
        match self {
            MapLayerKind::Territories { plan, .. } => Some(*plan),
            _ => None,
        }
    }

    pub fn uses_links(&self, links: &[LinkId]) -> bool {
        match self {
            MapLayerKind::Choropleth { source, .. } | MapLayerKind::Symbols { source, .. } => {
                source.uses_links(links)
            }
            MapLayerKind::Categories {
                source: TextSource::Linked { link, .. },
                ..
            } => links.contains(link),
            MapLayerKind::Charts { parts, .. } => parts.iter().any(|p| p.source.uses_links(links)),
            MapLayerKind::Bivariate { x, y, .. } => x.uses_links(links) || y.uses_links(links),
            _ => false,
        }
    }

    pub fn style(&self) -> Option<&BoundaryStyle> {
        match self {
            MapLayerKind::Boundaries { style, .. }
            | MapLayerKind::Choropleth { style, .. }
            | MapLayerKind::Bivariate { style, .. }
            | MapLayerKind::Categories { style, .. }
            | MapLayerKind::Symbols { style, .. }
            | MapLayerKind::Charts { style, .. } => Some(style),
            MapLayerKind::Territories { .. }
            | MapLayerKind::Rings { .. }
            | MapLayerKind::Flows { .. }
            | MapLayerKind::Heatmap { .. } => None,
        }
    }

    pub fn style_mut(&mut self) -> Option<&mut BoundaryStyle> {
        match self {
            MapLayerKind::Boundaries { style, .. }
            | MapLayerKind::Choropleth { style, .. }
            | MapLayerKind::Bivariate { style, .. }
            | MapLayerKind::Categories { style, .. }
            | MapLayerKind::Symbols { style, .. }
            | MapLayerKind::Charts { style, .. } => Some(style),
            MapLayerKind::Territories { .. }
            | MapLayerKind::Rings { .. }
            | MapLayerKind::Flows { .. }
            | MapLayerKind::Heatmap { .. } => None,
        }
    }

    pub fn type_label(&self) -> &'static str {
        match self {
            MapLayerKind::Boundaries { .. } => "Boundaries",
            MapLayerKind::Choropleth { .. } => "Coloured by value",
            MapLayerKind::Bivariate { .. } => "Coloured by two values",
            MapLayerKind::Categories { .. } => "Coloured by category",
            MapLayerKind::Territories { .. } => "Territories",
            MapLayerKind::Symbols { .. } => "Circles sized by value",
            MapLayerKind::Rings { .. } => "Catchment rings",
            MapLayerKind::Heatmap { .. } => "Heat map (density)",
            MapLayerKind::Flows { .. } => "Flows (lines between places)",
            MapLayerKind::Charts { chart, .. } => {
                if chart.shape.is_bars() {
                    "Bar charts"
                } else {
                    "Pie charts"
                }
            }
        }
    }

    /// Whether this layer shows something of `plan` (and goes with it).
    pub fn uses_plan(&self, plan: PlanId) -> bool {
        match self {
            MapLayerKind::Territories { plan: p, .. } => *p == plan,
            MapLayerKind::Rings {
                centres: crate::catchment::Centres::Sites(p),
                ..
            } => *p == plan,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryStyle {
    /// Area fill (for plain boundaries; analyses colour areas themselves).
    pub fill: Rgba,
    pub stroke: Rgba,
    /// Border width in screen points; 0 hides borders.
    pub stroke_width: f32,
    /// Point symbol radius in screen points.
    pub point_size: f32,
    pub labels: LabelMode,
    /// Text of the hover card, a template like `{Name}: {[Revenue]}`.
    #[serde(default)]
    pub tooltip: Option<String>,
}

impl Default for BoundaryStyle {
    fn default() -> Self {
        BoundaryStyle {
            fill: Rgba::rgb(0xe8, 0xe6, 0xdf),
            stroke: Rgba([0x6b, 0x6f, 0x78, 200]),
            stroke_width: 0.6,
            point_size: 3.5,
            labels: LabelMode::None,
            tooltip: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum LabelMode {
    #[default]
    None,
    Key,
    Name,
    Column(String),
    /// Text with formulas in braces, e.g. `{Name}` and `{[Revenue]}` on two lines.
    Template(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerritoryStyle {
    pub fill_opacity: f32,
    /// Borders between units of the same territory.
    pub unit_borders: bool,
    pub border_width: f32,
    pub labels: bool,
    pub show_sites: bool,
    /// Borders between regions, in points (0: drawn like territory borders).
    #[serde(default = "default_region_border")]
    pub region_border_width: f32,
}

fn default_region_border() -> f32 {
    4.5
}

impl Default for TerritoryStyle {
    fn default() -> Self {
        TerritoryStyle {
            fill_opacity: 0.85,
            unit_borders: true,
            border_width: 1.6,
            labels: true,
            show_sites: true,
            region_border_width: default_region_border(),
        }
    }
}
