mod auth;
mod db;
mod discovery;
mod import;
mod models;
mod routes;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

struct Config {
    db_path: PathBuf,
    import_csv: Option<PathBuf>,
    host: IpAddr,
    port: u16,
    serve_after_import: bool,
}

fn parse_args() -> Result<Config, String> {
    let default_db = std::env::var("WATCHLIST_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data/watchlist.db"));

    let default_host: IpAddr = std::env::var("WATCHLIST_HOST")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));

    let default_port: u16 = std::env::var("WATCHLIST_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);

    let mut cfg = Config {
        db_path: default_db,
        import_csv: None,
        host: default_host,
        port: default_port,
        serve_after_import: false,
    };

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                i += 1;
                let val = args.get(i).ok_or("--db requires a path")?;
                cfg.db_path = PathBuf::from(val);
            }
            "--import" => {
                i += 1;
                let val = args.get(i).ok_or("--import requires a CSV path")?;
                cfg.import_csv = Some(PathBuf::from(val));
            }
            "--host" => {
                i += 1;
                let val = args.get(i).ok_or("--host requires an IP address")?;
                cfg.host = val
                    .parse::<IpAddr>()
                    .map_err(|_| format!("invalid IP address '{val}'"))?;
            }
            "--port" => {
                i += 1;
                let val = args.get(i).ok_or("--port requires a port number")?;
                cfg.port = val
                    .parse::<u16>()
                    .map_err(|_| format!("invalid port '{val}'"))?;
            }
            "--serve" => {
                cfg.serve_after_import = true;
            }
            "--help" | "-h" => {
                println!("Watchlist");
                println!("Usage:");
                println!("  watchlist [--db <path>] [--host <ip>] [--port <port>] [--import <csv_path>] [--serve]");
                println!("Env vars: WATCHLIST_DB, WATCHLIST_HOST, WATCHLIST_PORT, WATCHLIST_PASSWORD, WATCHLIST_SESSION_SECRET, WATCHLIST_SECURE_COOKIE");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }

    Ok(cfg)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = parse_args().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let pool = db::init_pool(&cfg.db_path).await?;
    let fts5_enabled = db::verify_fts5(&pool).await?;
    println!(
        "SQLite initialized at {} (FTS5: {})",
        cfg.db_path.display(),
        if fts5_enabled { "ENABLED" } else { "NOT AVAILABLE" }
    );

    if let Some(csv_path) = &cfg.import_csv {
        let backup_path = db::backup_database(&pool, &cfg.db_path).await?;
        let report = import::import_csv(&pool, csv_path, backup_path).await?;
        println!("=== CSV Import Report ===");
        println!("Source CSV:       {}", report.csv_path.display());
        if let Some(bak) = &report.backup_path {
            println!("DB Backup:        {}", bak.display());
        }
        println!("Total CSV rows:   {}", report.total_rows);
        println!("Inserted:         {}", report.inserted);
        println!("Updated:          {}", report.updated);
        println!("Duplicates (CSV): {}", report.duplicates_in_csv);
        println!("Invalid rows:     {}", report.invalid_rows);
        println!("Missing channels: {}", report.missing_channels);
        if !report.warnings.is_empty() {
            println!("Warnings ({}):", report.warnings.len());
            for w in report.warnings.iter().take(10) {
                println!("  - {w}");
            }
        }
        if !cfg.serve_after_import {
            return Ok(());
        }
    }

    let auth = auth::AuthConfig::from_env();
    if !cfg.host.is_loopback() && !auth.is_enabled() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Refusing to bind to non-loopback address without WATCHLIST_PASSWORD set.",
        )
        .into());
    }

    let state = routes::AppState {
        pool,
        fts5_enabled,
        auth,
        login_failures: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let app = routes::build_router(state);
    let addr = SocketAddr::new(cfg.host, cfg.port);
    println!("Watchlist listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    Ok(())
}
