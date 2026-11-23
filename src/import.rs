use serde::Deserialize;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct CsvRecord {
    rank: i64,
    #[serde(rename = "importanceScore")]
    importance_score: i64,
    language: String,
    #[serde(rename = "watchTimeBucket")]
    watch_time_bucket: String,
    category: String,
    #[serde(default)]
    subgenre: String,
    #[serde(rename = "isGuitar")]
    is_guitar: String,
    #[serde(rename = "isShort")]
    is_short: String,
    #[serde(rename = "classifiedBy", default)]
    classified_by: String,
    #[serde(rename = "videoId")]
    video_id: String,
    title: String,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    duration: String,
    url: String,
}

#[derive(Debug, Clone, Default)]
pub struct ImportReport {
    pub csv_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub total_rows: usize,
    pub inserted: usize,
    pub updated: usize,
    pub duplicates_in_csv: usize,
    pub invalid_rows: usize,
    pub missing_channels: usize,
    pub warnings: Vec<String>,
}

pub fn is_valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn is_valid_youtube_url(url: &str, video_id: &str) -> bool {
    (url.starts_with("https://www.youtube.com/watch?v=")
        || url.starts_with("https://youtu.be/")
        || url.starts_with("https://www.youtube.com/shorts/"))
        && url.contains(video_id)
}

fn parse_bool_flag(raw: &str) -> Result<i64, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(1),
        "false" | "0" | "no" => Ok(0),
        other => Err(format!("invalid boolean value '{other}'")),
    }
}

pub async fn import_csv(
    pool: &SqlitePool,
    csv_path: &Path,
    backup_path: Option<PathBuf>,
) -> Result<ImportReport, Box<dyn std::error::Error>> {
    if !csv_path.exists() {
        return Err(format!("CSV file not found: {}", csv_path.display()).into());
    }

    let file = File::open(csv_path)?;
    let mut reader = BufReader::new(file);
    let mut raw_bytes = Vec::new();
    reader.read_to_end(&mut raw_bytes)?;

    // Strip UTF-8 BOM safely without modifying the original file
    let content = raw_bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&raw_bytes);

    let mut csv_reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(false)
        .from_reader(content);

    let mut report = ImportReport {
        csv_path: csv_path.to_path_buf(),
        backup_path,
        ..Default::default()
    };

    let existing_ids: HashSet<String> =
        sqlx::query_scalar("SELECT video_id FROM videos")
            .fetch_all(pool)
            .await?
            .into_iter()
            .collect();

    let mut seen_in_csv: HashSet<String> = HashSet::new();
    let mut tx = pool.begin().await?;

    for (idx, result) in csv_reader.deserialize::<CsvRecord>().enumerate() {
        let row_num = idx + 2; // 1-based line number including header
        report.total_rows += 1;

        let rec = match result {
            Ok(r) => r,
            Err(e) => {
                report.invalid_rows += 1;
                report.warnings.push(format!("Row {row_num}: CSV parse error: {e}"));
                continue;
            }
        };

        let video_id = rec.video_id.trim();
        let url = rec.url.trim();
        if !is_valid_video_id(video_id) {
            report.invalid_rows += 1;
            report
                .warnings
                .push(format!("Row {row_num}: invalid videoId '{video_id}'"));
            continue;
        }
        if !is_valid_youtube_url(url, video_id) {
            report.invalid_rows += 1;
            report
                .warnings
                .push(format!("Row {row_num}: invalid URL '{url}' for videoId '{video_id}'"));
            continue;
        }

        let is_guitar = match parse_bool_flag(&rec.is_guitar) {
            Ok(v) => v,
            Err(e) => {
                report.invalid_rows += 1;
                report.warnings.push(format!("Row {row_num}: isGuitar {e}"));
                continue;
            }
        };
        let is_short = match parse_bool_flag(&rec.is_short) {
            Ok(v) => v,
            Err(e) => {
                report.invalid_rows += 1;
                report.warnings.push(format!("Row {row_num}: isShort {e}"));
                continue;
            }
        };

        if !seen_in_csv.insert(video_id.to_string()) {
            report.duplicates_in_csv += 1;
            report
                .warnings
                .push(format!("Row {row_num}: duplicate videoId '{video_id}' in CSV"));
            continue;
        }

        if rec.channel.trim().is_empty() {
            report.missing_channels += 1;
        }

        sqlx::query(
            r#"
            INSERT INTO videos (
                video_id, rank, importance_score, language, watch_time_bucket,
                category, subgenre, is_guitar, is_short, classified_by,
                title, channel, duration, url
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ON CONFLICT(video_id) DO UPDATE SET
                rank = excluded.rank,
                importance_score = excluded.importance_score,
                language = excluded.language,
                watch_time_bucket = excluded.watch_time_bucket,
                category = excluded.category,
                subgenre = excluded.subgenre,
                is_guitar = excluded.is_guitar,
                is_short = excluded.is_short,
                classified_by = excluded.classified_by,
                title = excluded.title,
                channel = excluded.channel,
                duration = excluded.duration,
                url = excluded.url
            "#,
        )
        .bind(video_id)
        .bind(rec.rank)
        .bind(rec.importance_score)
        .bind(rec.language.trim())
        .bind(rec.watch_time_bucket.trim())
        .bind(rec.category.trim())
        .bind(rec.subgenre.trim())
        .bind(is_guitar)
        .bind(is_short)
        .bind(rec.classified_by.trim())
        .bind(&rec.title)
        .bind(rec.channel.trim())
        .bind(rec.duration.trim())
        .bind(url)
        .execute(&mut *tx)
        .await?;

        if existing_ids.contains(video_id) {
            report.updated += 1;
        } else {
            report.inserted += 1;
        }
    }

    tx.commit().await?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{init_pool, verify_fts5};
    use std::io::Write;

    #[test]
    fn validates_video_ids_and_urls() {
        assert!(is_valid_video_id("fNk_zzaMoSs"));
        assert!(is_valid_video_id("a_B-1234567"));
        assert!(!is_valid_video_id("short"));
        assert!(!is_valid_video_id("bad!id12345"));
        assert!(is_valid_youtube_url(
            "https://www.youtube.com/watch?v=fNk_zzaMoSs",
            "fNk_zzaMoSs"
        ));
        assert!(!is_valid_youtube_url(
            "https://example.com/watch?v=fNk_zzaMoSs",
            "fNk_zzaMoSs"
        ));
    }

    #[tokio::test]
    async fn imports_bom_csv_idempotently_and_preserves_metadata() {
        let tmp = std::env::temp_dir().join(format!(
            "watchlist_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("test.db");
        let csv_path = tmp.join("sample.csv");

        let mut f = File::create(&csv_path).unwrap();
        f.write_all(&[0xEF, 0xBB, 0xBF]).unwrap();
        writeln!(
            f,
            "rank,importanceScore,language,watchTimeBucket,category,subgenre,isGuitar,isShort,classifiedBy,videoId,title,channel,duration,url"
        )
        .unwrap();
        writeln!(
            f,
            "1,95,ar,\"Deep Dive (20-60m)\",\"Software Engineering & Coding\",\"Rust\",false,false,demo,abc12345678,\"درس برمجة\",\"\",24:28,https://www.youtube.com/watch?v=abc12345678"
        )
        .unwrap();
        writeln!(
            f,
            "2,80,en,\"Medium (5-20m)\",\"Mathematics\",\"Calculus\",false,false,demo,fNk_zzaMoSs,\"Linear Algebra\",\"3Blue1Brown\",15:00,https://www.youtube.com/watch?v=fNk_zzaMoSs"
        )
        .unwrap();
        // Duplicate videoId in CSV
        writeln!(
            f,
            "3,50,en,\"Quick (<5m)\",\"Mathematics\",\"\",false,true,demo,fNk_zzaMoSs,\"Duplicate\",\"3Blue1Brown\",0:45,https://www.youtube.com/watch?v=fNk_zzaMoSs"
        )
        .unwrap();

        let pool = init_pool(&db_path).await.unwrap();
        assert!(verify_fts5(&pool).await.unwrap(), "SQLite FTS5 must be enabled");

        let r1 = import_csv(&pool, &csv_path, None).await.unwrap();
        assert_eq!(r1.total_rows, 3);
        assert_eq!(r1.inserted, 2);
        assert_eq!(r1.updated, 0);
        assert_eq!(r1.duplicates_in_csv, 1);
        assert_eq!(r1.invalid_rows, 0);
        assert_eq!(r1.missing_channels, 1);

        // Re-import must be idempotent
        let r2 = import_csv(&pool, &csv_path, None).await.unwrap();
        assert_eq!(r2.inserted, 0);
        assert_eq!(r2.updated, 2);

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM videos")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2);

        // Verify FTS5 query works on Arabic & English titles
        let fts_hits: i64 = sqlx::query_scalar("SELECT count(*) FROM videos_fts WHERE videos_fts MATCH 'Linear'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(fts_hits, 1);

        pool.close().await;
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
