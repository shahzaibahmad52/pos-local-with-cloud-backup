//! Store server: the shop's database and the API every till uses.
//!
//! One PC in each store (the "main PC") runs this inside the desktop app.
//! Other tills in the same shop connect to it over the local network, so
//! all tills share one stock count. It works with no internet at all; when
//! cloud backup is on, a background worker copies changes to the store's
//! cloud database whenever the internet is up (see `cloud`).

pub mod api;
pub mod auth;
pub mod cloud;
pub mod dashboard;
pub mod db;
pub mod error;
pub mod expenses;
pub mod import;
pub mod inventory;
pub mod khata;
pub mod ledger;
pub mod pricing;
pub mod products;
pub mod purchases;
pub mod returns;
pub mod rules;
pub mod sales;
pub mod seed;
pub mod settings;
pub mod shifts;
pub mod store;

use std::{net::SocketAddr, path::PathBuf};

pub use api::{router, AppState};

pub const DEFAULT_PORT: u16 = 7420;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub db_path: PathBuf,
    pub host: String,
    pub port: u16,
    /// Load the demo store into an empty database.
    pub demo: bool,
}

/// Opens (or creates) the database and serves the API until the process ends.
pub async fn run(config: ServerConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(dir) = config.db_path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let mut conn = db::open(&config.db_path)?;
    if config.demo && seed::demo(&mut conn).map_err(|e| e.to_string())? {
        println!(
            "Loaded the demo store. Owner password: {}  Cashier PINs: Ali 1234, Sana 2345, Usman 3456",
            seed::DEMO_OWNER_PASSWORD
        );
    }
    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Store server listening on http://{addr}  (database: {})", config.db_path.display());
    let backups = config.db_path.parent().unwrap_or(std::path::Path::new(".")).join("backups");
    let state = AppState::new(conn).with_backup_dir(backups);
    tokio::spawn(background(state.clone()));
    axum::serve(listener, router(state).into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}

/// How often waiting changes are sent to the cloud while the internet is up.
pub const SYNC_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// Runs forever: uploads changes to the cloud and makes the day's backup file.
async fn background(state: AppState) {
    let db = state.database();
    let cloud = state.cloud();
    let mut tick = tokio::time::interval(SYNC_EVERY);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = cloud.wake.notified() => {}
        }
        // Errors are kept in the cloud status for the backoffice to show.
        let _ = cloud::sync::sync_once(&db, &cloud).await;
        if let Some(dir) = state.backup_dir() {
            if cloud::backup::due(&dir) {
                let db = db.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let conn = db.lock().map_err(|_| "database lock poisoned".to_string())?;
                    cloud::backup::backup_now(&conn, &dir).map_err(|e| e.to_string())
                })
                .await;
                if let Ok(Err(e)) = result {
                    eprintln!("daily backup failed: {e}");
                }
            }
        }
    }
}
