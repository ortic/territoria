//! The command line: exports without a window, for scripts and scheduled
//! jobs.
//!
//! ```text
//! territoria --export-map OUT [--map NAME] PROJECT     (.png .svg .pdf .html)
//! territoria --export-report OUT [--plan NAME] PROJECT (.pdf)
//! territoria --export-plan OUT [--plan NAME] PROJECT   (.xlsx)
//! territoria --export-atlas OUT [--per COLUMN[,COLUMN]] [--separate] [--tables] PROJECT
//! territoria mcp                                        (a Model Context Protocol server)
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::app::{Export, TerritoriaApp};
use crate::export_map::Format;

pub const HELP: &str = "Territoria – geomarketing and territory planning

Usage:
  territoria [PROJECT]                                 open the app
  territoria --export-map OUT [--map NAME] PROJECT     the map as .png, .svg, .pdf or .html
  territoria --export-report OUT [--plan NAME] PROJECT a territory report (.pdf)
  territoria --export-plan OUT [--plan NAME] PROJECT   territories and assignment (.xlsx)
  territoria --export-atlas OUT [--map NAME] [--per COLUMN[,COLUMN]] [--separate] [--tables] PROJECT
                                                       the map as a PDF with a page per value
                                                       of the columns (or per area); --separate
                                                       writes a PDF per page, --tables a workbook
                                                       with each page's areas
  territoria mcp                                       an MCP server on stdin/stdout, for AI
                                                       agents: claude mcp add territoria -- territoria mcp
  territoria --help

Without --map or --plan, the first one in the project is used. Without
--per, the atlas picks a column of a few dozen values like the dialog.";

#[derive(Debug, Clone, PartialEq)]
enum Task {
    Map(PathBuf),
    Report(PathBuf),
    Plan(PathBuf),
    Atlas(PathBuf),
}

#[derive(Debug, Default, PartialEq)]
struct Args {
    tasks: Vec<Task>,
    map: Option<String>,
    plan: Option<String>,
    /// Atlas pages per these columns' values; empty: as the dialog picks.
    per: Vec<String>,
    separate: bool,
    tables: bool,
    project: Option<PathBuf>,
    help: bool,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut out = Args::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--help" | "-h" => out.help = true,
            "--export-map" => out.tasks.push(Task::Map(value(a)?.into())),
            "--export-report" => out.tasks.push(Task::Report(value(a)?.into())),
            "--export-plan" => out.tasks.push(Task::Plan(value(a)?.into())),
            "--export-atlas" => out.tasks.push(Task::Atlas(value(a)?.into())),
            "--per" => {
                out.per = value(a)?
                    .split(',')
                    .map(|c| c.trim().to_owned())
                    .filter(|c| !c.is_empty())
                    .collect();
            }
            "--separate" => out.separate = true,
            "--tables" => out.tables = true,
            "--map" => out.map = Some(value(a)?),
            "--plan" => out.plan = Some(value(a)?),
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s => out.project = Some(s.into()),
        }
    }
    Ok(out)
}

/// Runs a command-line task if the arguments ask for one: `Some(exit
/// code)`; `None` means open the app.
pub fn run(args: &[String]) -> Option<i32> {
    if args.first().is_some_and(|a| a == "mcp") {
        return Some(crate::mcp::serve());
    }
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("territoria: {e}\n\n{HELP}");
            return Some(2);
        }
    };
    if a.help {
        println!("{HELP}");
        return Some(0);
    }
    if a.tasks.is_empty() {
        return None;
    }
    let Some(project) = a.project.as_deref() else {
        eprintln!("territoria: which project? Give its file last.\n\n{HELP}");
        return Some(2);
    };
    Some(match export(project, &a) {
        Ok(lines) => {
            for l in lines {
                println!("{l}");
            }
            0
        }
        Err(e) => {
            eprintln!("territoria: {e}");
            1
        }
    })
}

fn export(path: &Path, a: &Args) -> Result<Vec<String>, String> {
    let project =
        territoria_model::file::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut app = TerritoriaApp::with_settings(Default::default(), None);
    app.set_project(project, None);
    let map = match &a.map {
        Some(name) => {
            app.project
                .maps
                .iter()
                .find(|m| &m.name == name)
                .ok_or(format!("no map “{name}”"))?
                .id
        }
        None => app.project.maps.first().ok_or("the project has no map")?.id,
    };
    app.use_map(map);
    let plan = match &a.plan {
        Some(name) => Some(
            app.project
                .plans
                .iter()
                .find(|p| &p.name == name)
                .ok_or(format!("no plan “{name}”"))?
                .id,
        ),
        None => app.project.plans.first().map(|p| p.id),
    };
    // Shapes for the map are made in the background: wait for them.
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    let settings = crate::dialogs::MapExportSettings {
        format: Format::Png,
        scale: 2.0,
        title: app.project.title.clone(),
        legend: true,
        scale_bar: true,
        page: Default::default(),
        footer: String::new(),
        overview: false,
        figures: false,
        layout: None,
    };
    while let Err(e) = app.map_svg_for_cli(&settings) {
        if !e.contains("prepared") || std::time::Instant::now() > deadline {
            return Err(e);
        }
        app.finish_jobs(Duration::from_millis(200));
    }
    for task in &a.tasks {
        match task {
            Task::Map(out) => {
                let ext = out
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or_default()
                    .to_lowercase();
                let what = match ext.as_str() {
                    "png" => Export::Map(Format::Png),
                    "svg" => Export::Map(Format::Svg),
                    "pdf" => Export::Map(Format::Pdf),
                    "html" | "htm" => Export::Html,
                    _ => {
                        return Err(format!(
                            "{}: the map can be .png, .svg, .pdf or .html",
                            out.display()
                        ));
                    }
                };
                if let Export::Map(format) = what {
                    app.set_map_export_settings(crate::dialogs::MapExportSettings {
                        format,
                        ..settings.clone()
                    });
                }
                app.export(what, out.clone());
            }
            Task::Report(out) => app.export(
                Export::Report(plan.ok_or("the project has no plan")?),
                out.clone(),
            ),
            Task::Plan(out) => app.export(
                Export::PlanExcel(plan.ok_or("the project has no plan")?),
                out.clone(),
            ),
            Task::Atlas(out) => {
                let map = app.project.map(map).ok_or("the map was removed")?;
                let d = crate::dialogs::AtlasDialog::new(&app.project, map);
                let layer = d.layer.ok_or("the map has no areas for an atlas")?;
                let columns = if a.per.is_empty() {
                    d.column.into_iter().collect()
                } else {
                    let l = app.project.layer(layer).ok_or("the layer was removed")?;
                    if let Some(c) = a.per.iter().find(|c| l.attributes.column(c).is_none()) {
                        return Err(format!("{} has no column “{c}”", l.name));
                    }
                    a.per.clone()
                };
                let spec = crate::app::AtlasSpec {
                    layer,
                    columns,
                    separate: a.separate,
                    tables: a.tables,
                };
                app.export(Export::Atlas(spec), out.clone());
            }
        }
        app.finish_jobs(Duration::from_secs(600));
    }
    let errors = app.errors();
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(app.messages())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arguments() {
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let a = parse(&s(&[
            "--export-map",
            "m.png",
            "--plan",
            "Sales",
            "p.territoria",
        ]))
        .unwrap();
        assert_eq!(a.tasks, vec![Task::Map("m.png".into())]);
        assert_eq!(a.plan.as_deref(), Some("Sales"));
        assert_eq!(a.project, Some("p.territoria".into()));
        assert!(parse(&s(&["--export-map"])).is_err());
        assert!(parse(&s(&["--nonsense"])).is_err());
        assert_eq!(run(&s(&["p.territoria"])), None, "just a project: the app");
    }

    #[test]
    fn exports_without_a_window() {
        let dir = std::env::temp_dir().join(format!("territoria-cli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A project with boundaries and a plan, saved.
        let geojson = dir.join("areas.geojson");
        let mut features = Vec::new();
        for i in 0..6 {
            let (x, y) = ((i % 3) as f64 * 0.1, (i / 3) as f64 * 0.1);
            features.push(format!(
                r#"{{"type":"Feature","properties":{{"code":"A{i}","zone":"{}","kind":"{}"}},"geometry":{{"type":"Polygon","coordinates":[[[{x},{y}],[{},{y}],[{},{}],[{x},{}],[{x},{y}]]]}}}}"#,
                if i < 3 { "North" } else { "South" },
                if i % 2 == 0 { "shop" } else { "office" },
                x + 0.1, x + 0.1, y + 0.1, y + 0.1
            ));
        }
        std::fs::write(
            &geojson,
            format!(
                r#"{{"type":"FeatureCollection","features":[{}]}}"#,
                features.join(",")
            ),
        )
        .unwrap();
        let project = dir.join("p.territoria");
        let mut app = TerritoriaApp::with_settings(Default::default(), None);
        let layer = app.add_boundaries_now(&geojson).unwrap()[0];
        app.add_balanced_plan_now(layer, 2);
        app.save_to_for_test(project.clone());
        app.finish_jobs(Duration::from_secs(30));
        assert!(project.exists());
        let (png, pdf, xlsx) = (
            dir.join("map.png"),
            dir.join("report.pdf"),
            dir.join("plan.xlsx"),
        );
        let p = |x: &Path| x.to_string_lossy().into_owned();
        let code = run(&[
            "--export-map".into(),
            p(&png),
            "--export-report".into(),
            p(&pdf),
            "--export-plan".into(),
            p(&xlsx),
            p(&project),
        ]);
        assert_eq!(code, Some(0));
        assert_eq!(&std::fs::read(&png).unwrap()[1..4], b"PNG");
        assert_eq!(&std::fs::read(&pdf).unwrap()[..5], b"%PDF-");
        assert!(std::fs::metadata(&xlsx).unwrap().len() > 1000);
        // An atlas per zone and kind: a PDF per page and a workbook.
        let atlas = dir.join("atlas.pdf");
        let code = run(&[
            "--export-atlas".into(),
            p(&atlas),
            "--per".into(),
            "zone,kind".into(),
            "--separate".into(),
            "--tables".into(),
            p(&project),
        ]);
        assert_eq!(code, Some(0));
        let mut pdfs: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with("atlas – ") && n.ends_with(".pdf"))
            .collect();
        pdfs.sort();
        assert_eq!(
            pdfs,
            [
                "atlas – North · office.pdf",
                "atlas – North · shop.pdf",
                "atlas – South · office.pdf",
                "atlas – South · shop.pdf"
            ]
        );
        assert!(!atlas.exists(), "pages only, no merged PDF");
        assert!(std::fs::metadata(dir.join("atlas.xlsx")).unwrap().len() > 1000);
        assert_eq!(
            run(&[
                "--export-atlas".into(),
                p(&atlas),
                "--per".into(),
                "nope".into(),
                p(&project)
            ]),
            Some(1)
        );
        // A missing project fails cleanly.
        assert_eq!(
            run(&[
                "--export-map".into(),
                p(&png),
                p(&dir.join("nope.territoria"))
            ]),
            Some(1)
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
