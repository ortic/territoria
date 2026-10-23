//! The application: project, undo history, panels, tabs and dialogs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Align, Layout, RichText, Ui};
use egui_dock::{DockArea, DockState, Style as DockStyle, TabViewer};
use serde::{Deserialize, Serialize};
use territoria_analysis::{CategoryScale, ColorScale};
use territoria_config::{Paths, Settings, ThemeChoice, load_toml, save_toml};
use territoria_io::{GeometryKind, TableOptions, VectorOptions};
use territoria_model::{
    BoundaryStyle, Cache, History, LayerId, MapId, MapLayer, MapLayerId, MapLayerKind, PlanId,
    Project, SourceInfo, TableId, TerritoryId,
};
use tracing::{error, info};

use crate::commands::{Command, Keymap};
use crate::dialogs::{
    ColorByDialog, ColorByOutcome, Colorable, DownloadDialog, DownloadOutcome, ImportLayers,
    ImportLayersOutcome, ImportTable, ImportTableOutcome, LinkDialog, LinkOutcome, MergeDialog,
    MergeOutcome, NewPlanDialog, NewPlanOutcome,
};
use crate::jobs::{JobResult, Jobs, PreparedLayer};
use crate::map_view::{self, MapContext, MapEvent, MapState, TerritoryCache, Tool};
use crate::meshes::{AnalysisCache, MeshCache};
use crate::territories::{StatsCache, TerritoriesPanel};
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataRef {
    Layer(LayerId),
    Table(TableId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Map(MapId),
    Table(DataRef),
    /// A plan's territories as a table.
    Plan(PlanId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Selection {
    #[default]
    None,
    MapLayer(MapId, MapLayerId),
    Data(DataRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RightTab {
    #[default]
    Properties,
    Territories,
}

pub type EditFn = Box<dyn FnOnce(&mut Project)>;

/// What panels and dialogs ask the app to do.
pub enum Action {
    Command(Command),
    Select(Selection),
    OpenTab(Tab),
    /// A project change with an undo label.
    Edit(String, EditFn),
    LinkTable(TableId),
    ColorBy(Option<LayerId>),
    /// Classes kept by name in the settings.
    SaveClassSet(String, territoria_analysis::ColorScale),
    /// A table column dropped on the map: show it.
    ShowColumn(DataRef, String),
    /// Show a data layer's boundaries on the current map.
    AddToMap(LayerId),
    /// Show and activate a plan.
    ShowPlan(PlanId),
    ActivateTerritory(Option<TerritoryId>),
    ZoomTo(LayerId, Vec<u32>),
    RemoveLayer(LayerId),
    RemoveTable(TableId),
    SetTool(Tool),
    OpenRecent(PathBuf),
    Balance(PlanId),
    AcceptProposal(PlanId),
    DiscardProposal,
    /// Select features and zoom to them.
    SelectUnits(LayerId, Vec<u32>),
    /// Show a kept view of a map.
    GoToView(MapId, [f64; 2], f64),
    /// Read a table again from its file.
    ReloadTable(TableId),
    /// Open the dialog for columns from the map or another table.
    Join(DataRef),
    /// Open the dialog for a summary table.
    Summarise(DataRef),
    /// Open the dialog for a weighted score column.
    Score(DataRef),
    /// Set a cell from text: row, column name, text.
    EditCell(DataRef, usize, String, String),
    AddRow(DataRef),
    DeleteRows(DataRef, Vec<usize>),
    /// Draw a map in another projection.
    SetMapCrs(MapId, territoria_geo::Crs),
    /// Show a message.
    Notify(String),
    /// Open the dialog for sites from a point layer.
    SitesFromPoints(PlanId),
    /// Select the features of table rows (adding to the selection: `true`).
    SelectRows(DataRef, Vec<usize>, bool),
    /// Open the formula editor for a new (`None`) or calculated column.
    Formula(DataRef, Option<String>),
    /// A filter kept as a yes/no calculated column of `data`.
    SaveSegment {
        data: DataRef,
        name: String,
        formula: String,
    },
    DeleteColumn(DataRef, String),
    /// Copy a plan as a scenario and compare the copy with it.
    DuplicatePlan(PlanId),
    RemovePlan(PlanId),
    Export(Export),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileAction {
    OpenProject,
    SaveProjectAs,
    AddBoundaries,
    AddData,
    Export(Export),
    /// A newer version of this layer's boundaries.
    ReplaceLayer(LayerId),
    /// The company logo for exports.
    Logo,
}

/// One page of a map atlas: its view and the areas it is about.
pub struct AtlasPage {
    pub view: territoria_render::View,
    pub veil: Option<(LayerId, std::collections::HashSet<usize>)>,
}

/// A table column being dragged (to the map, to show it there).
#[derive(Clone)]
pub struct ColumnDrag {
    pub data: DataRef,
    pub column: String,
}

/// What File → Export writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Export {
    /// Territories and assignment as an Excel workbook.
    PlanExcel(PlanId),
    /// The assignment (area → territory) as CSV.
    PlanCsv(PlanId),
    /// Territory outlines with their figures as GeoJSON.
    PlanGeoJson(PlanId),
    /// A layer with its attributes as GeoJSON.
    LayerGeoJson(LayerId),
    /// The current map as an image or document.
    Map(crate::export_map::Format),
    /// The current map as a web page to zoom and hover.
    Html,
    /// What changes from the first plan to the second, as an Excel workbook.
    Changes(PlanId, PlanId),
    /// A table or a layer's attributes with each row's territories, as
    /// Excel (`true`) or CSV; only a segment's rows when one is named.
    Data(DataRef, bool, Option<String>),
    /// A PDF with an overview and a page per territory.
    Report(PlanId),
    /// The current map as a PDF with a page per value of a layer's column
    /// (or per area), each zoomed to its areas.
    Atlas(LayerId, Option<String>),
}

struct Confirm {
    title: String,
    text: String,
    action: ConfirmAction,
}

enum ConfirmAction {
    RemoveLayer(LayerId),
    RemoveTable(TableId),
    RemovePlan(PlanId),
    Discard(Box<Command>),
    DiscardThen(PathBuf),
    /// Start a new project from this template.
    DiscardThenTemplate(String, PathBuf),
}

struct Toast {
    text: String,
    error: bool,
    at: Instant,
}

/// Window layout restored on the next start.
#[derive(Serialize, Deserialize)]
struct Workspace {
    show_contents: bool,
    show_inspector: bool,
}

const WORKSPACE_KEY: &str = "territoria-workspace-v1";

/// Where the user was in a project: saved with it, restored on opening.
#[derive(Serialize, Deserialize)]
struct ProjectWorkspace {
    dock: DockState<Tab>,
    current_map: Option<MapId>,
    active_plan: Option<PlanId>,
    active_territory: Option<TerritoryId>,
    #[serde(default)]
    selection: Selection,
    #[serde(default)]
    right_tab: RightTab,
}

pub struct TerritoriaApp {
    paths: Option<Paths>,
    pub settings: Settings,
    keymap: Keymap,

    pub project: Project,
    pub history: History,
    pub path: Option<PathBuf>,

    values: Cache,
    meshes: MeshCache,
    pub(crate) analyses: AnalysisCache,
    territory_cache: TerritoryCache,
    pub(crate) tiles: crate::tiles::Tiles,
    stats: StatsCache,
    pub jobs: Jobs,

    dock: DockState<Tab>,
    map_states: HashMap<MapId, MapState>,
    pub current_map: Option<MapId>,
    pub selection: Selection,
    pub tool: Tool,
    pub active_plan: Option<PlanId>,
    /// Features picked with the Select tool (or by list).
    pub selected: Option<(LayerId, Vec<u32>)>,
    select_list: Option<crate::dialogs::SelectListDialog>,
    pub active_territory: Option<TerritoryId>,
    right_tab: RightTab,
    show_contents: bool,
    show_inspector: bool,
    territories_panel: TerritoriesPanel,

    import_layers: Option<ImportLayers>,
    import_table: Option<ImportTable>,
    link: Option<LinkDialog>,
    color_by: Option<ColorByDialog>,
    new_plan: Option<NewPlanDialog>,
    merge: Option<MergeDialog>,
    formula: Option<crate::dialogs::FormulaDialog>,
    hex_grid: Option<crate::dialogs::HexGridDialog>,
    sites: Option<crate::dialogs::SitesDialog>,
    catchment: Option<crate::dialogs::CatchmentDialog>,
    join: Option<crate::dialogs::JoinDialog>,
    summarise: Option<crate::dialogs::SummariseDialog>,
    flows: Option<crate::dialogs::FlowDialog>,
    atlas: Option<crate::dialogs::AtlasDialog>,
    templates_dialog: Option<crate::dialogs::TemplatesDialog>,
    score: Option<crate::dialogs::ScoreDialog>,
    /// The search box: query and what it found.
    search: String,
    search_hits: Vec<crate::search::Hit>,
    search_focus: bool,
    /// An image waiting to go to the clipboard (needs the context).
    to_clipboard: Option<egui::ColorImage>,
    /// Full screen with only the map.
    presenting: bool,
    /// Fade everything but the selection.
    focus: bool,
    settings_dialog: Option<crate::dialogs::SettingsDialog>,
    download: Option<DownloadDialog>,
    export_map: Option<crate::dialogs::ExportMapDialog>,
    /// Settings of the map export waiting for its file name.
    map_export: Option<crate::dialogs::MapExportSettings>,
    /// Credits and names for downloaded files, by path, until they're
    /// imported.
    attributions: HashMap<PathBuf, (String, String)>,
    palette: Option<(String, usize)>,
    about: bool,
    help: Option<crate::help::HelpWindow>,
    confirm: Option<Confirm>,
    file_dialog: Option<(std::sync::mpsc::Receiver<Option<PathBuf>>, FileAction)>,
    toasts: Vec<Toast>,
    /// Project before the current paint stroke.
    stroke_before: Option<Project>,
    stroke_changes: usize,
    last_autosave: Instant,
    quit_confirmed: bool,
}

impl TerritoriaApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        paths: Option<Paths>,
        open: Option<PathBuf>,
    ) -> Self {
        theme::install(&cc.egui_ctx);
        let settings: Settings = paths
            .as_ref()
            .and_then(|p| load_toml(&p.settings_file()).ok())
            .unwrap_or_default();
        let workspace: Option<Workspace> =
            cc.storage.and_then(|s| eframe::get_value(s, WORKSPACE_KEY));
        let mut app = Self::with_settings(settings, paths);
        app.jobs.set_context(cc.egui_ctx.clone());
        app.apply_theme(&cc.egui_ctx);
        if let Some(w) = workspace {
            app.show_contents = w.show_contents;
            app.show_inspector = w.show_inspector;
        }
        if let Some(path) = open {
            app.open_path(&path);
        }
        app
    }

    /// Runs the background jobs to the end (for the command line).
    pub(crate) fn finish_jobs(&mut self, timeout: Duration) {
        let start = Instant::now();
        loop {
            for r in self.jobs.poll() {
                self.handle_job(r);
            }
            if !self.jobs.is_busy() || start.elapsed() > timeout {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Error messages shown so far (for the command line).
    pub(crate) fn errors(&self) -> Vec<String> {
        self.toasts
            .iter()
            .filter(|t| t.error)
            .map(|t| t.text.clone())
            .collect()
    }

    /// Messages shown so far (for the command line).
    pub(crate) fn messages(&self) -> Vec<String> {
        self.toasts
            .iter()
            .filter(|t| !t.error)
            .map(|t| t.text.clone())
            .collect()
    }

    /// Makes `map` the one exports use.
    pub(crate) fn use_map(&mut self, map: MapId) {
        self.current_map = Some(map);
    }

    pub fn with_settings(settings: Settings, paths: Option<Paths>) -> Self {
        let tile_dir = match &paths {
            Some(p) => p.data_dir.join("tiles"),
            None => std::env::temp_dir().join("territoria-tiles"),
        };
        TerritoriaApp {
            paths,
            keymap: Keymap::with_changes(&settings.shortcuts),
            settings,
            project: Project::new(),
            history: History::default(),
            path: None,
            values: Cache::default(),
            meshes: MeshCache::default(),
            analyses: AnalysisCache::default(),
            territory_cache: TerritoryCache::default(),
            tiles: crate::tiles::Tiles::new(tile_dir),
            stats: StatsCache::default(),
            jobs: Jobs::default(),
            dock: DockState::new(Vec::new()),
            map_states: HashMap::new(),
            current_map: None,
            selection: Selection::None,
            tool: Tool::Navigate,
            active_plan: None,
            selected: None,
            select_list: None,
            active_territory: None,
            right_tab: RightTab::Properties,
            show_contents: true,
            show_inspector: true,
            territories_panel: TerritoriesPanel::default(),
            import_layers: None,
            import_table: None,
            link: None,
            color_by: None,
            new_plan: None,
            merge: None,
            formula: None,
            hex_grid: None,
            sites: None,
            catchment: None,
            join: None,
            summarise: None,
            flows: None,
            atlas: None,
            templates_dialog: None,
            score: None,
            search: String::new(),
            search_hits: Vec::new(),
            search_focus: false,
            to_clipboard: None,
            presenting: false,
            focus: false,
            settings_dialog: None,
            download: None,
            export_map: None,
            map_export: None,
            attributions: HashMap::new(),
            palette: None,
            help: None,
            about: false,
            confirm: None,
            file_dialog: None,
            toasts: Vec::new(),
            stroke_before: None,
            stroke_changes: 0,
            last_autosave: Instant::now(),
            quit_confirmed: false,
        }
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        ctx.set_theme(match self.settings.theme {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
            ThemeChoice::Light => egui::ThemePreference::Light,
        });
        ctx.set_zoom_factor(self.settings.ui_scale);
    }

    fn save_settings(&self) {
        if let Some(p) = &self.paths
            && let Err(e) = save_toml(&p.settings_file(), &self.settings)
        {
            error!("saving settings: {e:#}");
        }
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        let text = text.into();
        info!("{text}");
        self.toasts.push(Toast {
            text,
            error: false,
            at: Instant::now(),
        });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        let text = text.into();
        error!("{text}");
        self.toasts.push(Toast {
            text,
            error: true,
            at: Instant::now(),
        });
    }

    /// Applies a change with undo.
    pub fn edit(&mut self, label: impl Into<String>, change: impl FnOnce(&mut Project)) {
        let before = self.project.clone();
        change(&mut self.project);
        self.history.push(label, before);
    }

    // -- Files ---------------------------------------------------------------

    fn ask_file(&mut self, action: FileAction) {
        if self.file_dialog.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = &self.settings.last_dir {
            dialog = dialog.set_directory(dir);
        }
        let dialog = match &action {
            FileAction::OpenProject => {
                dialog.add_filter("Territoria project", &[territoria_model::file::EXTENSION])
            }
            FileAction::SaveProjectAs => dialog
                .add_filter("Territoria project", &[territoria_model::file::EXTENSION])
                .set_file_name(format!(
                    "{}.{}",
                    self.project.title,
                    territoria_model::file::EXTENSION
                )),
            FileAction::AddBoundaries | FileAction::ReplaceLayer(_) => dialog
                .add_filter(
                    "Boundaries (Shapefile, GeoJSON, GeoPackage, KML, zip)",
                    territoria_io::VECTOR_EXTENSIONS,
                )
                .add_filter("All files", &["*"]),
            FileAction::AddData => dialog
                .add_filter("Tables (CSV, Excel)", territoria_io::TABLE_EXTENSIONS)
                .add_filter("All files", &["*"]),
            FileAction::Logo => {
                dialog.add_filter("Images (PNG, JPEG, SVG)", &["png", "jpg", "jpeg", "svg"])
            }
            FileAction::Export(e) => {
                let (filter, ext, name) = match e.clone() {
                    Export::PlanExcel(p) => (
                        "Excel workbook",
                        "xlsx",
                        self.project.plan(p).map(|p| p.name.clone()),
                    ),
                    Export::PlanCsv(p) => {
                        ("CSV", "csv", self.project.plan(p).map(|p| p.name.clone()))
                    }
                    Export::PlanGeoJson(p) => (
                        "GeoJSON",
                        "geojson",
                        self.project.plan(p).map(|p| p.name.clone()),
                    ),
                    Export::Map(f) => (
                        match f {
                            crate::export_map::Format::Png => "PNG image",
                            crate::export_map::Format::Svg => "SVG drawing",
                            crate::export_map::Format::Pdf => "PDF document",
                        },
                        f.extension(),
                        self.current_map
                            .and_then(|m| self.project.map(m))
                            .map(|m| m.name.clone()),
                    ),
                    Export::LayerGeoJson(l) => (
                        "GeoJSON",
                        "geojson",
                        self.project.layer(l).map(|l| l.name.clone()),
                    ),
                    Export::Changes(_, b) => (
                        "Excel workbook",
                        "xlsx",
                        self.project.plan(b).map(|p| format!("Changes {}", p.name)),
                    ),
                    Export::Html => (
                        "Web page",
                        "html",
                        self.current_map
                            .and_then(|m| self.project.map(m))
                            .map(|m| m.name.clone()),
                    ),
                    Export::Report(p) => (
                        "PDF document",
                        "pdf",
                        self.project.plan(p).map(|p| format!("Report {}", p.name)),
                    ),
                    Export::Atlas(l, _) => (
                        "PDF document",
                        "pdf",
                        self.project.layer(l).map(|l| format!("Atlas {}", l.name)),
                    ),
                    Export::Data(d, excel, segment) => (
                        if excel { "Excel workbook" } else { "CSV" },
                        if excel { "xlsx" } else { "csv" },
                        data_name(&self.project, d).map(|n| match segment {
                            Some(s) => format!("{n} – {s}"),
                            None => n,
                        }),
                    ),
                };
                dialog
                    .add_filter(filter, &[ext])
                    .set_file_name(format!("{}.{ext}", name.unwrap_or_else(|| "export".into())))
            }
        };
        let save = matches!(action, FileAction::SaveProjectAs | FileAction::Export(_));
        std::thread::spawn(move || {
            let path = if save {
                dialog.save_file()
            } else {
                dialog.pick_file()
            };
            let _ = tx.send(path);
        });
        self.file_dialog = Some((rx, action));
    }

    fn poll_file_dialog(&mut self) {
        let Some((rx, action)) = &self.file_dialog else {
            return;
        };
        let Ok(path) = rx.try_recv() else { return };
        let action = action.clone();
        self.file_dialog = None;
        let Some(path) = path else { return };
        if let Some(dir) = path.parent() {
            self.settings.last_dir = Some(dir.to_owned());
        }
        match action {
            FileAction::OpenProject => self.open_path(&path),
            FileAction::SaveProjectAs => {
                let path = if path.extension().is_none() {
                    path.with_extension(territoria_model::file::EXTENSION)
                } else {
                    path
                };
                self.save_to(path);
            }
            FileAction::AddBoundaries => self.import_boundaries(path, VectorOptions::default()),
            FileAction::ReplaceLayer(layer) => self.read_replacement(layer, path),
            FileAction::AddData => self.import_table_file(path, TableOptions::default()),
            FileAction::Export(e) => self.export(e, path),
            FileAction::Logo => {
                if crate::export_map::Logo::load(&path, 140.0, 44.0).is_some() {
                    self.settings.logo = Some(path);
                    self.save_settings();
                } else {
                    self.error(format!(
                        "{} isn't a PNG, JPEG or SVG image.",
                        file_name(&path)
                    ));
                }
            }
        }
    }

    /// Writes an export in the background.
    pub(crate) fn export(&mut self, what: Export, path: PathBuf) {
        if let Export::Map(format) = what {
            let Some(settings) = self.map_export.take() else {
                return;
            };
            let svg = match self.map_svg(&settings) {
                Ok(svg) => svg,
                Err(e) => {
                    self.error(e);
                    return;
                }
            };
            let font = egui::FontDefinitions::default().font_data["Ubuntu-Light"]
                .font
                .to_vec();
            let label = format!("Writing {}", file_name(&path));
            self.jobs.spawn(label, move |_| {
                let result = crate::export_map::encode(&svg, format, settings.scale, &font)
                    .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()));
                JobResult::Exported { path, result }
            });
            return;
        }
        if let Export::Html = what {
            let settings = crate::dialogs::MapExportSettings {
                format: crate::export_map::Format::Svg,
                scale: 1.0,
                title: if self.project.title == "Untitled" {
                    self.current_map
                        .and_then(|m| self.project.map(m))
                        .map_or_else(String::new, |m| m.name.clone())
                } else {
                    self.project.title.clone()
                },
                legend: true,
                scale_bar: false,
                page: Default::default(),
                footer: String::new(),
            };
            let page = match self.map_page(&settings) {
                Ok(p) => p,
                Err(e) => {
                    self.error(e);
                    return;
                }
            };
            let label = format!("Writing {}", file_name(&path));
            self.jobs.spawn(label, move |_| {
                let result = std::fs::write(&path, page).map_err(|e| e.to_string());
                JobResult::Exported { path, result }
            });
            return;
        }
        if let Export::Atlas(layer, column) = &what {
            let pages = match self.atlas_pages(*layer, column.as_deref()) {
                Ok(p) => p,
                Err(e) => {
                    self.error(e);
                    return;
                }
            };
            let font = egui::FontDefinitions::default().font_data["Ubuntu-Light"]
                .font
                .to_vec();
            let label = format!("Writing {}", file_name(&path));
            self.jobs.spawn(label, move |_| {
                let result = crate::export_map::pdf_pages(&pages, &font)
                    .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()));
                JobResult::Exported { path, result }
            });
            return;
        }
        if let Export::Report(plan) = what {
            let data = match self.report_data(plan) {
                Ok(d) => d,
                Err(e) => {
                    self.error(e);
                    return;
                }
            };
            let font = egui::FontDefinitions::default().font_data["Ubuntu-Light"]
                .font
                .to_vec();
            let label = format!("Writing {}", file_name(&path));
            self.jobs.spawn(label, move |_| {
                let pages = crate::report::pages(&data);
                let result = crate::export_map::pdf_pages(&pages, &font)
                    .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()));
                JobResult::Exported { path, result }
            });
            return;
        }
        // Tables with the territory of each row; the lookups need the caches.
        let data_sheets: Vec<(String, territoria_data::Table)> = match what {
            Export::PlanExcel(p) => self
                .project
                .plan(p)
                .map(|plan| plan_data_sheets(&self.project, &mut self.values, plan))
                .unwrap_or_default(),
            Export::Data(d, _, ref segment) => {
                with_territories(&self.project, &mut self.values, d, None)
                    .map(|(name, table)| {
                        // A segment: its rows only, with the values as shown.
                        let table = match segment.as_deref().and_then(|s| table.column(s)) {
                            Some(c) => {
                                let keep: Vec<usize> = (0..table.rows())
                                    .filter(|&r| {
                                        matches!(
                                            c.get(r),
                                            territoria_data::table::Value::Bool(true)
                                        )
                                    })
                                    .collect();
                                table.take_rows(&keep)
                            }
                            None => table,
                        };
                        (name, table)
                    })
                    .into_iter()
                    .collect()
            }
            _ => Vec::new(),
        };
        let project = self.project.clone();
        let comparison = match what {
            Export::Changes(a, b) => project.compare_plans(a, b, &mut self.values),
            _ => None,
        };
        let stats = match what {
            Export::PlanExcel(p) | Export::PlanGeoJson(p) => project
                .plan(p)
                .map(|plan| plan.stats(&project, &mut self.values)),
            _ => None,
        };
        let label = format!("Writing {}", file_name(&path));
        self.jobs.spawn(label, move |_| {
            let result = (|| -> Result<(), String> {
                let err = |e: territoria_io::ImportError| e.to_string();
                match what {
                    Export::PlanExcel(p) => {
                        let plan = project.plan(p).ok_or("the plan was removed")?;
                        let stats = stats.unwrap_or_default();
                        let territories = plan.territory_table(&project, &stats);
                        let assignment = plan.assignment_table(&project);
                        let mut sheets =
                            vec![("Territories", &territories), ("Assignment", &assignment)];
                        sheets.extend(data_sheets.iter().map(|(n, t)| (n.as_str(), t)));
                        territoria_io::export::write_xlsx(&path, &sheets).map_err(err)
                    }
                    Export::PlanCsv(p) => {
                        let plan = project.plan(p).ok_or("the plan was removed")?;
                        territoria_io::export::write_csv(
                            &path,
                            &plan.assignment_table(&project),
                            b',',
                        )
                        .map_err(err)
                    }
                    Export::PlanGeoJson(p) => {
                        let plan = project.plan(p).ok_or("the plan was removed")?;
                        let stats = stats.unwrap_or_default();
                        let outlines: Vec<geo_types::Geometry<f64>> = plan
                            .outlines(&project)
                            .into_iter()
                            .map(Into::into)
                            .collect();
                        territoria_io::export::write_geojson(
                            &path,
                            &outlines,
                            &plan.territory_table(&project, &stats),
                        )
                        .map_err(err)
                    }
                    Export::LayerGeoJson(l) => {
                        let layer = project.layer(l).ok_or("the layer was removed")?;
                        let g = &layer.geometry;
                        let geometries: Vec<geo_types::Geometry<f64>> = (0..layer.len())
                            .map(|f| match g.kind {
                                GeometryKind::Area => g.polygon(f).into(),
                                GeometryKind::Line => g.lines[f].clone().into(),
                                GeometryKind::Point => g.points[f].clone().into(),
                            })
                            .collect();
                        territoria_io::export::write_geojson(&path, &geometries, &layer.attributes)
                            .map_err(err)
                    }
                    Export::Data(_, excel, _) => {
                        let (name, table) = data_sheets.first().ok_or("the data was removed")?;
                        if excel {
                            territoria_io::export::write_xlsx(&path, &[(name.as_str(), table)])
                        } else {
                            territoria_io::export::write_csv(&path, table, b',')
                        }
                        .map_err(err)
                    }
                    Export::Changes(..) => {
                        let c = comparison.ok_or("the plans can't be compared")?;
                        territoria_io::export::write_xlsx(
                            &path,
                            &[
                                ("Changes", &c.table(&project)),
                                ("Territories", &c.territory_table()),
                            ],
                        )
                        .map_err(err)
                    }
                    // Written on the UI thread's request above.
                    Export::Map(_) | Export::Report(_) | Export::Html | Export::Atlas(..) => Ok(()),
                }
            })();
            JobResult::Exported { path, result }
        });
    }

    /// Opens a project, or imports boundaries/data by file type.
    pub fn open_path(&mut self, path: &Path) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if ext == territoria_model::file::EXTENSION {
            if self.history.dirty && !self.project.layers.is_empty() {
                self.confirm = Some(Confirm {
                    title: "Unsaved changes".into(),
                    text: "Open another project and discard the changes to this one?".into(),
                    action: ConfirmAction::DiscardThen(path.to_owned()),
                });
                return;
            }
            let p = path.to_owned();
            self.jobs
                .spawn(format!("Opening {}", file_name(path)), move |_| {
                    JobResult::Loaded {
                        result: territoria_model::file::load(&p).map_err(|e| e.to_string()),
                        path: p,
                    }
                });
        } else if territoria_io::VECTOR_EXTENSIONS.contains(&ext.as_str()) {
            self.import_boundaries(path.to_owned(), VectorOptions::default());
        } else if territoria_io::TABLE_EXTENSIONS.contains(&ext.as_str()) {
            self.import_table_file(path.to_owned(), TableOptions::default());
        } else {
            self.error(format!("Territoria can't open “{}” files.", ext));
        }
    }

    pub fn import_boundaries(&mut self, path: PathBuf, options: VectorOptions) {
        let label = format!("Reading {}", file_name(&path));
        self.jobs.spawn(label, move |_| {
            let result = territoria_io::read_vector(&path, &options)
                .map(|layers| {
                    layers
                        .into_iter()
                        .map(|imported| {
                            let geographic = imported.is_geographic();
                            let geometry = territoria_model::LayerGeometry::from_imported(
                                imported.geometries.clone(),
                                geographic,
                            );
                            PreparedLayer { imported, geometry }
                        })
                        .collect()
                })
                .map_err(|e| e.to_string());
            JobResult::Boundaries { path, result }
        });
    }

    /// Reads a newer version of a layer's boundaries in the background.
    pub(crate) fn read_replacement(&mut self, layer: LayerId, path: PathBuf) {
        let label = format!("Reading {}", file_name(&path));
        self.jobs.spawn(label, move |_| {
            let result = territoria_io::read_vector(&path, &VectorOptions::default())
                .map(|layers| {
                    layers
                        .into_iter()
                        .map(|imported| {
                            let geometry = territoria_model::LayerGeometry::from_imported(
                                imported.geometries.clone(),
                                imported.is_geographic(),
                            );
                            PreparedLayer { imported, geometry }
                        })
                        .collect()
                })
                .map_err(|e| e.to_string());
            JobResult::Replacement {
                layer,
                path,
                result,
            }
        });
    }

    /// Reads a table again from the file it came from, in the background.
    pub(crate) fn reload_table(&mut self, id: TableId) {
        let Some(path) = self.project.table(id).and_then(|t| t.source.path.clone()) else {
            self.toast("This table didn't come from a file.");
            return;
        };
        if !path.exists() {
            self.error(format!("{} is no longer there.", path.display()));
            return;
        }
        let label = format!("Reading {}", file_name(&path));
        self.jobs.spawn(label, move |_| JobResult::Reloaded {
            table: id,
            result: territoria_io::read_table(&path, &TableOptions::default())
                .map(|(t, _)| t)
                .map_err(|e| e.to_string()),
        });
    }

    /// Puts the newer boundaries in place of the layer's.
    fn apply_replacement(&mut self, layer: LayerId, path: PathBuf, prepared: Vec<PreparedLayer>) {
        let Some(old) = self.project.layer(layer) else {
            return;
        };
        // The file's layer of the same kind, preferably of the same name.
        let kind = old.kind();
        let name = old.name.to_lowercase();
        let pick = prepared
            .iter()
            .position(|p| p.geometry.kind == kind && p.imported.name.to_lowercase() == name)
            .or_else(|| prepared.iter().position(|p| p.geometry.kind == kind));
        let Some(i) = pick else {
            self.error(format!(
                "The file has no {} to replace “{}” with.",
                kind.label(),
                old.name
            ));
            return;
        };
        let p = prepared.into_iter().nth(i).expect("picked");
        let attributes = p.imported.attributes;
        // The key column: the same name as before, else the suggested one.
        let key = old
            .key_column
            .clone()
            .filter(|k| attributes.column(k).is_some())
            .or_else(|| {
                territoria_data::suggest::key_column(&attributes)
                    .map(|c| attributes.columns[c].name.clone())
            });
        let before = self.project.clone();
        let Some(r) = self
            .project
            .replace_layer(layer, p.geometry, attributes, key)
        else {
            return;
        };
        if let Some(l) = self.project.layer_mut(layer) {
            l.source.path = Some(path);
            l.source.crs_note = p.imported.crs_note;
        }
        self.history.push("Replace boundaries", before);
        self.meshes.retain(&self.project);
        self.selected = None;
        let fmt = territoria_analysis::format::format_value;
        let mut text = format!(
            "Replaced: {} areas kept their place by key",
            fmt(r.by_key as f64)
        );
        if r.by_place > 0 {
            text.push_str(&format!(", {} by position", fmt(r.by_place as f64)));
        }
        if r.new > 0 {
            text.push_str(&format!(
                ", {} are new (no territory yet)",
                fmt(r.new as f64)
            ));
        }
        if r.gone > 0 {
            text.push_str(&format!("; {} old areas are gone", fmt(r.gone as f64)));
        }
        self.toast(format!("{text}. Undo brings the old ones back."));
    }

    pub fn import_table_file(&mut self, path: PathBuf, options: TableOptions) {
        let label = format!("Reading {}", file_name(&path));
        self.jobs.spawn(label, move |_| JobResult::Table {
            result: territoria_io::read_table(&path, &options).map_err(|e| e.to_string()),
            path,
            options,
        });
    }

    fn save(&mut self) {
        match self.path.clone() {
            Some(p) => self.save_to(p),
            None => self.ask_file(FileAction::SaveProjectAs),
        }
    }

    /// Keeps the open tabs and the active plan with the project.
    fn store_workspace(&mut self) {
        let w = ProjectWorkspace {
            dock: self.dock.clone(),
            current_map: self.current_map,
            active_plan: self.active_plan,
            active_territory: self.active_territory,
            selection: self.selection,
            right_tab: self.right_tab,
        };
        self.project.workspace = serde_json::to_value(w).ok();
    }

    /// Reopens what was open when the project was saved, as far as it
    /// still exists.
    fn restore_workspace(&mut self) {
        let Some(w) = self
            .project
            .workspace
            .clone()
            .and_then(|v| serde_json::from_value::<ProjectWorkspace>(v).ok())
        else {
            return;
        };
        let p = &self.project;
        let exists = |tab: &Tab| match tab {
            Tab::Map(m) => p.map(*m).is_some(),
            Tab::Table(DataRef::Layer(l)) => p.layer(*l).is_some(),
            Tab::Table(DataRef::Table(t)) => p.table(*t).is_some(),
            Tab::Plan(id) => p.plan(*id).is_some(),
        };
        let mut dock = w.dock;
        dock.retain_tabs(|t| exists(t));
        if dock.iter_all_tabs().next().is_some() {
            self.dock = dock;
        }
        if let Some(m) = w.current_map.filter(|m| p.map(*m).is_some()) {
            self.current_map = Some(m);
        }
        if let Some(plan) = w.active_plan.and_then(|id| p.plan(id)) {
            self.active_plan = Some(plan.id);
            self.active_territory = w.active_territory.filter(|t| plan.territory(*t).is_some());
        }
        self.selection = match w.selection {
            Selection::MapLayer(m, l)
                if p.map(m).is_some_and(|m| m.layers.iter().any(|x| x.id == l)) =>
            {
                w.selection
            }
            Selection::Data(DataRef::Layer(l)) if p.layer(l).is_some() => w.selection,
            Selection::Data(DataRef::Table(t)) if p.table(t).is_some() => w.selection,
            _ => Selection::None,
        };
        self.right_tab = w.right_tab;
    }

    fn save_to(&mut self, path: PathBuf) {
        if self.project.title == "Untitled"
            && let Some(stem) = path.file_stem()
        {
            self.project.title = stem.to_string_lossy().into_owned();
        }
        self.store_workspace();
        let project = self.project.clone();
        let label = format!("Saving {}", file_name(&path));
        self.jobs.spawn(label, move |_| JobResult::Saved {
            result: territoria_model::file::save(&project, &path).map_err(|e| e.to_string()),
            path,
        });
        // Saved state is what was just handed to the job.
        self.history.dirty = false;
    }

    /// Where templates are kept.
    pub(crate) fn templates_dir(&self) -> PathBuf {
        match &self.paths {
            Some(p) => p.templates_dir(),
            None => {
                std::env::temp_dir().join(format!("territoria-templates-{}", std::process::id()))
            }
        }
    }

    /// The templates there are, by name.
    pub(crate) fn templates(&self) -> Vec<(String, PathBuf)> {
        let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(self.templates_dir())
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e == territoria_model::file::EXTENSION)
            })
            .filter_map(|p| Some((p.file_stem()?.to_string_lossy().into_owned(), p)))
            .collect();
        out.sort();
        out
    }

    /// The project as it is now, kept as a template under `name`.
    pub(crate) fn save_template(&mut self, name: &str) {
        let name: String = name
            .chars()
            .map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c })
            .collect();
        let dir = self.templates_dir();
        self.store_workspace();
        let project = self.project.clone();
        let label = format!("Keeping the template {name}");
        self.jobs.spawn(label, move |_| {
            let path = dir.join(format!("{name}.{}", territoria_model::file::EXTENSION));
            let result = std::fs::create_dir_all(&dir)
                .map_err(|e| e.to_string())
                .and_then(|_| {
                    territoria_model::file::save(&project, &path).map_err(|e| e.to_string())
                });
            JobResult::TemplateSaved { name, result }
        });
    }

    /// A new project from a template (after asking about unsaved changes).
    pub(crate) fn new_from_template(&mut self, name: String, path: PathBuf) {
        self.jobs
            .spawn(format!("Opening the template {name}"), move |_| {
                JobResult::TemplateLoaded {
                    result: territoria_model::file::load(&path).map_err(|e| e.to_string()),
                    name,
                }
            });
    }

    /// Autosaves unsaved changes to the recovery folder now and then.
    fn autosave(&mut self) {
        let minutes = self.settings.autosave_minutes;
        if minutes == 0 || !self.history.dirty || self.project.layers.is_empty() {
            return;
        }
        if self.last_autosave.elapsed() < Duration::from_secs(minutes as u64 * 60) {
            return;
        }
        self.last_autosave = Instant::now();
        let Some(dir) = self.paths.as_ref().map(Paths::recovery_dir) else {
            return;
        };
        self.store_workspace();
        let project = self.project.clone();
        self.jobs.spawn("", move |_| {
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join(format!("recovered.{}", territoria_model::file::EXTENSION));
            let result = territoria_model::file::save(&project, &path).map_err(|e| e.to_string());
            JobResult::Saved {
                path,
                result: result.and(Err("autosave".into())),
            }
        });
    }

    pub(crate) fn handle_job(&mut self, result: JobResult) {
        match result {
            JobResult::Boundaries { path, result } => match result {
                Ok(layers) => {
                    let mut dialog = ImportLayers::new(path.clone(), layers);
                    if let Some((_, title)) = self.attributions.get(&path)
                        && dialog.layers.len() == 1
                    {
                        dialog.layers[0].name = title.clone();
                    }
                    self.import_layers = Some(dialog);
                }
                Err(e) => self.error(format!("Couldn't read {}: {e}", file_name(&path))),
            },
            JobResult::Table {
                path,
                options,
                result,
            } => match result {
                Ok((table, info)) => {
                    let name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.import_table = Some(ImportTable::new(path, name, table, info, options));
                }
                Err(e) => self.error(format!("Couldn't read {}: {e}", file_name(&path))),
            },
            JobResult::Mesh {
                layer,
                crs,
                kind,
                rev,
                mesh,
            } => self.meshes.finished(layer, crs, kind, rev, mesh),
            JobResult::TemplateLoaded { name, result } => match result {
                Ok(mut project) => {
                    // A new project: untitled and not saved anywhere yet.
                    project.title = "Untitled".into();
                    self.set_project(project, None);
                    self.toast(format!("New project from the template {name}."));
                }
                Err(e) => self.error(format!("Couldn't open the template {name}: {e}")),
            },
            JobResult::TemplateSaved { name, result } => match result {
                Ok(()) => self.toast(format!("Kept as the template {name}.")),
                Err(e) => self.error(format!("Couldn't keep the template {name}: {e}")),
            },
            JobResult::Loaded { path, result } => match result {
                Ok(project) => self.set_project(project, Some(path)),
                Err(e) => self.error(format!("Couldn't open {}: {e}", file_name(&path))),
            },
            JobResult::Saved { path, result } => match result {
                Ok(()) => {
                    self.settings.add_recent(&path);
                    self.save_settings();
                    self.toast(format!("Saved {}", file_name(&path)));
                    self.path = Some(path);
                }
                // Autosaves report themselves as an error to stay quiet.
                Err(e) if e == "autosave" => {}
                Err(e) => {
                    self.history.dirty = true;
                    self.error(format!("Couldn't save {}: {e}", file_name(&path)));
                }
            },
            JobResult::Catalog(result) => {
                if let Some(d) = &mut self.download {
                    d.catalog = Some(result.map(std::sync::Arc::new));
                }
            }
            JobResult::Downloaded { set, result } => match result {
                Ok(path) => {
                    self.attributions
                        .insert(path.clone(), (set.attribution(), set.title()));
                    self.import_boundaries(path, VectorOptions::default());
                }
                Err(e) => self.error(format!(
                    "Couldn't download {} {}: {e}",
                    set.country, set.level
                )),
            },
            JobResult::Round {
                layer,
                units,
                result,
            } => match result {
                Ok(trip) => self.add_round(layer, &units, trip),
                Err(e) => self.error(format!("Couldn't plan the round: {e}")),
            },
            JobResult::Columns { target, result } => match result {
                Ok(columns) => {
                    let n = columns.len();
                    self.edit("Add columns", |p| {
                        if let Some(t) = data_table_mut(p, target) {
                            for mut c in columns {
                                c.name = t.unique_name(&c.name);
                                let _ = t.push_column(c);
                            }
                        }
                    });
                    self.toast(format!("{n} columns added."));
                }
                Err(e) => self.error(format!("Couldn't add the columns: {e}")),
            },
            JobResult::Exported { path, result } => match result {
                Ok(()) => self.toast(format!("Wrote {}", file_name(&path))),
                Err(e) => self.error(format!("Couldn't write {}: {e}", file_name(&path))),
            },
            JobResult::Geocoded {
                name,
                mut table,
                path,
                result,
            } => match result {
                Ok(found) => {
                    use territoria_data::Column;
                    let hits = &found.hits;
                    let column = |t: &territoria_data::Table, n: &str| t.unique_name(n);
                    let (lat, lon, quality) = (
                        column(&table, "Latitude"),
                        column(&table, "Longitude"),
                        column(&table, "Geocoding"),
                    );
                    let _ = table.push_column(Column::number(
                        lat.clone(),
                        hits.iter().map(|h| h.as_ref().map(|h| h.lat)).collect(),
                    ));
                    let _ = table.push_column(Column::number(
                        lon.clone(),
                        hits.iter().map(|h| h.as_ref().map(|h| h.lon)).collect(),
                    ));
                    let _ = table.push_column(Column::text(
                        quality,
                        territoria_data::ColumnKind::Text,
                        hits.iter()
                            .map(|h| {
                                Some(
                                    h.as_ref()
                                        .map_or("not found", |h| h.quality.label())
                                        .to_owned(),
                                )
                            })
                            .collect(),
                    ));
                    let found_n = hits.iter().flatten().count();
                    let rows = hits.len();
                    self.add_table(
                        name,
                        table,
                        path,
                        crate::dialogs::Placement::Points { x: lon, y: lat },
                        None,
                    );
                    self.toast(if found.stopped {
                        format!("Stopped: {found_n} of {rows} addresses found so far. Add the table again to go on; found ones are remembered.")
                    } else {
                        format!(
                            "{found_n} of {rows} addresses found ({} remembered from before). The Geocoding column tells how precisely.",
                            found.cached
                        )
                    });
                }
                Err(e) => self.error(format!("Couldn't find the addresses: {e}")),
            },
            JobResult::Replacement {
                layer,
                path,
                result,
            } => match result {
                Ok(prepared) => self.apply_replacement(layer, path, prepared),
                Err(e) => self.error(format!("Couldn't read {}: {e}", file_name(&path))),
            },
            JobResult::Reloaded { table, result } => match result {
                Ok(rows) => {
                    let mut changes = None;
                    self.edit("Reload table", |p| changes = p.replace_table(table, rows));
                    if let Some(c) = changes {
                        let fmt = territoria_analysis::format::format_value;
                        let mut text = format!(
                            "Reloaded: {} rows (before {})",
                            fmt(c.rows.1 as f64),
                            fmt(c.rows.0 as f64)
                        );
                        if !c.added.is_empty() {
                            text.push_str(&format!("; new columns: {}", c.added.join(", ")));
                        }
                        if !c.removed.is_empty() {
                            text.push_str(&format!("; gone: {}", c.removed.join(", ")));
                        }
                        if c.broken_links > 0 {
                            self.error(format!(
                                "{text}. {} links lost their key column: link the table again.",
                                c.broken_links
                            ));
                        } else {
                            self.toast(format!("{text}."));
                        }
                    }
                }
                Err(e) => self.error(format!("Couldn't reload the table: {e}")),
            },
            JobResult::Copied(result) => match result {
                Ok(image) => {
                    self.to_clipboard = Some(image);
                    self.toast("The map is on the clipboard: paste it into a mail or slide.");
                }
                Err(e) => self.error(format!("Couldn't copy the map: {e}")),
            },
            JobResult::Optimized { plan, result } => match result {
                Ok(proposal) => self.territories_panel.proposal = Some((plan, proposal)),
                Err(e) => self.error(format!("Balancing failed: {e}")),
            },
        }
    }

    pub fn set_project(&mut self, project: Project, path: Option<PathBuf>) {
        self.project = project;
        self.history.clear();
        self.values = Cache::default();
        self.meshes = MeshCache::default();
        self.analyses = AnalysisCache::default();
        self.territory_cache = TerritoryCache::default();
        self.stats = StatsCache::default();
        self.map_states.clear();
        self.selection = Selection::None;
        self.active_plan = self.project.plans.first().map(|p| p.id);
        self.active_territory = None;
        self.tool = Tool::Navigate;
        self.dock = DockState::new(self.project.maps.iter().map(|m| Tab::Map(m.id)).collect());
        self.current_map = self.project.maps.first().map(|m| m.id);
        self.restore_workspace();
        if let Some(p) = &path {
            self.settings.add_recent(p);
            self.save_settings();
        }
        self.path = path;
    }

    /// Reads boundaries and adds them with the suggested key and name
    /// columns, without the dialog.
    #[cfg(test)]
    pub fn add_boundaries_now(&mut self, path: &Path) -> Result<Vec<LayerId>, String> {
        let layers = territoria_io::read_vector(path, &VectorOptions::default())
            .map_err(|e| e.to_string())?;
        let prepared: Vec<PreparedLayer> = layers
            .into_iter()
            .map(|imported| {
                let geographic = imported.is_geographic();
                let geometry = territoria_model::LayerGeometry::from_imported(
                    imported.geometries.clone(),
                    geographic,
                );
                PreparedLayer { imported, geometry }
            })
            .collect();
        let before: Vec<LayerId> = self.project.layers.iter().map(|l| l.id).collect();
        let choices = ImportLayers::new(path.to_owned(), prepared).layers;
        self.add_layers(choices, path.to_owned());
        Ok(self
            .project
            .layers
            .iter()
            .map(|l| l.id)
            .filter(|id| !before.contains(id))
            .collect())
    }

    /// Adds a table and links it, colouring the map by `color_by`.
    #[cfg(test)]
    pub fn add_linked_table_now(
        &mut self,
        name: &str,
        table: territoria_data::Table,
        key: &str,
        layer: LayerId,
        color_by: Option<&str>,
    ) -> TableId {
        let id = self.project.add_table(name, table, SourceInfo::default());
        let mut dialog = LinkDialog::new(&self.project, id);
        dialog.layer = Some(layer);
        dialog.column = Some(key.to_owned());
        let mode = dialog
            .matching
            .as_ref()
            .map_or(territoria_data::MatchMode::Numeric, |m| m.mode);
        self.apply_link(LinkOutcome::Link {
            table: id,
            layer,
            column: key.to_owned(),
            mode,
            color_by: color_by.map(|c| (c.to_owned(), territoria_data::Aggregate::Sum)),
        });
        id
    }

    #[cfg(test)]
    pub fn set_map_export_for_test(&mut self, settings: crate::dialogs::MapExportSettings) {
        self.map_export = Some(settings);
    }

    #[cfg(test)]
    pub fn run_command_for_test(&mut self, command: Command) {
        let ctx = egui::Context::default();
        self.run_command(&ctx, command);
    }

    #[cfg(test)]
    pub fn open_formula_for_test(&mut self, data: DataRef, name: &str, source: &str) {
        let mut d = crate::dialogs::FormulaDialog::new(&self.project, data, None);
        d.name = name.into();
        d.source = source.into();
        self.formula = Some(d);
    }

    #[cfg(test)]
    pub fn report_pages_for_test(&mut self, plan: PlanId) -> Vec<String> {
        crate::report::pages(&self.report_data(plan).expect("report data"))
    }

    #[cfg(test)]
    pub fn apply_action_for_test(&mut self, action: Action) {
        let ctx = egui::Context::default();
        self.apply_action(&ctx, action);
    }

    #[cfg(test)]
    pub fn map_events_for_test(&mut self, events: Vec<MapEvent>) {
        let map = self.ensure_map();
        self.handle_map_events(events, map);
    }

    #[cfg(test)]
    pub fn save_to_for_test(&mut self, path: PathBuf) {
        self.save_to(path);
    }

    #[cfg(test)]
    pub fn open_tabs_for_test(&self) -> Vec<Tab> {
        self.dock.iter_all_tabs().map(|(_, t)| *t).collect()
    }

    #[cfg(test)]
    pub fn open_settings_for_test(&mut self) {
        self.settings_dialog = Some(crate::dialogs::SettingsDialog::new(self.tiles.cache_dir()));
    }

    #[cfg(test)]
    pub fn find_shortcut_for_test(&mut self, text: &str) {
        if let Some(d) = &mut self.settings_dialog {
            d.find_for_test(text);
        }
    }

    #[cfg(test)]
    pub fn import_as_addresses_for_test(&mut self) {
        if let Some(d) = &mut self.import_table {
            d.placement = crate::dialogs::Placement::Address(
                crate::dialogs::AddressColumns::suggest(&d.table),
            );
        }
    }

    #[cfg(test)]
    pub fn analyses_for_test(
        &mut self,
        ml: &MapLayer,
    ) -> std::sync::Arc<territoria_analysis::Scaled> {
        self.analyses
            .get(&self.project, &mut self.values, ml.id, &ml.kind)
            .expect("an analysis")
    }

    #[cfg(test)]
    pub fn search_for_test(&mut self, query: &str) {
        self.search = query.into();
        self.search_hits = crate::search::search(&self.project, query, 12);
        self.search_focus = true;
    }

    #[cfg(test)]
    pub fn help_topic_for_test(&self) -> Option<&'static str> {
        self.help.as_ref().map(|h| h.topic)
    }

    #[cfg(test)]
    pub fn add_choropleth_for_test(
        &mut self,
        map: MapId,
        layer: LayerId,
        source: territoria_model::ValueSource,
    ) -> Option<MapLayerId> {
        self.add_choropleth(map, layer, "Value".into(), source)
    }

    #[cfg(test)]
    pub fn atlas_page_for_test(&mut self, layer: LayerId, column: &str, page: usize) -> String {
        self.atlas_pages(layer, Some(column))
            .unwrap()
            .swap_remove(page)
    }

    #[cfg(test)]
    pub fn path_for_test(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    #[cfg(test)]
    pub fn keymap_for_test(&self) -> &Keymap {
        &self.keymap
    }

    #[cfg(test)]
    pub fn toasts_for_test(&self) -> Vec<String> {
        self.toasts.iter().map(|t| t.text.clone()).collect()
    }

    #[cfg(test)]
    pub fn measure_for_test(&mut self, points: Vec<[f64; 2]>) {
        self.tool = Tool::Measure;
        let map = self.ensure_map();
        self.map_states.entry(map).or_default().measure = points;
    }

    #[cfg(test)]
    pub fn view_for_test(&self) -> Option<([f64; 2], f64)> {
        let map = self.current_map?;
        self.map_states.get(&map)?.view.map(|v| (v.center, v.scale))
    }

    #[cfg(test)]
    pub fn presenting_for_test(&self) -> bool {
        self.presenting
    }

    #[cfg(test)]
    pub fn apply_formula_for_test(&mut self, data: DataRef, name: &str, source: &str) {
        self.apply_formula(data, None, name, source);
    }

    #[cfg(test)]
    pub fn import_table_for_test(&self) -> Option<(&territoria_data::Table, &str)> {
        self.import_table
            .as_ref()
            .map(|d| (&d.table, d.name.as_str()))
    }

    #[cfg(test)]
    pub fn open_table_for_test(&mut self, data: DataRef) {
        self.dock.push_to_focused_leaf(Tab::Table(data));
    }

    #[cfg(test)]
    pub fn open_link_dialog(&mut self, table: TableId) {
        self.link = Some(LinkDialog::new(&self.project, table));
    }

    #[cfg(test)]
    pub fn close_dialogs(&mut self) {
        self.import_layers = None;
        self.import_table = None;
        self.link = None;
        self.color_by = None;
        self.new_plan = None;
        self.formula = None;
    }

    #[cfg(test)]
    pub fn duplicate_plan_for_test(&mut self, plan: PlanId) -> Option<PlanId> {
        self.duplicate_plan(plan);
        self.active_plan.filter(|p| *p != plan)
    }

    #[cfg(test)]
    pub fn marked_for_test(&self) -> Option<Vec<u32>> {
        self.territories_panel
            .highlight(self.active_plan)
            .map(|(_, u)| u.to_vec())
    }

    /// Creates a plan of `count` territories, balanced from scratch.
    #[cfg(test)]
    pub fn add_balanced_plan_now(&mut self, layer: LayerId, count: usize) -> PlanId {
        let plan = self.project.add_plan("Sales territories", layer, count);
        self.apply_default_palette(plan);
        self.show_plan(plan);
        let settings = crate::territories::BalanceSettings {
            start_over: true,
            ..Default::default()
        };
        if let Some(input) =
            crate::territories::balance_input(&self.project, &mut self.values, plan, &settings)
            && let Ok(proposal) = crate::territories::run_balance(input, &mut |_| true)
        {
            let rev = self.project.bump();
            if let Some(p) = self.project.plan_mut(plan) {
                for (u, t) in &proposal.moves {
                    p.assign(&[*u], Some(*t));
                }
                p.rev = rev;
            }
            let g = self
                .project
                .plan(plan)
                .and_then(|p| self.project.layer(p.layer))
                .map(|l| l.geometry.clone());
            if let (Some(g), Some(p)) = (g, self.project.plan_mut(plan)) {
                p.recolor(&g);
            }
        }
        plan
    }

    // -- Applying dialog results -------------------------------------------------

    fn add_layers(&mut self, choices: Vec<crate::dialogs::LayerChoice>, path: PathBuf) {
        let before = self.project.clone();
        let mut added = Vec::new();
        for c in choices.into_iter().filter(|c| c.include) {
            let imported = c.prepared.imported;
            let source = SourceInfo {
                path: Some(path.clone()),
                crs_note: imported.crs_note.clone(),
                attribution: self.attributions.get(&path).map(|(a, _)| a.clone()),
            };
            let key = c
                .key_column
                .as_deref()
                .and_then(|k| imported.attributes.column_index(k));
            let (geometry, attributes) = match key {
                Some(k)
                    if c.merge && territoria_model::duplicate_keys(&imported.attributes, k) > 0 =>
                {
                    territoria_model::merge_by_column(
                        &c.prepared.geometry,
                        &imported.attributes,
                        k,
                        None,
                        false,
                    )
                }
                _ => (c.prepared.geometry, imported.attributes),
            };
            let id = self
                .project
                .add_layer(&c.name, geometry, attributes, source);
            if let Some(l) = self.project.layer_mut(id) {
                l.key_column = c.key_column;
                l.name_column = c.name_column;
            }
            added.push(id);
        }
        let map = self.ensure_map();
        for &id in &added {
            self.add_boundaries_to_map(map, id);
        }
        self.history.push("Add boundaries", before);
        if let Some(s) = self.map_states.get_mut(&map) {
            s.fit_requested = true;
        }
        if let Some(&id) = added.last() {
            self.selection = Selection::Data(DataRef::Layer(id));
        }
    }

    #[cfg(test)]
    pub fn merge_areas_for_test(
        &mut self,
        layer: LayerId,
        column: &str,
        prefix: Option<usize>,
        name: &str,
    ) {
        self.merge_areas(layer, column, prefix, name);
    }

    fn cache_dir(&self) -> PathBuf {
        match &self.paths {
            Some(p) => p.data_dir.join("boundaries"),
            None => std::env::temp_dir().join("territoria-boundaries"),
        }
    }

    fn load_catalog(&mut self) {
        let dir = self.cache_dir();
        self.jobs.spawn("", move |_| {
            JobResult::Catalog(territoria_io::online::catalog(&dir).map_err(|e| e.to_string()))
        });
    }

    fn download_boundaries(&mut self, set: territoria_io::online::BoundarySet, simplified: bool) {
        let dir = self.cache_dir();
        let label = format!("Downloading {} {}", set.country, set.level);
        self.jobs.spawn_cancellable(label, move |job| {
            let result =
                territoria_io::online::download(&set, simplified, &dir, &mut |done, total| {
                    if let Some(t) = total.filter(|t| *t > 0) {
                        job.progress(done as f32 / t as f32);
                    }
                    !job.cancelled()
                })
                .map_err(|e| e.to_string());
            JobResult::Downloaded { set, result }
        });
    }

    /// The current map as SVG, as it is on screen.
    fn map_svg(&mut self, settings: &crate::dialogs::MapExportSettings) -> Result<String, String> {
        self.map_render(settings, false, None)
    }

    pub(crate) fn map_svg_for_cli(
        &mut self,
        settings: &crate::dialogs::MapExportSettings,
    ) -> Result<String, String> {
        self.map_render(settings, false, None)
    }

    pub(crate) fn set_map_export_settings(&mut self, settings: crate::dialogs::MapExportSettings) {
        self.map_export = Some(settings);
    }

    /// The current map as a web page (all of it, with hover texts).
    pub(crate) fn map_page(
        &mut self,
        settings: &crate::dialogs::MapExportSettings,
    ) -> Result<String, String> {
        self.map_render(settings, true, None)
    }

    fn map_render(
        &mut self,
        settings: &crate::dialogs::MapExportSettings,
        page: bool,
        at: Option<&AtlasPage>,
    ) -> Result<String, String> {
        use territoria_model::map::Background;
        let map_id = self.current_map.ok_or("There's no map to export.")?;
        let map = self
            .project
            .map(map_id)
            .cloned()
            .ok_or("The map was removed.")?;
        // The view on screen, else the one kept with the map, else all of it.
        let on_screen = self.map_states.get(&map_id).and_then(|s| s.view);
        // A printed page: the map in its frame, showing what the screen shows.
        let page_size = if at.is_none() && !page {
            settings.page.size()
        } else {
            None
        };
        let frame = page_size
            .map(|p| crate::export_map::page_frame(p, !settings.title.is_empty(), settings.legend));
        let rect = frame.unwrap_or_else(|| {
            egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                on_screen.map_or(egui::vec2(1600.0, 1000.0), |v| v.rect.size()),
            )
        });
        let kept_view = match at {
            Some(a) => Some(a.view),
            None => on_screen
                .map(|v| {
                    let k = (v.rect.width() / rect.width()).max(v.rect.height() / rect.height());
                    territoria_render::View {
                        rect,
                        scale: v.scale * k.max(0.01) as f64,
                        ..v
                    }
                })
                .or(map.view.map(|(center, scale)| territoria_render::View {
                    center,
                    scale,
                    rect,
                })),
        };
        let mut meshes = HashMap::new();
        let mut cells = HashMap::new();
        for ml in map.layers.iter().filter(|l| l.visible) {
            let id = match &ml.kind {
                MapLayerKind::Territories { plan, .. } => self.project.plan(*plan).map(|p| p.layer),
                k => k.layer(),
            };
            let Some(layer) = id.and_then(|id| self.project.layer(id)) else {
                continue;
            };
            match self.meshes.get(layer, &map.crs, &mut self.jobs) {
                Some(m) => {
                    meshes.insert(layer.id, m);
                }
                None => {
                    return Err("The map is still being prepared; try again in a moment.".into());
                }
            }
            if matches!(ml.kind, MapLayerKind::Territories { .. })
                && layer.kind() == GeometryKind::Point
            {
                match self.meshes.get_cells(layer, &map.crs, &mut self.jobs) {
                    Some(m) => {
                        cells.insert(layer.id, m);
                    }
                    None => {
                        return Err(
                            "The map is still being prepared; try again in a moment.".into()
                        );
                    }
                }
            }
        }
        let view = match kept_view {
            Some(v) => v,
            None => {
                let mut extent = territoria_render::Bounds::EMPTY;
                for m in meshes.values() {
                    if !m.extent.is_empty() {
                        let (a, b) = (m.to_display(m.extent.min), m.to_display(m.extent.max));
                        extent.add([a[0] as f32, a[1] as f32]);
                        extent.add([b[0] as f32, b[1] as f32]);
                    }
                }
                if extent.is_empty() {
                    return Err("The map is empty.".into());
                }
                territoria_render::View::fit(
                    [extent.min[0] as f64, extent.min[1] as f64],
                    [extent.max[0] as f64, extent.max[1] as f64],
                    rect,
                )
            }
        };
        let mut rcx = crate::resolve::Context {
            project: &self.project,
            values: &mut self.values,
            analyses: &mut self.analyses,
            territories: &mut self.territory_cache,
            dark: false,
        };
        let mut resolved = Vec::new();
        let mut legends = Vec::new();
        let mut grids = Vec::new();
        let mut credits: Vec<String> = Vec::new();
        if let Background::Tiles { attribution, .. } = &map.background
            && !attribution.is_empty()
        {
            credits.push(attribution.clone());
        }
        // Per map layer how many resolved layers it gave (for hover texts).
        let mut counts: Vec<(MapLayer, usize)> = Vec::new();
        for ml in map.layers.iter().filter(|l| l.shown_at(view.scale)) {
            let r = crate::resolve::resolve(&mut rcx, &meshes, &cells, ml, &map.crs);
            counts.push((ml.clone(), r.len()));
            resolved.extend(r);
            match &ml.kind {
                MapLayerKind::Flows { style, value, .. } => {
                    let max = rcx
                        .analyses
                        .flows(rcx.project, ml.id, &ml.kind)
                        .map_or(0.0, |l| l.iter().map(|l| l.value).fold(0.0, f64::max));
                    legends.push((
                        ml.name.clone(),
                        vec![territoria_analysis::LegendEntry {
                            color: style.color,
                            label: format!(
                                "Line widths follow {}; widest {}",
                                value.as_deref().unwrap_or("the number of rows"),
                                territoria_analysis::format::format_value(max)
                            ),
                            count: 0,
                        }],
                    ));
                }
                MapLayerKind::Bivariate { x, y, .. } => {
                    if let Some(grid) = rcx
                        .analyses
                        .get(rcx.project, rcx.values, ml.id, &ml.kind)
                        .and_then(|s| s.grid.clone())
                    {
                        grids.push(crate::export_map::GridLegend {
                            title: ml.name.clone(),
                            x: x.label(rcx.project),
                            y: y.label(rcx.project),
                            grid,
                        });
                    }
                }
                MapLayerKind::Charts { parts, .. } => {
                    legends.push((
                        ml.name.clone(),
                        parts
                            .iter()
                            .map(|p| territoria_analysis::LegendEntry {
                                color: p.color,
                                label: p.label.clone(),
                                count: 0,
                            })
                            .collect(),
                    ));
                }
                MapLayerKind::Heatmap { ramp, .. } => {
                    let stops = ramp.colors(3);
                    legends.push((
                        ml.name.clone(),
                        vec![
                            territoria_analysis::LegendEntry {
                                color: stops[0],
                                label: "fewer".into(),
                                count: 0,
                            },
                            territoria_analysis::LegendEntry {
                                color: stops[2],
                                label: "more".into(),
                                count: 0,
                            },
                        ],
                    ));
                }
                MapLayerKind::Rings {
                    radii_km, color, ..
                } => {
                    let radii: Vec<String> = radii_km
                        .iter()
                        .map(|r| territoria_data::table::format_number(*r))
                        .collect();
                    legends.push((
                        ml.name.clone(),
                        vec![territoria_analysis::LegendEntry {
                            color: *color,
                            label: format!("Within {} km", radii.join(" / ")),
                            count: 0,
                        }],
                    ));
                }
                MapLayerKind::Choropleth { .. } | MapLayerKind::Categories { .. } => {
                    if let Some(scaled) = rcx.analyses.get(rcx.project, rcx.values, ml.id, &ml.kind)
                    {
                        legends.push((ml.name.clone(), scaled.legend.clone()));
                    }
                }
                MapLayerKind::Territories { plan, .. } => {
                    if let Some(p) = rcx.project.plan(*plan) {
                        let counts = p.members();
                        legends.push((
                            ml.name.clone(),
                            p.territories
                                .iter()
                                .zip(&counts)
                                .map(|(t, m)| territoria_analysis::LegendEntry {
                                    color: t.color,
                                    label: t.name.clone(),
                                    count: m.len(),
                                })
                                .collect(),
                        ));
                    }
                }
                MapLayerKind::Symbols {
                    layer,
                    source,
                    normalize,
                    symbol,
                    ..
                } => {
                    let v = rcx
                        .values
                        .normalized(rcx.project, *layer, source, normalize);
                    let max = v.iter().flatten().fold(0.0f64, |a, b| a.max(*b));
                    let mut entries = vec![territoria_analysis::LegendEntry {
                        color: symbol.color,
                        label: format!(
                            "Circle areas grow with the value; largest {}",
                            territoria_analysis::format::format_value(max)
                        ),
                        count: v.iter().flatten().count(),
                    }];
                    // Coloured by category: the categories follow.
                    if symbol.color_by.is_some()
                        && let Some(scaled) =
                            rcx.analyses.get(rcx.project, rcx.values, ml.id, &ml.kind)
                    {
                        entries[0].color = territoria_analysis::Rgba::rgb(0xc4, 0xc4, 0xca);
                        entries.extend(scaled.legend.iter().cloned());
                    }
                    legends.push((ml.name.clone(), entries));
                }
                MapLayerKind::Boundaries { .. } => {}
            }
            let source = match &ml.kind {
                MapLayerKind::Territories { plan, .. } => rcx.project.plan(*plan).map(|p| p.layer),
                k => k.layer(),
            };
            if let Some(a) = source
                .and_then(|l| rcx.project.layer(l))
                .and_then(|l| l.source.attribution.clone())
                && !credits.contains(&a)
            {
                credits.push(a);
            }
        }
        // An atlas page: everything but its areas veiled.
        if let Some((layer, keep)) = at.and_then(|a| a.veil.as_ref())
            && let Some(mesh) = meshes.get(layer)
        {
            let veil = egui::Color32::from_white_alpha(190);
            resolved.push(crate::resolve::Resolved {
                mesh: mesh.clone(),
                fills: (0..mesh.bounds.len())
                    .map(|f| (!keep.contains(&f)).then_some(veil))
                    .collect(),
                arcs: Vec::new(),
                line_width: 1.0,
                point_radius: 0.0,
                point_stroke: egui::Stroke::NONE,
                labels: crate::resolve::Labels::None,
                sites: Vec::new(),
                symbols: Vec::new(),
                symbol_fill: egui::Color32::TRANSPARENT,
                symbol_colors: Vec::new(),
                symbol_stroke: egui::Stroke::NONE,
                charts: Vec::new(),
                flows: Vec::new(),
                rings: Vec::new(),
            });
        }
        // Legends top layer first, like the Contents panel.
        legends.reverse();
        grids.reverse();
        let company = self.settings.company.trim();
        if !company.is_empty() {
            credits.insert(0, company.to_owned());
        }
        let today = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| (d.as_secs() / 86_400) as i32);
        let footer = settings
            .footer
            .replace("{project}", &self.project.title)
            .replace("{map}", &map.name)
            .replace("{date}", &territoria_data::date::format(today));
        let deco = crate::export_map::Decorations {
            footer,
            title: settings.title.clone(),
            logo: self
                .settings
                .logo
                .as_deref()
                .and_then(|p| crate::export_map::Logo::load(p, 140.0, 44.0)),
            legends: if settings.legend { legends } else { Vec::new() },
            grids: if settings.legend { grids } else { Vec::new() },
            credits,
            scale_bar: settings.scale_bar,
        };
        if page {
            // The whole map, finer, with each feature's hover text.
            let mut extent = territoria_render::Bounds::EMPTY;
            for m in meshes.values().chain(cells.values()) {
                if !m.extent.is_empty() {
                    let (a, b) = (m.to_display(m.extent.min), m.to_display(m.extent.max));
                    extent.add([a[0] as f32, a[1] as f32]);
                    extent.add([b[0] as f32, b[1] as f32]);
                }
            }
            if extent.is_empty() {
                return Err("The map is empty.".into());
            }
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
            let full = territoria_render::View::fit(
                [extent.min[0] as f64, extent.min[1] as f64],
                [extent.max[0] as f64, extent.max[1] as f64],
                rect,
            );
            // Per resolved layer the data layer it shows.
            let mut data: Vec<Option<LayerId>> = Vec::new();
            for (ml, n) in &counts {
                let d = match &ml.kind {
                    MapLayerKind::Territories { plan, .. } => {
                        self.project.plan(*plan).map(|p| p.layer)
                    }
                    MapLayerKind::Rings { .. } => None,
                    k => k.layer(),
                };
                data.extend(std::iter::repeat_n(d, *n));
            }
            // Layers hidden under an opaque one of the same data stay out,
            // and only the top one of each data layer carries hover texts.
            let opaque = |r: &crate::resolve::Resolved| {
                r.mesh.kind == GeometryKind::Area
                    && !r.fills.is_empty()
                    && r.fills.iter().all(|f| f.is_some_and(|c| c.a() == 255))
            };
            let mut keep = vec![true; resolved.len()];
            let mut top_seen: std::collections::HashSet<LayerId> = Default::default();
            let mut tips: Vec<Option<Vec<String>>> = vec![None; resolved.len()];
            let mut covered: std::collections::HashSet<LayerId> = Default::default();
            for i in (0..resolved.len()).rev() {
                let Some(l) = data[i] else { continue };
                if covered.contains(&l) && resolved[i].mesh.kind == GeometryKind::Area {
                    keep[i] = false;
                    continue;
                }
                if opaque(&resolved[i]) {
                    covered.insert(l);
                }
                if top_seen.insert(l) {
                    tips[i] = Some(crate::map_view::feature_tips(
                        &map,
                        &self.project,
                        &mut self.values,
                        l,
                    ));
                }
            }
            let (resolved, tips): (Vec<_>, Vec<_>) = resolved
                .into_iter()
                .zip(tips)
                .zip(keep)
                .filter(|(_, k)| *k)
                .map(|(rt, _)| rt)
                .unzip();
            let mut body = String::new();
            crate::export_map::map_into_with(&mut body, &resolved, &full, "", &tips, true);
            return Ok(crate::export_map::html(
                &body,
                &full,
                &settings.title,
                &deco,
            ));
        }
        // Heat maps as images over the layers.
        let mut overlays = String::new();
        for ml in map.layers.iter().filter(|l| l.visible) {
            if let MapLayerKind::Heatmap {
                layer,
                weight,
                radius_km,
                ramp,
            } = &ml.kind
                && let Some(mesh) = meshes.get(layer)
            {
                let weights = weight.as_deref().and_then(|w| {
                    self.project
                        .layer(*layer)
                        .and_then(|l| l.attributes.column(w))
                });
                let pts = crate::heat::screen_points(mesh, &view, weights);
                let radius = crate::heat::radius_px(&view, &map.crs, *radius_km);
                let image = crate::heat::raster(&pts, view.rect, radius, ramp, 2.0);
                if let Some(uri) = crate::heat::png_data_uri(&image) {
                    let r = view.rect;
                    overlays.push_str(&format!(
                        r#"<image x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" opacity="{:.3}" preserveAspectRatio="none" href="{uri}"/>"#,
                        r.min.x,
                        r.min.y,
                        r.width(),
                        r.height(),
                        ml.opacity
                    ));
                }
            }
        }
        if let Some(size) = page_size {
            return Ok(crate::export_map::page_svg(
                &resolved,
                &view,
                &map.crs,
                &deco,
                size,
                "sans-serif",
                &overlays,
            ));
        }
        Ok(crate::export_map::svg(
            &resolved,
            &view,
            &map.crs,
            &deco,
            "sans-serif",
            &overlays,
        ))
    }

    /// A new layer with the areas of `layer` merged by `column`.
    fn merge_areas(&mut self, layer: LayerId, column: &str, prefix: Option<usize>, name: &str) {
        let Some(l) = self.project.layer(layer) else {
            return;
        };
        let Some(c) = l.attributes.column_index(column) else {
            return;
        };
        let (geometry, attributes) =
            territoria_model::merge_by_column(&l.geometry, &l.attributes, c, prefix, true);
        let source = l.source.clone();
        let before = self.project.clone();
        let id = self.project.add_layer(name, geometry, attributes, source);
        if let Some(new) = self.project.layer_mut(id) {
            new.key_column = Some(column.to_owned());
        }
        let map = self.ensure_map();
        self.add_boundaries_to_map(map, id);
        self.history.push("Merge areas", before);
        self.selection = Selection::Data(DataRef::Layer(id));
    }

    /// A new table from a summary dialog, opened.
    pub(crate) fn create_summary(&mut self, d: &crate::dialogs::SummariseDialog) {
        let Some(table) = d.table(&self.project) else {
            return;
        };
        let before = self.project.clone();
        let name = self.project.unique_table_name(d.name.trim());
        let id = self.project.add_table(&name, table, SourceInfo::default());
        self.history.push("Summarise", before);
        self.dock
            .push_to_focused_leaf(Tab::Table(DataRef::Table(id)));
        self.selection = Selection::Data(DataRef::Table(id));
    }

    /// Adds the columns a join dialog gives.
    pub(crate) fn apply_join(&mut self, d: &crate::dialogs::JoinDialog) {
        if d.mode == crate::dialogs::JoinMode::ByRoad {
            if let (DataRef::Layer(from), Some(to)) = (d.target, d.layer) {
                self.join_by_road(from, to);
            }
            return;
        }
        let Some(columns) = d.columns_for(&self.project, &mut self.values) else {
            return;
        };
        if columns.is_empty() {
            self.toast("Nothing to add.");
            return;
        }
        let n = columns.len();
        let target = d.target;
        self.edit("Add columns", |p| {
            if let Some(t) = data_table_mut(p, target) {
                for mut c in columns {
                    c.name = t.unique_name(&c.name);
                    let _ = t.push_column(c);
                }
            }
        });
        self.toast(format!("{n} columns added."));
    }

    /// Asks the routing service for the quickest round through `units`
    /// (points), starting at the first.
    fn plan_round(&mut self, layer: LayerId, units: Vec<u32>) {
        let Some(l) = self.project.layer(layer) else {
            return;
        };
        let stops: Vec<geo_types::Coord<f64>> = units
            .iter()
            .filter_map(|&u| l.geometry.points.get(u as usize)?.0.first().map(|p| p.0))
            .collect();
        if stops.len() != units.len() {
            self.error("Some of the selected points have no place.");
            return;
        }
        let settings = self.settings.routing.clone();
        let label = format!("Planning a round of {} stops", units.len());
        self.jobs.spawn(label, move |_| JobResult::Round {
            layer,
            units,
            result: territoria_io::routing::trip(&settings, &stops).map_err(|e| e.to_string()),
        });
    }

    /// The round as a line layer on the map, and the visiting order as a
    /// column of the points.
    fn add_round(&mut self, layer: LayerId, units: &[u32], trip: territoria_io::routing::Trip) {
        use territoria_data::{Column, ColumnKind, Table};
        let Some(name) = self.project.layer(layer).map(|l| l.name.clone()) else {
            return;
        };
        let before = self.project.clone();
        let n = self.project.layer(layer).map_or(0, |l| l.len());
        let mut order = vec![None; n];
        for (u, k) in units.iter().zip(&trip.order) {
            if let Some(o) = order.get_mut(*u as usize) {
                *o = Some((*k + 1) as f64);
            }
        }
        if let Some(t) = data_table_mut(&mut self.project, DataRef::Layer(layer)) {
            let mut c = Column::number(t.unique_name("Visit order"), order);
            c.kind = ColumnKind::Integer;
            let _ = t.push_column(c);
        }
        let hours = (trip.minutes / 60.0).floor();
        let summary = format!(
            "{} stops, {} km, {} h {:02} min",
            units.len(),
            territoria_analysis::format::format_value(trip.km.round()),
            hours,
            (trip.minutes - hours * 60.0).round() as u32
        );
        let table = Table::new(vec![
            Column::text(
                "Round",
                ColumnKind::Key,
                vec![Some(format!("Round of {name}"))],
            ),
            Column::number("Stops", vec![Some(units.len() as f64)]),
            Column::number("Km", vec![Some((trip.km * 10.0).round() / 10.0)]),
            Column::number("Minutes", vec![Some(trip.minutes.round())]),
        ])
        .expect("one row each");
        let geometry = territoria_model::LayerGeometry::from_imported(
            territoria_io::Geometries::Lines(vec![geo_types::MultiLineString(vec![
                geo_types::LineString(trip.path),
            ])]),
            true,
        );
        let title = format!("Round of {name}");
        let id = self
            .project
            .add_layer(&title, geometry, table, SourceInfo::default());
        let map = self.ensure_map();
        let ml = self.project.new_map_layer_id();
        self.push_map_layer(
            map,
            MapLayer {
                id: ml,
                name: format!("{title} ({summary})"),
                visible: true,
                opacity: 1.0,
                zoom: Default::default(),
                kind: MapLayerKind::Boundaries {
                    layer: id,
                    style: BoundaryStyle {
                        fill: territoria_analysis::Rgba([0x1d, 0x4e, 0xd8, 220]),
                        stroke_width: 3.0,
                        ..Default::default()
                    },
                },
            },
        );
        self.history.push("Plan a round", before);
        self.toast(format!("Round planned: {summary}."));
    }

    /// Travel time and distance by road from each feature of `from` to the
    /// nearest point of `to`, asked from the routing service.
    fn join_by_road(&mut self, from: LayerId, to: LayerId) {
        use territoria_data::{Column, ColumnKind};
        let (Some(a), Some(b)) = (self.project.layer(from), self.project.layer(to)) else {
            return;
        };
        let place = |l: &territoria_model::Layer, f: usize| {
            l.geometry
                .points
                .get(f)
                .and_then(|p| p.0.first().map(|p| p.0))
                .or_else(|| l.geometry.label_points.get(f).copied().flatten())
        };
        let origins: Vec<Option<geo_types::Coord<f64>>> =
            (0..a.len()).map(|f| place(a, f)).collect();
        let dests: Vec<(String, geo_types::Coord<f64>)> = (0..b.len())
            .filter_map(|f| Some((b.display_name(f), place(b, f)?)))
            .collect();
        if dests.is_empty() {
            self.error(format!("{} has no points to travel to.", b.name));
            return;
        }
        let settings = self.settings.routing.clone();
        let name = b.name.clone();
        let label = format!("Travel times to {name}");
        self.jobs.spawn_cancellable(label, move |job| {
            let present: Vec<geo_types::Coord<f64>> = origins.iter().flatten().copied().collect();
            let targets: Vec<geo_types::Coord<f64>> = dests.iter().map(|d| d.1).collect();
            let result = territoria_io::routing::nearest_by_road(
                &settings,
                &present,
                &targets,
                &mut |done, total| {
                    if total > 0 {
                        job.progress(done as f32 / total as f32);
                    }
                    !job.cancelled()
                },
            )
            .map_err(|e| e.to_string())
            .map(|found| {
                // Back to one value per feature (features without a place get none).
                let mut found = found.into_iter();
                let per: Vec<Option<territoria_io::routing::Nearest>> = origins
                    .iter()
                    .map(|o| o.and_then(|_| found.next().flatten()))
                    .collect();
                let mut minutes = Column::number(
                    format!("Minutes to {name}"),
                    per.iter()
                        .map(|n| n.map(|n| (n.minutes * 10.0).round() / 10.0))
                        .collect(),
                );
                minutes.kind = ColumnKind::Number;
                vec![
                    Column::text(
                        format!("Nearest {name} by road"),
                        ColumnKind::Text,
                        per.iter()
                            .map(|n| n.map(|n| dests[n.destination].0.clone()))
                            .collect(),
                    ),
                    minutes,
                    Column::number(
                        format!("Road km to {name}"),
                        per.iter()
                            .map(|n| {
                                n.map(|n| (n.km * 10.0).round() / 10.0)
                                    .filter(|k| k.is_finite())
                            })
                            .collect(),
                    ),
                ]
            });
            JobResult::Columns {
                target: DataRef::Layer(from),
                result,
            }
        });
    }

    /// Rings on the map and a table of what lies within them.
    pub(crate) fn create_catchment(&mut self, spec: &territoria_model::catchment::CatchmentSpec) {
        let table =
            territoria_model::catchment::catchment_table(&self.project, &mut self.values, spec);
        let label = spec.centres.label(&self.project);
        let before = self.project.clone();
        let map = self.ensure_map();
        let id = self.project.new_map_layer_id();
        let radii: Vec<String> = spec
            .radii_km
            .iter()
            .map(|r| territoria_data::table::format_number(*r))
            .collect();
        self.push_map_layer(
            map,
            MapLayer {
                id,
                name: format!("Rings {} km", radii.join(" / ")),
                visible: true,
                opacity: 1.0,
                zoom: Default::default(),
                kind: MapLayerKind::Rings {
                    centres: spec.centres,
                    radii_km: spec.radii_km.clone(),
                    color: territoria_analysis::Rgba::rgb(0x7c, 0x3a, 0xed),
                },
            },
        );
        let name = self
            .project
            .unique_table_name(&format!("Catchment of {label}"));
        let t = self.project.add_table(&name, table, SourceInfo::default());
        self.history.push("Catchment rings", before);
        self.dock
            .push_to_focused_leaf(Tab::Table(DataRef::Table(t)));
        self.selection = Selection::MapLayer(map, id);
    }

    /// Sites for a plan from a point layer: matched to the territories, or
    /// the best of them chosen first.
    pub(crate) fn place_sites(&mut self, d: &crate::dialogs::SitesDialog) {
        let (Some(plan), Some(points)) = (
            self.project.plan(d.plan),
            d.layer.and_then(|l| self.project.layer(l)),
        ) else {
            return;
        };
        let Some(layer) = self.project.layer(plan.layer) else {
            return;
        };
        let geometry = layer.geometry.clone();
        let weights = plan
            .measures
            .first()
            .filter(|m| m.source != territoria_model::ValueSource::Units)
            .map(|m| self.values.values(&self.project, plan.layer, &m.source));
        // Points with a location, and their names.
        let located: Vec<(usize, geo_types::Coord<f64>)> = points
            .geometry
            .points
            .iter()
            .enumerate()
            .filter_map(|(i, mp)| mp.0.first().map(|p| (i, p.0)))
            .collect();
        let coords: Vec<geo_types::Coord<f64>> = located.iter().map(|(_, c)| *c).collect();
        let names: Vec<String> = located
            .iter()
            .map(|(i, _)| points.display_name(*i))
            .collect();
        let (plan_id, best, rename) = (d.plan, d.best, d.rename && !d.best);
        let mut placed = 0;
        self.edit(
            if best {
                "Choose sites"
            } else {
                "Sites from points"
            },
            |p| {
                let rev = p.bump();
                if let Some(pl) = p.plan_mut(plan_id) {
                    let picked = if best {
                        pl.best_sites(&geometry, &coords, weights.as_deref())
                    } else {
                        pl.sites_from(&geometry, &coords, weights.as_deref())
                    };
                    for (t, m) in pl.territories.iter_mut().zip(&picked) {
                        if let Some(j) = m {
                            placed += 1;
                            if rename {
                                t.name = names[*j].clone();
                            }
                        }
                    }
                    pl.rev = rev;
                }
            },
        );
        self.toast(if best {
            format!("{placed} sites chosen. Balance → Start over draws territories around them.")
        } else {
            format!("{placed} territories got a site.")
        });
    }

    /// A new layer of hexagons; coloured by the count when points were
    /// counted.
    pub(crate) fn create_hex_grid(&mut self, spec: &territoria_model::grid::HexSpec, name: &str) {
        let (geometries, table) = match territoria_model::grid::hex_grid(&self.project, spec) {
            Ok(g) => g,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        let before = self.project.clone();
        let geometry = territoria_model::LayerGeometry::from_imported(geometries, true);
        let id = self
            .project
            .add_layer(name, geometry, table, SourceInfo::default());
        if let Some(l) = self.project.layer_mut(id) {
            l.key_column = Some("Cell".into());
        }
        let map = self.ensure_map();
        let count = spec
            .count
            .and_then(|p| self.project.layer(p))
            .map(|p| p.name.clone())
            .and_then(|c| {
                let column = self.project.layer(id)?.attributes.column(&c)?.name.clone();
                Some(column)
            });
        let shown = match count {
            Some(column) => self.add_choropleth(
                map,
                id,
                format!("{column} per cell"),
                territoria_model::ValueSource::Attribute { column },
            ),
            None => self.add_boundaries_to_map(map, id),
        };
        self.history.push("New hexagon grid", before);
        if let Some(ml) = shown {
            self.selection = Selection::MapLayer(map, ml);
            self.right_tab = RightTab::Properties;
        }
    }

    /// The current map, creating one if the project has none.
    fn ensure_map(&mut self) -> MapId {
        if let Some(m) = self.current_map.filter(|m| self.project.map(*m).is_some()) {
            return m;
        }
        if let Some(m) = self.project.maps.first().map(|m| m.id) {
            self.current_map = Some(m);
            return m;
        }
        let id = self.project.add_map("Map");
        // The background chosen in the settings.
        if let Some(preset) = self
            .settings
            .background
            .as_deref()
            .and_then(|name| crate::tiles::PRESETS.iter().find(|p| p.name == name))
            && let Some(m) = self.project.map_mut(id)
        {
            m.background = territoria_model::map::Background::Tiles {
                url: preset.url.to_owned(),
                attribution: preset.attribution.to_owned(),
            };
        }
        self.dock.push_to_focused_leaf(Tab::Map(id));
        self.current_map = Some(id);
        id
    }

    fn add_boundaries_to_map(&mut self, map: MapId, layer: LayerId) -> Option<MapLayerId> {
        let l = self.project.layer(layer)?;
        let name = l.name.clone();
        let style = match l.kind() {
            GeometryKind::Area => BoundaryStyle::default(),
            GeometryKind::Line => BoundaryStyle {
                fill: territoria_analysis::Rgba::rgb(0x3b, 0x6e, 0xa8),
                stroke_width: 1.2,
                ..Default::default()
            },
            GeometryKind::Point => BoundaryStyle {
                fill: territoria_analysis::Rgba::rgb(0xd9, 0x4f, 0x30),
                stroke: territoria_analysis::Rgba::WHITE,
                stroke_width: 1.0,
                ..Default::default()
            },
        };
        let id = self.project.new_map_layer_id();
        self.push_map_layer(
            map,
            MapLayer {
                id,
                name,
                visible: true,
                opacity: 1.0,
                zoom: Default::default(),
                kind: MapLayerKind::Boundaries { layer, style },
            },
        )
    }

    /// Adds a map layer: areas go below point and line layers, which stay
    /// on top.
    fn push_map_layer(&mut self, map: MapId, layer: MapLayer) -> Option<MapLayerId> {
        let project = &self.project;
        let overlay = |kind: &MapLayerKind| {
            if matches!(
                kind,
                MapLayerKind::Symbols { .. }
                    | MapLayerKind::Rings { .. }
                    | MapLayerKind::Charts { .. }
                    | MapLayerKind::Flows { .. }
            ) {
                return true;
            }
            let source = match kind {
                MapLayerKind::Territories { plan, .. } => project.plan(*plan).map(|p| p.layer),
                k => k.layer(),
            };
            source
                .and_then(|l| project.layer(l))
                .is_some_and(|l| l.kind() != GeometryKind::Area)
        };
        let new_is_overlay = overlay(&layer.kind);
        let first_overlay = project
            .map(map)?
            .layers
            .iter()
            .position(|l| overlay(&l.kind));
        let id = layer.id;
        let m = self.project.map_mut(map)?;
        match first_overlay {
            Some(i) if !new_is_overlay => m.layers.insert(i, layer),
            _ => m.layers.push(layer),
        }
        Some(id)
    }

    pub(crate) fn add_table(
        &mut self,
        name: String,
        table: territoria_data::Table,
        path: PathBuf,
        placement: crate::dialogs::Placement,
        crs: Option<territoria_geo::Crs>,
    ) {
        use crate::dialogs::Placement;
        if let Placement::LatLon(column) = &placement {
            // Into two number columns, then as points.
            let mut table = table;
            let parsed: Vec<Option<(f64, f64)>> = match table.column(column) {
                Some(c) => (0..c.len())
                    .map(|r| territoria_data::coords::parse_lat_lon(&c.display(r)))
                    .collect(),
                None => {
                    self.error("Choose the column with the coordinates.");
                    return;
                }
            };
            let (lat, lon) = (
                table.unique_name("Latitude"),
                table.unique_name("Longitude"),
            );
            let _ = table.push_column(territoria_data::Column::number(
                lat.clone(),
                parsed.iter().map(|p| p.map(|p| p.0)).collect(),
            ));
            let _ = table.push_column(territoria_data::Column::number(
                lon.clone(),
                parsed.iter().map(|p| p.map(|p| p.1)).collect(),
            ));
            self.add_table(
                name,
                table,
                path,
                Placement::Points { x: lon, y: lat },
                None,
            );
            return;
        }
        if let Placement::Address(columns) = &placement {
            let addresses = columns.addresses(&table);
            let settings = self.settings.geocoder.clone();
            let dir = self.cache_dir().with_file_name("geocoding");
            let n = addresses.iter().filter(|a| !a.is_empty()).count();
            let label = format!(
                "Finding {} addresses",
                territoria_analysis::format::format_value(n as f64)
            );
            self.jobs.spawn_cancellable(label, move |job| {
                let result = territoria_io::geocode::geocode(
                    &settings,
                    &addresses,
                    &dir,
                    &mut |done, total| {
                        if total > 0 {
                            job.progress(done as f32 / total as f32);
                        }
                        !job.cancelled()
                    },
                )
                .map_err(|e| e.to_string());
                JobResult::Geocoded {
                    name,
                    table,
                    path,
                    result,
                }
            });
            return;
        }
        let before = self.project.clone();
        if let Placement::Points { x, y } = &placement {
            let (Some(xi), Some(yi)) = (table.column_index(x), table.column_index(y)) else {
                self.error("Choose the coordinate columns.");
                return;
            };
            let (geometry, missing) =
                match territoria_model::points_from_table(&table, xi, yi, crs.as_ref()) {
                    Ok(g) => g,
                    Err(e) => {
                        self.error(format!("Couldn't convert the coordinates: {e}"));
                        return;
                    }
                };
            let source = SourceInfo {
                path: Some(path),
                crs_note: crs
                    .as_ref()
                    .map_or("WGS 84 longitude/latitude".into(), |c| c.label()),
                attribution: None,
            };
            let rows = table.rows();
            let id = self.project.add_layer(&name, geometry, table, source);
            let map = self.ensure_map();
            self.add_boundaries_to_map(map, id);
            self.history.push("Add points", before);
            self.selection = Selection::Data(DataRef::Layer(id));
            if let Some(s) = self.map_states.get_mut(&map) {
                s.fit_requested = self.project.layers.len() == 1;
            }
            if missing > 0 {
                self.toast(format!(
                    "{missing} of {rows} rows have no usable coordinates and aren't on the map."
                ));
            }
            return;
        }
        let source = SourceInfo {
            path: Some(path),
            ..Default::default()
        };
        let id = self.project.add_table(&name, table, source);
        self.history.push("Add data", before);
        self.selection = Selection::Data(DataRef::Table(id));
        if placement == Placement::ByKey {
            if !self.project.layers.is_empty() {
                self.link = Some(LinkDialog::new(&self.project, id));
            } else {
                self.toast(format!("Added {name}. Add boundaries to put it on a map."));
            }
        }
    }

    fn apply_link(&mut self, outcome: LinkOutcome) {
        let LinkOutcome::Link {
            table,
            layer,
            column,
            mode,
            color_by,
        } = outcome;
        let before = self.project.clone();
        let link = self.project.add_link(table, &column, layer, mode);
        let mut selected = None;
        if let Some((value, agg)) = color_by {
            let map = self.ensure_map();
            let table_name = self
                .project
                .table(table)
                .map(|t| t.name.clone())
                .unwrap_or_default();
            let source = territoria_model::ValueSource::Linked {
                link,
                column: value.clone(),
                agg,
            };
            selected = self.add_choropleth(map, layer, format!("{value} ({table_name})"), source);
        }
        self.history.push("Link table", before);
        if let (Some(id), Some(map)) = (selected, self.current_map) {
            self.selection = Selection::MapLayer(map, id);
            self.right_tab = RightTab::Properties;
        }
    }

    fn add_choropleth(
        &mut self,
        map: MapId,
        layer: LayerId,
        name: String,
        source: territoria_model::ValueSource,
    ) -> Option<MapLayerId> {
        let id = self.project.new_map_layer_id();
        // Sums of things grow with area size; the ramp default is fine for both.
        let kind = MapLayerKind::Choropleth {
            layer,
            source,
            normalize: Default::default(),
            scale: ColorScale::default(),
            style: BoundaryStyle {
                stroke: territoria_analysis::Rgba([0xff, 0xff, 0xff, 140]),
                stroke_width: 0.5,
                ..Default::default()
            },
        };
        self.push_map_layer(
            map,
            MapLayer {
                id,
                name,
                visible: true,
                opacity: 1.0,
                zoom: Default::default(),
                kind,
            },
        )?;
        Some(id)
    }

    /// A flow layer from the dialog's choice, on top of the map.
    pub(crate) fn add_flows(&mut self, d: &crate::dialogs::FlowDialog) {
        let Some(rows) = d.rows else {
            return;
        };
        let before = self.project.clone();
        let map = self.ensure_map();
        let id = self.project.new_map_layer_id();
        let what = d.value.clone().unwrap_or_else(|| rows.name(&self.project));
        let added = self.push_map_layer(
            map,
            MapLayer {
                id,
                name: format!("Flows of {what}"),
                visible: true,
                opacity: 1.0,
                zoom: Default::default(),
                kind: MapLayerKind::Flows {
                    rows,
                    from: d.from.clone(),
                    to: d.to.clone(),
                    value: d.value.clone(),
                    style: Default::default(),
                },
            },
        );
        self.history.push("Add flow lines", before);
        if let Some(id) = added {
            self.selection = Selection::MapLayer(map, id);
            self.right_tab = RightTab::Properties;
        }
    }

    /// Areas of `layer` coloured by two values at once.
    pub(crate) fn add_bivariate(
        &mut self,
        layer: LayerId,
        x: territoria_model::ValueSource,
        y: territoria_model::ValueSource,
        name: String,
    ) {
        let before = self.project.clone();
        let map = self.ensure_map();
        let id = self.project.new_map_layer_id();
        let added = self.push_map_layer(
            map,
            MapLayer {
                id,
                name,
                visible: true,
                opacity: 1.0,
                zoom: Default::default(),
                kind: MapLayerKind::Bivariate {
                    layer,
                    x,
                    y,
                    scale: Default::default(),
                    style: BoundaryStyle {
                        stroke: territoria_analysis::Rgba([0xff, 0xff, 0xff, 140]),
                        stroke_width: 0.5,
                        ..Default::default()
                    },
                },
            },
        );
        self.history.push("Colour by two values", before);
        if let Some(id) = added {
            self.selection = Selection::MapLayer(map, id);
            self.right_tab = RightTab::Properties;
        }
    }

    /// A column on the map the way that suits it: numbers as colours (or
    /// circles on points), texts as categories.
    pub(crate) fn show_column(&mut self, data: DataRef, column: &str) {
        use crate::dialogs::Colorable;
        use territoria_model::{TextSource, ValueSource};
        let found = match data {
            DataRef::Layer(l) => crate::dialogs::colorables(&self.project, l)
                .into_iter()
                .find(|(_, c)| match c {
                    Colorable::Number(ValueSource::Attribute { column: c })
                    | Colorable::Category(TextSource::Attribute { column: c }) => c == column,
                    _ => false,
                })
                .map(|o| (l, o)),
            DataRef::Table(t) => self
                .project
                .links
                .iter()
                .filter(|k| k.table == t)
                .find_map(|k| {
                    crate::dialogs::colorables(&self.project, k.layer)
                        .into_iter()
                        .find(|(_, c)| match c {
                            Colorable::Number(ValueSource::Linked {
                                link, column: c, ..
                            })
                            | Colorable::Category(TextSource::Linked { link, column: c }) => {
                                *link == k.id && c == column
                            }
                            _ => false,
                        })
                        .map(|o| (k.layer, o))
                }),
        };
        let Some((layer, (name, what))) = found else {
            self.toast(match data {
                DataRef::Table(_) => {
                    format!("Link the table to boundaries first to show {column} on the map.")
                }
                DataRef::Layer(_) => format!("{column} can't be shown on the map."),
            });
            return;
        };
        let points = self
            .project
            .layer(layer)
            .is_some_and(|l| l.kind() == GeometryKind::Point);
        self.apply_color_by(ColorByOutcome::Create {
            layer,
            name,
            what,
            circles: points,
            heat: false,
        });
        // The map in front, to see it.
        if let Some(map) = self.current_map
            && let Some(found) = self.dock.find_tab(&Tab::Map(map))
        {
            let _ = self.dock.set_active_tab(found);
        }
    }

    /// A chart layer on `layer` of several values, each a slice or bar.
    pub(crate) fn add_charts(
        &mut self,
        layer: LayerId,
        shape: territoria_model::ChartShape,
        values: Vec<(String, territoria_model::ValueSource)>,
    ) {
        let before = self.project.clone();
        let map = self.ensure_map();
        let n = self.project.layer(layer).map_or(0, |l| l.len());
        let parts: Vec<territoria_model::ChartPart> = values
            .into_iter()
            .enumerate()
            .map(|(i, (label, source))| territoria_model::ChartPart {
                label,
                source,
                color: territoria_model::ChartPart::color_for(i),
            })
            .collect();
        let name = match parts.as_slice() {
            [one] => one.label.clone(),
            [a, b] => format!("{} and {}", a.label, b.label),
            [a, rest @ ..] => format!("{} and {} more", a.label, rest.len()),
            [] => return,
        };
        let id = self.project.new_map_layer_id();
        let added = self.push_map_layer(
            map,
            MapLayer {
                id,
                name,
                visible: true,
                opacity: 1.0,
                zoom: Default::default(),
                kind: MapLayerKind::Charts {
                    layer,
                    parts,
                    chart: territoria_model::ChartStyle::for_count(shape, n),
                    style: BoundaryStyle::default(),
                },
            },
        );
        self.history.push("Add charts", before);
        if let Some(id) = added {
            self.selection = Selection::MapLayer(map, id);
            self.right_tab = RightTab::Properties;
        }
    }

    /// The map's view on screen: its scale and metres per point.
    fn view_scale(&self, map: MapId) -> Option<(f64, f64)> {
        let view = self.map_states.get(&map)?.view?;
        let crs = &self.project.map(map)?.crs;
        Some((view.scale, crate::map_view::metres_per_point(&view, crs)?))
    }

    /// New plans take the colours chosen in Settings.
    fn apply_default_palette(&mut self, plan: PlanId) {
        let Some(name) = self.settings.territory_colors.clone() else {
            return;
        };
        if let Some((_, colors)) = crate::territories::palettes()
            .into_iter()
            .find(|(n, _)| *n == name)
            && let Some(p) = self.project.plan_mut(plan)
        {
            p.set_palette(colors);
        }
    }

    fn apply_color_by(&mut self, outcome: ColorByOutcome) {
        let (layer, name, what, circles, heat) = match outcome {
            ColorByOutcome::Create {
                layer,
                name,
                what,
                circles,
                heat,
            } => (layer, name, what, circles, heat),
            ColorByOutcome::Chart {
                layer,
                shape,
                values,
            } => return self.add_charts(layer, shape, values),
            ColorByOutcome::Bivariate { layer, x, y, name } => {
                return self.add_bivariate(layer, x, y, name);
            }
        };
        let before = self.project.clone();
        let map = self.ensure_map();
        let id = match what {
            Colorable::Number(source) if heat => {
                let id = self.project.new_map_layer_id();
                let weight = match source {
                    territoria_model::ValueSource::Attribute { column } => Some(column),
                    _ => None,
                };
                // Each point reaches about a 40th of the data's extent.
                let radius_km = self
                    .project
                    .layer(layer)
                    .and_then(|l| l.geometry.bbox)
                    .map_or(5.0, |b| {
                        (territoria_geo::haversine(b.min(), b.max()) / 1000.0 / 40.0).max(0.1)
                    });
                self.push_map_layer(
                    map,
                    MapLayer {
                        id,
                        name: format!("Density of {name}"),
                        visible: true,
                        opacity: 0.85,
                        zoom: Default::default(),
                        kind: MapLayerKind::Heatmap {
                            layer,
                            weight,
                            radius_km: (radius_km * 10.0).round() / 10.0,
                            ramp: crate::heat::default_ramp(),
                        },
                    },
                )
            }
            Colorable::Number(source) if circles => {
                let id = self.project.new_map_layer_id();
                let n = self.project.layer(layer).map_or(0, |l| l.len());
                self.push_map_layer(
                    map,
                    MapLayer {
                        id,
                        name,
                        visible: true,
                        opacity: 1.0,
                        zoom: Default::default(),
                        kind: MapLayerKind::Symbols {
                            layer,
                            source,
                            normalize: Default::default(),
                            symbol: territoria_model::SymbolStyle::for_count(n),
                            style: BoundaryStyle::default(),
                        },
                    },
                )
            }
            Colorable::Number(source) => self.add_choropleth(map, layer, name, source),
            Colorable::Category(source) => {
                let id = self.project.new_map_layer_id();
                let kind = MapLayerKind::Categories {
                    layer,
                    source,
                    scale: CategoryScale::default(),
                    style: BoundaryStyle {
                        stroke: territoria_analysis::Rgba([0xff, 0xff, 0xff, 140]),
                        stroke_width: 0.5,
                        ..Default::default()
                    },
                };
                self.push_map_layer(
                    map,
                    MapLayer {
                        id,
                        name,
                        visible: true,
                        opacity: 1.0,
                        zoom: Default::default(),
                        kind,
                    },
                )
            }
        };
        self.history.push("Colour by value", before);
        if let Some(id) = id {
            self.selection = Selection::MapLayer(map, id);
            self.right_tab = RightTab::Properties;
        }
    }

    fn apply_new_plan(&mut self, outcome: NewPlanOutcome) {
        let before = self.project.clone();
        let plan = match outcome {
            NewPlanOutcome::Empty { name, layer, count } => {
                let plan = self.project.add_plan(&name, layer, count);
                self.apply_default_palette(plan);
                plan
            }
            NewPlanOutcome::FromTable {
                name,
                layer,
                import,
            } => self.project.add_plan_from_assignment(&name, layer, &import),
        };
        self.show_plan(plan);
        self.history.push("New territory plan", before);
        if self
            .project
            .plan(plan)
            .is_some_and(|p| p.assignment.iter().all(Option::is_none))
        {
            self.tool = Tool::Paint;
            self.toast("Paint territories: drag over areas. Shift+click picks a territory, Alt+drag erases.");
        }
    }

    /// Puts a plan on the current map (once) and makes it the active one.
    /// Changes the selection.
    fn select(&mut self, layer: LayerId, units: Vec<u32>, mode: crate::map_view::SelectMode) {
        use crate::map_view::SelectMode;
        let mut set: std::collections::BTreeSet<u32> = match &self.selected {
            Some((l, u)) if *l == layer && mode != SelectMode::Replace => {
                u.iter().copied().collect()
            }
            _ => Default::default(),
        };
        for u in units {
            if mode == SelectMode::Toggle && set.contains(&u) {
                set.remove(&u);
            } else {
                set.insert(u);
            }
        }
        self.selected = (!set.is_empty()).then(|| (layer, set.into_iter().collect()));
    }

    /// Adds or updates a calculated column.
    fn apply_formula(&mut self, data: DataRef, editing: Option<String>, name: &str, source: &str) {
        let label = if editing.is_some() {
            format!("Edit formula of {name}")
        } else {
            format!("Add column {name}")
        };
        let mut result = Ok(Vec::new());
        self.edit(label, |p| {
            let Some(t) = data_table_mut(p, data) else {
                return;
            };
            result = territoria_expr::Formula::compile(source, t).map(|f| {
                let column = f.column(name, source, t);
                match editing.as_deref().and_then(|c| t.column_index(c)) {
                    Some(i) => t.columns[i] = column,
                    None => {
                        let _ = t.push_column(column);
                    }
                }
                territoria_expr::recalculate(t)
            });
        });
        match result {
            Ok(broken) => self.report_broken(&broken),
            Err(e) => self.error(e.message),
        }
    }

    fn report_broken(&mut self, broken: &[String]) {
        if !broken.is_empty() {
            self.error(format!(
                "These calculated columns no longer work and keep their last values: {}. Edit their formulas to fix them.",
                broken.join(", ")
            ));
        }
    }

    /// What a territory report of `plan` shows, from the project and caches.
    fn report_data(&mut self, plan: PlanId) -> Result<crate::report::ReportData, String> {
        let p = self
            .project
            .plan(plan)
            .ok_or("The plan was removed.")?
            .clone();
        let layer = self
            .project
            .layer(p.layer)
            .ok_or("The plan's areas were removed.")?;
        let crs = self
            .current_map
            .and_then(|m| self.project.map(m))
            .or(self.project.maps.first())
            .map_or_else(territoria_geo::Crs::web_mercator, |m| m.crs.clone());
        let mesh = if layer.kind() == GeometryKind::Point {
            self.meshes.get_cells(layer, &crs, &mut self.jobs)
        } else {
            self.meshes.get(layer, &crs, &mut self.jobs)
        }
        .ok_or("The map is still being prepared; try again in a moment.")?;
        let stats = self
            .stats
            .get(&self.project, &mut self.values, plan)
            .ok_or("The plan was removed.")?;
        let measure = p
            .measures
            .first()
            .filter(|m| m.source != territoria_model::ValueSource::Units)
            .map(|m| {
                (
                    m.source.label(&self.project),
                    self.values.values(&self.project, p.layer, &m.source),
                )
            });
        let label_units = self
            .territory_cache
            .get(&self.project, plan)
            .map(|t| t.label_units.clone())
            .unwrap_or_default();
        let names = (0..layer.len()).map(|f| layer.display_name(f)).collect();
        let company = self.settings.company.trim();
        let credits = (!company.is_empty())
            .then(|| company.to_owned())
            .into_iter()
            .chain(layer.source.attribution.iter().cloned())
            .collect();
        let today = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| (d.as_secs() / 86_400) as i32);
        Ok(crate::report::ReportData {
            title: p.name.clone(),
            tolerance: p.measures.first().map_or(0.15, |m| m.tolerance),
            plan: p,
            mesh,
            stats,
            measure,
            label_units,
            names,
            credits,
            date: territoria_data::date::format(today),
            logo: self
                .settings
                .logo
                .as_deref()
                .and_then(|p| crate::export_map::Logo::load(p, 120.0, 36.0)),
            crs,
        })
    }

    /// The groups of an atlas: per value of `column` (each feature without
    /// one), its name and features, by name.
    pub(crate) fn atlas_groups(
        &self,
        layer: LayerId,
        column: Option<&str>,
    ) -> Vec<(String, Vec<usize>)> {
        let Some(l) = self.project.layer(layer) else {
            return Vec::new();
        };
        let mut groups: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
        let c = column.and_then(|c| l.attributes.column(c));
        for f in 0..l.len() {
            let name = match c {
                Some(c) => match c.get(f).to_text() {
                    Some(t) if !t.trim().is_empty() => t,
                    _ => continue,
                },
                None => l.display_name(f),
            };
            groups.entry(name).or_default().push(f);
        }
        groups.into_iter().collect()
    }

    /// The current map's pages for an atlas, as SVG (A4 landscape).
    fn atlas_pages(&mut self, layer: LayerId, column: Option<&str>) -> Result<Vec<String>, String> {
        const MAX_PAGES: usize = 300;
        let groups = self.atlas_groups(layer, column);
        if groups.is_empty() {
            return Err("Nothing to make pages of.".into());
        }
        if groups.len() > MAX_PAGES {
            return Err(format!(
                "{} pages are too many; choose a column with at most {MAX_PAGES} values.",
                groups.len()
            ));
        }
        let map = self
            .current_map
            .and_then(|m| self.project.map(m))
            .ok_or("There's no map to export.")?;
        let l = self.project.layer(layer).ok_or("The layer was removed.")?;
        let mesh = self
            .meshes
            .get(l, &map.crs, &mut self.jobs)
            .ok_or("The map is still being prepared; try again in a moment.")?;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(842.0, 595.0));
        let mut pages = Vec::new();
        for (name, members) in groups {
            let mut extent = territoria_render::Bounds::EMPTY;
            for &f in &members {
                if let Some(b) = mesh.bounds.get(f).filter(|b| !b.is_empty()) {
                    let (a, c) = (mesh.to_display(b.min), mesh.to_display(b.max));
                    extent.add([a[0] as f32, a[1] as f32]);
                    extent.add([c[0] as f32, c[1] as f32]);
                }
            }
            if extent.is_empty() {
                continue;
            }
            let mut view = territoria_render::View::fit(
                [extent.min[0] as f64, extent.min[1] as f64],
                [extent.max[0] as f64, extent.max[1] as f64],
                rect,
            );
            // A margin around the areas, for their neighbours.
            view.scale *= 1.15;
            let page = AtlasPage {
                view,
                veil: Some((layer, members.into_iter().collect())),
            };
            // Bare numbers say little alone: "Region 02".
            let title = match column {
                Some(c) if name.chars().all(|ch| ch.is_ascii_digit()) => format!("{c} {name}"),
                _ => name,
            };
            let settings = crate::dialogs::MapExportSettings {
                format: crate::export_map::Format::Pdf,
                scale: 1.0,
                title,
                legend: true,
                scale_bar: true,
                page: Default::default(),
                footer: String::new(),
            };
            pages.push(self.map_render(&settings, false, Some(&page))?);
        }
        Ok(pages)
    }

    /// Copies a plan as a scenario, shows the copy and compares it with
    /// the original.
    fn duplicate_plan(&mut self, plan: PlanId) {
        let Some(name) = self.project.plan(plan).map(|p| p.name.clone()) else {
            return;
        };
        let before = self.project.clone();
        let names: Vec<String> = self.project.plans.iter().map(|p| p.name.clone()).collect();
        let name = (1..)
            .map(|i| {
                if i == 1 {
                    format!("{name} – scenario")
                } else {
                    format!("{name} – scenario {i}")
                }
            })
            .find(|n| !names.contains(n))
            .unwrap_or_default();
        let Some(copy) = self.project.duplicate_plan(plan, &name) else {
            return;
        };
        self.show_plan(copy);
        self.history.push("Duplicate plan", before);
        self.territories_panel.compare_with(plan);
        self.toast(format!(
            "“{name}” is a copy to try changes on. The Territories panel compares it with the original."
        ));
    }

    fn show_plan(&mut self, plan: PlanId) {
        let map = self.ensure_map();
        let on_map = self
            .project
            .map(map)
            .is_some_and(|m| m.layers.iter().any(|l| l.kind.plan() == Some(plan)));
        if !on_map {
            let id = self.project.new_map_layer_id();
            let name = self
                .project
                .plan(plan)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            self.push_map_layer(
                map,
                MapLayer {
                    id,
                    name,
                    visible: true,
                    opacity: 1.0,
                    zoom: Default::default(),
                    kind: MapLayerKind::Territories {
                        plan,
                        style: Default::default(),
                    },
                },
            );
        }
        // One plan at a time: territories of several plans on top of each
        // other can't be read.
        if let Some(m) = self.project.map_mut(map) {
            for ml in &mut m.layers {
                if let Some(p) = ml.kind.plan() {
                    ml.visible = p == plan;
                }
            }
        }
        self.active_plan = Some(plan);
        self.active_territory = self
            .project
            .plan(plan)
            .and_then(|p| p.territories.first().map(|t| t.id));
        self.right_tab = RightTab::Territories;
        self.show_inspector = true;
    }

    // -- Commands ----------------------------------------------------------------

    fn run_command(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::NewProject => {
                if self.history.dirty && !self.project.layers.is_empty() {
                    self.confirm = Some(Confirm {
                        title: "Unsaved changes".into(),
                        text: "Start a new project and discard the changes to this one?".into(),
                        action: ConfirmAction::Discard(Box::new(Command::NewProject)),
                    });
                } else {
                    self.set_project(Project::new(), None);
                }
            }
            Command::OpenProject => self.ask_file(FileAction::OpenProject),
            Command::SaveProject => self.save(),
            Command::SaveProjectAs => self.ask_file(FileAction::SaveProjectAs),
            Command::AddBoundaries => self.ask_file(FileAction::AddBoundaries),
            Command::AddData => self.ask_file(FileAction::AddData),
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::Undo => match self.history.undo(&mut self.project) {
                Some(label) => self.toast(format!("Undid: {label}")),
                None => self.toast("Nothing to undo"),
            },
            Command::Redo => match self.history.redo(&mut self.project) {
                Some(label) => self.toast(format!("Redid: {label}")),
                None => self.toast("Nothing to redo"),
            },
            Command::LinkData => match self.project.tables.last() {
                Some(t) => self.link = Some(LinkDialog::new(&self.project, t.id)),
                None => self.toast("Add a table first: File → Add Data."),
            },
            Command::ExportHtml => {
                if self.current_map.and_then(|m| self.project.map(m)).is_some() {
                    self.ask_file(FileAction::Export(Export::Html));
                } else {
                    self.toast("There's no map to export yet.");
                }
            }
            Command::CopyMap => {
                let settings = crate::dialogs::MapExportSettings {
                    format: crate::export_map::Format::Png,
                    scale: 2.0,
                    title: String::new(),
                    legend: true,
                    scale_bar: true,
                    page: Default::default(),
                    footer: String::new(),
                };
                match self.map_svg(&settings) {
                    Ok(svg) => {
                        let font = egui::FontDefinitions::default().font_data["Ubuntu-Light"]
                            .font
                            .to_vec();
                        self.jobs.spawn("Copying the map", move |_| {
                            JobResult::Copied(crate::export_map::rasterize(&svg, 2.0, &font))
                        });
                    }
                    Err(e) => self.error(e),
                }
            }
            Command::ExportMap => match self.current_map.and_then(|m| self.project.map(m)) {
                Some(m) => {
                    let title = if self.project.title == "Untitled" {
                        m.name.clone()
                    } else {
                        self.project.title.clone()
                    };
                    self.export_map = Some(crate::dialogs::ExportMapDialog::new(title));
                }
                None => self.toast("There's no map to export yet."),
            },
            Command::DownloadBoundaries => {
                self.download = Some(DownloadDialog::new());
                self.load_catalog();
            }
            Command::Find => self.search_focus = true,
            Command::Presentation => {
                self.presenting = !self.presenting;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.presenting));
                if self.presenting {
                    self.toast("Presentation: F11 or Esc to leave.");
                }
            }
            Command::SaveView => {
                if let Some(map) = self.current_map
                    && let Some(view) = self.map_states.get(&map).and_then(|s| s.view)
                {
                    let n = self.project.map(map).map_or(0, |m| m.views.len()) + 1;
                    self.edit("Keep view", |p| {
                        if let Some(m) = p.map_mut(map) {
                            m.views.push(territoria_model::map::NamedView {
                                name: format!("View {n}"),
                                center: view.center,
                                scale: view.scale,
                            });
                        }
                    });
                } else {
                    self.toast("Show a map first.");
                }
            }
            Command::Templates => {
                self.templates_dialog = Some(crate::dialogs::TemplatesDialog::new(
                    self.templates(),
                    &self.project.title,
                ));
            }
            Command::ExportAtlas => match self.current_map.and_then(|m| self.project.map(m)) {
                Some(map) => {
                    let d = crate::dialogs::AtlasDialog::new(&self.project, map);
                    if d.layer.is_some() {
                        self.atlas = Some(d);
                    } else {
                        self.toast("The map needs areas for an atlas.");
                    }
                }
                None => self.toast("There's no map yet."),
            },
            Command::FlowLines => {
                let d = crate::dialogs::FlowDialog::new(&self.project);
                if d.rows.is_none() {
                    self.toast("Add data first: points or a table whose rows name where they start and go.");
                } else {
                    self.flows = Some(d);
                }
            }
            Command::Catchment => {
                let d = crate::dialogs::CatchmentDialog::new(&self.project);
                if d.centres.is_none() {
                    self.toast("Add points (branches, reps) or place territory sites first.");
                } else {
                    self.catchment = Some(d);
                }
            }
            Command::HexGrid => {
                if self.project.layers.is_empty() {
                    self.toast("Add points or boundaries first.");
                } else {
                    let layer = match self.selection {
                        Selection::Data(DataRef::Layer(l)) => Some(l),
                        _ => None,
                    };
                    self.hex_grid = Some(crate::dialogs::HexGridDialog::new(&self.project, layer));
                }
            }
            Command::CalculatedColumn => {
                let data = match self.selection {
                    Selection::Data(d) => Some(d),
                    _ => None,
                }
                .or_else(|| self.project.tables.first().map(|t| DataRef::Table(t.id)))
                .or_else(|| self.project.layers.first().map(|l| DataRef::Layer(l.id)));
                match data {
                    Some(d) => {
                        self.formula =
                            Some(crate::dialogs::FormulaDialog::new(&self.project, d, None))
                    }
                    None => self.toast("Add data or boundaries first."),
                }
            }
            Command::ReplaceLayer => {
                let layer = match self.selection {
                    Selection::Data(DataRef::Layer(l)) => Some(l),
                    _ => None,
                }
                .or_else(|| {
                    self.project
                        .layers
                        .iter()
                        .find(|l| l.kind() == GeometryKind::Area)
                        .map(|l| l.id)
                });
                match layer {
                    Some(l) => self.ask_file(FileAction::ReplaceLayer(l)),
                    None => self.toast("Add boundaries first."),
                }
            }
            Command::MergeAreas => {
                let layer = match self.selection {
                    Selection::Data(DataRef::Layer(l)) => Some(l),
                    _ => None,
                };
                self.merge = Some(MergeDialog::new(&self.project, layer));
            }
            Command::ColorByValue => {
                let layer = match self.selection {
                    Selection::Data(DataRef::Layer(l)) => Some(l),
                    _ => self.project.layers.first().map(|l| l.id),
                };
                self.color_by = Some(ColorByDialog::new(layer));
            }
            Command::NewMap => {
                let before = self.project.clone();
                let n = self.project.maps.len() + 1;
                let id = self.project.add_map(&format!("Map {n}"));
                self.history.push("New map", before);
                self.dock.push_to_focused_leaf(Tab::Map(id));
                self.current_map = Some(id);
            }
            Command::ZoomToAll => {
                if let Some(s) = self.current_map.and_then(|m| self.map_states.get_mut(&m)) {
                    s.fit_requested = true;
                }
            }
            Command::ToolNavigate => self.tool = Tool::Navigate,
            Command::ToolSelect => self.tool = Tool::Select,
            Command::SelectByList => {
                let layer = self
                    .selected
                    .as_ref()
                    .map(|(l, _)| *l)
                    .or_else(|| {
                        self.active_plan
                            .and_then(|p| self.project.plan(p))
                            .map(|p| p.layer)
                    })
                    .or_else(|| self.project.layers.first().map(|l| l.id));
                match layer {
                    Some(layer) => {
                        self.select_list = Some(crate::dialogs::SelectListDialog {
                            layer: Some(layer),
                            text: String::new(),
                        })
                    }
                    None => self.toast("Add boundaries first."),
                }
            }
            Command::ToolPaint => {
                if self.active_plan.is_some() {
                    self.tool = Tool::Paint;
                } else {
                    self.toast("Create a territory plan first: Territories → New Territory Plan.");
                }
            }
            Command::NewPlan => {
                if self.project.layers.is_empty() {
                    self.toast("Add boundaries first: the areas territories are made of.");
                } else {
                    self.new_plan = Some(NewPlanDialog::new(&self.project));
                }
            }
            Command::NewTerritory => {
                if let Some(plan) = self.active_plan {
                    let mut new = None;
                    self.edit("New territory", |p| {
                        if let Some(pl) = p.plan_mut(plan) {
                            new = Some(pl.add_territory(None, None));
                        }
                        let rev = p.bump();
                        if let Some(pl) = p.plan_mut(plan) {
                            pl.rev = rev;
                        }
                    });
                    self.active_territory = new;
                    self.tool = Tool::Paint;
                    self.right_tab = RightTab::Territories;
                }
            }
            Command::FillGaps => {
                if let Some(plan) = self.active_plan {
                    let mut changed = 0;
                    self.edit("Assign unassigned areas", |p| {
                        let Some(layer) = p
                            .plan(plan)
                            .and_then(|pl| p.layer(pl.layer))
                            .map(|l| l.geometry.clone())
                        else {
                            return;
                        };
                        let rev = p.bump();
                        if let Some(pl) = p.plan_mut(plan) {
                            changed = pl.fill_unassigned(&layer);
                            pl.rev = rev;
                        }
                    });
                    self.toast(format!(
                        "Assigned {changed} areas to their neighbours' territories."
                    ));
                }
            }
            Command::Recolor => {
                if let Some(plan) = self.active_plan {
                    self.edit("Recolour territories", |p| {
                        let Some(layer) = p
                            .plan(plan)
                            .and_then(|pl| p.layer(pl.layer))
                            .map(|l| l.geometry.clone())
                        else {
                            return;
                        };
                        let rev = p.bump();
                        if let Some(pl) = p.plan_mut(plan) {
                            pl.recolor(&layer);
                            pl.rev = rev;
                        }
                    });
                }
            }
            Command::Balance => {
                if let Some(plan) = self.active_plan {
                    self.start_balance(plan);
                }
            }
            Command::ShowPalette => self.palette = Some((String::new(), 0)),
            Command::ToggleContents => self.show_contents = !self.show_contents,
            Command::ToggleInspector => self.show_inspector = !self.show_inspector,
            Command::ThemeDark | Command::ThemeLight | Command::ThemeSystem => {
                self.settings.theme = match command {
                    Command::ThemeDark => ThemeChoice::Dark,
                    Command::ThemeLight => ThemeChoice::Light,
                    _ => ThemeChoice::System,
                };
                self.apply_theme(ctx);
                self.save_settings();
            }
            Command::ZoomIn | Command::ZoomOut | Command::ZoomReset => {
                self.settings.ui_scale = match command {
                    Command::ZoomIn => (self.settings.ui_scale + 0.1).min(2.5),
                    Command::ZoomOut => (self.settings.ui_scale - 0.1).max(0.6),
                    _ => 1.0,
                };
                self.apply_theme(ctx);
                self.save_settings();
            }
            Command::About => self.about = true,
            Command::Help => {
                let topic = crate::help::topic_for(
                    self.selection,
                    &self.project,
                    self.right_tab == RightTab::Territories,
                );
                self.help = Some(crate::help::HelpWindow::new(topic));
            }
            Command::Settings => {
                self.settings_dialog =
                    Some(crate::dialogs::SettingsDialog::new(self.tiles.cache_dir()));
            }
        }
    }

    fn start_balance(&mut self, plan: PlanId) {
        let Some(input) = crate::territories::balance_input(
            &self.project,
            &mut self.values,
            plan,
            &self.territories_panel.settings,
        ) else {
            self.toast("The plan needs territories and areas to balance.");
            return;
        };
        let label = "Balancing territories".to_owned();
        self.jobs
            .spawn_cancellable(label, move |job| JobResult::Optimized {
                plan,
                result: crate::territories::run_balance(input, &mut |share| {
                    job.progress(share);
                    !job.cancelled()
                }),
            });
    }

    fn apply_action(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Command(c) => self.run_command(ctx, c),
            Action::Select(s) => {
                self.selection = s;
                self.right_tab = RightTab::Properties;
                if let Selection::MapLayer(map, id) = s
                    && let Some(plan) = self
                        .project
                        .map(map)
                        .and_then(|m| m.layer(id))
                        .and_then(|l| l.kind.plan())
                {
                    self.active_plan = Some(plan);
                    self.right_tab = RightTab::Territories;
                }
            }
            Action::OpenTab(tab) => {
                if let Some(found) = self.dock.find_tab(&tab) {
                    let _ = self.dock.set_active_tab(found);
                } else {
                    self.dock.push_to_focused_leaf(tab);
                }
            }
            Action::Edit(label, change) => {
                let before = self.project.clone();
                change(&mut self.project);
                self.history.push(label, before);
            }
            Action::LinkTable(t) => self.link = Some(LinkDialog::new(&self.project, t)),
            Action::ColorBy(layer) => self.color_by = Some(ColorByDialog::new(layer)),
            Action::ShowColumn(data, column) => self.show_column(data, &column),
            Action::SaveClassSet(name, scale) => {
                self.settings.class_sets.retain(|s| s.name != name);
                self.toast(format!("Kept the classes as {name}."));
                self.settings
                    .class_sets
                    .push(territoria_config::ClassSet { name, scale });
                self.save_settings();
            }
            Action::AddToMap(layer) => {
                let before = self.project.clone();
                let map = self.ensure_map();
                self.add_boundaries_to_map(map, layer);
                self.history.push("Add to map", before);
            }
            Action::ShowPlan(plan) => {
                let before = self.project.clone();
                self.show_plan(plan);
                self.history.push("Show territories", before);
            }
            Action::ActivateTerritory(t) => self.active_territory = t,
            Action::ZoomTo(layer, features) => {
                if let Some(s) = self.current_map.and_then(|m| self.map_states.get_mut(&m)) {
                    s.zoom_to = Some((layer, features));
                }
            }
            Action::RemoveLayer(id) => {
                let name = self
                    .project
                    .layer(id)
                    .map(|l| l.name.clone())
                    .unwrap_or_default();
                self.confirm = Some(Confirm {
                    title: format!("Remove {name}?"),
                    text: "Map layers and territory plans built on it are removed too. You can undo this.".into(),
                    action: ConfirmAction::RemoveLayer(id),
                });
            }
            Action::RemoveTable(id) => {
                let name = self
                    .project
                    .table(id)
                    .map(|t| t.name.clone())
                    .unwrap_or_default();
                self.confirm = Some(Confirm {
                    title: format!("Remove {name}?"),
                    text: "Map layers coloured by its values are removed too. You can undo this."
                        .into(),
                    action: ConfirmAction::RemoveTable(id),
                });
            }
            Action::SetTool(t) => self.tool = t,
            Action::OpenRecent(p) => self.open_path(&p),
            Action::Balance(plan) => self.start_balance(plan),
            Action::AcceptProposal(plan) => {
                if let Some((p, proposal)) = self.territories_panel.proposal.take()
                    && p == plan
                {
                    let moves = proposal.moves.len();
                    self.edit("Balance territories", |pr| {
                        let rev = pr.bump();
                        if let Some(pl) = pr.plan_mut(plan) {
                            for (unit, t) in &proposal.moves {
                                pl.assign(&[*unit], Some(*t));
                            }
                            pl.rev = rev;
                        }
                    });
                    self.toast(format!("Moved {moves} areas."));
                }
            }
            Action::DiscardProposal => self.territories_panel.proposal = None,
            Action::SelectUnits(layer, units) => {
                self.selected = Some((layer, units.clone()));
                if let Some(s) = self.current_map.and_then(|m| self.map_states.get_mut(&m)) {
                    s.zoom_to = Some((layer, units));
                }
            }
            Action::ReloadTable(id) => self.reload_table(id),
            Action::Join(data) => {
                self.join = Some(crate::dialogs::JoinDialog::new(&self.project, data))
            }
            Action::Score(data) => {
                self.score = Some(crate::dialogs::ScoreDialog::new(&self.project, data))
            }
            Action::Summarise(data) => {
                self.summarise = Some(crate::dialogs::SummariseDialog::new(&self.project, data))
            }
            Action::EditCell(data, row, column, text) => {
                let Some(col) = crate::dialogs::data_table(&self.project, data)
                    .and_then(|(t, _)| t.column(&column))
                    .cloned()
                else {
                    return;
                };
                if col.display(row) != text.trim() {
                    // Check on a copy first: only fitting text becomes a step.
                    if col.clone().set_text(row, &text) {
                        self.edit("Edit cell", |p| {
                            if let Some(t) = data_table_mut(p, data) {
                                if let Some(c) = t.columns.iter_mut().find(|c| c.name == column) {
                                    c.set_text(row, &text);
                                }
                                territoria_expr::recalculate(t);
                            }
                        });
                    } else {
                        self.error(format!(
                            "“{}” isn't a {} for {column}.",
                            text.trim(),
                            col.kind.label().to_lowercase()
                        ));
                    }
                }
            }
            Action::AddRow(data) => self.edit("Add row", |p| {
                if let Some(t) = data_table_mut(p, data) {
                    t.push_row();
                    territoria_expr::recalculate(t);
                }
            }),
            Action::DeleteRows(data, rows) => self.edit("Delete row", |p| {
                if let Some(t) = data_table_mut(p, data) {
                    t.remove_rows(&rows);
                    territoria_expr::recalculate(t);
                }
            }),
            Action::Notify(text) => self.toast(text),
            Action::SetMapCrs(map, crs) => {
                self.edit("Change projection", |p| {
                    if let Some(m) = p.map_mut(map) {
                        m.crs = crs;
                        // Views are in the old projection's units.
                        m.view = None;
                        m.views.clear();
                    }
                });
                if let Some(s) = self.map_states.get_mut(&map) {
                    s.view = None;
                    s.fit_requested = true;
                }
            }
            Action::GoToView(map, center, scale) => {
                self.current_map = Some(map);
                self.map_states.entry(map).or_default().go_to_view = Some((center, scale));
            }
            Action::SitesFromPoints(plan) => {
                self.sites = Some(crate::dialogs::SitesDialog::new(&self.project, plan));
            }
            Action::SelectRows(data, rows, add) => {
                let target = match data {
                    DataRef::Layer(l) => Some((l, rows.iter().map(|&r| r as u32).collect())),
                    DataRef::Table(t) => self
                        .project
                        .links
                        .iter()
                        .find(|l| l.table == t)
                        .map(|l| (l.id, l.layer))
                        .and_then(|(link, layer)| {
                            let m = self.values.matching(&self.project, link)?;
                            Some((
                                layer,
                                rows.iter()
                                    .filter_map(|&r| m.row_target.get(r).copied().flatten())
                                    .collect::<Vec<u32>>(),
                            ))
                        }),
                };
                match target {
                    Some((layer, units)) if !units.is_empty() => {
                        let mode = if add {
                            crate::map_view::SelectMode::Toggle
                        } else {
                            crate::map_view::SelectMode::Replace
                        };
                        self.select(layer, units, mode);
                    }
                    Some(_) => self.toast("This row isn't on the map: its key matched no area."),
                    None => self.toast("Link this table to areas to see its rows on the map."),
                }
            }
            Action::SaveSegment {
                data,
                name,
                formula,
            } => self.apply_formula(data, None, &name, &formula),
            Action::Formula(data, column) => {
                self.formula = Some(crate::dialogs::FormulaDialog::new(
                    &self.project,
                    data,
                    column,
                ));
            }
            Action::DeleteColumn(data, column) => {
                let mut broken = Vec::new();
                self.edit(format!("Delete column {column}"), |p| {
                    if let Some(t) = data_table_mut(p, data) {
                        t.columns.retain(|c| c.name != column);
                        broken = territoria_expr::recalculate(t);
                    }
                });
                self.report_broken(&broken);
            }
            Action::DuplicatePlan(plan) => self.duplicate_plan(plan),
            Action::RemovePlan(id) => {
                let name = self
                    .project
                    .plan(id)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                self.confirm = Some(Confirm {
                    title: format!("Delete {name}?"),
                    text: "The plan and its territories are removed from the project. You can undo this.".into(),
                    action: ConfirmAction::RemovePlan(id),
                });
            }
            Action::Export(e) => self.ask_file(FileAction::Export(e)),
        }
    }

    fn handle_map_events(&mut self, events: Vec<MapEvent>, map: MapId) {
        for e in events {
            match e {
                MapEvent::Paint { plan, units, to } => {
                    if self.stroke_before.is_none() {
                        self.stroke_before = Some(self.project.clone());
                        self.stroke_changes = 0;
                    }
                    let rev = self.project.bump();
                    if let Some(p) = self.project.plan_mut(plan) {
                        let n = p.assign(&units, to);
                        if n > 0 {
                            p.rev = rev;
                            self.stroke_changes += n;
                        }
                    }
                }
                MapEvent::PaintEnd => {
                    if let Some(before) = self.stroke_before.take()
                        && self.stroke_changes > 0
                    {
                        let name = self
                            .active_plan
                            .and_then(|p| self.project.plan(p))
                            .and_then(|p| self.active_territory.and_then(|t| p.territory(t)))
                            .map_or("territory".into(), |t| t.name.clone());
                        self.history.seal();
                        self.history.push(
                            format!("Assign {} areas to {name}", self.stroke_changes),
                            before,
                        );
                        self.history.seal();
                    }
                }
                MapEvent::PlaceSite { plan, lonlat } => {
                    if let Some(t) = self.active_territory {
                        let name = self
                            .project
                            .plan(plan)
                            .and_then(|p| p.territory(t))
                            .map(|t| t.name.clone())
                            .unwrap_or_default();
                        self.edit(format!("Place site of {name}"), |p| {
                            let rev = p.bump();
                            if let Some(pl) = p.plan_mut(plan) {
                                if let Some(t) = pl.territory_mut(t) {
                                    t.site = Some(lonlat);
                                }
                                pl.rev = rev;
                            }
                        });
                        self.tool = Tool::Navigate;
                    }
                }
                MapEvent::PickTerritory { plan, territory }
                | MapEvent::ClickTerritory { plan, territory } => {
                    self.active_plan = Some(plan);
                    if territory.is_some() {
                        self.active_territory = territory;
                        self.right_tab = RightTab::Territories;
                    }
                }
                MapEvent::Select { layer, units, mode } => self.select(layer, units, mode),
                MapEvent::AssignSelection { plan, to } => {
                    if let Some((_, units)) = self.selected.clone() {
                        let label = if to.is_some() {
                            "Assign selection"
                        } else {
                            "Unassign selection"
                        };
                        let mut changed = 0;
                        self.edit(label, |p| {
                            let rev = p.bump();
                            if let Some(pl) = p.plan_mut(plan) {
                                changed = pl.assign(&units, to);
                                pl.rev = rev;
                            }
                        });
                        if changed < units.len() {
                            self.toast(format!(
                                "{} of {} areas changed; the others are locked or already there.",
                                changed,
                                units.len()
                            ));
                        }
                    }
                }
                MapEvent::ZoomToSelection => {
                    if let Some((layer, units)) = self.selected.clone()
                        && let Some(s) = self.map_states.get_mut(&map)
                    {
                        s.zoom_to = Some((layer, units));
                    }
                }
                MapEvent::ClearSelection => {
                    self.selected = None;
                    self.focus = false;
                }
                MapEvent::ShowColumn(data, column) => self.show_column(data, &column),
                MapEvent::Round => {
                    if let Some((layer, units)) = self.selected.clone() {
                        self.plan_round(layer, units);
                    }
                }
                MapEvent::TransferSelection { to, relation } => {
                    if let Some((from, units)) = self.selected.clone()
                        && let (Some(a), Some(b)) =
                            (self.project.layer(from), self.project.layer(to))
                    {
                        let found = territoria_model::transfer::transfer(a, &units, b, relation);
                        if found.is_empty() {
                            self.toast(format!("Nothing in {} is {}.", b.name, relation.label()));
                        } else {
                            self.toast(format!(
                                "{} of {} selected",
                                territoria_analysis::format::format_value(found.len() as f64),
                                b.name
                            ));
                            self.selected = Some((to, found));
                        }
                    }
                }
                MapEvent::Focus(on) => {
                    self.focus = on;
                    if on
                        && let Some((layer, units)) = self.selected.clone()
                        && let Some(s) = self.map_states.get_mut(&map)
                    {
                        s.zoom_to = Some((layer, units));
                    }
                }
                MapEvent::ViewChanged(center, scale) => {
                    // Not an undo step: just remembered with the project.
                    if let Some(m) = self.project.map_mut(map) {
                        m.view = Some((center, scale));
                    }
                }
            }
        }
    }

    // -- UI ------------------------------------------------------------------------

    fn menu_bar(&mut self, ui: &mut Ui, commands: &mut Vec<Command>) {
        let ctx = ui.ctx().clone();
        let mut exports = Vec::new();
        egui::MenuBar::new().ui(ui, |ui| {
            let item = |ui: &mut Ui, commands: &mut Vec<Command>, keymap: &Keymap, c: Command| {
                let shortcut = keymap.shortcut_text(&ctx, c);
                if ui
                    .add(egui::Button::new(c.short_label()).shortcut_text(shortcut))
                    .clicked()
                {
                    commands.push(c);
                }
            };
            ui.menu_button("File", |ui| {
                for c in [Command::NewProject, Command::OpenProject] {
                    item(ui, commands, &self.keymap, c);
                }
                ui.menu_button("Open Recent", |ui| {
                    if self.settings.recent_projects.is_empty() {
                        ui.label(RichText::new("No recent projects").weak());
                    }
                    for p in self.settings.recent_projects.clone() {
                        if ui
                            .button(file_name(&p))
                            .on_hover_text(p.display().to_string())
                            .clicked()
                        {
                            self.open_path(&p);
                        }
                    }
                });
                ui.separator();
                for c in [Command::SaveProject, Command::SaveProjectAs, Command::Templates] {
                    item(ui, commands, &self.keymap, c);
                }
                ui.separator();
                for c in [
                    Command::AddBoundaries,
                    Command::DownloadBoundaries,
                    Command::AddData,
                ] {
                    item(ui, commands, &self.keymap, c);
                }
                ui.menu_button("Export", |ui| {
                    if self.project.plans.is_empty() && self.project.layers.is_empty() {
                        ui.label(RichText::new("Nothing to export yet").weak());
                    }
                    for p in &self.project.plans {
                        ui.label(RichText::new(&p.name).strong());
                        if ui
                            .button("Territories and assignment (Excel)…")
                            .on_hover_text("Also has a sheet for each linked table and point layer, with each row's territory")
                            .clicked()
                        {
                            exports.push(Export::PlanExcel(p.id));
                        }
                        if ui.button("Assignment (CSV)…").clicked() {
                            exports.push(Export::PlanCsv(p.id));
                        }
                        if ui.button("Territory outlines (GeoJSON)…").clicked() {
                            exports.push(Export::PlanGeoJson(p.id));
                        }
                        if ui
                            .button("Report per territory (PDF)…")
                            .on_hover_text("An overview and a page per territory with its map, figures and largest areas")
                            .clicked()
                        {
                            exports.push(Export::Report(p.id));
                        }
                        ui.separator();
                    }
                    let data: Vec<(DataRef, String)> = self
                        .project
                        .tables
                        .iter()
                        .map(|t| (DataRef::Table(t.id), t.name.clone()))
                        .chain(
                            self.project
                                .layers
                                .iter()
                                .map(|l| (DataRef::Layer(l.id), l.name.clone())),
                        )
                        .collect();
                    if !data.is_empty() {
                        ui.label(RichText::new("Data").strong());
                    }
                    for (d, name) in data {
                        ui.menu_button(name, |ui| {
                            if ui.button("Excel…").clicked() {
                                exports.push(Export::Data(d, true, None));
                            }
                            if ui.button("CSV…").clicked() {
                                exports.push(Export::Data(d, false, None));
                            }
                            if let DataRef::Layer(l) = d
                                && ui.button("GeoJSON (with shapes)…").clicked()
                            {
                                exports.push(Export::LayerGeoJson(l));
                            }
                        });
                    }
                });
                ui.separator();
                item(ui, commands, &self.keymap, Command::Settings);
                item(ui, commands, &self.keymap, Command::Quit);
            });
            ui.menu_button("Edit", |ui| {
                let undo = self.history.undo_label().map(|l| format!("Undo {l}"));
                let redo = self.history.redo_label().map(|l| format!("Redo {l}"));
                if ui
                    .add_enabled(
                        undo.is_some(),
                        egui::Button::new(undo.unwrap_or("Undo".into()))
                            .shortcut_text(self.keymap.shortcut_text(&ctx, Command::Undo)),
                    )
                    .clicked()
                {
                    commands.push(Command::Undo);
                }
                if ui
                    .add_enabled(
                        redo.is_some(),
                        egui::Button::new(redo.unwrap_or("Redo".into()))
                            .shortcut_text(self.keymap.shortcut_text(&ctx, Command::Redo)),
                    )
                    .clicked()
                {
                    commands.push(Command::Redo);
                }
            });
            ui.menu_button("Map", |ui| {
                for c in [
                    Command::ColorByValue,
                    Command::LinkData,
                    Command::MergeAreas,
                    Command::ReplaceLayer,
                    Command::HexGrid,
                    Command::Catchment,
                    Command::FlowLines,
                    Command::CalculatedColumn,
                    Command::ExportMap,
                    Command::ExportHtml,
                    Command::ExportAtlas,
                    Command::CopyMap,
                    Command::NewMap,
                    Command::ZoomToAll,
                    Command::ToolSelect,
                    Command::SelectByList,
                ] {
                    item(ui, commands, &self.keymap, c);
                }
            });
            ui.menu_button("Territories", |ui| {
                item(ui, commands, &self.keymap, Command::NewPlan);
                ui.separator();
                for c in [
                    Command::NewTerritory,
                    Command::ToolPaint,
                    Command::FillGaps,
                    Command::Balance,
                    Command::Recolor,
                ] {
                    ui.add_enabled_ui(self.active_plan.is_some(), |ui| {
                        item(ui, commands, &self.keymap, c)
                    });
                }
            });
            ui.menu_button("View", |ui| {
                for c in [
                    Command::ToggleContents,
                    Command::ToggleInspector,
                    Command::ShowPalette,
                ] {
                    item(ui, commands, &self.keymap, c);
                }
                ui.separator();
                for c in [
                    Command::ThemeSystem,
                    Command::ThemeLight,
                    Command::ThemeDark,
                ] {
                    item(ui, commands, &self.keymap, c);
                }
                ui.separator();
                for c in [Command::ZoomIn, Command::ZoomOut, Command::ZoomReset] {
                    item(ui, commands, &self.keymap, c);
                }
            });
            ui.menu_button("Help", |ui| {
                item(ui, commands, &self.keymap, Command::Help);
                item(ui, commands, &self.keymap, Command::About);
            });
        });
        for e in exports {
            self.ask_file(FileAction::Export(e));
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, commands: &mut Vec<Command>) {
        ui.horizontal(|ui| {
            let ctx = ui.ctx().clone();
            let mut button = |ui: &mut Ui, text: &str, c: Command, hint: &str| {
                let shortcut = self.keymap.shortcut_text(&ctx, c);
                let tip = if shortcut.is_empty() { hint.to_owned() } else { format!("{hint} ({shortcut})") };
                if ui.button(text).on_hover_text(tip).clicked() {
                    commands.push(c);
                }
            };
            button(ui, "📂 Open", Command::OpenProject, "Open a project");
            button(ui, "💾 Save", Command::SaveProject, "Save the project");
            ui.separator();
            button(ui, "🗺 Add boundaries", Command::AddBoundaries, "Postcodes, municipalities, regions… from a Shapefile, GeoJSON, GeoPackage, KML or zip");
            button(ui, "📊 Add data", Command::AddData, "A table from CSV or Excel (or paste cells with Ctrl+V), linked to areas by a key");
            button(ui, "🎨 Colour by value", Command::ColorByValue, "Colour areas, or draw circles sized by a number");
            ui.separator();
            button(ui, "⬣ New territories", Command::NewPlan, "Plan sales or service territories");
            ui.separator();
            let has_plan = self.active_plan.is_some();
            ui.selectable_value(&mut self.tool, Tool::Navigate, "✋ Navigate")
                .on_hover_text("Drag to move, scroll to zoom, click an area to inspect it (V)");
            ui.selectable_value(&mut self.tool, Tool::Measure, "📏 Measure")
                .on_hover_text("Click points to measure the distance along them; right-click takes the last one back, Esc starts over");
            ui.selectable_value(&mut self.tool, Tool::Select, "⬚ Select")
                .on_hover_text("Click or drag a rectangle over areas, Alt+drag for a lasso, Shift adds. Then assign them in one go (S)");
            ui.add_enabled_ui(has_plan, |ui| {
                ui.selectable_value(&mut self.tool, Tool::Paint, "✏ Paint")
                    .on_hover_text("Drag over areas to assign them to the active territory. Shift+click picks a territory, Alt+drag erases, right-drag moves the map (B)")
                    .on_disabled_hover_text("Create a territory plan first");
            });
            if self.tool == Tool::Paint
                && let Some(t) = self
                    .active_plan
                    .and_then(|p| self.project.plan(p))
                    .and_then(|p| self.active_territory.and_then(|t| p.territory(t)))
            {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 3.0, map_view::color(t.color));
                ui.label(RichText::new(&t.name).strong());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("⌘").on_hover_text("All commands (Ctrl+Shift+P)").clicked() {
                    commands.push(Command::ShowPalette);
                }
                self.search_ui(ui);
            });
        });
    }

    /// The search box with its matches below it.
    fn search_ui(&mut self, ui: &mut Ui) {
        let id = egui::Id::new("search-box");
        let before = self.search.clone();
        let r = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .id(id)
                .hint_text("🔍 Find an area or territory")
                .desired_width(210.0),
        );
        if std::mem::take(&mut self.search_focus) {
            r.request_focus();
        }
        if self.search != before {
            self.search_hits = crate::search::search(&self.project, &self.search, 12);
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && r.has_focus() {
            self.search.clear();
            self.search_hits.clear();
        }
        let mut pick = None;
        if enter {
            pick = self.search_hits.first().cloned();
        }
        if (r.has_focus() || r.lost_focus())
            && !self.search_hits.is_empty()
            && !self.search.trim().is_empty()
        {
            egui::Area::new(id.with("hits"))
                .order(egui::Order::Foreground)
                .fixed_pos(r.rect.left_bottom() + egui::vec2(0.0, 4.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(260.0);
                        for h in &self.search_hits {
                            let b = ui.add(
                                egui::Button::new(RichText::new(&h.label))
                                    .frame(false)
                                    .right_text(RichText::new(&h.detail).weak().small()),
                            );
                            if b.clicked() {
                                pick = Some(h.clone());
                            }
                        }
                    });
                });
        } else if r.has_focus() && !self.search.trim().is_empty() {
            r.clone().on_hover_text("Nothing found");
        }
        if let Some(h) = pick {
            self.go_to(&h.target);
            self.search_hits.clear();
        }
    }

    /// Shows a search hit: selects the feature or activates the territory,
    /// and zooms there.
    pub(crate) fn go_to(&mut self, target: &crate::search::Target) {
        use crate::search::Target;
        let map = self.ensure_map();
        match *target {
            Target::Feature(layer, f) => {
                self.select(layer, vec![f], crate::map_view::SelectMode::Replace);
                if let Some(s) = self.map_states.get_mut(&map) {
                    s.zoom_to = Some((layer, vec![f]));
                }
            }
            Target::Territory(plan, t) => {
                let before = self.project.clone();
                self.show_plan(plan);
                self.history.push("Show territories", before);
                self.active_territory = Some(t);
                if let Some(p) = self.project.plan(plan) {
                    let units: Vec<u32> = (0..p.assignment.len() as u32)
                        .filter(|&u| p.assignment[u as usize] == Some(t))
                        .collect();
                    let layer = p.layer;
                    if let Some(s) = self.map_states.get_mut(&map) {
                        s.zoom_to = Some((layer, units));
                    }
                }
            }
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let hover = self
                .current_map
                .and_then(|m| self.map_states.get(&m))
                .and_then(|s| s.hover.clone());
            if let Some(h) = &hover
                && let Some(layer) = self.project.layer(h.layer)
            {
                ui.label(RichText::new(layer.display_name(h.feature)).strong());
                if let Some(plan) = self.active_plan.and_then(|p| self.project.plan(p)).filter(|p| p.layer == h.layer) {
                    let t = plan.assignment.get(h.feature).copied().flatten().and_then(|t| plan.territory(t));
                    ui.label(match t {
                        Some(t) => format!("→ {}", t.name),
                        None => "→ unassigned".into(),
                    });
                }
                if let Some([lon, lat]) = h.lonlat {
                    ui.label(RichText::new(format!("{lat:.5}°, {lon:.5}°")).weak());
                }
            } else {
                ui.label(RichText::new(match self.tool {
                    Tool::Navigate => "Drag to move the map, scroll to zoom.",
                    Tool::Paint => "Drag over areas to assign them. Shift+click picks a territory, Alt+drag erases.",
                    Tool::Site => "Click on the map where the territory's site is (Esc cancels).",
                    Tool::Select => "Click or drag a rectangle to select, Alt+drag for a lasso, Shift adds. Right-drag moves the map, Esc clears.",
                    Tool::Measure => "Click points to measure; right-click takes the last one back, Esc starts over.",
                }).weak());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut cancel = None;
                for job in self.jobs.visible() {
                    if job.cancellable && ui.small_button("Stop").clicked() {
                        cancel = Some(job.id);
                    }
                    match job.progress() {
                        Some(p) => {
                            ui.add(egui::ProgressBar::new(p).desired_width(90.0).show_percentage());
                        }
                        None => {
                            ui.spinner();
                        }
                    }
                    ui.label(&job.label);
                }
                if let Some(id) = cancel {
                    self.jobs.cancel(id);
                }
                if self.history.dirty {
                    ui.label(RichText::new("● unsaved").weak());
                }
            });
        });
    }

    fn welcome(&mut self, ui: &mut Ui, commands: &mut Vec<Command>) -> Vec<Action> {
        let mut actions = Vec::new();
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.12).max(20.0));
            ui.label(RichText::new("Territoria").size(34.0).strong());
            ui.label(RichText::new("Put your data on the map and plan territories that are fair and compact.").size(15.0).weak());
            ui.add_space(28.0);
            let card = |ui: &mut Ui, title: &str, text: &str| -> bool {
                let resp = egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::same(14))
                    .corner_radius(8)
                    .show(ui, |ui| {
                        ui.set_width(250.0);
                        ui.set_height(92.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(title).size(16.0).strong());
                            ui.add_space(4.0);
                            ui.label(RichText::new(text).weak());
                        });
                    })
                    .response
                    .interact(egui::Sense::click());
                resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
                resp.clicked()
            };
            let width = 3.0 * 290.0;
            ui.allocate_ui_with_layout(egui::vec2(width, 120.0), Layout::left_to_right(Align::Min), |ui| {
                if card(ui, "1  Add boundaries", "Postcodes, municipalities or any areas: Shapefile, GeoJSON, GeoPackage or a zip download.") {
                    commands.push(Command::AddBoundaries);
                }
                if card(ui, "2  Add your data", "A CSV or Excel table, or cells pasted with Ctrl+V. Keys are matched to areas automatically.") {
                    commands.push(Command::AddData);
                }
                if card(ui, "3  Plan territories", "Paint territories or start from a list, then balance them.") {
                    commands.push(Command::NewPlan);
                }
            });
            ui.add_space(10.0);
            if ui
                .link("No boundaries at hand? Download open boundaries for any country.")
                .clicked()
            {
                commands.push(Command::DownloadBoundaries);
            }
            ui.add_space(20.0);
            if ui.button("📂 Open a project…").clicked() {
                commands.push(Command::OpenProject);
            }
            if !self.settings.recent_projects.is_empty() {
                ui.add_space(18.0);
                ui.label(RichText::new("Recent projects").strong());
                for p in self.settings.recent_projects.iter().take(6) {
                    if ui.link(file_name(p)).on_hover_text(p.display().to_string()).clicked() {
                        actions.push(Action::OpenRecent(p.clone()));
                    }
                }
            }
            ui.add_space(18.0);
            ui.label(RichText::new("Tip: drop files anywhere on this window.").weak().small());
        });
        actions
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut d) = self.import_layers.take() {
            let mut open = true;
            match d.show(ctx, &mut open) {
                Some(ImportLayersOutcome::Add(choices, path)) => self.add_layers(choices, path),
                Some(ImportLayersOutcome::Reread(path, crs)) => self.import_boundaries(
                    path,
                    VectorOptions {
                        crs: Some(crs),
                        encoding: None,
                    },
                ),
                None if open => self.import_layers = Some(d),
                None => {}
            }
        }
        if let Some(mut d) = self.import_table.take() {
            let mut open = true;
            match d.show(ctx, &mut open) {
                Some(ImportTableOutcome::Add {
                    name,
                    table,
                    path,
                    placement,
                    crs,
                }) => self.add_table(name, table, path, placement, crs),
                Some(ImportTableOutcome::Reread(path, options)) => {
                    self.import_table_file(path, options)
                }
                None if open => self.import_table = Some(d),
                None => {}
            }
        }
        if let Some(mut d) = self.link.take() {
            let mut open = true;
            match d.show(ctx, &self.project, &mut open) {
                Some(o) => self.apply_link(o),
                None if open => self.link = Some(d),
                None => {}
            }
        }
        if let Some(mut d) = self.color_by.take() {
            let mut open = true;
            match d.show(ctx, &self.project, &mut open) {
                Some(o) => self.apply_color_by(o),
                None if open => self.color_by = Some(d),
                None => {}
            }
        }
        if let Some(mut d) = self.export_map.take() {
            let mut open = true;
            match d.show(ctx, &mut open) {
                Some(settings) => {
                    let format = settings.format;
                    self.map_export = Some(settings);
                    self.ask_file(FileAction::Export(Export::Map(format)));
                }
                None if open => self.export_map = Some(d),
                None => {}
            }
        }
        if let Some(mut d) = self.download.take() {
            let mut open = true;
            match d.show(ctx, &mut open) {
                Some(DownloadOutcome::Download { set, simplified }) => {
                    self.download_boundaries(*set, simplified)
                }
                Some(DownloadOutcome::Retry) => {
                    d.catalog = None;
                    self.download = Some(d);
                    self.load_catalog();
                }
                None if open => self.download = Some(d),
                None => {}
            }
        }
        if let Some(mut d) = self.merge.take() {
            let mut open = true;
            match d.show(ctx, &self.project, &mut open) {
                Some(MergeOutcome::Create {
                    layer,
                    column,
                    prefix,
                    name,
                }) => self.merge_areas(layer, &column, prefix, &name),
                None if open => self.merge = Some(d),
                None => {}
            }
        }
        if let Some(mut d) = self.score.take() {
            let mut open = true;
            if d.show(ctx, &self.project, &mut open) {
                // On to the formula editor, to see and adjust it.
                let mut f = crate::dialogs::FormulaDialog::new(&self.project, d.data, None);
                f.name = "Score".into();
                f.source = d.formula();
                self.formula = Some(f);
            } else if open {
                self.score = Some(d);
            }
        }
        if let Some(mut d) = self.templates_dialog.take() {
            let mut open = true;
            match d.show(ctx, &mut open) {
                Some(crate::dialogs::TemplatesOutcome::Use(name, path)) => {
                    open = false;
                    if self.history.dirty && !self.project.layers.is_empty() {
                        self.confirm = Some(Confirm {
                            title: "Unsaved changes".into(),
                            text: "Start a new project from the template and discard the changes to this one?".into(),
                            action: ConfirmAction::DiscardThenTemplate(name, path),
                        });
                    } else {
                        self.new_from_template(name, path);
                    }
                }
                Some(crate::dialogs::TemplatesOutcome::Keep(name)) => {
                    open = false;
                    self.save_template(&name);
                }
                Some(crate::dialogs::TemplatesOutcome::Delete(path)) => {
                    let _ = std::fs::remove_file(&path);
                    d.templates = self.templates();
                }
                None => {}
            }
            if open {
                self.templates_dialog = Some(d);
            }
        }
        if let Some(mut d) = self.atlas.take() {
            let mut open = true;
            if d.show(ctx, &self.project, &mut open) {
                if let Some(layer) = d.layer {
                    self.ask_file(FileAction::Export(Export::Atlas(layer, d.column.clone())));
                }
            } else if open {
                self.atlas = Some(d);
            }
        }
        if let Some(mut d) = self.flows.take() {
            let mut open = true;
            if d.show(ctx, &self.project, &mut open) {
                self.add_flows(&d);
            } else if open {
                self.flows = Some(d);
            }
        }
        if let Some(mut d) = self.summarise.take() {
            let mut open = true;
            if d.show(ctx, &self.project, &mut open) {
                self.create_summary(&d);
            } else if open {
                self.summarise = Some(d);
            }
        }
        if let Some(mut d) = self.join.take() {
            let mut open = true;
            if d.show(ctx, &self.project, &mut open) {
                self.apply_join(&d);
            } else if open {
                self.join = Some(d);
            }
        }
        if let Some(mut d) = self.catchment.take() {
            let mut open = true;
            match d.show(ctx, &self.project, &mut open) {
                Some(spec) => self.create_catchment(&spec),
                None if open => self.catchment = Some(d),
                None => {}
            }
        }
        if let Some(mut d) = self.sites.take() {
            let mut open = true;
            if d.show(ctx, &self.project, &mut open) {
                self.place_sites(&d);
            } else if open {
                self.sites = Some(d);
            }
        }
        if let Some(mut d) = self.hex_grid.take() {
            let mut open = true;
            match d.show(ctx, &self.project, &mut open) {
                Some((spec, name)) => self.create_hex_grid(&spec, &name),
                None if open => self.hex_grid = Some(d),
                None => {}
            }
        }
        if let Some(mut d) = self.formula.take() {
            let mut open = true;
            match d.show(ctx, &self.project, &mut open) {
                Some(crate::dialogs::FormulaOutcome::Apply {
                    data,
                    editing,
                    name,
                    source,
                }) => self.apply_formula(data, editing, &name, &source),
                None if open => self.formula = Some(d),
                None => {}
            }
        }
        if let Some(mut d) = self.select_list.take() {
            let mut open = true;
            match d.show(ctx, &self.project, &mut open) {
                Some((layer, units)) => {
                    self.selected = Some((layer, units.clone()));
                    if let Some(s) = self.current_map.and_then(|m| self.map_states.get_mut(&m)) {
                        s.zoom_to = Some((layer, units));
                    }
                }
                None if open => self.select_list = Some(d),
                None => {}
            }
        }
        if let Some(mut d) = self.new_plan.take() {
            let mut open = true;
            match d.show(ctx, &self.project, &mut open) {
                Some(o) => self.apply_new_plan(o),
                None if open => self.new_plan = Some(d),
                None => {}
            }
        }
        if let Some(c) = self.confirm.take() {
            let mut answer = None;
            egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
                ui.set_width(400.0);
                ui.heading(&c.title);
                ui.add_space(6.0);
                ui.label(&c.text);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("OK").strong()).clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        answer = Some(false);
                    }
                });
            });
            match answer {
                Some(true) => match c.action {
                    ConfirmAction::RemoveLayer(id) => {
                        self.edit("Remove layer", |p| {
                            p.remove_layer(id);
                        });
                        self.meshes.retain(&self.project);
                        self.selection = Selection::None;
                        if self
                            .active_plan
                            .is_some_and(|p| self.project.plan(p).is_none())
                        {
                            self.active_plan = None;
                            self.tool = Tool::Navigate;
                        }
                    }
                    ConfirmAction::RemovePlan(id) => {
                        self.edit("Delete plan", |p| p.remove_plan(id));
                        if self.active_plan == Some(id) {
                            self.active_plan = self.project.plans.first().map(|p| p.id);
                            self.active_territory = None;
                            self.tool = Tool::Navigate;
                        }
                        if self.territories_panel.compare == Some(id) {
                            self.territories_panel.compare = None;
                        }
                    }
                    ConfirmAction::RemoveTable(id) => {
                        self.edit("Remove table", |p| p.remove_table(id));
                        self.selection = Selection::None;
                    }
                    ConfirmAction::Discard(command) => {
                        self.history.dirty = false;
                        self.run_command(ctx, *command);
                    }
                    ConfirmAction::DiscardThen(path) => {
                        self.history.dirty = false;
                        self.open_path(&path);
                    }
                    ConfirmAction::DiscardThenTemplate(name, path) => {
                        self.history.dirty = false;
                        self.new_from_template(name, path);
                    }
                },
                Some(false) => {}
                None => self.confirm = Some(c),
            }
        }
        if let Some(mut d) = self.settings_dialog.take() {
            let before = self.settings.clone();
            let mut open = true;
            let outcome = d.show(ctx, &mut self.settings, &mut open);
            if self.settings != before {
                self.apply_theme(ctx);
                if self.settings.shortcuts != before.shortcuts {
                    self.keymap = Keymap::with_changes(&self.settings.shortcuts);
                }
            }
            match outcome {
                Some(crate::dialogs::SettingsOutcome::ChooseLogo) => {
                    self.ask_file(FileAction::Logo);
                }
                Some(crate::dialogs::SettingsOutcome::ClearTiles) => {
                    let dir = self.tiles.cache_dir().to_owned();
                    let _ = std::fs::remove_dir_all(&dir);
                    self.tiles = crate::tiles::Tiles::new(dir);
                    d.measure_cache(self.tiles.cache_dir());
                }
                None => {}
            }
            if open {
                self.settings_dialog = Some(d);
            } else {
                self.save_settings();
            }
        }
        if let Some(mut help) = self.help.take() {
            let mut open = true;
            help.show(ctx, &self.keymap, &mut open);
            if open {
                self.help = Some(help);
            }
        }
        if self.about {
            egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("Territoria");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(6.0);
                ui.label("Geomarketing and territory planning.");
                ui.label(
                    RichText::new("Colour ramps: ColorBrewer (Brewer, Harrower, Penn State).")
                        .weak()
                        .small(),
                );
                ui.add_space(8.0);
                if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.about = false;
                }
            });
        }
    }

    fn palette_ui(&mut self, ctx: &egui::Context, commands: &mut Vec<Command>) {
        let Some((mut query, mut selected)) = self.palette.take() else {
            return;
        };
        let mut close = false;
        let matches: Vec<Command> = Command::ALL
            .iter()
            .copied()
            .filter(|c| {
                let label = c.label().to_lowercase();
                query
                    .to_lowercase()
                    .split_whitespace()
                    .all(|w| label.contains(w))
            })
            .collect();
        egui::Modal::new(egui::Id::new("palette")).show(ctx, |ui| {
            ui.set_width(460.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut query)
                    .hint_text("Type a command…")
                    .desired_width(f32::INFINITY),
            );
            r.request_focus();
            if r.changed() {
                selected = 0;
            }
            let (down, up, enter, esc) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if down {
                selected = (selected + 1).min(matches.len().saturating_sub(1));
            }
            if up {
                selected = selected.saturating_sub(1);
            }
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (i, c) in matches.iter().enumerate() {
                        let shortcut = self.keymap.shortcut_text(ctx, *c);
                        let r = ui.add(
                            egui::Button::selectable(i == selected, c.label())
                                .shortcut_text(shortcut),
                        );
                        if r.clicked() {
                            commands.push(*c);
                            close = true;
                        }
                    }
                });
            if enter && let Some(c) = matches.get(selected) {
                commands.push(*c);
                close = true;
            }
            if esc {
                close = true;
            }
        });
        if !close {
            self.palette = Some((query, selected));
        }
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        self.toasts
            .retain(|t| t.at.elapsed() < Duration::from_secs(if t.error { 10 } else { 4 }));
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -36.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for t in &self.toasts {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(380.0);
                        let text = RichText::new(&t.text);
                        ui.label(if t.error {
                            text.color(theme::DANGER)
                        } else {
                            text
                        });
                    });
                    ui.add_space(4.0);
                }
            });
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    /// Cells pasted outside a text field (copied from a spreadsheet)
    /// become a table, through the usual import dialog.
    fn handle_paste(&mut self, ctx: &egui::Context, modal_open: bool) {
        if modal_open || ctx.egui_wants_keyboard_input() {
            return;
        }
        let pasted = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Paste(t) => Some(t.clone()),
                _ => None,
            })
        });
        if let Some(text) = pasted {
            self.paste_table(&text);
        }
    }

    pub(crate) fn paste_table(&mut self, text: &str) {
        let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
        if lines < 2 || !text.contains(['\t', ';', ',']) {
            self.toast("To paste a table, copy cells with a header row from a spreadsheet.");
            return;
        }
        let dir = self.paths.as_ref().map_or_else(
            || std::env::temp_dir().join(format!("territoria-pasted-{}", std::process::id())),
            |p| p.data_dir.join("pasted"),
        );
        let _ = std::fs::create_dir_all(&dir);
        let path = (1..)
            .map(|i| {
                dir.join(if i == 1 {
                    "Pasted table.txt".to_owned()
                } else {
                    format!("Pasted table {i}.txt")
                })
            })
            .find(|p| !p.exists())
            .unwrap_or_else(|| dir.join("Pasted table.txt"));
        match std::fs::write(&path, text) {
            Ok(()) => self.import_table_file(path, TableOptions::default()),
            Err(e) => self.error(format!("Couldn't keep the pasted table: {e}")),
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let files: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        for f in files {
            self.open_path(&f);
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested())
            && self.history.dirty
            && !self.quit_confirmed
            && !self.project.layers.is_empty()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Confirm {
                title: "Unsaved changes".into(),
                text: "Quit and discard the changes? (A recovery copy is kept in the data folder.)"
                    .into(),
                action: ConfirmAction::Discard(Box::new(Command::Quit)),
            });
            self.quit_confirmed = true;
        }
    }
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default()
}

struct Viewer<'a> {
    app: &'a mut TerritoriaApp,
    actions: Vec<Action>,
    events: Vec<(MapId, Vec<MapEvent>)>,
    focused_map: Option<MapId>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        let p = &self.app.project;
        match tab {
            Tab::Map(id) => p
                .map(*id)
                .map_or("Map".into(), |m| format!("🗺 {}", m.name))
                .into(),
            Tab::Table(DataRef::Layer(id)) => p
                .layer(*id)
                .map_or("Table".into(), |l| format!("▦ {}", l.name))
                .into(),
            Tab::Table(DataRef::Table(id)) => p
                .table(*id)
                .map_or("Table".into(), |t| format!("▦ {}", t.name))
                .into(),
            Tab::Plan(id) => p
                .plan(*id)
                .map_or("Territories".into(), |p| format!("⬣ {}", p.name))
                .into(),
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match *tab {
            Tab::Map(id) => {
                let app = &mut *self.app;
                let Some(map) = app.project.map(id) else {
                    ui.label("This map was removed.");
                    return;
                };
                if ui.ui_contains_pointer() || app.current_map.is_none() {
                    self.focused_map = Some(id);
                }
                let state = app.map_states.entry(id).or_default();
                let mut cx = MapContext {
                    project: &app.project,
                    values: &mut app.values,
                    meshes: &mut app.meshes,
                    analyses: &mut app.analyses,
                    territories: &mut app.territory_cache,
                    tiles: &mut app.tiles,
                    jobs: &mut app.jobs,
                    tool: app.tool,
                    active_plan: app.active_plan,
                    active_territory: app.active_territory,
                    dark: ui.visuals().dark_mode,
                    highlight: app.territories_panel.highlight(app.active_plan),
                    selection: app.selected.as_ref().map(|(l, u)| (*l, u.as_slice())),
                    focus: app.focus && app.selected.is_some(),
                };
                let map = map.clone();
                let events = map_view::show(ui, &map, state, &mut cx);
                self.events.push((id, events));
            }
            Tab::Table(data) => {
                let selected = self.app.selected.as_ref().map(|(l, u)| (*l, u.as_slice()));
                let actions = crate::table_view::show(
                    ui,
                    &self.app.project,
                    &mut self.app.values,
                    data,
                    selected,
                );
                self.actions.extend(actions);
            }
            Tab::Plan(plan) => {
                let app = &mut *self.app;
                let actions = crate::plan_table::show(
                    ui,
                    &app.project,
                    &mut app.values,
                    &mut app.stats,
                    plan,
                );
                self.actions.extend(actions);
            }
        }
    }

    fn on_close(&mut self, _tab: &mut Tab) -> egui_dock::widgets::tab_viewer::OnCloseResponse {
        egui_dock::widgets::tab_viewer::OnCloseResponse::Close
    }
}

impl eframe::App for TerritoriaApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for r in self.jobs.poll() {
            self.handle_job(r);
        }
        if let Some(image) = self.to_clipboard.take() {
            ctx.copy_image(image);
        }
        self.poll_file_dialog();
        self.handle_dropped_files(&ctx);
        self.handle_close_request(&ctx);
        self.autosave();

        let modal_open = self.import_layers.is_some()
            || self.import_table.is_some()
            || self.link.is_some()
            || self.color_by.is_some()
            || self.new_plan.is_some()
            || self.merge.is_some()
            || self.formula.is_some()
            || self.hex_grid.is_some()
            || self.sites.is_some()
            || self.catchment.is_some()
            || self.join.is_some()
            || self.summarise.is_some()
            || self.flows.is_some()
            || self.atlas.is_some()
            || self.templates_dialog.is_some()
            || self.score.is_some()
            || self.settings_dialog.is_some()
            || self.select_list.is_some()
            || self.download.is_some()
            || self.export_map.is_some()
            || self.palette.is_some()
            || self.confirm.is_some()
            || self.about;
        self.handle_paste(&ctx, modal_open);
        if self.tool == Tool::Site && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.tool = Tool::Navigate;
        }
        if self.presenting && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.presenting = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        } else if !modal_open
            && self.selected.is_some()
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.selected = None;
            self.focus = false;
        }
        if self.tool == Tool::Measure && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            for s in self.map_states.values_mut() {
                s.measure.clear();
            }
        }
        if let Some((layer, units)) = &self.selected
            && self
                .project
                .layer(*layer)
                .is_none_or(|l| units.iter().any(|&u| u as usize >= l.len()))
        {
            self.selected = None;
        }
        let mut commands = if modal_open {
            Vec::new()
        } else {
            self.keymap.triggered(&ctx)
        };
        let mut actions: Vec<Action> = Vec::new();

        if !self.presenting {
            egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui, &mut commands));
            egui::Panel::top("toolbar")
                .frame(theme::toolbar_frame(ui.style()))
                .show(ui, |ui| self.toolbar(ui, &mut commands));
            egui::Panel::bottom("status")
                .frame(theme::status_frame(ui.style()))
                .show(ui, |ui| self.status_bar(ui));
        }

        let has_content = !self.project.layers.is_empty()
            || !self.project.tables.is_empty()
            || !self.project.maps.is_empty();
        let panels = has_content && !self.presenting;
        if panels && self.show_contents {
            egui::Panel::left("contents")
                .resizable(true)
                .default_size(270.0)
                .min_size(200.0)
                .max_size(480.0)
                .show(ui, |ui| {
                    let scale = self
                        .current_map
                        .and_then(|m| self.view_scale(m))
                        .map(|v| v.0);
                    let cx = crate::contents::ContentsContext {
                        project: &self.project,
                        values: &mut self.values,
                        analyses: &mut self.analyses,
                        scale,
                        current_map: self.current_map,
                        selection: self.selection,
                        active_plan: self.active_plan,
                    };
                    actions.extend(crate::contents::show(ui, cx));
                });
        }
        if panels && self.show_inspector {
            egui::Panel::right("inspector")
                .resizable(true)
                .default_size(360.0)
                .min_size(280.0)
                .max_size(560.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.right_tab,
                            RightTab::Properties,
                            "Properties",
                        );
                        ui.selectable_value(
                            &mut self.right_tab,
                            RightTab::Territories,
                            "Territories",
                        );
                    });
                    ui.separator();
                    match self.right_tab {
                        RightTab::Properties => {
                            let view = match self.selection {
                                Selection::MapLayer(map, _) => self.view_scale(map),
                                _ => None,
                            };
                            actions.extend(crate::inspector::show(
                                ui,
                                &self.project,
                                &mut self.values,
                                &mut self.analyses,
                                self.selection,
                                view,
                                &self.settings.class_sets,
                            ));
                        }
                        RightTab::Territories => {
                            let cx = crate::territories::PanelContext {
                                project: &self.project,
                                values: &mut self.values,
                                stats: &mut self.stats,
                                active_plan: self.active_plan,
                                active_territory: self.active_territory,
                                tool: self.tool,
                                busy: self
                                    .jobs
                                    .running
                                    .iter()
                                    .any(|j| j.label == "Balancing territories"),
                            };
                            actions.extend(self.territories_panel.show(ui, cx));
                        }
                    }
                });
        }

        if !has_content {
            egui::CentralPanel::default().show(ui, |ui| {
                let a = self.welcome(ui, &mut commands);
                actions.extend(a);
            });
        } else {
            if self.dock.iter_all_tabs().next().is_none()
                && let Some(m) = self.project.maps.first()
            {
                self.dock.push_to_focused_leaf(Tab::Map(m.id));
            }
            let mut dock = std::mem::replace(&mut self.dock, DockState::new(Vec::new()));
            let mut viewer = Viewer {
                app: self,
                actions: Vec::new(),
                events: Vec::new(),
                focused_map: None,
            };
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    DockArea::new(&mut dock)
                        .style(DockStyle::from_egui(ui.style().as_ref()))
                        .show_leaf_collapse_buttons(false)
                        .show_leaf_close_all_buttons(false)
                        .show_inside(ui, &mut viewer);
                });
            let Viewer {
                actions: tab_actions,
                events,
                focused_map,
                ..
            } = viewer;
            self.dock = dock;
            actions.extend(tab_actions);
            if let Some(m) = focused_map {
                self.current_map = Some(m);
            }
            for (map, ev) in events {
                self.handle_map_events(ev, map);
            }
        }

        self.dialogs(&ctx);
        self.palette_ui(&ctx, &mut commands);
        self.toasts_ui(&ctx);
        // A dragged column follows the pointer with where it can go.
        if let Some(drag) = egui::DragAndDrop::payload::<ColumnDrag>(&ctx)
            && let Some(p) = ctx.pointer_hover_pos()
        {
            egui::Area::new(egui::Id::new("column-drag"))
                .order(egui::Order::Tooltip)
                .fixed_pos(p + egui::vec2(14.0, 10.0))
                .interactable(false)
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(format!(
                            "{}: drop on the map or its layer list",
                            drag.column
                        ));
                    });
                });
        }
        for a in actions {
            self.apply_action(&ctx, a);
        }
        for c in commands {
            self.run_command(&ctx, c);
        }
        if self.jobs.is_busy() {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(
            storage,
            WORKSPACE_KEY,
            &Workspace {
                show_contents: self.show_contents,
                show_inspector: self.show_inspector,
            },
        );
        self.save_settings();
    }
}

/// The table behind a data reference, to change; marks it changed.
pub fn data_table_mut(p: &mut Project, data: DataRef) -> Option<&mut territoria_data::Table> {
    let rev = p.bump();
    match data {
        DataRef::Layer(id) => p.layer_mut(id).map(|l| {
            l.rev = rev;
            std::sync::Arc::make_mut(&mut l.attributes)
        }),
        DataRef::Table(id) => p.table_mut(id).map(|t| {
            t.rev = rev;
            std::sync::Arc::make_mut(&mut t.table)
        }),
    }
}

pub fn data_name(project: &Project, data: DataRef) -> Option<String> {
    match data {
        DataRef::Layer(l) => project.layer(l).map(|l| l.name.clone()),
        DataRef::Table(t) => project.table(t).map(|t| t.name.clone()),
    }
}

/// How the rows of `data` belong to the units of `plan`, if they do.
pub fn plan_rows(
    project: &Project,
    values: &mut Cache,
    data: DataRef,
    plan: &territoria_model::Plan,
) -> Option<territoria_model::RowUnits> {
    match data {
        DataRef::Layer(l) => project.layer_rows(values, l, plan.layer),
        DataRef::Table(t) => project.table_rows(values, t, plan.layer),
    }
}

/// A copy of a table with each row's territory in every plan it relates
/// to (or only in `only`).
pub fn with_territories(
    project: &Project,
    values: &mut Cache,
    data: DataRef,
    only: Option<PlanId>,
) -> Option<(String, territoria_data::Table)> {
    let (table, _) = crate::dialogs::data_table(project, data)?;
    let mut out = table.clone();
    for plan in project
        .plans
        .iter()
        .filter(|p| only.is_none_or(|o| o == p.id))
    {
        if let Some(rows) = plan_rows(project, values, data, plan) {
            for mut c in plan.territory_columns(&rows, out.rows()) {
                c.name = out.unique_name(&c.name);
                let _ = out.push_column(c);
            }
        }
    }
    Some((data_name(project, data)?, out))
}

/// Linked tables and points of a plan with their territories, for the
/// plan's workbook.
fn plan_data_sheets(
    project: &Project,
    values: &mut Cache,
    plan: &territoria_model::Plan,
) -> Vec<(String, territoria_data::Table)> {
    let data: Vec<DataRef> = project
        .tables
        .iter()
        .map(|t| DataRef::Table(t.id))
        .chain(
            project
                .layers
                .iter()
                .filter(|l| l.id != plan.layer)
                .map(|l| DataRef::Layer(l.id)),
        )
        .collect();
    let mut sheets = Vec::new();
    for d in data {
        if plan_rows(project, values, d, plan).is_some()
            && let Some(sheet) = with_territories(project, values, d, Some(plan.id))
        {
            sheets.push(sheet);
        }
    }
    sheets
}
