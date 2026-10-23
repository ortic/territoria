//! Dialogs: importing boundaries and tables, linking tables to maps,
//! colouring by value, new territory plans.
//!
//! Each dialog shows itself and returns an outcome; the app applies it
//! (with undo) so dialogs never mutate the project directly.

use std::path::PathBuf;
use std::sync::Arc;

use egui::{ComboBox, RichText, Ui};
use territoria_analysis::format::{format_percent, format_value};
use territoria_data::{Aggregate, ColumnKind, MatchMode, MatchStats, Matching, Table, keys};
use territoria_io::{GeometryKind, TableInfo, TableOptions};
use territoria_model::plan::{AssignmentImport, read_assignment};
use territoria_model::values::{column_keys, layer_keys};
use territoria_model::{LayerId, Project, TableId, TextSource, ValueSource};

use crate::jobs::PreparedLayer;
use crate::theme;

/// Shows `add_contents` in a centred modal window; `false` when closed.
fn modal(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    width: f32,
    add_contents: impl FnOnce(&mut Ui),
) -> bool {
    let mut open = true;
    egui::Modal::new(egui::Id::new(id)).show(ctx, |ui| {
        ui.set_width(width);
        ui.horizontal(|ui| {
            ui.heading(title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✖").on_hover_text("Close (Esc)").clicked() {
                    open = false;
                }
            });
        });
        ui.add_space(6.0);
        add_contents(ui);
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    open
}

/// A coloured dot and text for a share: green from 95 %, amber from 60 %.
fn rate_label(ui: &mut Ui, rate: f64, text: String) {
    let color = if rate >= 0.95 {
        theme::SUCCESS
    } else if rate >= 0.6 {
        theme::WARN
    } else {
        theme::DANGER
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("●").color(color));
        ui.label(text);
    });
}

/// How well rows found areas, with what didn't match.
pub fn match_report(ui: &mut Ui, stats: &MatchStats, rows: &str, targets: &str) {
    let with_key = stats.rows - stats.rows_empty;
    rate_label(
        ui,
        stats.row_rate(),
        format!(
            "{} of {} {rows} found their area ({})",
            format_value(stats.rows_matched as f64),
            format_value(with_key as f64),
            format_percent(stats.row_rate())
        ),
    );
    rate_label(
        ui,
        stats.target_rate().max(0.6),
        format!(
            "{} of {} {targets} have data ({})",
            format_value(stats.targets_matched as f64),
            format_value(stats.targets as f64),
            format_percent(stats.target_rate())
        ),
    );
    if stats.rows_empty > 0 {
        ui.label(
            RichText::new(format!(
                "{} rows have no key and were skipped.",
                stats.rows_empty
            ))
            .weak(),
        );
    }
    if !stats.unmatched_samples.is_empty() {
        ui.label(
            RichText::new(format!("Not found: {}", stats.unmatched_samples.join(", ")))
                .weak()
                .small(),
        );
    }
}

fn column_combo(
    ui: &mut Ui,
    id: &str,
    table: &Table,
    selected: &mut Option<String>,
    filter: impl Fn(ColumnKind) -> bool,
    none: Option<&str>,
) -> bool {
    let before = selected.clone();
    ComboBox::from_id_salt(id)
        .width(220.0)
        .selected_text(
            selected
                .clone()
                .or_else(|| none.map(str::to_owned))
                .unwrap_or_else(|| "Choose…".into()),
        )
        .show_ui(ui, |ui| {
            if let Some(n) = none {
                ui.selectable_value(selected, None, n);
            }
            for c in table.columns.iter().filter(|c| filter(c.kind)) {
                ui.selectable_value(
                    selected,
                    Some(c.name.clone()),
                    format!("{}  ·  {}", c.name, c.kind.label()),
                );
            }
        });
    *selected != before
}

fn layer_combo(
    ui: &mut Ui,
    id: &str,
    project: &Project,
    selected: &mut Option<LayerId>,
    kind: Option<GeometryKind>,
) -> bool {
    let before = *selected;
    let text = selected
        .and_then(|l| project.layer(l))
        .map_or("Choose…".to_owned(), |l| l.name.clone());
    ComboBox::from_id_salt(id)
        .width(220.0)
        .selected_text(text)
        .show_ui(ui, |ui| {
            for l in project
                .layers
                .iter()
                .filter(|l| kind.is_none_or(|k| l.kind() == k))
            {
                ui.selectable_value(
                    selected,
                    Some(l.id),
                    format!(
                        "{}  ·  {} {}",
                        l.name,
                        format_value(l.len() as f64),
                        l.kind().label()
                    ),
                );
            }
        });
    *selected != before
}

// ---------------------------------------------------------------------------
// Importing boundaries

pub struct LayerChoice {
    pub include: bool,
    pub name: String,
    pub key_column: Option<String>,
    pub name_column: Option<String>,
    /// Merge features that share a key into one.
    pub merge: bool,
    pub prepared: PreparedLayer,
}

pub struct ImportLayers {
    pub path: PathBuf,
    pub layers: Vec<LayerChoice>,
    pub crs_text: String,
    pub crs_error: Option<String>,
}

pub enum ImportLayersOutcome {
    Add(Vec<LayerChoice>, PathBuf),
    /// Read the file again with this coordinate system.
    Reread(PathBuf, territoria_geo::Crs),
}

impl ImportLayers {
    pub fn new(path: PathBuf, prepared: Vec<PreparedLayer>) -> ImportLayers {
        let layers = prepared
            .into_iter()
            .map(|p| {
                let attrs = &p.imported.attributes;
                let key = territoria_data::suggest::key_column(attrs)
                    .map(|i| attrs.columns[i].name.clone());
                let name = territoria_data::suggest::name_column(attrs)
                    .map(|i| attrs.columns[i].name.clone());
                LayerChoice {
                    include: true,
                    name: p.imported.name.clone(),
                    key_column: key,
                    name_column: name,
                    merge: true,
                    prepared: p,
                }
            })
            .collect();
        ImportLayers {
            path,
            layers,
            crs_text: String::new(),
            crs_error: None,
        }
    }

    /// Returns the outcome when the user decided; `open` turns false on cancel.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) -> Option<ImportLayersOutcome> {
        let mut outcome = None;
        let mut cancel = false;
        let file = self
            .path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        *open = modal(ctx, "import-layers", "Add boundaries", 560.0, |ui| {
            ui.label(RichText::new(file).weak());
            ui.add_space(4.0);
            let several = self.layers.len() > 1;
            egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
                for (i, l) in self.layers.iter_mut().enumerate() {
                    let imported = &l.prepared.imported;
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if several {
                                ui.checkbox(&mut l.include, "");
                            }
                            ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(260.0));
                            ui.label(
                                RichText::new(format!(
                                    "{} {}",
                                    format_value(imported.geometries.len() as f64),
                                    imported.geometries.kind().label()
                                ))
                                .weak(),
                            );
                        });
                        egui::Grid::new(("import-grid", i)).num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                            ui.label("Coordinates");
                            let color = if imported.source_crs.is_some() { ui.visuals().text_color() } else { theme::WARN };
                            ui.label(RichText::new(&imported.crs_note).color(color));
                            ui.end_row();
                            ui.label("Key").on_hover_text("The column that identifies each area: what your data's codes are matched against (postcode, municipality number, …).");
                            column_combo(ui, &format!("key{i}"), &imported.attributes, &mut l.key_column, |_| true, Some("Row number"));
                            ui.end_row();
                            ui.label("Name").on_hover_text("A readable name for lists, labels and tooltips.");
                            column_combo(ui, &format!("name{i}"), &imported.attributes, &mut l.name_column, |k| k == ColumnKind::Text || k == ColumnKind::Key, Some("None"));
                            ui.end_row();
                        });
                        if let Some(c) = l.key_column.as_deref().and_then(|k| imported.attributes.column_index(k)) {
                            let dups = territoria_model::duplicate_keys(&imported.attributes, c);
                            if dups > 0 {
                                ui.checkbox(
                                    &mut l.merge,
                                    format!("Merge areas that share a key ({} keys are on several areas)", format_value(dups as f64)),
                                )
                                .on_hover_text("One area per key: data for a key then colours all its parts. Without merging, data for such a key goes to the first area only.");
                            }
                        }
                        for w in &imported.warnings {
                            ui.label(RichText::new(w).color(theme::WARN));
                        }
                        if l.prepared.geometry.approximate_neighbors {
                            ui.label(
                                RichText::new("Neighbouring areas don't share their borders exactly. Territories still work; their borders may look busier.")
                                    .color(theme::WARN),
                            );
                        }
                    });
                    ui.add_space(4.0);
                }
            });
            if self
                .layers
                .iter()
                .any(|l| l.prepared.imported.source_crs.is_none())
                || !self.crs_text.is_empty()
            {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Coordinate system:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.crs_text)
                            .hint_text("EPSG code, e.g. 2056, 25832, 27700")
                            .desired_width(200.0),
                    );
                    if ui.button("Read again").clicked() {
                        match territoria_geo::Crs::parse(&self.crs_text) {
                            Ok(crs) => {
                                outcome = Some(ImportLayersOutcome::Reread(self.path.clone(), crs))
                            }
                            Err(e) => self.crs_error = Some(e.to_string()),
                        }
                    }
                });
                if let Some(e) = &self.crs_error {
                    ui.label(RichText::new(e).color(theme::DANGER));
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let any = self.layers.iter().any(|l| l.include);
                if ui
                    .add_enabled(any, egui::Button::new(RichText::new("Add to map").strong()))
                    .clicked()
                {
                    outcome = Some(ImportLayersOutcome::Add(
                        std::mem::take(&mut self.layers),
                        self.path.clone(),
                    ));
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if cancel {
            *open = false;
        }
        outcome
    }
}

// ---------------------------------------------------------------------------
// Importing a table

pub struct ImportTable {
    pub path: PathBuf,
    pub name: String,
    pub table: Table,
    pub info: TableInfo,
    pub options: TableOptions,
    pub placement: Placement,
    /// Coordinate system of the coordinate columns (EPSG code or empty).
    pub crs_text: String,
    pub crs_error: Option<String>,
}

/// How a new table gets onto the map.
#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    /// Rows belong to areas by a key (the link dialog follows).
    ByKey,
    /// Rows are points at their coordinates.
    Points { x: String, y: String },
    /// Just add the table.
    None,
}

pub enum ImportTableOutcome {
    Add {
        name: String,
        table: Table,
        path: PathBuf,
        placement: Placement,
        crs: Option<territoria_geo::Crs>,
    },
    Reread(PathBuf, TableOptions),
}

impl ImportTable {
    pub fn new(
        path: PathBuf,
        name: String,
        table: Table,
        info: TableInfo,
        options: TableOptions,
    ) -> ImportTable {
        let (placement, crs_text) = match territoria_data::suggest::coordinate_columns(&table) {
            Some((x, y, degrees)) => (
                Placement::Points {
                    x: table.columns[x].name.clone(),
                    y: table.columns[y].name.clone(),
                },
                if degrees { String::new() } else { "?".into() },
            ),
            None => (Placement::ByKey, String::new()),
        };
        ImportTable {
            path,
            name,
            table,
            info,
            options,
            placement,
            crs_text,
            crs_error: None,
        }
    }
}

impl ImportTable {
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) -> Option<ImportTableOutcome> {
        let mut outcome = None;
        let mut cancel = false;
        *open = modal(ctx, "import-table", "Add data", 760.0, |ui| {
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(260.0));
                ui.label(
                    RichText::new(format!("{} rows", format_value(self.table.rows() as f64)))
                        .weak(),
                );
            });
            // Reading options.
            ui.horizontal(|ui| {
                if !self.info.sheets.is_empty() {
                    ui.label("Sheet");
                    let mut sheet = self.info.sheet.clone();
                    ComboBox::from_id_salt("sheet")
                        .selected_text(sheet.clone().unwrap_or_default())
                        .show_ui(ui, |ui| {
                            for s in &self.info.sheets {
                                ui.selectable_value(&mut sheet, Some(s.clone()), s);
                            }
                        });
                    if sheet != self.info.sheet {
                        let mut o = self.options.clone();
                        o.sheet = sheet;
                        outcome = Some(ImportTableOutcome::Reread(self.path.clone(), o));
                    }
                }
                if let Some(d) = self.info.delimiter {
                    ui.label("Separator");
                    let mut sel = d;
                    ComboBox::from_id_salt("delim")
                        .selected_text(delimiter_label(d))
                        .show_ui(ui, |ui| {
                            for c in [b',', b';', b'\t', b'|'] {
                                ui.selectable_value(&mut sel, c, delimiter_label(c));
                            }
                        });
                    if sel != d {
                        let mut o = self.options.clone();
                        o.delimiter = Some(sel);
                        outcome = Some(ImportTableOutcome::Reread(self.path.clone(), o));
                    }
                }
                if let Some(enc) = &self.info.encoding {
                    ui.label("Encoding");
                    let mut sel = enc.clone();
                    ComboBox::from_id_salt("enc")
                        .selected_text(&sel)
                        .show_ui(ui, |ui| {
                            for e in [
                                "UTF-8",
                                "windows-1252",
                                "ISO-8859-15",
                                "UTF-16LE",
                                "windows-1250",
                                "IBM866",
                            ] {
                                ui.selectable_value(&mut sel, e.to_owned(), e);
                            }
                        });
                    if &sel != enc {
                        let mut o = self.options.clone();
                        o.encoding = Some(sel);
                        outcome = Some(ImportTableOutcome::Reread(self.path.clone(), o));
                    }
                }
                let mut header = self.info.header;
                if ui
                    .checkbox(&mut header, "First row has column names")
                    .changed()
                {
                    let mut o = self.options.clone();
                    o.header = Some(header);
                    outcome = Some(ImportTableOutcome::Reread(self.path.clone(), o));
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new("Check the column types: codes such as postcodes should be “Key / code” so leading zeros survive.").weak());
            ui.add_space(2.0);
            preview_grid(ui, &mut self.table);
            ui.add_space(8.0);
            ui.label(RichText::new("Put it on the map").strong());
            let coords = territoria_data::suggest::coordinate_columns(&self.table);
            ui.radio_value(
                &mut self.placement,
                Placement::ByKey,
                "Rows belong to areas by a key (postcode, municipality number, …)",
            );
            let as_points = matches!(self.placement, Placement::Points { .. });
            if ui
                .radio(
                    as_points,
                    "Rows are points at their coordinates (customers, branches, …)",
                )
                .clicked()
                && !as_points
            {
                self.placement = match coords {
                    Some((x, y, _)) => Placement::Points {
                        x: self.table.columns[x].name.clone(),
                        y: self.table.columns[y].name.clone(),
                    },
                    None => {
                        let numeric: Vec<String> = self
                            .table
                            .columns
                            .iter()
                            .filter(|c| c.kind.is_numeric())
                            .map(|c| c.name.clone())
                            .collect();
                        Placement::Points {
                            x: numeric.first().cloned().unwrap_or_default(),
                            y: numeric.get(1).cloned().unwrap_or_default(),
                        }
                    }
                };
            }
            if let Placement::Points { x, y } = &mut self.placement {
                ui.indent("coords", |ui| {
                    ui.horizontal(|ui| {
                        let mut xs = Some(x.clone());
                        let mut ys = Some(y.clone());
                        ui.label("Longitude / x");
                        column_combo(
                            ui,
                            "coord-x",
                            &self.table,
                            &mut xs,
                            |k| k.is_numeric(),
                            None,
                        );
                        ui.label("Latitude / y");
                        column_combo(
                            ui,
                            "coord-y",
                            &self.table,
                            &mut ys,
                            |k| k.is_numeric(),
                            None,
                        );
                        *x = xs.unwrap_or_default();
                        *y = ys.unwrap_or_default();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Coordinate system");
                        if self.crs_text == "?" {
                            self.crs_text.clear();
                            self.crs_error = Some(
                                "These aren't degrees: enter the EPSG code of the coordinates."
                                    .into(),
                            );
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut self.crs_text)
                                .hint_text("WGS 84 degrees, or an EPSG code")
                                .desired_width(200.0),
                        );
                    });
                    if let Some(e) = &self.crs_error {
                        ui.label(RichText::new(e).color(theme::WARN));
                    }
                });
            }
            ui.radio_value(
                &mut self.placement,
                Placement::None,
                "Not now: just add the table",
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Add data").strong()).clicked() {
                    let crs = if self.crs_text.trim().is_empty() {
                        Ok(None)
                    } else {
                        territoria_geo::Crs::parse(&self.crs_text).map(Some)
                    };
                    match crs {
                        Ok(crs) => {
                            outcome = Some(ImportTableOutcome::Add {
                                name: self.name.clone(),
                                table: std::mem::take(&mut self.table),
                                path: self.path.clone(),
                                placement: self.placement.clone(),
                                crs,
                            });
                        }
                        Err(e) => self.crs_error = Some(e.to_string()),
                    }
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if cancel {
            *open = false;
        }
        outcome
    }
}

fn delimiter_label(d: u8) -> &'static str {
    match d {
        b',' => "Comma ,",
        b';' => "Semicolon ;",
        b'\t' => "Tab",
        b'|' => "Bar |",
        _ => "Other",
    }
}

/// First rows with a type picker per column.
fn preview_grid(ui: &mut Ui, table: &mut Table) {
    let rows = table.rows().min(12);
    egui::ScrollArea::horizontal()
        .max_height(330.0)
        .show(ui, |ui| {
            egui::Grid::new("preview")
                .striped(true)
                .spacing([14.0, 4.0])
                .show(ui, |ui| {
                    for c in table.columns.iter_mut() {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&c.name).strong());
                            let mut kind = c.kind;
                            ComboBox::from_id_salt(("kind", c.name.clone()))
                                .width(110.0)
                                .selected_text(kind.label())
                                .show_ui(ui, |ui| {
                                    for k in ColumnKind::ALL {
                                        ui.selectable_value(&mut kind, k, k.label());
                                    }
                                });
                            if kind != c.kind {
                                let lost = c.convert(kind);
                                if lost > 0 {
                                    tracing::info!(
                                        "{lost} values of {} didn't convert to {}",
                                        c.name,
                                        kind.label()
                                    );
                                }
                            }
                        });
                    }
                    ui.end_row();
                    for r in 0..rows {
                        for c in &table.columns {
                            let text = c.display(r);
                            if c.kind.is_numeric() {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| ui.label(text),
                                );
                            } else {
                                ui.label(text);
                            }
                        }
                        ui.end_row();
                    }
                });
        });
}

// ---------------------------------------------------------------------------
// Linking a table to a layer

pub struct LinkDialog {
    pub table: TableId,
    pub layer: Option<LayerId>,
    pub column: Option<String>,
    pub mode: MatchMode,
    pub matching: Option<Matching>,
    /// Colour the map by this column after linking.
    pub color_by: Option<String>,
    pub agg: Aggregate,
}

pub enum LinkOutcome {
    Link {
        table: TableId,
        layer: LayerId,
        column: String,
        mode: MatchMode,
        color_by: Option<(String, Aggregate)>,
    },
}

impl LinkDialog {
    /// Prefilled with the best guess: the layer and column that match most.
    pub fn new(project: &Project, table: TableId) -> LinkDialog {
        let mut best: Option<(LayerId, String, Matching)> = None;
        if let Some(t) = project.table(table) {
            for l in &project.layers {
                let keys = layer_keys(project, l.id);
                let refs: Vec<Option<&str>> = keys.iter().map(|k| k.as_deref()).collect();
                if let Some((col, m)) = territoria_data::suggest::match_column(&t.table, &refs)
                    && best
                        .as_ref()
                        .is_none_or(|b| m.stats.rows_matched > b.2.stats.rows_matched)
                {
                    best = Some((l.id, t.table.columns[col].name.clone(), m));
                }
            }
        }
        let numeric = project.table(table).and_then(|t| {
            t.table
                .columns
                .iter()
                .filter(|c| c.kind.is_numeric())
                .map(|c| c.name.clone())
                .next_back()
        });
        match best {
            Some((layer, column, m)) => LinkDialog {
                table,
                layer: Some(layer),
                column: Some(column),
                mode: m.mode,
                matching: Some(m),
                color_by: numeric,
                agg: Aggregate::Sum,
            },
            None => LinkDialog {
                table,
                layer: project.layers.first().map(|l| l.id),
                column: None,
                mode: MatchMode::Exact,
                matching: None,
                color_by: numeric,
                agg: Aggregate::Sum,
            },
        }
    }

    fn rematch(&mut self, project: &Project, auto_mode: bool) {
        self.matching = None;
        let (Some(layer), Some(column), Some(t)) =
            (self.layer, &self.column, project.table(self.table))
        else {
            return;
        };
        let targets = layer_keys(project, layer);
        let source = column_keys(&t.table, column);
        let trefs: Vec<Option<&str>> = targets.iter().map(|k| k.as_deref()).collect();
        let srefs: Vec<Option<&str>> = source.iter().map(|k| k.as_deref()).collect();
        let m = if auto_mode {
            keys::best_match(&srefs, &trefs)
        } else {
            let index = territoria_data::KeyIndex::new(trefs.iter().copied(), self.mode);
            keys::match_keys(srefs.iter().copied(), &index)
        };
        self.mode = m.mode;
        self.matching = Some(m);
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        open: &mut bool,
    ) -> Option<LinkOutcome> {
        let Some(t) = project.table(self.table) else {
            *open = false;
            return None;
        };
        let table = t.table.clone();
        let mut outcome = None;
        let mut cancel = false;
        let title = format!("Link “{}” to the map", t.name);
        *open = modal(ctx, "link", &title, 560.0, |ui| {
            ui.label(RichText::new("Rows of the table belong to areas by a key, such as a postcode or a municipality number.").weak());
            ui.add_space(6.0);
            let mut changed = false;
            let mut auto = false;
            egui::Grid::new("link-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Areas");
                    if layer_combo(ui, "link-layer", project, &mut self.layer, None) {
                        changed = true;
                        auto = true;
                    }
                    ui.end_row();
                    ui.label("Key in the table");
                    if column_combo(
                        ui,
                        "link-col",
                        &table,
                        &mut self.column,
                        |k| k != ColumnKind::Bool && k != ColumnKind::Date,
                        None,
                    ) {
                        changed = true;
                        auto = true;
                    }
                    ui.end_row();
                    ui.label("Compare keys");
                    let before = self.mode;
                    ComboBox::from_id_salt("link-mode")
                        .width(220.0)
                        .selected_text(self.mode.label())
                        .show_ui(ui, |ui| {
                            let mut modes =
                                vec![MatchMode::Exact, MatchMode::Loose, MatchMode::Numeric];
                            for n in 1..=6 {
                                modes.push(MatchMode::Prefix(n));
                            }
                            for m in modes {
                                ui.selectable_value(&mut self.mode, m, m.label());
                            }
                        });
                    changed |= before != self.mode;
                    ui.end_row();
                });
            if changed {
                self.rematch(project, auto);
            }
            ui.add_space(6.0);
            match &self.matching {
                Some(m) => {
                    let layer_name = self
                        .layer
                        .and_then(|l| project.layer(l))
                        .map_or("areas".into(), |l| l.name.clone());
                    match_report(ui, &m.stats, "rows", &format!("areas of {layer_name}"));
                    if m.stats.row_rate() < 0.9 && m.mode == MatchMode::Exact {
                        ui.label(
                            RichText::new(
                                "Tip: try “Ignore leading zeros” or “Ignore case and spaces”.",
                            )
                            .weak(),
                        );
                    }
                }
                None => {
                    ui.label(RichText::new("Choose the areas and the table's key column.").weak());
                }
            }
            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                let mut color = self.color_by.is_some();
                ui.checkbox(&mut color, "Colour the areas by");
                if !color {
                    self.color_by = None;
                } else if self.color_by.is_none() {
                    self.color_by = table
                        .columns
                        .iter()
                        .find(|c| c.kind.is_numeric())
                        .map(|c| c.name.clone());
                }
                ui.add_enabled_ui(color, |ui| {
                    column_combo(
                        ui,
                        "link-value",
                        &table,
                        &mut self.color_by,
                        |k| k.is_numeric(),
                        None,
                    );
                    ComboBox::from_id_salt("link-agg")
                        .selected_text(self.agg.label())
                        .show_ui(ui, |ui| {
                            for a in Aggregate::ALL {
                                ui.selectable_value(&mut self.agg, a, a.label());
                            }
                        });
                });
            });
            ui.label(
                RichText::new("When several rows belong to one area, their values are combined.")
                    .weak()
                    .small(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ok = self
                    .matching
                    .as_ref()
                    .is_some_and(|m| m.stats.rows_matched > 0);
                if ui
                    .add_enabled(ok, egui::Button::new(RichText::new("Link").strong()))
                    .clicked()
                    && let (Some(layer), Some(column)) = (self.layer, self.column.clone())
                {
                    outcome = Some(LinkOutcome::Link {
                        table: self.table,
                        layer,
                        column,
                        mode: self.mode,
                        color_by: self.color_by.clone().map(|c| (c, self.agg)),
                    });
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if cancel {
            *open = false;
        }
        outcome
    }
}

// ---------------------------------------------------------------------------
// Colouring areas by a value

/// A value the map can be coloured by, with its label.
#[derive(Clone, PartialEq)]
pub enum Colorable {
    Number(ValueSource),
    Category(TextSource),
}

/// Everything a layer can be coloured by: its own columns and the columns
/// of linked tables.
pub fn colorables(project: &Project, layer: LayerId) -> Vec<(String, Colorable)> {
    let mut out = Vec::new();
    let Some(l) = project.layer(layer) else {
        return out;
    };
    for c in &l.attributes.columns {
        if c.kind.is_numeric() {
            out.push((
                c.name.clone(),
                Colorable::Number(ValueSource::Attribute {
                    column: c.name.clone(),
                }),
            ));
        } else if c.kind == ColumnKind::Text || c.kind == ColumnKind::Bool {
            out.push((
                format!("{} (categories)", c.name),
                Colorable::Category(TextSource::Attribute {
                    column: c.name.clone(),
                }),
            ));
        }
    }
    for link in project.links_to(layer) {
        let Some(t) = project.table(link.table) else {
            continue;
        };
        out.push((
            format!("Rows of {}", t.name),
            Colorable::Number(ValueSource::RowCount { link: link.id }),
        ));
        for c in &t.table.columns {
            if c.kind.is_numeric() {
                out.push((
                    format!("{} ({})", c.name, t.name),
                    Colorable::Number(ValueSource::Linked {
                        link: link.id,
                        column: c.name.clone(),
                        agg: Aggregate::Sum,
                    }),
                ));
            } else if c.kind == ColumnKind::Text && c.name != link.column {
                out.push((
                    format!("{} ({}, categories)", c.name, t.name),
                    Colorable::Category(TextSource::Linked {
                        link: link.id,
                        column: c.name.clone(),
                    }),
                ));
            }
        }
    }
    if l.kind() == GeometryKind::Area {
        // Points inside the areas: customers per postcode and their figures.
        for p in project
            .layers
            .iter()
            .filter(|p| p.kind() == GeometryKind::Point && p.id != layer)
        {
            out.push((
                format!("Number of {}", p.name),
                Colorable::Number(ValueSource::Points {
                    layer: p.id,
                    column: None,
                    agg: Aggregate::Count,
                }),
            ));
            for c in p.attributes.columns.iter().filter(|c| c.kind.is_numeric()) {
                out.push((
                    format!("{} ({})", c.name, p.name),
                    Colorable::Number(ValueSource::Points {
                        layer: p.id,
                        column: Some(c.name.clone()),
                        agg: Aggregate::Sum,
                    }),
                ));
            }
        }
        out.push(("Area (km²)".into(), Colorable::Number(ValueSource::Area)));
    }
    // Distance zones: each area's distance to its territory's site.
    for p in project
        .plans
        .iter()
        .filter(|p| p.layer == layer && p.territories.iter().any(|t| t.site.is_some()))
    {
        out.push((
            format!("km to the site ({})", p.name),
            Colorable::Number(ValueSource::SiteDistance { plan: p.id }),
        ));
    }
    out
}

pub struct ColorByDialog {
    pub layer: Option<LayerId>,
    pub choice: usize,
    /// Circles sized by the value instead of colours (`None`: by layer kind).
    pub circles: Option<bool>,
}

impl ColorByDialog {
    pub fn new(layer: Option<LayerId>) -> ColorByDialog {
        ColorByDialog {
            layer,
            choice: 0,
            circles: None,
        }
    }
}

pub enum ColorByOutcome {
    Create {
        layer: LayerId,
        name: String,
        what: Colorable,
        circles: bool,
    },
}

impl ColorByDialog {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        open: &mut bool,
    ) -> Option<ColorByOutcome> {
        let mut outcome = None;
        let mut cancel = false;
        *open = modal(ctx, "color-by", "Show values on the map", 480.0, |ui| {
            egui::Grid::new("cb")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Areas");
                    if layer_combo(ui, "cb-layer", project, &mut self.layer, None) {
                        self.choice = 0;
                    }
                    ui.end_row();
                    ui.label("Value");
                    let options = self
                        .layer
                        .map(|l| colorables(project, l))
                        .unwrap_or_default();
                    ComboBox::from_id_salt("cb-value")
                        .width(280.0)
                        .selected_text(
                            options
                                .get(self.choice)
                                .map_or("No values: add data first".into(), |o| o.0.clone()),
                        )
                        .show_ui(ui, |ui| {
                            for (i, (label, _)) in options.iter().enumerate() {
                                ui.selectable_value(&mut self.choice, i, label);
                            }
                        });
                    ui.end_row();
                    // Numbers can also be circles; on points that's the default.
                    let points = self
                        .layer
                        .and_then(|l| project.layer(l))
                        .is_some_and(|l| l.kind() == GeometryKind::Point);
                    if matches!(options.get(self.choice), Some((_, Colorable::Number(_)))) {
                        let mut circles = self.circles.unwrap_or(points);
                        ui.label("Show as");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut circles, false, "Colours")
                                .on_hover_text("Each area coloured by its class");
                            ui.radio_value(&mut circles, true, "Circles sized by value")
                                .on_hover_text(
                                    "A circle on each area or point; its area grows with the value",
                                );
                        });
                        self.circles = Some(circles);
                        ui.end_row();
                    }
                    ui.end_row();
                    ui.label("");
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                options.get(self.choice).is_some(),
                                egui::Button::new(RichText::new("Create").strong()),
                            )
                            .clicked()
                            && let (Some(layer), Some((label, what))) =
                                (self.layer, options.get(self.choice))
                        {
                            let points = project
                                .layer(layer)
                                .is_some_and(|l| l.kind() == GeometryKind::Point);
                            outcome = Some(ColorByOutcome::Create {
                                layer,
                                name: label.clone(),
                                what: what.clone(),
                                circles: self.circles.unwrap_or(points),
                            });
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                    ui.end_row();
                });
        });
        if cancel {
            *open = false;
        }
        outcome
    }
}

// ---------------------------------------------------------------------------
// New territory plan

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlanStart {
    Empty,
    FromTable,
}

pub struct NewPlanDialog {
    pub name: String,
    pub layer: Option<LayerId>,
    pub start: PlanStart,
    pub count: usize,
    pub table: Option<TableId>,
    pub unit_column: Option<String>,
    pub territory_column: Option<String>,
    pub name_column: Option<String>,
    pub mode: MatchMode,
    pub preview: Option<Arc<AssignmentImport>>,
}

pub enum NewPlanOutcome {
    Empty {
        name: String,
        layer: LayerId,
        count: usize,
    },
    FromTable {
        name: String,
        layer: LayerId,
        import: Arc<AssignmentImport>,
    },
}

impl NewPlanDialog {
    pub fn new(project: &Project) -> NewPlanDialog {
        NewPlanDialog {
            name: format!("Territories {}", project.plans.len() + 1),
            layer: project
                .layers
                .iter()
                .find(|l| l.kind() == GeometryKind::Area)
                .map(|l| l.id),
            start: PlanStart::Empty,
            count: 8,
            table: project.tables.first().map(|t| t.id),
            unit_column: None,
            territory_column: None,
            name_column: None,
            mode: MatchMode::Numeric,
            preview: None,
        }
    }

    fn update_preview(&mut self, project: &Project) {
        self.preview = None;
        let (Some(layer), Some(t), Some(unit), Some(terr)) = (
            self.layer,
            self.table.and_then(|t| project.table(t)),
            &self.unit_column,
            &self.territory_column,
        ) else {
            return;
        };
        self.preview = Some(Arc::new(read_assignment(
            project,
            layer,
            &t.table,
            unit,
            terr,
            self.name_column.as_deref(),
            self.mode,
        )));
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        open: &mut bool,
    ) -> Option<NewPlanOutcome> {
        let mut outcome = None;
        let mut cancel = false;
        *open = modal(ctx, "new-plan", "New territory plan", 560.0, |ui| {
            let mut changed = false;
            egui::Grid::new("np")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(260.0));
                    ui.end_row();
                    ui.label("Built from").on_hover_text(
                        "The areas territories consist of: postcodes, municipalities, …",
                    );
                    changed |= layer_combo(ui, "np-layer", project, &mut self.layer, None);
                    ui.end_row();
                    ui.label("Start");
                    ui.vertical(|ui| {
                        ui.radio_value(
                            &mut self.start,
                            PlanStart::Empty,
                            "With empty territories to paint",
                        );
                        let has_tables = !project.tables.is_empty();
                        ui.add_enabled_ui(has_tables, |ui| {
                            changed |= ui
                                .radio_value(
                                    &mut self.start,
                                    PlanStart::FromTable,
                                    "From an assignment table (area → territory)",
                                )
                                .on_disabled_hover_text("Add a table first: File → Add Data")
                                .changed();
                        });
                    });
                    ui.end_row();
                    if self.start == PlanStart::Empty {
                        ui.label("Territories");
                        ui.add(egui::DragValue::new(&mut self.count).range(1..=500));
                        ui.end_row();
                    }
                });
            if self.start == PlanStart::FromTable {
                ui.add_space(6.0);
                egui::Grid::new("np-table")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Table");
                        let text = self
                            .table
                            .and_then(|t| project.table(t))
                            .map_or("Choose…".into(), |t| t.name.clone());
                        let before = self.table;
                        ComboBox::from_id_salt("np-table-combo")
                            .width(220.0)
                            .selected_text(text)
                            .show_ui(ui, |ui| {
                                for t in &project.tables {
                                    ui.selectable_value(&mut self.table, Some(t.id), &t.name);
                                }
                            });
                        if before != self.table {
                            self.unit_column = None;
                            self.territory_column = None;
                            changed = true;
                        }
                        ui.end_row();
                        if let Some(t) = self.table.and_then(|t| project.table(t)) {
                            if self.unit_column.is_none()
                                && let Some(layer) = self.layer
                            {
                                let keys = layer_keys(project, layer);
                                let refs: Vec<Option<&str>> =
                                    keys.iter().map(|k| k.as_deref()).collect();
                                self.unit_column =
                                    territoria_data::suggest::match_column(&t.table, &refs)
                                        .map(|(c, _)| t.table.columns[c].name.clone());
                                changed = true;
                            }
                            ui.label("Area key");
                            changed |= column_combo(
                                ui,
                                "np-unit",
                                &t.table,
                                &mut self.unit_column,
                                |_| true,
                                None,
                            );
                            ui.end_row();
                            ui.label("Territory");
                            changed |= column_combo(
                                ui,
                                "np-terr",
                                &t.table,
                                &mut self.territory_column,
                                |_| true,
                                None,
                            );
                            ui.end_row();
                            ui.label("Territory name");
                            changed |= column_combo(
                                ui,
                                "np-tname",
                                &t.table,
                                &mut self.name_column,
                                |_| true,
                                Some("Same as territory"),
                            );
                            ui.end_row();
                        }
                    });
                ui.label(
                    RichText::new(
                        "Area keys may also be lists and ranges such as “8000-8099; 8105; 81*”.",
                    )
                    .weak()
                    .small(),
                );
                if changed {
                    self.update_preview(project);
                }
                if let Some(p) = &self.preview {
                    ui.add_space(4.0);
                    match_report(ui, &p.stats, "rows", "areas");
                    ui.label(format!("{} territories", p.names.len()));
                    if p.conflicts > 0 {
                        ui.label(
                            RichText::new(format!(
                                "{} areas are listed for more than one territory; the first wins.",
                                p.conflicts
                            ))
                            .color(theme::WARN),
                        );
                    }
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ready = self.layer.is_some()
                    && (self.start == PlanStart::Empty
                        || self.preview.as_ref().is_some_and(|p| !p.names.is_empty()));
                if ui
                    .add_enabled(ready, egui::Button::new(RichText::new("Create").strong()))
                    .clicked()
                    && let Some(layer) = self.layer
                {
                    outcome = Some(match self.start {
                        PlanStart::Empty => NewPlanOutcome::Empty {
                            name: self.name.clone(),
                            layer,
                            count: self.count,
                        },
                        PlanStart::FromTable => NewPlanOutcome::FromTable {
                            name: self.name.clone(),
                            layer,
                            import: self.preview.clone().unwrap(),
                        },
                    });
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if cancel {
            *open = false;
        }
        outcome
    }
}

// ---------------------------------------------------------------------------
// Merging areas into a new layer

pub struct MergeDialog {
    pub layer: Option<LayerId>,
    pub column: Option<String>,
    pub use_prefix: bool,
    pub prefix: usize,
    pub name: String,
}

pub enum MergeOutcome {
    Create {
        layer: LayerId,
        column: String,
        prefix: Option<usize>,
        name: String,
    },
}

impl MergeDialog {
    pub fn new(project: &Project, layer: Option<LayerId>) -> MergeDialog {
        let layer = layer.or_else(|| {
            project
                .layers
                .iter()
                .find(|l| l.kind() == GeometryKind::Area)
                .map(|l| l.id)
        });
        let column = layer
            .and_then(|l| project.layer(l))
            .and_then(|l| l.key_column.clone());
        MergeDialog {
            layer,
            column,
            use_prefix: false,
            prefix: 2,
            name: String::new(),
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        open: &mut bool,
    ) -> Option<MergeOutcome> {
        let mut outcome = None;
        let mut cancel = false;
        *open = modal(ctx, "merge", "New layer by merging areas", 520.0, |ui| {
            ui.label(RichText::new("Areas with the same value become one area: municipalities into districts, postcodes into postcode regions (by their first digits), sales areas into regions.").weak());
            ui.add_space(6.0);
            egui::Grid::new("merge-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Areas");
                    if layer_combo(
                        ui,
                        "merge-layer",
                        project,
                        &mut self.layer,
                        Some(GeometryKind::Area),
                    ) {
                        self.column = self
                            .layer
                            .and_then(|l| project.layer(l))
                            .and_then(|l| l.key_column.clone());
                    }
                    ui.end_row();
                    if let Some(l) = self.layer.and_then(|l| project.layer(l)) {
                        ui.label("Merge by");
                        column_combo(
                            ui,
                            "merge-col",
                            &l.attributes,
                            &mut self.column,
                            |k| k != ColumnKind::Number,
                            None,
                        );
                        ui.end_row();
                        ui.label("");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.use_prefix, "Only the first");
                            ui.add_enabled(
                                self.use_prefix,
                                egui::DragValue::new(&mut self.prefix).range(1..=12),
                            );
                            ui.label("characters");
                        });
                        ui.end_row();
                        if let Some(c) = self
                            .column
                            .as_deref()
                            .and_then(|c| l.attributes.column_index(c))
                        {
                            let prefix = self.use_prefix.then_some(self.prefix);
                            let groups = l.attributes.groups_by(c, prefix).len();
                            ui.label("Result");
                            ui.label(format!(
                                "{} areas become {}",
                                format_value(l.len() as f64),
                                format_value(groups as f64)
                            ));
                            ui.end_row();
                            if self.name.is_empty() {
                                self.name = format!(
                                    "{} by {}",
                                    l.name,
                                    self.column.clone().unwrap_or_default()
                                );
                            }
                        }
                        ui.label("Name");
                        ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(260.0));
                        ui.end_row();
                    }
                });
            ui.label(RichText::new("Numbers in the attributes are added up; an “Areas” column counts what was merged.").weak().small());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ready = self.layer.is_some() && self.column.is_some();
                if ui
                    .add_enabled(ready, egui::Button::new(RichText::new("Create").strong()))
                    .clicked()
                    && let (Some(layer), Some(column)) = (self.layer, self.column.clone())
                {
                    outcome = Some(MergeOutcome::Create {
                        layer,
                        column,
                        prefix: self.use_prefix.then_some(self.prefix),
                        name: self.name.clone(),
                    });
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if cancel {
            *open = false;
        }
        outcome
    }
}

// ---------------------------------------------------------------------------
// Downloading open boundaries

use territoria_io::online::BoundarySet;

pub struct DownloadDialog {
    /// `None` while the catalogue loads.
    pub catalog: Option<Result<Arc<Vec<BoundarySet>>, String>>,
    pub search: String,
    pub country: Option<String>,
    pub selected: Option<usize>,
    pub simplified: bool,
}

pub enum DownloadOutcome {
    Download {
        set: Box<BoundarySet>,
        simplified: bool,
    },
    Retry,
}

impl DownloadDialog {
    pub fn new() -> DownloadDialog {
        DownloadDialog {
            catalog: None,
            search: String::new(),
            country: None,
            selected: None,
            simplified: false,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) -> Option<DownloadOutcome> {
        let mut outcome = None;
        let mut cancel = false;
        *open = modal(ctx, "download", "Download open boundaries", 720.0, |ui| {
            ui.label(RichText::new("Administrative areas of about 230 countries from geoBoundaries (William & Mary geoLab). Each set has its own licence; the map credits it.").weak());
            ui.add_space(6.0);
            let sets = match &self.catalog {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading the list of countries…");
                    });
                    return;
                }
                Some(Err(e)) => {
                    ui.label(
                        RichText::new(format!("The list couldn't be loaded: {e}"))
                            .color(theme::DANGER),
                    );
                    if ui.button("Try again").clicked() {
                        outcome = Some(DownloadOutcome::Retry);
                    }
                    return;
                }
                Some(Ok(sets)) => sets.clone(),
            };
            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Country or ISO code")
                        .desired_width(240.0),
                )
                .request_focus();
            });
            ui.add_space(4.0);
            ui.columns(2, |cols| {
                // Countries.
                let needle = self.search.to_lowercase();
                let mut countries: Vec<(&str, &str)> = sets
                    .iter()
                    .filter(|s| {
                        needle.is_empty()
                            || s.country.to_lowercase().contains(&needle)
                            || s.iso.to_lowercase() == needle
                    })
                    .map(|s| (s.iso.as_str(), s.country.as_str()))
                    .collect();
                countries.dedup();
                egui::ScrollArea::vertical()
                    .id_salt("countries")
                    .max_height(320.0)
                    .show(&mut cols[0], |ui| {
                        for (iso, name) in &countries {
                            let sel = self.country.as_deref() == Some(*iso);
                            if ui
                                .selectable_label(sel, format!("{name}  ·  {iso}"))
                                .clicked()
                            {
                                self.country = Some((*iso).to_owned());
                                // Default to the finest level with a manageable count.
                                self.selected = sets
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, s)| s.iso == *iso)
                                    .filter(|(_, s)| s.units <= 6000)
                                    .map(|(i, _)| i)
                                    .next_back();
                            }
                        }
                        if countries.is_empty() {
                            ui.label(RichText::new("No country matches.").weak());
                        }
                    });
                // Levels of the chosen country.
                let ui = &mut cols[1];
                match &self.country {
                    None => {
                        ui.label(RichText::new("Choose a country.").weak());
                    }
                    Some(iso) => {
                        for (i, s) in sets.iter().enumerate().filter(|(_, s)| &s.iso == iso) {
                            let kind = if s.kind.is_empty() {
                                String::new()
                            } else {
                                format!(" · {}", s.kind)
                            };
                            let label = format!(
                                "{}{kind} · {} areas",
                                s.level,
                                format_value(s.units as f64)
                            );
                            if ui
                                .selectable_label(self.selected == Some(i), label)
                                .clicked()
                            {
                                self.selected = Some(i);
                            }
                        }
                        if let Some(s) = self.selected.and_then(|i| sets.get(i)) {
                            ui.add_space(8.0);
                            egui::Grid::new("dl-info")
                                .num_columns(2)
                                .spacing([10.0, 4.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new("Source").weak());
                                    ui.label(&s.source);
                                    ui.end_row();
                                    ui.label(RichText::new("Licence").weak());
                                    ui.label(&s.license);
                                    ui.end_row();
                                    if !s.year.is_empty() {
                                        ui.label(RichText::new("As of").weak());
                                        ui.label(&s.year);
                                        ui.end_row();
                                    }
                                });
                            ui.add_space(4.0);
                            ui.checkbox(
                                &mut self.simplified,
                                "Simplified outlines (smaller download)",
                            )
                            .on_hover_text(
                                "Fine for overview maps; neighbouring borders may not fit exactly.",
                            );
                        }
                    }
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let set = self
                    .selected
                    .and_then(|i| sets.get(i))
                    .filter(|s| Some(&s.iso) == self.country.as_ref());
                if ui
                    .add_enabled(
                        set.is_some(),
                        egui::Button::new(RichText::new("Download").strong()),
                    )
                    .clicked()
                    && let Some(set) = set
                {
                    outcome = Some(DownloadOutcome::Download {
                        set: Box::new(set.clone()),
                        simplified: self.simplified,
                    });
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if cancel {
            *open = false;
        }
        outcome
    }
}

// ---------------------------------------------------------------------------
// Exporting the map

use crate::export_map::Format;

#[derive(Clone)]
pub struct MapExportSettings {
    pub format: Format,
    pub scale: f32,
    pub title: String,
    pub legend: bool,
    pub scale_bar: bool,
}

pub struct ExportMapDialog {
    pub settings: MapExportSettings,
}

impl ExportMapDialog {
    pub fn new(title: String) -> ExportMapDialog {
        ExportMapDialog {
            settings: MapExportSettings {
                format: Format::Png,
                scale: 2.0,
                title,
                legend: true,
                scale_bar: true,
            },
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) -> Option<MapExportSettings> {
        let mut outcome = None;
        let mut cancel = false;
        *open = modal(ctx, "export-map", "Export map", 440.0, |ui| {
            ui.label(
                RichText::new(
                    "Exports what the map shows now, with title, legend, scale bar and credits.",
                )
                .weak(),
            );
            ui.add_space(6.0);
            let s = &mut self.settings;
            egui::Grid::new("export-map")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Format");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut s.format, Format::Png, "PNG image");
                        ui.selectable_value(&mut s.format, Format::Pdf, "PDF");
                        ui.selectable_value(&mut s.format, Format::Svg, "SVG");
                    });
                    ui.end_row();
                    if s.format == Format::Png {
                        ui.label("Resolution");
                        ui.horizontal(|ui| {
                            for (k, label) in
                                [(1.0, "Screen"), (2.0, "Sharp (2×)"), (4.0, "Print (4×)")]
                            {
                                ui.selectable_value(&mut s.scale, k, label);
                            }
                        });
                        ui.end_row();
                    }
                    ui.label("Title");
                    ui.add(egui::TextEdit::singleline(&mut s.title).desired_width(260.0));
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut s.legend, "Legend");
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut s.scale_bar, "Scale bar");
                    ui.end_row();
                });
            ui.label(
                RichText::new("Background maps aren't part of exports yet.")
                    .weak()
                    .small(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Export…").strong()).clicked() {
                    outcome = Some(self.settings.clone());
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if cancel {
            *open = false;
        }
        outcome
    }
}

/// A calculated column: a formula over the other columns of a table.
pub struct FormulaDialog {
    pub data: crate::app::DataRef,
    /// The calculated column being edited.
    pub editing: Option<String>,
    pub name: String,
    pub source: String,
    /// Byte offset of the cursor in `source`.
    cursor: usize,
    /// The checked formula and a preview, for `source` at `rev`.
    preview: Option<(String, u64, Result<FormulaPreview, territoria_expr::Error>)>,
}

struct FormulaPreview {
    ty: territoria_expr::Type,
    /// (row label, result) for the first rows.
    rows: Vec<(String, String)>,
    summary: String,
}

pub enum FormulaOutcome {
    Apply {
        data: crate::app::DataRef,
        editing: Option<String>,
        name: String,
        source: String,
    },
}

/// The table behind a data reference with its revision.
pub fn data_table(project: &Project, data: crate::app::DataRef) -> Option<(&Table, u64)> {
    match data {
        crate::app::DataRef::Layer(id) => project.layer(id).map(|l| (&*l.attributes, l.rev)),
        crate::app::DataRef::Table(id) => project.table(id).map(|t| (&*t.table, t.rev)),
    }
}

impl FormulaDialog {
    pub fn new(
        project: &Project,
        data: crate::app::DataRef,
        editing: Option<String>,
    ) -> FormulaDialog {
        let source = editing
            .as_deref()
            .and_then(|c| data_table(project, data)?.0.column(c)?.formula.clone())
            .unwrap_or_default();
        let name = editing.clone().unwrap_or_default();
        FormulaDialog {
            data,
            editing,
            name,
            cursor: source.len(),
            source,
            preview: None,
        }
    }

    fn update_preview(&mut self, table: &Table, rev: u64) {
        if self
            .preview
            .as_ref()
            .is_some_and(|(s, r, _)| *s == self.source && *r == rev)
        {
            return;
        }
        let editing = self.editing.as_deref().and_then(|c| table.column_index(c));
        let result = territoria_expr::Formula::compile(&self.source, table).and_then(|f| {
            if editing.is_some_and(|i| f.columns().contains(&i)) {
                return Err(territoria_expr::Error {
                    message: "A column can't use its own values.".into(),
                    span: 0..self.source.len(),
                });
            }
            let values = f.evaluate(table);
            let label_col = table
                .columns
                .iter()
                .position(|c| c.kind == ColumnKind::Key)
                .or_else(|| {
                    table
                        .columns
                        .iter()
                        .position(|c| c.kind == ColumnKind::Text)
                })
                .filter(|i| Some(*i) != editing);
            let rows = values
                .iter()
                .take(8)
                .enumerate()
                .map(|(r, v)| {
                    let label = label_col
                        .map_or_else(|| format!("Row {}", r + 1), |c| table.columns[c].display(r));
                    let text = match v {
                        territoria_expr::Val::Num(n) => format_value(*n),
                        v => v.text().unwrap_or_else(|| "–".into()),
                    };
                    (label, text)
                })
                .collect();
            let empty = values
                .iter()
                .filter(|v| **v == territoria_expr::Val::Null)
                .count();
            let mut summary = match f.ty() {
                territoria_expr::Type::Number => {
                    let n: Vec<f64> = values
                        .iter()
                        .filter_map(territoria_expr::Val::as_num)
                        .collect();
                    let (lo, hi) = n
                        .iter()
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| {
                            (a.min(x), b.max(x))
                        });
                    if n.is_empty() {
                        String::new()
                    } else {
                        format!("from {} to {}", format_value(lo), format_value(hi))
                    }
                }
                territoria_expr::Type::Bool => {
                    let yes = values
                        .iter()
                        .filter(|v| **v == territoria_expr::Val::Bool(true))
                        .count();
                    format!(
                        "{} yes, {} no",
                        format_value(yes as f64),
                        format_value((values.len() - yes - empty) as f64)
                    )
                }
                _ => {
                    let distinct: std::collections::HashSet<String> = values
                        .iter()
                        .filter_map(territoria_expr::Val::text)
                        .collect();
                    format!("{} different values", format_value(distinct.len() as f64))
                }
            };
            if empty > 0 {
                if !summary.is_empty() {
                    summary.push_str(" · ");
                }
                summary.push_str(&format!("{} empty", format_value(empty as f64)));
            }
            Ok(FormulaPreview {
                ty: f.ty(),
                rows,
                summary,
            })
        });
        self.preview = Some((self.source.clone(), rev, result));
    }

    /// Replaces `range` of the formula with `text` and puts the cursor after it.
    fn insert(
        &mut self,
        ctx: &egui::Context,
        editor: egui::Id,
        range: std::ops::Range<usize>,
        text: &str,
    ) {
        let range = range.start.min(self.source.len())..range.end.min(self.source.len());
        self.source.replace_range(range.clone(), text);
        self.cursor = range.start + text.len();
        let chars = self.source[..self.cursor].chars().count();
        let mut state = egui::TextEdit::load_state(ctx, editor).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(chars),
            )));
        egui::TextEdit::store_state(ctx, editor, state);
        ctx.memory_mut(|m| m.request_focus(editor));
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        open: &mut bool,
    ) -> Option<FormulaOutcome> {
        let Some((table, rev)) = data_table(project, self.data) else {
            *open = false;
            return None;
        };
        self.update_preview(table, rev);
        let mut outcome = None;
        let mut cancel = false;
        let title = match &self.editing {
            Some(c) => format!("Formula of [{c}]"),
            None => "New calculated column".to_owned(),
        };
        let editor = egui::Id::new("formula-editor");
        let completions = territoria_expr::completions(&self.source, self.cursor, table);
        // Tab takes the first suggestion.
        if ctx.memory(|m| m.has_focus(editor))
            && let Some(first) = completions.items.first()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab))
        {
            let insert = first.insert.clone();
            self.insert(ctx, editor, completions.range.clone(), &insert);
        }
        *open = modal(ctx, "formula", &title, 620.0, |ui| {
            ui.label(RichText::new("A column computed from the others, e.g. revenue per customer: [Revenue] / [Customers]. It updates when you edit the formula.").weak());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(
                    egui::TextEdit::singleline(&mut self.name)
                        .hint_text("e.g. Revenue per customer")
                        .desired_width(280.0),
                );
            });
            ui.add_space(4.0);
            let error = match &self.preview {
                Some((_, _, Err(e))) => Some(e.clone()),
                _ => None,
            };
            let dark = ui.visuals().dark_mode;
            let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                let text = text.as_str();
                let mut job = egui::text::LayoutJob::default();
                let font = egui::FontId::monospace(14.0);
                let base = ui.visuals().text_color();
                for (range, kind) in territoria_expr::highlight(text) {
                    let color = match kind {
                        territoria_expr::Highlight::Column => theme::ACCENT,
                        territoria_expr::Highlight::Text => {
                            if dark {
                                egui::Color32::from_rgb(0x8f, 0xd1, 0x9e)
                            } else {
                                egui::Color32::from_rgb(0x1f, 0x7a, 0x3a)
                            }
                        }
                        territoria_expr::Highlight::Number => {
                            if dark {
                                egui::Color32::from_rgb(0xf0, 0xb4, 0x6a)
                            } else {
                                egui::Color32::from_rgb(0xa8, 0x5a, 0x00)
                            }
                        }
                        territoria_expr::Highlight::Function => {
                            if dark {
                                egui::Color32::from_rgb(0xc7, 0x9b, 0xf2)
                            } else {
                                egui::Color32::from_rgb(0x7b, 0x3f, 0xc4)
                            }
                        }
                        territoria_expr::Highlight::Keyword => base.gamma_multiply(0.7),
                        territoria_expr::Highlight::Plain => base,
                    };
                    let wrong = error.as_ref().is_some_and(|e| {
                        let s = e.span.clone();
                        (s.start < range.end && range.start < s.end)
                            || (s.is_empty() && range.start == s.start)
                    });
                    let mut format = egui::TextFormat::simple(font.clone(), color);
                    if wrong {
                        format.underline = egui::Stroke::new(1.5, theme::DANGER);
                        format.background = theme::DANGER.gamma_multiply(0.15);
                    }
                    job.append(&text[range], 0.0, format);
                }
                job.wrap.max_width = wrap;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let out = egui::TextEdit::multiline(&mut self.source)
                .id(editor)
                .hint_text("[Revenue] / [Customers]")
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .layouter(&mut layouter)
                .show(ui);
            if let Some(r) = out.cursor_range {
                self.cursor = self
                    .source
                    .char_indices()
                    .nth(r.primary.index.0)
                    .map_or(self.source.len(), |(i, _)| i);
            }
            // Suggestions for what is being typed (unless it's complete).
            {
                let items: Vec<_> = completions.items.iter().take(8).collect();
                let complete = items.len() == 1
                    && self.source[completions.range.clone()]
                        .eq_ignore_ascii_case(&items[0].insert);
                if !items.is_empty() && !complete {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Tab ↹").weak().small());
                        for s in items {
                            if ui.small_button(&s.label).on_hover_text(&s.detail).clicked() {
                                let insert = s.insert.clone();
                                self.insert(ui.ctx(), editor, completions.range.clone(), &insert);
                            }
                        }
                    });
                }
            }
            ui.add_space(4.0);
            match &self.preview {
                Some((_, _, Err(e))) if !self.source.trim().is_empty() => {
                    ui.label(RichText::new(format!("⚠ {}", e.message)).color(theme::DANGER));
                }
                Some((_, _, Ok(p))) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("→ {} column", p.ty.label())).strong());
                        ui.label(RichText::new(&p.summary).weak());
                    });
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::Grid::new("formula-preview")
                            .num_columns(2)
                            .striped(true)
                            .spacing([24.0, 2.0])
                            .show(ui, |ui| {
                                for (label, value) in &p.rows {
                                    ui.label(RichText::new(label).weak());
                                    ui.label(value);
                                    ui.end_row();
                                }
                            });
                    });
                }
                _ => {}
            }
            ui.collapsing("Functions", |ui| {
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    let mut group = "";
                    for f in territoria_expr::FUNCTIONS {
                        if f.group != group {
                            group = f.group;
                            ui.add_space(4.0);
                            ui.label(RichText::new(group).strong());
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button(f.signature).on_hover_text("Insert at the cursor").clicked() {
                                let at = self.cursor.min(self.source.len());
                                self.insert(ui.ctx(), editor, at..at, &format!("{}(", f.name));
                            }
                            ui.label(RichText::new(f.doc).weak());
                        });
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new("Operators").strong());
                    ui.label(RichText::new("+ − * / ^ (power) % (remainder) · & joins text · = <> < <= > >= · and, or, not · null is empty. Columns in [brackets], text in \"quotes\", decimals with a point.").weak());
                });
            });
            ui.add_space(8.0);
            let name = self.name.trim();
            let taken = table.column(name).is_some() && self.editing.as_deref() != Some(name);
            if taken {
                ui.label(
                    RichText::new(format!("There is already a column “{name}”."))
                        .color(theme::WARN),
                );
            }
            let ok = !name.is_empty() && !taken && matches!(self.preview, Some((_, _, Ok(_))));
            ui.horizontal(|ui| {
                let label = if self.editing.is_some() {
                    "Update column"
                } else {
                    "Add column"
                };
                if ui
                    .add_enabled(ok, egui::Button::new(RichText::new(label).strong()))
                    .clicked()
                {
                    outcome = Some(FormulaOutcome::Apply {
                        data: self.data,
                        editing: self.editing.clone(),
                        name: name.to_owned(),
                        source: self.source.trim().to_owned(),
                    });
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if cancel || outcome.is_some() {
            *open = false;
        }
        outcome
    }
}

/// Selects areas by a list of keys, ranges and wildcards.
pub struct SelectListDialog {
    pub layer: Option<LayerId>,
    pub text: String,
}

impl SelectListDialog {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        open: &mut bool,
    ) -> Option<(LayerId, Vec<u32>)> {
        let mut outcome = None;
        let mut cancel = false;
        *open = modal(ctx, "select-list", "Select areas by list", 480.0, |ui| {
            ui.label(RichText::new("Keys of areas, separated by commas, semicolons or lines. Ranges like 8000-8099 and wildcards like 81* work too.").weak());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Areas");
                layer_combo(ui, "select-layer", project, &mut self.layer, None);
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.text)
                    .hint_text("8000-8099; 8105, 81*")
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            let units = self.layer.map(|layer| {
                let keys = layer_keys(project, layer);
                let index = territoria_data::KeyIndex::new(
                    keys.iter().map(|k| k.as_deref()),
                    MatchMode::Loose,
                );
                let mut u = territoria_model::plan::expand_unit_list(&self.text, &keys, &index);
                u.sort_unstable();
                u.dedup();
                (layer, u)
            });
            let n = units.as_ref().map_or(0, |(_, u)| u.len());
            ui.label(if self.text.trim().is_empty() {
                RichText::new(" ")
            } else if n == 0 {
                RichText::new("No area matches.").color(theme::WARN)
            } else {
                RichText::new(format!("{} areas match.", format_value(n as f64)))
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(n > 0, egui::Button::new(RichText::new("Select").strong()))
                    .clicked()
                {
                    outcome = units;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if cancel || outcome.is_some() {
            *open = false;
        }
        outcome
    }
}

/// Preferences: look, saving, new maps, stored data.
pub struct SettingsDialog {
    /// Size of the tile cache in bytes, measured on opening.
    tile_bytes: u64,
}

pub enum SettingsOutcome {
    ClearTiles,
}

fn folder_size(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .map(|e| match e.metadata() {
                Ok(m) if m.is_dir() => folder_size(&e.path()),
                Ok(m) => m.len(),
                Err(_) => 0,
            })
            .sum()
    })
}

impl SettingsDialog {
    pub fn new(tile_dir: &std::path::Path) -> SettingsDialog {
        let mut d = SettingsDialog { tile_bytes: 0 };
        d.measure_cache(tile_dir);
        d
    }

    pub fn measure_cache(&mut self, tile_dir: &std::path::Path) {
        self.tile_bytes = folder_size(tile_dir);
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        settings: &mut territoria_config::Settings,
        open: &mut bool,
    ) -> Option<SettingsOutcome> {
        use territoria_config::ThemeChoice;
        let mut outcome = None;
        let mut close = false;
        *open = modal(ctx, "settings", "Settings", 460.0, |ui| {
            egui::Grid::new("settings-grid")
                .num_columns(2)
                .spacing([14.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Theme");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut settings.theme, ThemeChoice::System, "Like the system");
                        ui.radio_value(&mut settings.theme, ThemeChoice::Light, "Light");
                        ui.radio_value(&mut settings.theme, ThemeChoice::Dark, "Dark");
                    });
                    ui.end_row();
                    ui.label("Size");
                    let mut pct = (settings.ui_scale * 100.0).round();
                    if ui
                        .add(egui::Slider::new(&mut pct, 60.0..=250.0).step_by(10.0).suffix(" %"))
                        .on_hover_text("Ctrl + and Ctrl − change it too")
                        .changed()
                    {
                        settings.ui_scale = pct / 100.0;
                    }
                    ui.end_row();
                    ui.label("Autosave");
                    ui.horizontal(|ui| {
                        ui.label("every");
                        ui.add(egui::DragValue::new(&mut settings.autosave_minutes).range(0..=60));
                        ui.label(if settings.autosave_minutes == 0 { "minutes (off)" } else { "minutes" });
                    })
                    .response
                    .on_hover_text("Unsaved changes are kept in a recovery copy, in case something goes wrong");
                    ui.end_row();
                    ui.label("New maps");
                    let current = settings.background.clone().unwrap_or_else(|| "No background".into());
                    ComboBox::from_id_salt("settings-background")
                        .width(240.0)
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(settings.background.is_none(), "No background").clicked() {
                                settings.background = None;
                            }
                            for p in crate::tiles::PRESETS {
                                if ui
                                    .selectable_label(settings.background.as_deref() == Some(p.name), p.name)
                                    .on_hover_text(p.note)
                                    .clicked()
                                {
                                    settings.background = Some(p.name.to_owned());
                                }
                            }
                        });
                    ui.end_row();
                    ui.label("Recent projects");
                    ui.horizontal(|ui| {
                        ui.label(format!("{}", settings.recent_projects.len()));
                        if ui
                            .add_enabled(!settings.recent_projects.is_empty(), egui::Button::new("Forget"))
                            .clicked()
                        {
                            settings.recent_projects.clear();
                        }
                    });
                    ui.end_row();
                    ui.label("Map tiles");
                    ui.horizontal(|ui| {
                        ui.label(format!("{:.1} MB stored", self.tile_bytes as f64 / 1_048_576.0));
                        if ui
                            .add_enabled(self.tile_bytes > 0, egui::Button::new("Clear"))
                            .on_hover_text("Background map tiles are kept on disk: maps load fast and also show offline")
                            .clicked()
                        {
                            outcome = Some(SettingsOutcome::ClearTiles);
                        }
                    });
                    ui.end_row();
                });
            ui.add_space(8.0);
            if ui.button("Close").clicked() {
                close = true;
            }
        });
        if close {
            *open = false;
        }
        outcome
    }
}

/// A new layer of equal hexagons over points or areas.
pub struct HexGridDialog {
    pub cover: Option<LayerId>,
    pub width_km: f64,
    pub count: Option<LayerId>,
    /// Cells for the spec they were computed for.
    estimate: Option<(
        territoria_model::grid::HexSpec,
        Result<(usize, f64), String>,
    )>,
}

impl HexGridDialog {
    pub fn new(project: &Project, layer: Option<LayerId>) -> HexGridDialog {
        let points = project
            .layers
            .iter()
            .find(|l| l.kind() == GeometryKind::Point)
            .map(|l| l.id);
        let cover = layer
            .or(points)
            .or_else(|| project.layers.first().map(|l| l.id));
        // About 30 cells across the extent, rounded to a nice size.
        let width_km = cover
            .and_then(|l| project.layer(l))
            .and_then(|l| l.geometry.bbox)
            .map_or(5.0, |b| {
                let km = territoria_geo::haversine(b.min(), b.max()) / 1000.0 / 30.0;
                let p = 10f64.powf(km.max(0.1).log10().floor());
                [1.0, 2.0, 5.0, 10.0]
                    .into_iter()
                    .map(|m| m * p)
                    .find(|v| *v >= km)
                    .unwrap_or(km)
            });
        HexGridDialog {
            cover,
            width_km,
            count: points,
            estimate: None,
        }
    }

    fn spec(&self, project: &Project) -> Option<territoria_model::grid::HexSpec> {
        use territoria_model::grid::{HexCover, HexSpec};
        let l = project.layer(self.cover?)?;
        Some(HexSpec {
            width_km: self.width_km,
            cover: if l.kind() == GeometryKind::Point {
                HexCover::Points(l.id)
            } else {
                HexCover::Areas(l.id)
            },
            count: self.count,
        })
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        open: &mut bool,
    ) -> Option<(territoria_model::grid::HexSpec, String)> {
        let mut outcome = None;
        let mut cancel = false;
        *open = modal(ctx, "hexgrid", "New hexagon grid", 480.0, |ui| {
            ui.label(RichText::new("Equal hexagons as areas: for density maps, and as units for territories where no boundaries fit.").weak());
            ui.add_space(6.0);
            egui::Grid::new("hex-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Over");
                    layer_combo(ui, "hex-cover", project, &mut self.cover, None);
                    ui.end_row();
                    ui.label("");
                    let points = self
                        .cover
                        .and_then(|l| project.layer(l))
                        .is_some_and(|l| l.kind() == GeometryKind::Point);
                    ui.label(
                        RichText::new(if points {
                            "Cells where there are points"
                        } else {
                            "Cells over the areas"
                        })
                        .weak(),
                    );
                    ui.end_row();
                    ui.label("Cell size");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.width_km)
                                .range(0.05..=1000.0)
                                .speed(0.1)
                                .suffix(" km"),
                        );
                        ui.label(RichText::new("across").weak());
                    });
                    ui.end_row();
                    ui.label("Count");
                    let text = self
                        .count
                        .and_then(|l| project.layer(l))
                        .map_or("Nothing".to_owned(), |l| l.name.clone());
                    ComboBox::from_id_salt("hex-count")
                        .selected_text(text)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.count, None, "Nothing");
                            for l in project
                                .layers
                                .iter()
                                .filter(|l| l.kind() == GeometryKind::Point)
                            {
                                ui.selectable_value(&mut self.count, Some(l.id), &l.name);
                            }
                        })
                        .response
                        .on_hover_text("Points counted per cell, their numbers summed");
                    ui.end_row();
                });
            let spec = self.spec(project);
            if let Some(spec) = &spec
                && self.estimate.as_ref().is_none_or(|(s, _)| s != spec)
            {
                let result = territoria_model::grid::hex_cells(project, spec)
                    .map(|(g, cells)| (cells.len(), g.cell_area_km2()));
                self.estimate = Some((spec.clone(), result));
            }
            ui.add_space(6.0);
            let ok = match &self.estimate {
                Some((_, Ok((n, area)))) => {
                    ui.label(format!(
                        "{} cells of {} km² each",
                        format_value(*n as f64),
                        format_value(*area)
                    ));
                    *n > 0 && *n <= 200_000
                }
                Some((_, Err(e))) => {
                    ui.label(RichText::new(e).color(theme::WARN));
                    false
                }
                None => false,
            };
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(ok, egui::Button::new(RichText::new("Create").strong()))
                    .clicked()
                    && let Some(spec) = spec
                {
                    let name = format!(
                        "Hexagons {} km",
                        territoria_data::table::format_number(self.width_km)
                    );
                    outcome = Some((spec, name));
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if cancel || outcome.is_some() {
            *open = false;
        }
        outcome
    }
}
