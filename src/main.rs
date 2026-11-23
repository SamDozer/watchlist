mod db;
mod discovery;
mod import;
mod models;
mod routes;

use std::net::SocketAddr;
use std::path::PathBuf;

struct Config {
    db_path: PathBuf,
    import_csv: Option<PathBuf>,
    port: u16,
    serve_after_import: bool,
}

fn parse_args() -> Result<Config, String> {
    let mut cfg = Config {
        db_path: PathBuf::from("./data/watchlist.db"),
        import_csv: None,
        port: 3000,
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
                println!("Watchlist (Stage 1)");
                println!("Usage:");
                println!("  watchlist [--db <path>] [--port <port>] [--import <csv_path>] [--serve]");
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

    let backup_path = if cfg.import_csv.is_some() {
        db::backup_database(&cfg.db_path)?
    } else {
        None
    };

    let pool = db::init_pool(&cfg.db_path).await?;
    let fts5_enabled = db::verify_fts5(&pool).await?;
    println!(
        "SQLite initialized at {} (FTS5: {})",
        cfg.db_path.display(),
        if fts5_enabled { "ENABLED" } else { "NOT AVAILABLE" }
    );

    if let Some(csv_path) = &cfg.import_csv {
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

    let state = routes::AppState { pool, fts5_enabled };
    let app = routes::build_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], cfg.port));
    println!("Watchlist listening on http://{addr} (local only)");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    Ok(())
}
