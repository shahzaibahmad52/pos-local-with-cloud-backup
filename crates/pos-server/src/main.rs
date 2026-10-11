use std::path::PathBuf;

use pos_server::{run, ServerConfig, DEFAULT_PORT};

const HELP: &str = "pos-server — store server for Bizflow POS

USAGE:
    pos-server [--db PATH] [--host HOST] [--port PORT] [--demo]

OPTIONS:
    --db PATH     SQLite database file (default: ./store.sqlite)
    --host HOST   Address to listen on (default: 0.0.0.0, i.e. the shop network)
    --port PORT   Port to listen on (default: 7420)
    --demo        Load the demo store into an empty database
";

#[tokio::main]
async fn main() {
    let mut config = ServerConfig {
        db_path: PathBuf::from("store.sqlite"),
        host: "0.0.0.0".into(),
        port: DEFAULT_PORT,
        demo: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next().unwrap_or_else(|| {
                eprintln!("{name} needs a value\n\n{HELP}");
                std::process::exit(2);
            })
        };
        match arg.as_str() {
            "--db" => config.db_path = PathBuf::from(value("--db")),
            "--host" => config.host = value("--host"),
            "--port" => {
                config.port = value("--port").parse().unwrap_or_else(|_| {
                    eprintln!("--port must be a number");
                    std::process::exit(2);
                })
            }
            "--demo" => config.demo = true,
            "-h" | "--help" => {
                print!("{HELP}");
                return;
            }
            other => {
                eprintln!("unknown option {other}\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }
    if let Err(e) = run(config).await {
        eprintln!("store server stopped: {e}");
        std::process::exit(1);
    }
}
