use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub async fn init_pool(db_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }

    let db_url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
    let options = SqliteConnectOptions::from_str(&db_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub async fn verify_fts5(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    let enabled: i64 = sqlx::query_scalar("SELECT sqlite_compileoption_used('ENABLE_FTS5')")
        .fetch_one(pool)
        .await?;
    if enabled != 1 {
        return Ok(false);
    }
    // Probe the FTS5 virtual table directly
    let _: i64 = sqlx::query_scalar("SELECT count(*) FROM videos_fts")
        .fetch_one(pool)
        .await?;
    Ok(true)
}

pub fn backup_database(db_path: &Path) -> std::io::Result<Option<PathBuf>> {
    if !db_path.exists() {
        return Ok(None);
    }
    let meta = fs::metadata(db_path)?;
    if meta.len() == 0 {
        return Ok(None);
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut backup_name = db_path.as_os_str().to_os_string();
    backup_name.push(format!(".bak.{ts}"));
    let backup_path = PathBuf::from(backup_name);
    fs::copy(db_path, &backup_path)?;
    Ok(Some(backup_path))
}
