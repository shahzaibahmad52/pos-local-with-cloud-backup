//! Store server: the shop's database and the API every till uses.
//!
//! One PC in each store (the "main PC") runs this inside the desktop app.
//! Other tills in the same shop connect to it over the local network, so
//! all tills share one stock count. It works with no internet at all; the
//! `outbox` table collects changes for the cloud backup.

pub mod api;
pub mod auth;
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
    axum::serve(listener, router(AppState::new(conn)).into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
