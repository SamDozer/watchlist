use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub async fn init_pool(db_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }

    let options = SqliteConnectOptions::new()
        .filename(db_path)
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
    let _: i64 = sqlx::query_scalar("SELECT count(*) FROM videos_fts")
        .fetch_one(pool)
        .await?;
    Ok(true)
}

/// Create a WAL-safe, consistent SQLite snapshot using `VACUUM INTO`.
pub async fn backup_database(pool: &SqlitePool, db_path: &Path) -> Result<Option<PathBuf>, sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM videos")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    if count == 0 {
        return Ok(None);
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut backup_name = db_path.as_os_str().to_os_string();
    backup_name.push(format!(".bak.{ts}"));
    let backup_path = PathBuf::from(backup_name);
    let target = backup_path.to_string_lossy().to_string();
    sqlx::query("VACUUM INTO ?1")
        .bind(&target)
        .execute(pool)
        .await?;
    Ok(Some(backup_path))
}
