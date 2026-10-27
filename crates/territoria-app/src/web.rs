//! The browser's side of things: files come from the file picker and leave
//! as downloads, settings live in the page's local storage, and map tiles
//! are fetched by the browser.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::Sender;

use territoria_config::Settings;
use territoria_io::files;
use wasm_bindgen_futures::JsFuture;
use web_sys::wasm_bindgen::{JsCast, JsValue};

/// Where picked files are kept among the session's files.
const PICKED: &str = "/picked";
/// Where files to download are written.
const DOWNLOADS: &str = "/downloads";
const SETTINGS_KEY: &str = "territoria-settings";

/// Files dropped on the page and read, to be opened.
static DROPPED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn js_error(e: JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

/// Shows the browser's file picker. The files chosen are kept for the
/// session and the one to open (a shapefile's `.shp`, else the first) is
/// sent; `None` if nothing was chosen.
pub fn pick(
    dialog: rfd::AsyncFileDialog,
    several: bool,
    tx: Sender<Option<PathBuf>>,
    ctx: Option<egui::Context>,
) {
    wasm_bindgen_futures::spawn_local(async move {
        let handles = if several {
            dialog.pick_files().await
        } else {
            dialog.pick_file().await.map(|h| vec![h])
        };
        let mut paths = Vec::new();
        for handle in handles.unwrap_or_default() {
            let path = Path::new(PICKED).join(handle.file_name());
            if files::write(&path, &handle.read().await).is_ok() {
                paths.push(path);
            }
        }
        let main = paths
            .iter()
            .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("shp")))
            .or(paths.first())
            .cloned();
        let _ = tx.send(main);
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    });
}

/// Reads files dropped on the page into the session's files; [`dropped`]
/// then has the ones to open (a shapefile's other parts are read with its
/// `.shp`).
pub fn read_dropped(handles: Vec<egui::DroppedFileHandle>, ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        let mut paths = Vec::new();
        for handle in handles {
            let path = Path::new(PICKED).join(handle.path());
            match handle.bytes_async().await {
                Ok(bytes) if files::write(&path, &bytes).is_ok() => paths.push(path),
                Ok(_) => {}
                Err(e) => tracing::warn!("reading the dropped {}: {e}", path.display()),
            }
        }
        let open: Vec<PathBuf> = paths
            .iter()
            .filter(|p| {
                let part = p.extension().is_some_and(|e| {
                    ["shx", "dbf", "prj", "cpg"]
                        .iter()
                        .any(|x| e.eq_ignore_ascii_case(x))
                });
                !part || !paths.contains(&p.with_extension("shp"))
            })
            .cloned()
            .collect();
        DROPPED.lock().unwrap().extend(open);
        ctx.request_repaint();
    });
}

/// Dropped files read since the last call.
pub fn dropped() -> Vec<PathBuf> {
    std::mem::take(&mut *DROPPED.lock().unwrap())
}

/// Where a file to download, named `name`, is written first.
pub fn download_path(name: &str) -> PathBuf {
    Path::new(DOWNLOADS).join(name)
}

/// Hands the files an export wrote (one, or several side by side) to the
/// browser as downloads.
pub fn download_all() -> Result<(), String> {
    for path in files::list(Path::new(DOWNLOADS)) {
        download(&path)?;
    }
    Ok(())
}

/// Hands a file the app wrote to the browser as a download, and forgets it.
pub fn download(path: &Path) -> Result<(), String> {
    let bytes = files::read(path).map_err(|e| e.to_string())?;
    files::remove(path);
    let name = path
        .file_name()
        .map_or_else(|| "download".into(), |n| n.to_string_lossy().into_owned());
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(&bytes[..]));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/octet-stream");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(js_error)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js_error)?;
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("the page has no document")?;
    let link: web_sys::HtmlAnchorElement = document
        .create_element("a")
        .map_err(js_error)?
        .dyn_into()
        .map_err(|_| "not a link")?;
    link.set_href(&url);
    link.set_download(&name);
    link.click();
    // The browser has the data once the download starts.
    let revoke = web_sys::wasm_bindgen::closure::Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    if let Some(w) = web_sys::window() {
        let _ =
            w.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 60_000);
    }
    Ok(())
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// Settings kept in the browser, if any.
pub fn load_settings() -> Option<Settings> {
    let text = storage()?.get_item(SETTINGS_KEY).ok()??;
    serde_json::from_str(&text).ok()
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_string(settings).map_err(|e| e.to_string())?;
    storage()
        .ok_or("the browser keeps no local storage")?
        .set_item(SETTINGS_KEY, &text)
        .map_err(js_error)
}

/// The body of a GET request. Other sites answer only if they allow
/// cross-origin requests (CORS).
pub async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(js_error)?
        .dyn_into()
        .map_err(|_| "not a response")?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    let body = JsFuture::from(response.array_buffer().map_err(js_error)?)
        .await
        .map_err(js_error)?;
    Ok(js_sys::Uint8Array::new(&body).to_vec())
}
