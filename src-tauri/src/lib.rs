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
    #[serde(default)]
    pub printer: PrinterSettings,
}

/// The receipt printer attached to this till. Each till has its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterSettings {
    /// Windows printer name; `None` = use the normal print dialog.
    pub name: Option<String>,
    /// Printable width in dots: 576 for 80 mm paper, 384 for 58 mm.
    pub width_dots: u32,
    pub auto_print: bool,
    pub open_drawer_on_cash: bool,
    pub cut: bool,
}

impl Default for PrinterSettings {
    fn default() -> Self {
        Self { name: None, width_dots: 576, auto_print: true, open_drawer_on_cash: true, cut: true }
    }
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
fn save_device_config(app: AppHandle, mut config: DeviceConfig) -> Result<(), String> {
    // Keep the printer chosen earlier; it has its own settings screen.
    config.printer = load_config(&app).printer;
    match config.mode.as_deref() {
        Some("main") => {}
        Some("till") if config.main_host.as_deref().is_some_and(|h| !h.trim().is_empty()) => {}
        Some("till") => return Err("Enter the main PC's address.".into()),
        _ => return Err("Choose main PC or extra till.".into()),
    }
    write_config(&app, &config)
}

fn write_config(app: &AppHandle, config: &DeviceConfig) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_printer_settings(app: AppHandle, printer: PrinterSettings) -> Result<(), String> {
    if !matches!(printer.width_dots, 384 | 512 | 576) {
        return Err("Paper width must be 58 mm or 80 mm.".into());
    }
    let mut config = load_config(&app);
    config.printer = printer;
    write_config(&app, &config)
}

#[tauri::command]
fn list_printers() -> Result<Vec<String>, String> {
    pos_printer::list_printers()
}

/// Prints a receipt the UI has already drawn as a 1-bit image.
#[tauri::command]
async fn print_raster(
    printer: String,
    width: usize,
    height: usize,
    bits_base64: String,
    cut: bool,
    open_drawer: bool,
) -> Result<(), String> {
    use base64::Engine as _;
    let bits = base64::engine::general_purpose::STANDARD.decode(bits_base64).map_err(|e| e.to_string())?;
    let job = pos_printer::escpos::raster_job(
        &pos_printer::escpos::Raster { width, height, bits: &bits },
        pos_printer::escpos::JobOptions { cut, open_drawer },
    )?;
    tauri::async_runtime::spawn_blocking(move || pos_printer::send_raw(&printer, &job))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_cash_drawer(printer: String) -> Result<(), String> {
    let pulse = pos_printer::escpos::drawer_pulse();
    tauri::async_runtime::spawn_blocking(move || pos_printer::send_raw(&printer, &pulse))
        .await
        .map_err(|e| e.to_string())?
}

/// Opens a WhatsApp message link (wa.me) in the default browser, which hands
/// it to WhatsApp. Only wa.me links are allowed, so the page can't open
/// anything else on the PC.
#[tauri::command]
fn open_whatsapp(url: String) -> Result<(), String> {
    let ok = url
        .strip_prefix("https://wa.me/")
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_alphanumeric() || "?=%&-_.~+".contains(c)));
    if !ok {
        return Err("Only WhatsApp links can be opened.".into());
    }
    #[cfg(windows)]
    let result = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", &url]).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(&url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(&url).spawn();
    result.map(|_| ()).map_err(|e| format!("Couldn't open WhatsApp: {e}"))
}

/// Saves a file the app made (an Excel sheet, a transfer file) in the user's
/// Downloads folder and shows it there. The app's window can't download
/// files the way a browser does. Returns where it was saved.
#[tauri::command]
fn save_download(app: AppHandle, name: String, data_base64: String) -> Result<String, String> {
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.decode(data_base64).map_err(|e| e.to_string())?;
    // Only a plain file name; never a path chosen by the page.
    let name: String = std::path::Path::new(&name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download")
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().document_dir())
        .map_err(|e| format!("No Downloads folder: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Never overwrite: "report.xlsx", then "report (2).xlsx"…
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    let mut path = dir.join(&name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    fs::write(&path, data).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
    show_in_folder(&path);
    Ok(path.display().to_string())
}

/// Opens the folder with the file selected, so the user sees where it went.
fn show_in_folder(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(dir) = path.parent() {
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
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
        .invoke_handler(tauri::generate_handler![
            get_runtime_info,
            save_device_config,
            save_printer_settings,
            list_printers,
            print_raster,
            open_cash_drawer,
            open_whatsapp,
            save_download,
            restart_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running the POS app");
}
