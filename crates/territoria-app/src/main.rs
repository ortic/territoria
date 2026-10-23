//! Territoria – geomarketing and territory planning.

// Release builds are GUI programs on Windows, without a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod commands;
mod contents;
mod dialogs;
mod export_map;
mod inspector;
mod jobs;
mod map_view;
mod meshes;
mod report;
mod resolve;
#[cfg(test)]
mod screenshots;
mod table_view;
mod territories;
mod theme;
mod tiles;

use std::fs::{self, File};
use std::sync::Mutex;

use territoria_config::Paths;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

fn main() -> anyhow::Result<()> {
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
    let open = std::env::args_os().nth(1).map(std::path::PathBuf::from);

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

fn run(
    renderer: eframe::Renderer,
    paths: Option<Paths>,
    open: Option<std::path::PathBuf>,
) -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_title("Territoria")
            .with_app_id("territoria")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([720.0, 480.0]),
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

/// Logs to stderr and to `<data dir>/logs/territoria.log`. `RUST_LOG` overrides the filter.
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
