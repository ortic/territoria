//! `territoria mcp`: Territoria as a Model Context Protocol server, so an
//! AI agent (Claude Code, Claude Desktop, …) can build maps and plan
//! territories in project files: JSON-RPC over stdin and stdout, a line
//! per message. One project is open at a time; tools change it in memory
//! until `save_project`, and `render_map` shows the agent the result.
//!
//! ```text
//! claude mcp add territoria -- territoria mcp
//! ```

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use territoria_data::Table;
use territoria_model::{LayerId, PlanId, TableId, TerritoryId};

use crate::app::{DataRef, Export, TerritoriaApp};
use crate::dialogs::Colorable;
use crate::export_map::Format;

const INSTRUCTIONS: &str = "Territoria makes maps of areas (postcodes, municipalities, districts) from tables and plans sales territories on them. \
One project is open at a time: start with open_project or new_project, look at it with describe_project, and keep the changes with save_project. \
Typical flow: add_boundaries or download_boundaries; add_data (linked to the areas by a key column, or as points with longitude/latitude); \
list_values to see what areas can be coloured or balanced by; color_by; create_plan; plan_figures; assign or balance_plan; render_map to look; export. \
Layers, tables, plans, maps and territories are named; areas are given by their key (e.g. a postcode).";

/// Runs the server until stdin closes.
pub fn serve() -> i32 {
    let paths = territoria_config::Paths::detect().ok();
    let settings = paths
        .as_ref()
        .and_then(|p| territoria_config::load_toml(&p.settings_file()).ok())
        .unwrap_or_default();
    let mut server = Server {
        app: TerritoriaApp::with_settings(settings, paths.clone()),
        boundaries: paths.map_or_else(
            || std::env::temp_dir().join("territoria-boundaries"),
            |p| p.data_dir.join("boundaries"),
        ),
    };
    // The same services and keys as the app.
    server.app.load_keys();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => server.handle(&message),
            Err(e) => Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {"code": -32700, "message": format!("not JSON: {e}")},
            })),
        };
        if let Some(reply) = reply {
            let _ = writeln!(stdout, "{reply}");
            let _ = stdout.flush();
        }
    }
    0
}

struct Server {
    app: TerritoriaApp,
    /// Where downloaded boundaries are kept.
    boundaries: PathBuf,
}

/// What a tool gives back: text, or a picture with a caption.
enum Reply {
    Text(String),
    Image(String, String),
}

type ToolResult = Result<Reply, String>;

impl Server {
    /// Answers a request; notifications get no answer.
    fn handle(&mut self, message: &Value) -> Option<Value> {
        let id = message.get("id")?.clone();
        let method = message["method"].as_str().unwrap_or_default();
        let params = &message["params"];
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "territoria", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default();
                let empty = json!({});
                let args = params.get("arguments").unwrap_or(&empty);
                Ok(match self.call(name, args) {
                    Ok(Reply::Text(text)) => json!({
                        "content": [{"type": "text", "text": text}],
                    }),
                    Ok(Reply::Image(png, caption)) => json!({
                        "content": [
                            {"type": "image", "data": png, "mimeType": "image/png"},
                            {"type": "text", "text": caption},
                        ],
                    }),
                    Err(e) => json!({
                        "content": [{"type": "text", "text": e}],
                        "isError": true,
                    }),
                })
            }
            _ => Err(json!({"code": -32601, "message": format!("unknown method {method}")})),
        };
        Some(match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
        })
    }

    fn call(&mut self, name: &str, a: &Value) -> ToolResult {
        match name {
            "open_project" => self.open_project(a),
            "new_project" => {
                self.app.set_project(territoria_model::Project::new(), None);
                Ok(Reply::Text("A new, empty project.".into()))
            }
            "save_project" => self.save_project(a),
            "describe_project" => Ok(Reply::Text(self.describe())),
            "add_boundaries" => self.add_boundaries(a),
            "download_boundaries" => self.download_boundaries(a),
            "add_data" => self.add_data(a),
            "list_values" => self.list_values(a),
            "color_by" => self.color_by(a),
            "query" => self.query(a),
            "create_plan" => self.create_plan(a),
            "balance_plan" => self.balance_plan(a),
            "plan_figures" => self.plan_figures(a),
            "assign" => self.assign(a),
            "edit_territory" => self.edit_territory(a),
            "render_map" => self.render_map(a),
            "export" => self.export(a),
            "undo" => {
                let mut project = std::mem::take(&mut self.app.project);
                let label = self.app.history.undo(&mut project);
                self.app.project = project;
                Ok(Reply::Text(match label {
                    Some(l) => format!("Undid: {l}"),
                    None => "Nothing to undo.".into(),
                }))
            }
            _ => Err(format!("There's no tool “{name}”.")),
        }
    }

    // -- Finding things by name ------------------------------------------------

    fn layer(&self, a: &Value, key: &str) -> Result<LayerId, String> {
        let name = text(a, key)?;
        let p = &self.app.project;
        p.layers
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(&name))
            .map(|l| l.id)
            .ok_or_else(|| {
                let names: Vec<&str> = p.layers.iter().map(|l| l.name.as_str()).collect();
                format!("No layer “{name}”. Layers: {}", list(&names))
            })
    }

    fn table(&self, name: &str) -> Option<TableId> {
        self.app
            .project
            .tables
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .map(|t| t.id)
    }

    /// The named plan, or the only/first one.
    fn plan(&self, a: &Value) -> Result<PlanId, String> {
        let p = &self.app.project;
        match a.get("plan").and_then(Value::as_str) {
            Some(name) => p
                .plans
                .iter()
                .find(|pl| pl.name.eq_ignore_ascii_case(name))
                .map(|pl| pl.id)
                .ok_or_else(|| {
                    let names: Vec<&str> = p.plans.iter().map(|pl| pl.name.as_str()).collect();
                    format!("No plan “{name}”. Plans: {}", list(&names))
                }),
            None => p
                .plans
                .first()
                .map(|pl| pl.id)
                .ok_or("The project has no territory plan yet: create_plan makes one.".into()),
        }
    }

    fn territory(&self, plan: PlanId, name: &str) -> Result<TerritoryId, String> {
        let pl = self.app.project.plan(plan).ok_or("The plan was removed.")?;
        pl.territories
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name) || t.code.eq_ignore_ascii_case(name))
            .map(|t| t.id)
            .ok_or_else(|| {
                let names: Vec<String> = pl
                    .territories
                    .iter()
                    .map(|t| format!("{} ({})", t.name, t.code))
                    .collect();
                format!("No territory “{name}”. Territories: {}", names.join(", "))
            })
    }

    /// A number the areas of `layer` can be coloured or balanced by, by
    /// its label from `list_values`.
    fn value(&self, layer: LayerId, label: &str) -> Result<Colorable, String> {
        let options = crate::dialogs::colorables(&self.app.project, layer);
        options
            .iter()
            .find(|(l, _)| l.eq_ignore_ascii_case(label))
            .map(|(_, c)| c.clone())
            .ok_or_else(|| {
                let labels: Vec<&str> = options.iter().map(|(l, _)| l.as_str()).collect();
                format!(
                    "No value “{label}” for these areas. Values: {}",
                    list(&labels)
                )
            })
    }

    /// Runs background work (shapes, exports) to the end.
    fn finish(&mut self) {
        self.app.finish_jobs(Duration::from_secs(600));
    }

    /// Errors shown since `before` errors.
    fn new_errors(&self, before: usize) -> Option<String> {
        let errors = self.app.errors();
        (errors.len() > before).then(|| errors[before..].join("\n"))
    }

    // -- Tools -------------------------------------------------------------------

    fn open_project(&mut self, a: &Value) -> ToolResult {
        let path = PathBuf::from(text(a, "path")?);
        let project = territoria_model::file::load(&path)
            .map_err(|e| format!("Couldn't open {}: {e}", path.display()))?;
        self.app.set_project(project, Some(path));
        Ok(Reply::Text(self.describe()))
    }

    fn save_project(&mut self, a: &Value) -> ToolResult {
        let path = match a.get("path").and_then(Value::as_str) {
            Some(p) => PathBuf::from(p),
            None => self
                .app
                .path
                .clone()
                .ok_or("The project has no file yet: give a path.")?,
        };
        let path = if path.extension().is_none() {
            path.with_extension(territoria_model::file::EXTENSION)
        } else {
            path
        };
        self.app.store_workspace();
        territoria_model::file::save(&self.app.project, &path)
            .map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
        self.app.path = Some(path.clone());
        self.app.history.dirty = false;
        Ok(Reply::Text(format!("Saved {}", path.display())))
    }

    /// The project in short: what an agent needs to name things.
    fn describe(&mut self) -> String {
        let app = &mut self.app;
        let p = &app.project;
        let mut out = vec![format!(
            "Project “{}”{}",
            p.title,
            app.path
                .as_ref()
                .map_or(" (not saved yet)".to_owned(), |f| format!(
                    " at {}",
                    f.display()
                ))
        )];
        if p.layers.is_empty() {
            out.push("No layers yet: add_boundaries or download_boundaries.".into());
        }
        for l in &p.layers {
            let columns: Vec<String> = l
                .attributes
                .columns
                .iter()
                .map(|c| c.name.clone())
                .collect();
            out.push(format!(
                "Layer “{}”: {} {:?}, key column {}, name column {}; columns: {}",
                l.name,
                l.len(),
                l.kind(),
                l.key_column.as_deref().unwrap_or("–"),
                l.name_column.as_deref().unwrap_or("–"),
                columns.join(", ")
            ));
        }
        for t in &p.tables {
            let link = p.links.iter().find(|k| k.table == t.id).and_then(|k| {
                Some(format!(
                    ", linked to “{}” by {}",
                    p.layer(k.layer)?.name,
                    k.column
                ))
            });
            let columns: Vec<&str> = t.table.columns.iter().map(|c| c.name.as_str()).collect();
            out.push(format!(
                "Table “{}”: {} rows{}; columns: {}",
                t.name,
                t.table.rows(),
                link.unwrap_or_default(),
                columns.join(", ")
            ));
        }
        for pl in &p.plans {
            let measure = pl
                .measures
                .first()
                .map_or("areas".to_owned(), |m| m.source.label(p));
            let names: Vec<&str> = pl.territories.iter().map(|t| t.name.as_str()).collect();
            let unassigned = pl.assignment.iter().filter(|a| a.is_none()).count();
            out.push(format!(
                "Plan “{}” on “{}”, balanced by {measure}: {} territories ({}); {unassigned} areas unassigned",
                pl.name,
                p.layer(pl.layer).map_or("?", |l| l.name.as_str()),
                pl.territories.len(),
                names.join(", ")
            ));
        }
        for m in &p.maps {
            let layers: Vec<&str> = m.layers.iter().map(|l| l.name.as_str()).collect();
            out.push(format!(
                "Map “{}”, layers bottom to top: {}",
                m.name,
                layers.join(", ")
            ));
        }
        out.join("\n")
    }

    fn add_boundaries(&mut self, a: &Value) -> ToolResult {
        let path = PathBuf::from(text(a, "path")?);
        let added = self.app.add_boundaries_now(&path)?;
        self.tidy_layers(&added, a);
        self.finish();
        Ok(Reply::Text(format!(
            "Added {}.\n\n{}",
            self.layer_names(&added),
            self.describe()
        )))
    }

    /// Names and key columns as asked for.
    fn tidy_layers(&mut self, added: &[LayerId], a: &Value) {
        for &id in added {
            let Some(l) = self.app.project.layer_mut(id) else {
                continue;
            };
            if let Some(name) = a.get("name").and_then(Value::as_str)
                && added.len() == 1
            {
                l.name = name.to_owned();
            }
            for (key, column) in [
                ("key_column", &mut l.key_column),
                ("name_column", &mut l.name_column),
            ] {
                if let Some(c) = a.get(key).and_then(Value::as_str)
                    && l.attributes.column(c).is_some()
                {
                    *column = Some(c.to_owned());
                }
            }
            self.name_map_layers(id);
        }
    }

    /// Map layers drawing a layer take its name.
    fn name_map_layers(&mut self, id: LayerId) {
        let Some(name) = self.app.project.layer(id).map(|l| l.name.clone()) else {
            return;
        };
        for m in &mut self.app.project.maps {
            for ml in &mut m.layers {
                if ml.kind.layer() == Some(id) && ml.kind.plan().is_none() {
                    ml.name = name.clone();
                }
            }
        }
    }

    fn layer_names(&self, ids: &[LayerId]) -> String {
        let names: Vec<String> = ids
            .iter()
            .filter_map(|id| self.app.project.layer(*id))
            .map(|l| format!("“{}” ({} features)", l.name, l.len()))
            .collect();
        names.join(", ")
    }

    fn download_boundaries(&mut self, a: &Value) -> ToolResult {
        let country = text(a, "country")?;
        let level = a
            .get("level")
            .and_then(Value::as_str)
            .unwrap_or("ADM1")
            .to_uppercase();
        let catalog = territoria_io::online::catalog(&self.boundaries)
            .map_err(|e| format!("Couldn't load the list of boundaries: {e}"))?;
        let sets: Vec<_> = catalog
            .iter()
            .filter(|s| {
                s.iso.eq_ignore_ascii_case(&country) || s.country.eq_ignore_ascii_case(&country)
            })
            .collect();
        if sets.is_empty() {
            return Err(format!(
                "No boundaries for “{country}”: give the country's English name or ISO code (CHE, DEU…)."
            ));
        }
        let Some(set) = sets.iter().find(|s| s.level == level) else {
            let levels: Vec<String> = sets
                .iter()
                .map(|s| format!("{} ({}, {} areas)", s.level, s.kind, s.units))
                .collect();
            return Err(format!(
                "{} has no {level}. Levels: {}",
                sets[0].country,
                levels.join(", ")
            ));
        };
        let file = territoria_io::online::download(set, true, &self.boundaries, &mut |_, _| true)
            .map_err(|e| format!("Couldn't download {} {}: {e}", set.country, set.level))?;
        let added = self.app.add_boundaries_now(&file)?;
        for &id in &added {
            if let Some(l) = self.app.project.layer_mut(id) {
                l.name = set.title();
                l.source.attribution = Some(set.attribution());
            }
            self.name_map_layers(id);
        }
        self.tidy_layers(&added, a);
        self.finish();
        Ok(Reply::Text(format!(
            "Added {} ({}; licence: {}).\n\n{}",
            self.layer_names(&added),
            set.source,
            set.license,
            self.describe()
        )))
    }

    fn add_data(&mut self, a: &Value) -> ToolResult {
        use crate::dialogs::Placement;
        let path = PathBuf::from(text(a, "path")?);
        let options = territoria_io::TableOptions {
            sheet: a.get("sheet").and_then(Value::as_str).map(str::to_owned),
            ..Default::default()
        };
        let (table, _) = territoria_io::read_table(&path, &options)
            .map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
        let name = a
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                path.file_stem()
                    .map_or("Data".into(), |s| s.to_string_lossy().into_owned())
            });
        let errors = self.app.errors().len();
        // Points from coordinates.
        if let (Some(x), Some(y)) = (
            a.get("longitude").and_then(Value::as_str),
            a.get("latitude").and_then(Value::as_str),
        ) {
            for c in [x, y] {
                if table.column(c).is_none() {
                    return Err(format!("{} has no column “{c}”.", path.display()));
                }
            }
            self.app.add_table(
                name.clone(),
                table,
                path,
                Placement::Points {
                    x: x.to_owned(),
                    y: y.to_owned(),
                },
                None,
            );
            self.finish();
            if let Some(e) = self.new_errors(errors) {
                return Err(e);
            }
            return Ok(Reply::Text(format!(
                "Added the points “{name}”.\n\n{}",
                self.describe()
            )));
        }
        // A table linked to areas by a key column.
        let before = self.app.project.clone();
        let id = self.app.project.add_table(
            &name,
            table,
            territoria_model::SourceInfo {
                path: Some(path),
                ..Default::default()
            },
        );
        let mut link = crate::dialogs::LinkDialog::new(&self.app.project, id);
        if a.get("layer").is_some() {
            link.layer = Some(self.layer(a, "layer")?);
        }
        if let Some(c) = a.get("key_column").and_then(Value::as_str) {
            link.column = Some(c.to_owned());
        }
        link.rematch(&self.app.project, true);
        let (Some(layer), Some(column), Some(m)) =
            (link.layer, link.column.clone(), &link.matching)
        else {
            // Kept unlinked: the data is still there to look at.
            self.app.history.push("Add data", before);
            return Ok(Reply::Text(format!(
                "Added the table “{name}”, but found no column whose values match an area layer's keys. Give layer and key_column to link it, or longitude and latitude for points."
            )));
        };
        let stats = m.stats.clone();
        let mode = m.mode;
        self.app.history.push("Add data", before);
        self.app.apply_link(crate::dialogs::LinkOutcome::Link {
            table: id,
            layer,
            column: column.clone(),
            mode,
            color_by: None,
        });
        self.finish();
        Ok(Reply::Text(format!(
            "Added “{name}”, linked to “{}” by {column}: {} of {} rows match an area ({} areas have data).",
            self.app
                .project
                .layer(layer)
                .map_or("?", |l| l.name.as_str()),
            stats.rows_matched,
            stats.rows,
            stats.targets_matched,
        )))
    }

    fn list_values(&mut self, a: &Value) -> ToolResult {
        let layer = self.layer(a, "layer")?;
        let options = crate::dialogs::colorables(&self.app.project, layer);
        let mut numbers = Vec::new();
        let mut categories = Vec::new();
        for (label, c) in options {
            match c {
                Colorable::Number(_) => numbers.push(label),
                Colorable::Category(_) => categories.push(label),
            }
        }
        Ok(Reply::Text(format!(
            "Numbers (to colour by or balance territories by): {}\nCategories (to colour by): {}",
            list(&numbers),
            list(&categories)
        )))
    }

    fn color_by(&mut self, a: &Value) -> ToolResult {
        let layer = self.layer(a, "layer")?;
        let label = text(a, "value")?;
        let what = self.value(layer, &label)?;
        let circles = a.get("circles").and_then(Value::as_bool).unwrap_or(false);
        self.app
            .apply_color_by(crate::dialogs::ColorByOutcome::Create {
                layer,
                name: label.clone(),
                what,
                circles,
                heat: false,
            });
        self.finish();
        Ok(Reply::Text(format!(
            "The map shows {label} {}. render_map to look at it.",
            if circles { "as circles" } else { "in colours" }
        )))
    }

    fn query(&mut self, a: &Value) -> ToolResult {
        let name = text(a, "data")?;
        let (data, table): (DataRef, &Table) = if let Some(t) = self.table(&name) {
            (
                DataRef::Table(t),
                &self.app.project.table(t).ok_or("gone")?.table,
            )
        } else {
            let l = self.layer(a, "data")?;
            (
                DataRef::Layer(l),
                &self.app.project.layer(l).ok_or("gone")?.attributes,
            )
        };
        let _ = data;
        let columns: Vec<usize> = match a.get("columns").and_then(Value::as_array) {
            Some(names) => names
                .iter()
                .filter_map(Value::as_str)
                .map(|n| {
                    table
                        .column_index(n)
                        .ok_or(format!("“{name}” has no column “{n}”."))
                })
                .collect::<Result<_, _>>()?,
            None => (0..table.columns.len()).collect(),
        };
        let find = a
            .get("find")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();
        let mut rows: Vec<usize> = (0..table.rows())
            .filter(|&r| {
                find.is_empty()
                    || table
                        .columns
                        .iter()
                        .any(|c| c.display(r).to_lowercase().contains(&find))
            })
            .collect();
        if let Some(sort) = a.get("sort").and_then(Value::as_str) {
            let c = table
                .column(sort)
                .ok_or(format!("“{name}” has no column “{sort}”."))?;
            if c.kind.is_numeric() {
                rows.sort_by(|&x, &y| {
                    c.f64(x)
                        .partial_cmp(&c.f64(y))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            } else {
                rows.sort_by_key(|&r| c.display(r).to_lowercase());
            }
            if a.get("descending")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                rows.reverse();
            }
        }
        let total = rows.len();
        let offset = a.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let limit = a
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .min(1000) as usize;
        let mut out = String::new();
        let header: Vec<&str> = columns
            .iter()
            .map(|&c| table.columns[c].name.as_str())
            .collect();
        out.push_str(&header.join("\t"));
        for &r in rows.iter().skip(offset).take(limit) {
            out.push('\n');
            let cells: Vec<String> = columns
                .iter()
                .map(|&c| table.columns[c].display(r))
                .collect();
            out.push_str(&cells.join("\t"));
        }
        let shown = total.saturating_sub(offset).min(limit);
        out.push_str(&format!(
            "\n({shown} of {total} matching rows{})",
            if total == table.rows() {
                String::new()
            } else {
                format!("; {} rows in all", table.rows())
            }
        ));
        Ok(Reply::Text(out))
    }

    fn create_plan(&mut self, a: &Value) -> ToolResult {
        let layer = self.layer(a, "layer")?;
        let count = a
            .get("territories")
            .and_then(Value::as_u64)
            .ok_or("How many territories? Give territories.")?
            .clamp(1, 500) as usize;
        let name = a
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Territories")
            .to_owned();
        let measure = match a.get("balance_by").and_then(Value::as_str) {
            Some(label) => match self.value(layer, label)? {
                Colorable::Number(source) => Some(source),
                Colorable::Category(_) => {
                    return Err(format!("“{label}” isn't a number to balance by."));
                }
            },
            None => None,
        };
        let before = self.app.project.clone();
        let plan = self.app.project.add_plan(&name, layer, count);
        if let Some(source) = measure
            && let Some(p) = self.app.project.plan_mut(plan)
        {
            p.measures[0].source = source;
        }
        if let Some(t) = a.get("tolerance").and_then(Value::as_f64)
            && let Some(p) = self.app.project.plan_mut(plan)
        {
            p.measures[0].tolerance = t.clamp(0.0, 1.0);
        }
        self.app.apply_default_palette(plan);
        self.app.history.push("New territory plan", before);
        self.app.show_plan(plan);
        if a.get("balance").and_then(Value::as_bool).unwrap_or(true) {
            self.balance(plan, true, a)?;
        }
        self.finish();
        Ok(Reply::Text(self.figures(plan)))
    }

    fn balance_plan(&mut self, a: &Value) -> ToolResult {
        let plan = self.plan(a)?;
        let start_over = a
            .get("start_over")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let moved = self.balance(plan, start_over, a)?;
        Ok(Reply::Text(format!(
            "Moved {moved} areas.\n\n{}",
            self.figures(plan)
        )))
    }

    /// Balances `plan`, colouring its territories afresh; how many areas
    /// moved.
    fn balance(&mut self, plan: PlanId, start_over: bool, a: &Value) -> Result<usize, String> {
        let mut settings = crate::territories::BalanceSettings {
            start_over,
            ..Default::default()
        };
        if let Some(c) = a.get("compactness").and_then(Value::as_f64) {
            settings.compactness = c.clamp(0.0, 1.0);
        }
        let input = crate::territories::balance_input(
            &self.app.project,
            &mut self.app.values,
            plan,
            &settings,
        )
        .ok_or("This plan can't be balanced (no areas, or its layer was removed).")?;
        let proposal = crate::territories::run_balance(input, &mut |_| true)?;
        let before = self.app.project.clone();
        let rev = self.app.project.bump();
        let geometry = self
            .app
            .project
            .plan(plan)
            .and_then(|p| self.app.project.layer(p.layer))
            .map(|l| l.geometry.clone());
        let p = self
            .app
            .project
            .plan_mut(plan)
            .ok_or("The plan was removed.")?;
        for (u, t) in &proposal.moves {
            p.assign(&[*u], Some(*t));
        }
        if start_over && let Some(g) = &geometry {
            p.recolor(g);
        }
        p.rev = rev;
        self.app.history.push("Balance territories", before);
        Ok(proposal.moves.len())
    }

    fn plan_figures(&mut self, a: &Value) -> ToolResult {
        let plan = self.plan(a)?;
        Ok(Reply::Text(self.figures(plan)))
    }

    /// Each territory's figures, as a small table.
    fn figures(&mut self, plan: PlanId) -> String {
        let app = &mut self.app;
        let Some(p) = app.project.plan(plan) else {
            return "The plan was removed.".into();
        };
        let stats = p.stats(&app.project, &mut app.values);
        let measure = p
            .measures
            .first()
            .map_or("areas".to_owned(), |m| m.source.label(&app.project));
        let tolerance = p.measures.first().map_or(0.15, |m| m.tolerance);
        let mut out = format!(
            "Plan “{}”, balanced by {measure} (±{:.0} %):\nCode\tTerritory\tAreas\tkm²\t{measure}\tTarget\tDeviation\tPieces",
            p.name,
            tolerance * 100.0
        );
        let fmt = territoria_analysis::format::format_value;
        for (t, s) in p.territories.iter().zip(&stats.territories) {
            out.push_str(&format!(
                "\n{}\t{}\t{}\t{}\t{}\t{}\t{:+.1} %\t{}",
                t.code,
                t.name,
                s.units,
                fmt(s.area),
                s.values.first().map_or("–".into(), |v| fmt(*v)),
                s.targets.first().map_or("–".into(), |v| fmt(*v)),
                s.deviations.first().copied().unwrap_or(0.0) * 100.0,
                s.parts
            ));
        }
        let outside = stats
            .territories
            .iter()
            .filter(|s| s.deviations.first().is_some_and(|d| d.abs() > tolerance))
            .count();
        out.push_str(&format!(
            "\n{} areas unassigned; {outside} of {} territories outside the tolerance.",
            stats.unassigned,
            p.territories.len()
        ));
        out
    }

    fn assign(&mut self, a: &Value) -> ToolResult {
        let plan = self.plan(a)?;
        let to = match a.get("territory").and_then(Value::as_str) {
            Some(name) => Some(self.territory(plan, name)?),
            None => None,
        };
        let keys: Vec<String> = a
            .get("areas")
            .and_then(Value::as_array)
            .ok_or("Which areas? Give areas as a list of keys.")?
            .iter()
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect();
        let p = self.app.project.plan(plan).ok_or("The plan was removed.")?;
        let layer = self
            .app
            .project
            .layer(p.layer)
            .ok_or("The layer was removed.")?;
        let key = layer
            .key_column
            .as_deref()
            .and_then(|k| layer.attributes.column(k))
            .ok_or("The plan's layer has no key column.")?;
        let index: std::collections::HashMap<String, u32> =
            (0..key.len()).map(|r| (key.display(r), r as u32)).collect();
        let mut units = Vec::new();
        let mut unknown = Vec::new();
        for k in &keys {
            match index.get(k.trim()) {
                Some(&u) => units.push(u),
                None => unknown.push(k.as_str()),
            }
        }
        let before = self.app.project.clone();
        let rev = self.app.project.bump();
        let p = self
            .app
            .project
            .plan_mut(plan)
            .ok_or("The plan was removed.")?;
        let changed = p.assign(&units, to);
        p.rev = rev;
        self.app.history.push(
            if to.is_some() {
                "Assign areas"
            } else {
                "Unassign areas"
            },
            before,
        );
        let mut text = format!("Changed {changed} areas.");
        if !unknown.is_empty() {
            text.push_str(&format!(" No area has the key {}.", list(&unknown)));
        }
        Ok(Reply::Text(format!("{text}\n\n{}", self.figures(plan))))
    }

    fn edit_territory(&mut self, a: &Value) -> ToolResult {
        let plan = self.plan(a)?;
        let before = self.app.project.clone();
        let id = match a.get("territory").and_then(Value::as_str) {
            Some(name) => self.territory(plan, name)?,
            None => {
                let p = self
                    .app
                    .project
                    .plan_mut(plan)
                    .ok_or("The plan was removed.")?;
                p.add_territory(None, None)
            }
        };
        let color = match a.get("color").and_then(Value::as_str) {
            Some(c) => Some(parse_color(c).ok_or(format!("“{c}” isn't a colour like #3a7bd5."))?),
            None => None,
        };
        let rev = self.app.project.bump();
        let p = self
            .app
            .project
            .plan_mut(plan)
            .ok_or("The plan was removed.")?;
        if a.get("remove").and_then(Value::as_bool).unwrap_or(false) {
            p.remove_territory(id);
            p.rev = rev;
            self.app.history.push("Remove territory", before);
            return Ok(Reply::Text(self.figures(plan)));
        }
        let t = p.territory_mut(id).ok_or("The territory was removed.")?;
        if let Some(n) = a.get("name").and_then(Value::as_str) {
            t.name = n.to_owned();
        }
        if let Some(c) = a.get("code").and_then(Value::as_str) {
            t.code = c.to_owned();
        }
        if let Some(c) = color {
            t.color = c;
        }
        if let Some(w) = a.get("weight").and_then(Value::as_f64) {
            t.weight = w.clamp(0.0, 10.0);
        }
        if let Some(l) = a.get("locked").and_then(Value::as_bool) {
            t.locked = l;
        }
        if let Some(n) = a.get("notes").and_then(Value::as_str) {
            t.notes = n.to_owned();
        }
        p.rev = rev;
        self.app.history.push("Edit territory", before);
        Ok(Reply::Text(self.figures(plan)))
    }

    fn use_map(&mut self, a: &Value) -> Result<(), String> {
        let p = &self.app.project;
        let map = match a.get("map").and_then(Value::as_str) {
            Some(name) => {
                p.maps
                    .iter()
                    .find(|m| m.name.eq_ignore_ascii_case(name))
                    .ok_or_else(|| {
                        let names: Vec<&str> = p.maps.iter().map(|m| m.name.as_str()).collect();
                        format!("No map “{name}”. Maps: {}", list(&names))
                    })?
                    .id
            }
            None => {
                p.maps
                    .first()
                    .ok_or("The project has no map yet: add boundaries first.")?
                    .id
            }
        };
        self.app.use_map(map);
        Ok(())
    }

    fn export_settings(&self, a: &Value, format: Format) -> crate::dialogs::MapExportSettings {
        crate::dialogs::MapExportSettings {
            format,
            scale: 1.0,
            title: a
                .get("title")
                .and_then(Value::as_str)
                .map_or_else(|| self.app.project.title.clone(), str::to_owned),
            legend: a.get("legend").and_then(Value::as_bool).unwrap_or(true),
            scale_bar: true,
            page: Default::default(),
            footer: String::new(),
            overview: false,
            figures: false,
            layout: None,
        }
    }

    fn render_map(&mut self, a: &Value) -> ToolResult {
        self.use_map(a)?;
        // The map's own size is 1600 × 1000 points.
        let width = a
            .get("width")
            .and_then(Value::as_f64)
            .unwrap_or(1200.0)
            .clamp(200.0, 3200.0);
        let mut settings = self.export_settings(a, Format::Png);
        settings.scale = (width / 1600.0) as f32;
        let (svg, tiles) = loop {
            match self.app.map_svg(&settings) {
                Ok(svg) => break svg,
                Err(e) if e.contains("prepared") => {
                    self.app.finish_jobs(Duration::from_millis(200));
                }
                Err(e) => return Err(e),
            }
        };
        let (background, missing) = tiles.map_or((String::new(), 0), |t| t.svg());
        let svg = svg.replacen(crate::export_map::BACKGROUND, &background, 1);
        let font = egui::FontDefinitions::default().font_data["Ubuntu-Light"]
            .font
            .to_vec();
        let image = crate::export_map::rasterize(&svg, settings.scale, &font)?;
        let uri = crate::heat::png_data_uri(&image).ok_or("Couldn't encode the picture.")?;
        let png = uri.trim_start_matches("data:image/png;base64,").to_owned();
        let mut caption = format!(
            "The map “{}”, {}×{} pixels.",
            self.app
                .current_map()
                .and_then(|m| self.app.project.map(m))
                .map_or("", |m| m.name.as_str()),
            image.size[0],
            image.size[1]
        );
        if missing > 0 {
            caption.push_str(&format!(" {missing} background tiles couldn't be loaded."));
        }
        Ok(Reply::Image(png, caption))
    }

    fn export(&mut self, a: &Value) -> ToolResult {
        let what = text(a, "what")?;
        let path = PathBuf::from(text(a, "path")?);
        let data = |s: &Self| -> Result<DataRef, String> {
            let name = text(a, "data")?;
            match s.table(&name) {
                Some(t) => Ok(DataRef::Table(t)),
                None => s.layer(a, "data").map(DataRef::Layer),
            }
        };
        let export = match what.as_str() {
            "map_png" | "map_svg" | "map_pdf" => {
                self.use_map(a)?;
                let format = match what.as_str() {
                    "map_png" => Format::Png,
                    "map_svg" => Format::Svg,
                    _ => Format::Pdf,
                };
                let mut settings = self.export_settings(a, format);
                settings.scale = 2.0;
                // Wait for the map's shapes before handing over.
                while let Err(e) = self.app.map_svg(&settings) {
                    if !e.contains("prepared") {
                        return Err(e);
                    }
                    self.app.finish_jobs(Duration::from_millis(200));
                }
                self.app.set_map_export_settings(settings);
                Export::Map(format)
            }
            "web_page" => {
                self.use_map(a)?;
                Export::Html
            }
            "atlas" => {
                self.use_map(a)?;
                let layer = self.layer(a, "layer")?;
                Export::Atlas(crate::app::AtlasSpec::new(
                    layer,
                    a.get("per").and_then(Value::as_str),
                ))
            }
            "report" => Export::Report(self.plan(a)?),
            "plan_workbook" => Export::PlanExcel(self.plan(a)?),
            "plan_csv" => Export::PlanCsv(self.plan(a)?),
            "plan_geojson" => Export::PlanGeoJson(self.plan(a)?),
            "data_csv" => Export::Data(data(self)?, false, None),
            "data_xlsx" => Export::Data(data(self)?, true, None),
            other => return Err(format!("Can't export “{other}”.")),
        };
        let errors = self.app.errors().len();
        self.app.export(export, path.clone());
        self.finish();
        match self.new_errors(errors) {
            Some(e) => Err(e),
            None => Ok(Reply::Text(format!("Wrote {}", path.display()))),
        }
    }
}

fn text(a: &Value, key: &str) -> Result<String, String> {
    a.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(format!("Missing “{key}”."))
}

fn list<S: AsRef<str>>(items: &[S]) -> String {
    if items.is_empty() {
        return "none".into();
    }
    items
        .iter()
        .map(|s| format!("“{}”", s.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `#rrggbb` (or without `#`).
fn parse_color(s: &str) -> Option<territoria_analysis::Rgba> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(territoria_analysis::Rgba([
        (v >> 16) as u8,
        (v >> 8) as u8,
        v as u8,
        255,
    ]))
}

/// The tools with their arguments as JSON schemas.
fn tools() -> Value {
    let s = |p: Value, required: &[&str]| json!({"type": "object", "properties": p, "required": required});
    let name = |what: &str| json!({"type": "string", "description": what});
    json!([
        {"name": "open_project", "description": "Opens a Territoria project file (.territoria) and describes it.",
         "inputSchema": s(json!({"path": name("The project file")}), &["path"])},
        {"name": "new_project", "description": "Starts a new, empty project (unsaved changes to the open one are dropped).",
         "inputSchema": s(json!({}), &[])},
        {"name": "save_project", "description": "Saves the project, to its file or to path.",
         "inputSchema": s(json!({"path": name("Where to save (.territoria); default: where it was opened from")}), &[])},
        {"name": "describe_project", "description": "Lists the project's layers (with their columns), tables, plans and maps.",
         "inputSchema": s(json!({}), &[])},
        {"name": "add_boundaries", "description": "Adds areas or points from a Shapefile (or .zip), GeoJSON, GeoPackage, KML or FlatGeobuf file, with a map.",
         "inputSchema": s(json!({
            "path": name("The file"),
            "name": name("The layer's name"),
            "key_column": name("The column identifying each area (e.g. the postcode); guessed when not given"),
            "name_column": name("The column with each area's name")}), &["path"])},
        {"name": "download_boundaries", "description": "Downloads open administrative boundaries of a country (geoBoundaries) and adds them.",
         "inputSchema": s(json!({
            "country": name("English name or ISO 3166 alpha-3 code, e.g. Switzerland or CHE"),
            "level": name("ADM0 (country) to ADM5; ADM1 = states/cantons (default), ADM2 = districts…")}), &["country"])},
        {"name": "add_data", "description": "Adds a CSV or Excel table: linked to an area layer by a key column (best match guessed), or as points when longitude and latitude columns are given.",
         "inputSchema": s(json!({
            "path": name("The CSV or Excel file"),
            "name": name("The table's name"),
            "sheet": name("Excel: the sheet"),
            "layer": name("The area layer to link to"),
            "key_column": name("The table's column with the area keys"),
            "longitude": name("Column with longitudes (makes points)"),
            "latitude": name("Column with latitudes (makes points)")}), &["path"])},
        {"name": "list_values", "description": "What a layer's areas can be coloured or balanced by: their columns, linked tables' sums and counts, points inside.",
         "inputSchema": s(json!({"layer": name("The layer")}), &["layer"])},
        {"name": "color_by", "description": "Colours a layer's areas by a value from list_values (classes and legend chosen automatically), or draws circles sized by it.",
         "inputSchema": s(json!({
            "layer": name("The layer"),
            "value": name("A label from list_values"),
            "circles": {"type": "boolean", "description": "Circles sized by the value instead of colours"}}), &["layer", "value"])},
        {"name": "query", "description": "Rows of a table or a layer's attributes: some columns, found by text, sorted.",
         "inputSchema": s(json!({
            "data": name("A table's or layer's name"),
            "columns": {"type": "array", "items": {"type": "string"}, "description": "Columns to show (default: all)"},
            "find": name("Only rows containing this text"),
            "sort": name("Column to sort by"),
            "descending": {"type": "boolean"},
            "limit": {"type": "integer", "description": "At most this many rows (default 50)"},
            "offset": {"type": "integer"}}), &["data"])},
        {"name": "create_plan", "description": "Creates a territory plan on an area layer: a number of territories, balanced by a value from list_values (compact and contiguous), shown on the map. Answers with each territory's figures.",
         "inputSchema": s(json!({
            "layer": name("The area layer"),
            "territories": {"type": "integer", "description": "How many territories"},
            "name": name("The plan's name"),
            "balance_by": name("A number from list_values (default: number of areas)"),
            "tolerance": {"type": "number", "description": "Accepted deviation, e.g. 0.1 for ±10 %"},
            "balance": {"type": "boolean", "description": "Balance right away (default true)"},
            "compactness": {"type": "number", "description": "0–1: how much compact shapes count"}}), &["layer", "territories"])},
        {"name": "balance_plan", "description": "Balances a plan's territories by its measure, keeping locked ones; start_over rebuilds them.",
         "inputSchema": s(json!({
            "plan": name("The plan (default: the first)"),
            "start_over": {"type": "boolean"},
            "compactness": {"type": "number", "description": "0–1"}}), &[])},
        {"name": "plan_figures", "description": "Each territory's areas, km², measure, target and deviation.",
         "inputSchema": s(json!({"plan": name("The plan (default: the first)")}), &[])},
        {"name": "assign", "description": "Assigns areas (by their keys) to a territory, or unassigns them without territory.",
         "inputSchema": s(json!({
            "plan": name("The plan (default: the first)"),
            "areas": {"type": "array", "items": {"type": "string"}, "description": "Area keys, e.g. postcodes"},
            "territory": name("Territory name or code; leave out to unassign")}), &["areas"])},
        {"name": "edit_territory", "description": "Renames, recolours, resizes, locks or removes a territory; without territory, adds one.",
         "inputSchema": s(json!({
            "plan": name("The plan (default: the first)"),
            "territory": name("Name or code; leave out to add a territory"),
            "name": name("New name"),
            "code": name("New code"),
            "color": name("#rrggbb"),
            "weight": {"type": "number", "description": "Relative size: 1 normal, 0.5 half"},
            "locked": {"type": "boolean", "description": "Locked territories keep their areas when balancing"},
            "notes": name("Notes"),
            "remove": {"type": "boolean"}}), &[])},
        {"name": "render_map", "description": "Draws the map as a picture (with legend and background map) to look at.",
         "inputSchema": s(json!({
            "map": name("The map (default: the first)"),
            "width": {"type": "number", "description": "Pixels (default 1200)"},
            "title": name("A title on the map"),
            "legend": {"type": "boolean"}}), &[])},
        {"name": "export", "description": "Writes a file: map_png, map_svg, map_pdf, web_page (interactive HTML), atlas (PDF, a page per area or per value of column per), report (PDF per territory), plan_workbook (xlsx), plan_csv, plan_geojson, data_csv, data_xlsx.",
         "inputSchema": s(json!({
            "what": {"type": "string", "enum": ["map_png", "map_svg", "map_pdf", "web_page", "atlas", "report", "plan_workbook", "plan_csv", "plan_geojson", "data_csv", "data_xlsx"]},
            "path": name("The file to write"),
            "map": name("Maps: which map"),
            "title": name("Maps: the title"),
            "plan": name("Plans and reports: which plan"),
            "layer": name("Atlas: the layer of its pages"),
            "per": name("Atlas: a page per value of this column"),
            "data": name("Data: the table or layer")}), &["what", "path"])},
        {"name": "undo", "description": "Undoes the last change.", "inputSchema": s(json!({}), &[])},
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server {
            app: TerritoriaApp::with_settings(Default::default(), None),
            boundaries: std::env::temp_dir(),
        }
    }

    fn call(s: &mut Server, name: &str, args: Value) -> Value {
        s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": args}}))
            .unwrap()["result"]
            .clone()
    }

    fn text_of(result: &Value) -> String {
        result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    #[test]
    fn speaks_mcp() {
        let mut s = server();
        let init = s
            .handle(&json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}))
            .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "territoria");
        assert!(
            s.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
                .is_none()
        );
        let tools = s
            .handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
            .unwrap();
        let names: Vec<&str> = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"render_map") && names.contains(&"create_plan"));
        let unknown = s
            .handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "nope"}))
            .unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    /// The sample project through the tools: values, a plan, assigning,
    /// a picture, an export, saving.
    #[test]
    fn plans_territories_through_tools() {
        let dir = std::env::temp_dir().join(format!("territoria-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sample = dir.join("sample.territoria");
        territoria_model::file::save(&crate::sample::project().unwrap(), &sample).unwrap();
        let mut s = server();
        let r = call(&mut s, "open_project", json!({"path": sample}));
        assert!(
            text_of(&r).contains("Layer “Cantons”: 26"),
            "{}",
            text_of(&r)
        );

        let r = call(&mut s, "list_values", json!({"layer": "cantons"}));
        let values = text_of(&r);
        assert!(values.contains("Numbers"), "{values}");
        let sales = crate::dialogs::colorables(&s.app.project, s.app.project.layers[0].id)
            .into_iter()
            .find(|(l, c)| l.contains("Sales") && matches!(c, Colorable::Number(_)))
            .unwrap()
            .0;
        assert!(values.contains(&sales));

        let r = call(
            &mut s,
            "create_plan",
            json!({"layer": "Cantons", "territories": 3, "name": "Three", "balance_by": sales}),
        );
        let figures = text_of(&r);
        assert!(figures.contains("Plan “Three”"), "{figures}");
        assert!(figures.contains("0 areas unassigned"), "{figures}");

        let r = call(
            &mut s,
            "edit_territory",
            json!({"plan": "Three", "territory": "01", "name": "West", "color": "#3a7bd5"}),
        );
        assert!(text_of(&r).contains("West"), "{}", text_of(&r));
        let r = call(
            &mut s,
            "assign",
            json!({"plan": "Three", "areas": ["GE", "VD", "XX"], "territory": "West"}),
        );
        let t = text_of(&r);
        assert!(t.contains("No area has the key “XX”"), "{t}");
        let plan = s
            .app
            .project
            .plans
            .iter()
            .find(|p| p.name == "Three")
            .unwrap();
        let west = plan
            .territories
            .iter()
            .find(|t| t.name == "West")
            .unwrap()
            .id;
        let geneva = (0..26)
            .find(|&r| {
                s.app.project.layers[0]
                    .attributes
                    .column("Code")
                    .unwrap()
                    .display(r)
                    == "GE"
            })
            .unwrap();
        assert_eq!(plan.assignment[geneva], Some(west));

        let r = call(&mut s, "undo", json!({}));
        assert!(text_of(&r).starts_with("Undid"));

        let r = call(
            &mut s,
            "query",
            json!({"data": "Customers", "columns": ["Name", "Town", "Sales"], "sort": "Sales", "descending": true, "limit": 3}),
        );
        let rows = text_of(&r);
        assert_eq!(rows.lines().count(), 5, "header, three rows, count: {rows}");

        let r = call(&mut s, "render_map", json!({"width": 400}));
        assert_eq!(r["content"][0]["type"], "image");
        assert!(r["content"][0]["data"].as_str().unwrap().len() > 1000);

        let csv = dir.join("plan.csv");
        let r = call(
            &mut s,
            "export",
            json!({"what": "plan_csv", "plan": "Three", "path": csv}),
        );
        assert!(r.get("isError").is_none(), "{r}");
        assert!(std::fs::read_to_string(&csv).unwrap().lines().count() > 26);

        let saved = dir.join("again");
        call(&mut s, "save_project", json!({"path": saved}));
        let again = territoria_model::file::load(&dir.join("again.territoria")).unwrap();
        assert_eq!(again.plans.len(), 2);

        let r = call(
            &mut s,
            "color_by",
            json!({"layer": "Cantons", "value": "nothing"}),
        );
        assert_eq!(r["isError"], true);
        let _ = std::fs::remove_dir_all(dir);
    }
}
