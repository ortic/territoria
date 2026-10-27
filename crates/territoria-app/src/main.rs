//! Territoria – geomarketing and territory planning.

// Release builds are GUI programs on Windows, without a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod charts;
#[cfg(not(target_arch = "wasm32"))]
mod cli;
mod commands;
mod contents;
mod dialogs;
mod export_map;
mod flowlines;
mod heat;
mod help;
mod i18n;
mod inspector;
mod jobs;
mod map_view;
#[cfg(not(target_arch = "wasm32"))]
mod mcp;
mod meshes;
mod plan_table;
mod report;
mod resolve;
mod sample;
#[cfg(test)]
mod screenshots;
mod search;
mod table_view;
mod territories;
mod theme;
mod tiles;
mod vector_style;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
use std::fs::{self, File};
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

#[cfg(not(target_arch = "wasm32"))]
use territoria_config::Paths;
use tracing::info;
#[cfg(not(target_arch = "wasm32"))]
use tracing::{error, warn};
#[cfg(not(target_arch = "wasm32"))]
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    // Command-line tasks run without a window.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }
    let paths = match Paths::detect() {
        Ok(p) => Some(p),
        Err(e) => {
            startup_error(&format!(
                "Territoria could not find its settings folder ({e:#}). It starts without saved settings."
            ));
            None
        }
    };
    init_logging(paths.as_ref());
    install_panic_hook(paths.as_ref());
    info!(version = env!("CARGO_PKG_VERSION"), "starting Territoria");
    let open = std::env::args_os().nth(1).map(PathBuf::from);

    let result = run(eframe::Renderer::Wgpu, paths.clone(), open.clone()).or_else(|e| {
        warn!("wgpu renderer failed ({e}), falling back to OpenGL");
        run(eframe::Renderer::Glow, paths.clone(), open)
    });
    if let Err(e) = &result {
        startup_error(&format!(
            "Territoria could not open its window: neither Vulkan/DirectX/Metal nor OpenGL works here ({e})."
        ));
    }
    result.map_err(|e| anyhow::anyhow!("{e}"))
}

/// In the browser: the app on the page's canvas, without settings folder
/// or command line.
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    info!(version = env!("CARGO_PKG_VERSION"), "starting Territoria");
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .expect("a web page");
        let canvas = document
            .get_element_by_id("territoria")
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("a canvas with the id territoria");
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(app::TerritoriaApp::new(cc, None, None)))),
            )
            .await;
        if let Some(loading) = document.get_element_by_id("loading") {
            match result {
                Ok(()) => loading.remove(),
                Err(e) => {
                    loading.set_text_content(Some(&format!(
                        "Territoria couldn't start in this browser: {}",
                        e.as_string().unwrap_or_else(|| format!("{e:?}"))
                    )));
                }
            }
        }
    });
}

/// Where scratch files go: the system's folder for them, or in the browser
/// one among the session's files.
pub(crate) fn temp_dir() -> PathBuf {
    #[cfg(not(target_arch = "wasm32"))]
    return std::env::temp_dir();
    #[cfg(target_arch = "wasm32")]
    PathBuf::from("/tmp")
}

/// A scratch folder for this run alone (the browser has no process ids).
pub(crate) fn run_dir(name: &str) -> PathBuf {
    #[cfg(not(target_arch = "wasm32"))]
    return temp_dir().join(format!("territoria-{name}-{}", std::process::id()));
    #[cfg(target_arch = "wasm32")]
    temp_dir().join(format!("territoria-{name}"))
}

#[cfg(not(target_arch = "wasm32"))]
fn startup_error(text: &str) {
    error!("{text}");
    eprintln!("territoria: {text}");
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Territoria")
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

#[cfg(not(target_arch = "wasm32"))]
fn run(renderer: eframe::Renderer, paths: Option<Paths>, open: Option<PathBuf>) -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_title("Territoria")
            .with_app_id("territoria")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([720.0, 480.0])
            .with_icon(icon()),
        // Smooth area edges and borders on the map.
        multisampling: 4,
        persistence_path: paths.as_ref().map(|p| p.data_dir.join("workspace.ron")),
        ..Default::default()
    };
    eframe::run_native(
        "territoria",
        options,
        Box::new(move |cc| Ok(Box::new(app::TerritoriaApp::new(cc, paths, open)))),
    )
}

/// The window icon, drawn from the packaged SVG.
#[cfg(not(target_arch = "wasm32"))]
fn icon() -> egui::IconData {
    const SVG: &str = include_str!("../../../packaging/territoria.svg");
    const SIZE: u32 = 128;
    let rgba = resvg::usvg::Tree::from_str(SVG, &Default::default())
        .ok()
        .and_then(|tree| {
            let mut pixmap = resvg::tiny_skia::Pixmap::new(SIZE, SIZE)?;
            let k = SIZE as f32 / tree.size().width();
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(k, k),
                &mut pixmap.as_mut(),
            );
            // Premultiplied to straight alpha.
            Some(
                pixmap
                    .pixels()
                    .iter()
                    .flat_map(|p| {
                        let c = p.demultiply();
                        [c.red(), c.green(), c.blue(), c.alpha()]
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();
    egui::IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    }
}

/// Logs to stderr and to `<data dir>/logs/territoria.log`. `RUST_LOG` overrides the filter.
#[cfg(not(target_arch = "wasm32"))]
fn init_logging(paths: Option<&Paths>) {
    let filter = || {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,wgpu=warn,naga=warn,eframe=warn"))
    };
    let file = paths.and_then(|p| {
        let dir = p.log_dir();
        fs::create_dir_all(&dir).ok()?;
        File::create(dir.join("territoria.log")).ok()
    });
    let file_layer = file.map(|f| {
        fmt::layer()
            .with_ansi(false)
            .with_writer(Mutex::new(f))
            .with_filter(filter())
    });
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(filter()),
        )
        .with(file_layer)
        .init();
}

/// Logs panics and writes a crash report next to the log file.
#[cfg(not(target_arch = "wasm32"))]
fn install_panic_hook(paths: Option<&Paths>) {
    let crash_dir = paths.map(Paths::log_dir);
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        error!("panic: {panic}\n{backtrace}");
        if let Some(dir) = &crash_dir {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let _ = fs::write(
                dir.join(format!("crash-{stamp}.txt")),
                format!("{panic}\n\n{backtrace}"),
            );
        }
        default_hook(panic);
    }));
}

#[cfg(test)]
mod icon_tests {
    #[test]
    fn icon_is_drawn() {
        let icon = super::icon();
        assert_eq!(icon.rgba.len(), 128 * 128 * 4);
        assert!(icon.rgba.chunks(4).filter(|p| p[3] == 255).count() > 128 * 64);
    }
}
