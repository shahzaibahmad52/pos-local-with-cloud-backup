//! Desktop shell. Decides at start-up whether this PC is the store's main PC
//! (runs the store server and database) or an extra till (connects to the
//! main PC over the shop network), then shows the till UI.

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceConfig {
    /// `"main"` or `"till"`; `None` until the first-run screen is completed.
    pub mode: Option<String>,
    /// Main PC address for extra tills, e.g. `192.168.1.10`.
    pub main_host: Option<String>,
    pub port: Option<u16>,
    /// This till's code, printed on receipts, e.g. `T01`.
    pub till_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub config: DeviceConfig,
    /// Where the UI should send API requests; `None` until configured.
    pub server_url: Option<String>,
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("device.json"))
}

fn load_config(app: &AppHandle) -> DeviceConfig {
    config_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn server_url(config: &DeviceConfig) -> Option<String> {
    let port = config.port.unwrap_or(pos_server::DEFAULT_PORT);
    match config.mode.as_deref() {
        Some("main") => Some(format!("http://127.0.0.1:{port}")),
        Some("till") => config.main_host.as_ref().map(|h| format!("http://{}:{port}", h.trim())),
        _ => None,
    }
}

#[tauri::command]
fn get_runtime_info(app: AppHandle) -> RuntimeInfo {
    let config = load_config(&app);
    RuntimeInfo { server_url: server_url(&config), config }
}

#[tauri::command]
fn save_device_config(app: AppHandle, config: DeviceConfig) -> Result<(), String> {
    match config.mode.as_deref() {
        Some("main") => {}
        Some("till") if config.main_host.as_deref().is_some_and(|h| !h.trim().is_empty()) => {}
        Some("till") => return Err("Enter the main PC's address.".into()),
        _ => return Err("Choose main PC or extra till.".into()),
    }
    let path = config_path(&app)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

/// Restarts the app so a new device setting (e.g. becoming the main PC) takes effect.
#[tauri::command]
fn restart_app(app: AppHandle) {
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config = load_config(app.handle());
            if config.mode.as_deref() == Some("main") {
                let db_path = app.path().app_data_dir()?.join("store.sqlite");
                let server = pos_server::ServerConfig {
                    db_path,
                    host: "0.0.0.0".into(),
                    port: config.port.unwrap_or(pos_server::DEFAULT_PORT),
                    demo: std::env::var("MEDSTORE_DEMO").is_ok_and(|v| v == "1"),
                };
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = pos_server::run(server).await {
                        eprintln!("store server stopped: {e}");
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_runtime_info, save_device_config, restart_app])
        .run(tauri::generate_context!())
        .expect("error while running the POS app");
}
