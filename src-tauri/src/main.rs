#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use base64::{engine::general_purpose::STANDARD, Engine};
use broqr_core::{
    check_printer, prepare_file, prepare_rgba, print_label, LabelLayout, NetworkOptions,
    PreparedLabel, PrintOutcome, PrinterAddress, PrinterCheck,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

type CommandResult<T> = Result<T, String>;

#[derive(Default)]
struct AppState {
    next_id: AtomicU64,
    label: Mutex<Option<Arc<LoadedLabel>>>,
    printing: AtomicBool,
}

struct LoadedLabel {
    id: u64,
    label: PreparedLabel,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    printer_address: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LabelPreview {
    id: u64,
    name: String,
    layout: LabelLayout,
    preview_url: String,
}

fn settings_path(app: &AppHandle) -> CommandResult<PathBuf> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("settings.json"))
}

#[tauri::command]
fn load_settings(app: AppHandle) -> CommandResult<Settings> {
    let path = settings_path(&app)?;
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            format!("Could not read saved settings: {e}. Save the printer address again.")
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(format!("Could not read saved settings: {e}")),
    }
}

#[tauri::command]
fn save_settings(app: AppHandle, mut settings: Settings) -> CommandResult<()> {
    settings.printer_address = settings.printer_address.trim().to_owned();
    if !settings.printer_address.is_empty() {
        settings
            .printer_address
            .parse::<PrinterAddress>()
            .map_err(|e| e.to_string())?;
    }
    let path = settings_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| format!("Could not save settings: {e}"))
}

fn ensure_not_printing(state: &AppState) -> CommandResult<()> {
    if state.printing.load(Ordering::Acquire) {
        return Err("A label is already being printed. Wait for it to finish.".into());
    }
    Ok(())
}

fn cache_label(
    state: &AppState,
    id: u64,
    name: String,
    label: PreparedLabel,
) -> CommandResult<LabelPreview> {
    let layout = label.layout().clone();
    let png = label.preview_png().map_err(|e| e.to_string())?;
    let response = LabelPreview {
        id,
        name,
        layout,
        preview_url: format!("data:image/png;base64,{}", STANDARD.encode(png)),
    };
    *state
        .label
        .lock()
        .map_err(|_| "The image cache is unavailable.")? =
        Some(Arc::new(LoadedLabel { id, label }));
    Ok(response)
}

#[tauri::command]
async fn load_image(state: State<'_, Arc<AppState>>, path: String) -> CommandResult<LabelPreview> {
    ensure_not_printing(&state)?;
    let state = Arc::clone(state.inner());
    let id = state.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(path);
        let name = path
            .file_name()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Image".into());
        let label = prepare_file(&path).map_err(|e| e.to_string())?;
        cache_label(&state, id, name, label)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn paste_image(state: State<'_, Arc<AppState>>) -> CommandResult<LabelPreview> {
    ensure_not_printing(&state)?;
    let state = Arc::clone(state.inner());
    let id = state.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| format!("Could not access the clipboard: {e}"))?;
        let data = clipboard.get_image().map_err(|_| {
            "The clipboard does not contain an image. Copy an image or a screenshot first."
                .to_owned()
        })?;
        let width = u32::try_from(data.width).map_err(|_| "Clipboard image is too large.")?;
        let height = u32::try_from(data.height).map_err(|_| "Clipboard image is too large.")?;
        let label =
            prepare_rgba(width, height, data.bytes.into_owned()).map_err(|e| e.to_string())?;
        cache_label(&state, id, "Clipboard image".into(), label)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn test_printer(
    state: State<'_, Arc<AppState>>,
    address: String,
) -> CommandResult<PrinterCheck> {
    ensure_not_printing(&state)?;
    let address = address
        .parse::<PrinterAddress>()
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        check_printer(address, NetworkOptions::default()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Release the backend print lock on every return path, including errors/panics.
struct PrintGuard(Arc<AppState>);
impl Drop for PrintGuard {
    fn drop(&mut self) {
        self.0.printing.store(false, Ordering::Release);
    }
}

#[tauri::command]
async fn print_image(
    state: State<'_, Arc<AppState>>,
    id: u64,
    address: String,
) -> CommandResult<PrintOutcome> {
    let address = address
        .parse::<PrinterAddress>()
        .map_err(|e| e.to_string())?;
    let loaded = state
        .label
        .lock()
        .map_err(|_| "The image cache is unavailable.")?
        .clone()
        .ok_or_else(|| "Load an image before printing.".to_owned())?;
    if loaded.id != id {
        return Err("The preview is out of date. Load the image again before printing.".into());
    }
    let state = Arc::clone(state.inner());
    state
        .printing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "A label is already being printed. Wait for it to finish.".to_owned())?;
    let guard = PrintGuard(state);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        print_label(address, &loaded.label, NetworkOptions::default()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(AppState::default()))
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            load_image,
            paste_image,
            test_printer,
            print_image
        ])
        .run(tauri::generate_context!())
        .expect("Could not start BroQR");
}
