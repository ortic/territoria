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
    pub kind: MapLayerKind,
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
    /// Areas coloured by a text value.
    Categories {
        layer: LayerId,
        source: TextSource,
        scale: CategoryScale,
        style: BoundaryStyle,
    },
    /// Territories of a plan: fills, borders, labels.
    Territories { plan: PlanId, style: TerritoryStyle },
    /// Circles sized by a number: at each area's centre, or each point.
    Symbols {
        layer: LayerId,
        source: ValueSource,
        #[serde(default)]
        normalize: crate::values::Normalize,
        symbol: SymbolStyle,
        style: BoundaryStyle,
    },
}

/// How proportional circles look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolStyle {
    pub color: Rgba,
    pub outline: Rgba,
    /// Radius of the largest value, in screen points.
    pub max_radius: f32,
}

impl Default for SymbolStyle {
    fn default() -> Self {
        SymbolStyle {
            color: Rgba([0xe0, 0x6c, 0x3c, 190]),
            outline: Rgba([0xff, 0xff, 0xff, 230]),
            max_radius: 22.0,
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
            | MapLayerKind::Categories { layer, .. }
            | MapLayerKind::Symbols { layer, .. } => Some(*layer),
            MapLayerKind::Territories { .. } => None,
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
            _ => false,
        }
    }

    pub fn style(&self) -> Option<&BoundaryStyle> {
        match self {
            MapLayerKind::Boundaries { style, .. }
            | MapLayerKind::Choropleth { style, .. }
            | MapLayerKind::Categories { style, .. }
            | MapLayerKind::Symbols { style, .. } => Some(style),
            MapLayerKind::Territories { .. } => None,
        }
    }

    pub fn style_mut(&mut self) -> Option<&mut BoundaryStyle> {
        match self {
            MapLayerKind::Boundaries { style, .. }
            | MapLayerKind::Choropleth { style, .. }
            | MapLayerKind::Categories { style, .. }
            | MapLayerKind::Symbols { style, .. } => Some(style),
            MapLayerKind::Territories { .. } => None,
        }
    }

    pub fn type_label(&self) -> &'static str {
        match self {
            MapLayerKind::Boundaries { .. } => "Boundaries",
            MapLayerKind::Choropleth { .. } => "Coloured by value",
            MapLayerKind::Categories { .. } => "Coloured by category",
            MapLayerKind::Territories { .. } => "Territories",
            MapLayerKind::Symbols { .. } => "Circles sized by value",
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
}

impl Default for TerritoryStyle {
    fn default() -> Self {
        TerritoryStyle {
            fill_opacity: 0.85,
            unit_borders: true,
            border_width: 1.6,
            labels: true,
            show_sites: true,
        }
    }
}
