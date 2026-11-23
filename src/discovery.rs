use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::{
    BucketCounts, CategoryGroup, CountOption, FilterParams, NavCounts, Recommendation, VideoCardRow,
};

pub const PAGE_SIZE: i64 = 48;

/// Convert free-text user input into a safe FTS5 MATCH expression.
/// Each whitespace-separated term becomes a quoted prefix query (`"term"*`).
pub fn sanitize_fts_query(raw: &str) -> Option<String> {
    let terms: Vec<String> = raw
        .split_whitespace()
        .map(|t| {
            let cleaned: String = t
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            cleaned
        })
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();

    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

fn push_filter_clauses<'a>(qb: &mut QueryBuilder<'a, Sqlite>, params: &'a FilterParams) {
    let tab = params.normalized_tab();
    match tab {
        "queue" => {
            qb.push(" AND ua.added_to_queue_at IS NOT NULL ");
        }
        "favorites" => {
            qb.push(" AND COALESCE(ua.favorited, 0) = 1 ");
        }
        "watched" => {
            qb.push(" AND COALESCE(ua.status, 'unwatched') = 'watched' ");
        }
        _ => {}
    }

    if tab != "watched" {
        match params.effective_status() {
            "unwatched" => {
                qb.push(" AND COALESCE(ua.status, 'unwatched') = 'unwatched' ");
            }
            "watched" => {
                qb.push(" AND COALESCE(ua.status, 'unwatched') = 'watched' ");
            }
            "skipped" => {
                qb.push(" AND COALESCE(ua.status, 'unwatched') = 'skipped' ");
            }
            _ => {}
        }
    }

    if let Some(fts_expr) = sanitize_fts_query(&params.q) {
        qb.push(" AND v.rowid IN (SELECT rowid FROM videos_fts WHERE videos_fts MATCH ");
        qb.push_bind(fts_expr);
        qb.push(") ");
    }

    if !params.category.is_empty() {
        qb.push(" AND v.category = ");
        qb.push_bind(params.category.clone());
        qb.push(" ");
    }

    if !params.subgenre.is_empty() {
        qb.push(" AND v.subgenre = ");
        qb.push_bind(params.subgenre.clone());
        qb.push(" ");
    }

    if !params.language.is_empty() {
        qb.push(" AND v.language = ");
        qb.push_bind(params.language.clone());
        qb.push(" ");
    }

    if !params.bucket.is_empty() {
        qb.push(" AND v.watch_time_bucket = ");
        qb.push_bind(params.bucket.clone());
        qb.push(" ");
    }

    if params.guitar == "1" {
        qb.push(" AND v.is_guitar = 1 ");
    }

    match params.short.as_str() {
        "1" => {
            qb.push(" AND v.is_short = 1 ");
        }
        "0" => {
            qb.push(" AND v.is_short = 0 ");
        }
        _ => {}
    }
}

pub async fn get_nav_counts(pool: &SqlitePool) -> Result<NavCounts, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        r#"
        SELECT
            COUNT(*) AS total,
            SUM(CASE WHEN COALESCE(ua.status, 'unwatched') = 'unwatched' THEN 1 ELSE 0 END) AS unwatched,
            SUM(CASE WHEN ua.added_to_queue_at IS NOT NULL THEN 1 ELSE 0 END) AS queue_cnt,
            SUM(CASE WHEN v.is_guitar = 1 AND COALESCE(ua.status, 'unwatched') = 'unwatched' THEN 1 ELSE 0 END) AS guitar_cnt
        FROM videos v
        LEFT JOIN user_activity ua ON ua.video_id = v.video_id
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(NavCounts {
        total: row.0,
        unwatched: row.1,
        queue: row.2,
        guitar: row.3,
    })
}

pub async fn get_bucket_counts(pool: &SqlitePool) -> Result<BucketCounts, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        r#"
        SELECT
            SUM(CASE WHEN v.watch_time_bucket = 'Quick (<5m)' THEN 1 ELSE 0 END),
            SUM(CASE WHEN v.watch_time_bucket = 'Medium (5-20m)' THEN 1 ELSE 0 END),
            SUM(CASE WHEN v.watch_time_bucket = 'Deep Dive (20-60m)' THEN 1 ELSE 0 END),
            SUM(CASE WHEN v.watch_time_bucket = 'Long-Form (>60m)' THEN 1 ELSE 0 END)
        FROM videos v
        LEFT JOIN user_activity ua ON ua.video_id = v.video_id
        WHERE COALESCE(ua.status, 'unwatched') = 'unwatched'
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(BucketCounts {
        quick: row.0,
        medium: row.1,
        deep_dive: row.2,
        long_form: row.3,
    })
}

fn category_group_rank(name: &str) -> (&'static str, usize, usize) {
    const GROUPS: &[(&str, &[&str])] = &[
        (
            "Guitar & Music",
            &[
                "Guitar Lessons & Practice",
                "Guitar Gear & Rig Rundowns",
                "Music Theory & Composition",
                "Audio Production & Sound Design",
                "Music History & Appreciation",
                "Albums & Live Performances",
            ],
        ),
        (
            "Mind, Brain & Philosophy",
            &[
                "Neuroscience & Cognitive Neuroscience",
                "Psychology & Mental Health",
                "Philosophy & Ethics",
                "Religion & Theology",
            ],
        ),
        (
            "Biology, Medicine & Health",
            &[
                "Bioinformatics & Computational Biology",
                "Genetics, Molecular & Cell Biology",
                "Evolutionary Biology & Human Origins",
                "Microbiology, Immunology & Ecology",
                "Medicine, Clinical & Anatomy",
                "Fitness, Training & Mobility",
                "Nutrition & Sleep",
            ],
        ),
        (
            "Tech, AI & Coding",
            &[
                "AI & Machine Learning",
                "Software Engineering & Coding",
                "Tech, Hardware & Systems",
                "Engineering & Applied Tech",
            ],
        ),
        (
            "Math & Hard Sciences",
            &[
                "Mathematics",
                "Physics & Astronomy",
                "Chemistry & Biochemistry",
                "General Science",
            ],
        ),
        (
            "Arabic & Egyptian Explainers",
            &[
                "El Da7ee7 (الدحيح)",
                "في الحضارة & Arabic History",
                "Arabic Science & Explainers",
            ],
        ),
        (
            "History, Geopolitics & Society",
            &[
                "Geopolitics & World Affairs",
                "History & Civilizations",
                "News & Regional Commentary",
                "Culture, Anthropology & Society",
            ],
        ),
        (
            "Cinema, Art & Literature",
            &[
                "Cinema, Film & Storytelling",
                "Literature, Books & Poetry",
                "Visual Art & Design",
                "Architecture & Urbanism",
            ],
        ),
        (
            "Career, Learning & Finance",
            &[
                "Career & Job Skills",
                "Productivity & Habits",
                "Study Methods & Learning",
                "Business & Economics",
                "Finance & Investing",
            ],
        ),
        (
            "Languages",
            &[
                "Language Learning - German",
                "Language Learning - English",
                "Language Learning - Arabic",
                "Language Learning - Polyglot & General",
            ],
        ),
        (
            "Lifestyle, Games & Entertainment",
            &[
                "Travel & Places",
                "Food & Cooking",
                "Lifestyle & Vlogs",
                "Gaming & Esports",
                "Chess & Strategy",
                "Comedy, Satire & Memes",
                "Talk Shows & Interviews",
                "General Entertainment & Pop Culture",
                "Other / Misc",
            ],
        ),
    ];

    for (g_idx, (label, items)) in GROUPS.iter().enumerate() {
        if let Some(item_idx) = items.iter().position(|c| *c == name) {
            return (label, g_idx, item_idx);
        }
    }
    ("Other Topics", GROUPS.len(), usize::MAX)
}

pub async fn get_category_groups(pool: &SqlitePool) -> Result<Vec<CategoryGroup>, sqlx::Error> {
    let rows = sqlx::query_as::<_, CountOption>(
        r#"
        SELECT v.category AS name, COUNT(*) AS count
        FROM videos v
        LEFT JOIN user_activity ua ON ua.video_id = v.video_id
        WHERE v.category != '' AND COALESCE(ua.status, 'unwatched') = 'unwatched'
        GROUP BY v.category
        ORDER BY v.category ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut tagged: Vec<((&'static str, usize, usize), CountOption)> = rows
        .into_iter()
        .map(|row| (category_group_rank(&row.name), row))
        .collect();

    tagged.sort_by(|a, b| {
        a.0 .1
            .cmp(&b.0 .1)
            .then_with(|| a.0 .2.cmp(&b.0 .2))
            .then_with(|| a.1.name.cmp(&b.1.name))
    });

    let mut groups: Vec<CategoryGroup> = Vec::new();
    for ((label, _, _), opt) in tagged {
        if let Some(last) = groups.last_mut() {
            if last.label == label {
                last.items.push(opt);
                continue;
            }
        }
        groups.push(CategoryGroup {
            label: label.to_string(),
            items: vec![opt],
        });
    }

    Ok(groups)
}

pub async fn get_subgenres(pool: &SqlitePool, category: &str) -> Result<Vec<CountOption>, sqlx::Error> {
    if category.is_empty() {
        sqlx::query_as::<_, CountOption>(
            r#"
            SELECT v.subgenre AS name, COUNT(*) AS count
            FROM videos v
            LEFT JOIN user_activity ua ON ua.video_id = v.video_id
            WHERE v.subgenre != '' AND COALESCE(ua.status, 'unwatched') = 'unwatched'
            GROUP BY v.subgenre
            ORDER BY count DESC, v.subgenre ASC
            "#,
        )
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, CountOption>(
            r#"
            SELECT v.subgenre AS name, COUNT(*) AS count
            FROM videos v
            LEFT JOIN user_activity ua ON ua.video_id = v.video_id
            WHERE v.category = ?1 AND v.subgenre != '' AND COALESCE(ua.status, 'unwatched') = 'unwatched'
            GROUP BY v.subgenre
            ORDER BY count DESC, v.subgenre ASC
            "#,
        )
        .bind(category)
        .fetch_all(pool)
        .await
    }
}

pub async fn query_videos(
    pool: &SqlitePool,
    params: &FilterParams,
) -> Result<(i64, Vec<VideoCardRow>, Option<i64>), sqlx::Error> {
    let safe_offset = params.offset.max(0);

    let mut count_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT COUNT(*) FROM videos v LEFT JOIN user_activity ua ON ua.video_id = v.video_id WHERE 1=1 ",
    );
    push_filter_clauses(&mut count_qb, params);
    let filtered_total: i64 = count_qb.build_query_scalar().fetch_one(pool).await?;

    let mut list_qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT
            v.video_id, v.rank, v.importance_score, v.language, v.watch_time_bucket,
            v.category, v.subgenre, v.is_guitar, v.is_short, v.classified_by,
            v.title, v.channel, v.duration, v.url,
            COALESCE(ua.status, 'unwatched') AS status,
            COALESCE(ua.favorited, 0) AS favorited,
            CASE WHEN ua.added_to_queue_at IS NOT NULL THEN 1 ELSE 0 END AS in_queue
        FROM videos v
        LEFT JOIN user_activity ua ON ua.video_id = v.video_id
        WHERE 1=1
        "#,
    );
    push_filter_clauses(&mut list_qb, params);

    match params.normalized_tab() {
        "queue" => {
            list_qb.push(" ORDER BY ua.added_to_queue_at DESC, v.rank ASC ");
        }
        _ => {
            list_qb.push(
                r#"
                ORDER BY (
                    v.importance_score
                    + COALESCE((SELECT weight FROM user_preferences WHERE pref_type = 'category' AND pref_key = v.category), 0.0)
                    + COALESCE((SELECT weight FROM user_preferences WHERE pref_type = 'subgenre' AND pref_key = v.subgenre), 0.0)
                    + COALESCE((SELECT weight FROM user_preferences WHERE pref_type = 'duration' AND pref_key = v.watch_time_bucket), 0.0)
                    + COALESCE((SELECT weight FROM user_preferences WHERE pref_type = 'language' AND pref_key = v.language), 0.0)
                ) DESC, v.rank ASC
                "#,
            );
        }
    }

    list_qb.push(" LIMIT ");
    list_qb.push_bind(PAGE_SIZE);
    list_qb.push(" OFFSET ");
    list_qb.push_bind(safe_offset);

    let videos: Vec<VideoCardRow> = list_qb.build_query_as().fetch_all(pool).await?;
    let next_offset = if safe_offset + (videos.len() as i64) < filtered_total {
        Some(safe_offset + PAGE_SIZE)
    } else {
        None
    };

    Ok((filtered_total, videos, next_offset))
}

pub async fn get_video_card(
    pool: &SqlitePool,
    video_id: &str,
) -> Result<Option<VideoCardRow>, sqlx::Error> {
    sqlx::query_as::<_, VideoCardRow>(
        r#"
        SELECT
            v.video_id, v.rank, v.importance_score, v.language, v.watch_time_bucket,
            v.category, v.subgenre, v.is_guitar, v.is_short, v.classified_by,
            v.title, v.channel, v.duration, v.url,
            COALESCE(ua.status, 'unwatched') AS status,
            COALESCE(ua.favorited, 0) AS favorited,
            CASE WHEN ua.added_to_queue_at IS NOT NULL THEN 1 ELSE 0 END AS in_queue
        FROM videos v
        LEFT JOIN user_activity ua ON ua.video_id = v.video_id
        WHERE v.video_id = ?1
        "#,
    )
    .bind(video_id)
    .fetch_optional(pool)
    .await
}

#[derive(Debug, sqlx::FromRow)]
struct PrefWeightRow {
    pref_type: String,
    pref_key: String,
    weight: f64,
}

#[derive(Debug, sqlx::FromRow)]
struct RecentShownRow {
    video_id: String,
    category: String,
    channel: String,
}

#[derive(Debug, sqlx::FromRow)]
struct CandidateRow {
    video_id: String,
    rank: i64,
    importance_score: i64,
    language: String,
    watch_time_bucket: String,
    category: String,
    subgenre: String,
    is_guitar: i64,
    is_short: i64,
    classified_by: String,
    title: String,
    channel: String,
    duration: String,
    url: String,
    status: String,
    favorited: i64,
    in_queue: i64,
    recently_shown: i64,
}

/// Select an explainable, diverse unwatched recommendation ("Choose for me").
pub async fn choose_for_me(
    pool: &SqlitePool,
    params: &FilterParams,
) -> Result<Option<Recommendation>, sqlx::Error> {
    let mut rec_params = params.clone();
    rec_params.status = "unwatched".to_string();
    rec_params.tab = if params.normalized_tab() == "queue" {
        "queue".to_string()
    } else {
        "explore".to_string()
    };

    let pref_rows: Vec<PrefWeightRow> =
        sqlx::query_as("SELECT pref_type, pref_key, weight FROM user_preferences")
            .fetch_all(pool)
            .await?;

    let mut prefs: HashMap<(String, String), f64> = HashMap::new();
    for p in pref_rows {
        prefs.insert((p.pref_type, p.pref_key), p.weight);
    }

    let recent_rows: Vec<RecentShownRow> = sqlx::query_as(
        r#"
        SELECT ua.video_id, v.category, v.channel
        FROM user_activity ua
        JOIN videos v ON v.video_id = ua.video_id
        WHERE ua.last_shown_at IS NOT NULL
        ORDER BY ua.last_shown_at DESC
        LIMIT 25
        "#,
    )
    .fetch_all(pool)
    .await?;

    let recent_ids: HashSet<String> = recent_rows.iter().map(|r| r.video_id.clone()).collect();
    let recent_categories: HashSet<String> = recent_rows
        .iter()
        .take(3)
        .map(|r| r.category.clone())
        .collect();
    let recent_channels: HashSet<String> = recent_rows
        .iter()
        .take(5)
        .filter(|r| !r.channel.trim().is_empty())
        .map(|r| r.channel.clone())
        .collect();

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        r#"
        SELECT
            v.video_id, v.rank, v.importance_score, v.language, v.watch_time_bucket,
            v.category, v.subgenre, v.is_guitar, v.is_short, v.classified_by,
            v.title, v.channel, v.duration, v.url,
            COALESCE(ua.status, 'unwatched') AS status,
            COALESCE(ua.favorited, 0) AS favorited,
            CASE WHEN ua.added_to_queue_at IS NOT NULL THEN 1 ELSE 0 END AS in_queue,
            CASE WHEN ua.last_shown_at IS NOT NULL THEN 1 ELSE 0 END AS recently_shown
        FROM videos v
        LEFT JOIN user_activity ua ON ua.video_id = v.video_id
        WHERE 1=1
        "#,
    );
    push_filter_clauses(&mut qb, &rec_params);
    qb.push(" ORDER BY v.importance_score DESC, v.rank ASC LIMIT 250 ");

    let candidates: Vec<CandidateRow> = qb.build_query_as().fetch_all(pool).await?;
    if candidates.is_empty() {
        return Ok(None);
    }

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(42);

    let mut scored: Vec<(f64, String, VideoCardRow)> = Vec::with_capacity(candidates.len());
    for (idx, c) in candidates.into_iter().enumerate() {
        let mut score = c.importance_score as f64;
        let rank_bonus = (1.0 - (c.rank as f64 / 5000.0)).clamp(0.0, 1.0) * 10.0;
        score += rank_bonus;

        let cat_w = prefs
            .get(&("category".to_string(), c.category.clone()))
            .copied()
            .unwrap_or(0.0);
        let sub_w = if !c.subgenre.is_empty() {
            prefs
                .get(&("subgenre".to_string(), c.subgenre.clone()))
                .copied()
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let dur_w = prefs
            .get(&("duration".to_string(), c.watch_time_bucket.clone()))
            .copied()
            .unwrap_or(0.0);
        let lang_w = prefs
            .get(&("language".to_string(), c.language.clone()))
            .copied()
            .unwrap_or(0.0);

        let pref_total = (cat_w + sub_w + dur_w + lang_w).clamp(-15.0, 25.0);
        score += pref_total;

        if c.favorited != 0 {
            score += 8.0;
        }
        if c.in_queue != 0 {
            score += 4.0;
        }

        // Diversity & anti-repeat penalties
        if recent_ids.contains(&c.video_id) || c.recently_shown != 0 {
            score -= 35.0;
        }
        if recent_categories.contains(&c.category) && params.category.is_empty() {
            score -= 10.0;
        }
        if !c.channel.trim().is_empty() && recent_channels.contains(&c.channel) {
            score -= 14.0;
        }

        // Lightweight deterministic-per-click jitter so top-tier ties rotate cleanly
        let hash = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add((idx as u64 + 1).wrapping_mul(1442695040888963407));
        let jitter = ((hash % 400) as f64) / 100.0; // 0.00..3.99
        score += jitter;

        let mut reasons = vec![format!("Importance {}/100", c.importance_score)];
        if rank_bonus >= 6.0 {
            reasons.push(format!("top rank #{}", c.rank));
        }
        if pref_total >= 1.5 {
            reasons.push("matches your preferences".to_string());
        } else if c.favorited != 0 {
            reasons.push("favorited".to_string());
        } else if c.in_queue != 0 {
            reasons.push("in Watch Later".to_string());
        }
        if !recent_categories.contains(&c.category) && !recent_rows.is_empty() {
            reasons.push("fresh category".to_string());
        }

        let card = VideoCardRow {
            video_id: c.video_id,
            rank: c.rank,
            importance_score: c.importance_score,
            language: c.language,
            watch_time_bucket: c.watch_time_bucket,
            category: c.category,
            subgenre: c.subgenre,
            is_guitar: c.is_guitar,
            is_short: c.is_short,
            classified_by: c.classified_by,
            title: c.title,
            channel: c.channel,
            duration: c.duration,
            url: c.url,
            status: c.status,
            favorited: c.favorited,
            in_queue: c.in_queue,
        };

        scored.push((score, reasons.join(" · "), card));
    }

    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let Some((best_score, reason, video)) = scored.into_iter().next() else {
        return Ok(None);
    };

    // Record last_shown_at so consecutive "Choose for me" clicks diversify
    sqlx::query(
        r#"
        INSERT INTO user_activity (video_id, status, last_shown_at)
        VALUES (?1, 'unwatched', datetime('now'))
        ON CONFLICT(video_id) DO UPDATE SET last_shown_at = datetime('now')
        "#,
    )
    .bind(&video.video_id)
    .execute(pool)
    .await?;

    Ok(Some(Recommendation {
        video,
        reason,
        computed_score: best_score.round() as i64,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityAction {
    ToggleWatched,
    ToggleSkipped,
    ToggleFavorite,
    ToggleQueue,
}

impl ActivityAction {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "watched" => Some(Self::ToggleWatched),
            "skip" => Some(Self::ToggleSkipped),
            "favorite" => Some(Self::ToggleFavorite),
            "queue" => Some(Self::ToggleQueue),
            _ => None,
        }
    }
}

async fn bump_preference(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    pref_type: &str,
    pref_key: &str,
    watched_delta: i64,
    skipped_delta: i64,
    favorited_delta: i64,
) -> Result<(), sqlx::Error> {
    if pref_key.trim().is_empty() {
        return Ok(());
    }
    sqlx::query(
        r#"
        INSERT INTO user_preferences (pref_type, pref_key, watched_count, skipped_count, favorited_count, weight)
        VALUES (
            ?1,
            ?2,
            MAX(0, ?3),
            MAX(0, ?4),
            MAX(0, ?5),
            MIN(12.0, MAX(-12.0, (MAX(0, ?3) * 1.5) + (MAX(0, ?5) * 2.5) - (MAX(0, ?4) * 1.5)))
        )
        ON CONFLICT(pref_type, pref_key) DO UPDATE SET
            watched_count = MAX(0, user_preferences.watched_count + ?3),
            skipped_count = MAX(0, user_preferences.skipped_count + ?4),
            favorited_count = MAX(0, user_preferences.favorited_count + ?5),
            weight = MIN(
                12.0,
                MAX(
                    -12.0,
                    (MAX(0, user_preferences.watched_count + ?3) * 1.5)
                    + (MAX(0, user_preferences.favorited_count + ?5) * 2.5)
                    - (MAX(0, user_preferences.skipped_count + ?4) * 1.5)
                )
            )
        "#,
    )
    .bind(pref_type)
    .bind(pref_key)
    .bind(watched_delta)
    .bind(skipped_delta)
    .bind(favorited_delta)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn apply_activity_action(
    pool: &SqlitePool,
    video_id: &str,
    action: ActivityAction,
) -> Result<Option<VideoCardRow>, sqlx::Error> {
    let existing = get_video_card(pool, video_id).await?;
    let Some(video) = existing else {
        return Ok(None);
    };

    let mut tx = pool.begin().await?;
    let mut watched_delta = 0;
    let mut skipped_delta = 0;
    let mut favorited_delta = 0;

    match action {
        ActivityAction::ToggleWatched => {
            if video.status == "watched" {
                sqlx::query(
                    r#"
                    INSERT INTO user_activity (video_id, status, watched_at)
                    VALUES (?1, 'unwatched', NULL)
                    ON CONFLICT(video_id) DO UPDATE SET status = 'unwatched', watched_at = NULL
                    "#,
                )
                .bind(video_id)
                .execute(&mut *tx)
                .await?;
                watched_delta = -1;
            } else {
                if video.status == "skipped" {
                    skipped_delta = -1;
                }
                sqlx::query(
                    r#"
                    INSERT INTO user_activity (video_id, status, watched_at, skipped_at)
                    VALUES (?1, 'watched', datetime('now'), NULL)
                    ON CONFLICT(video_id) DO UPDATE SET status = 'watched', watched_at = datetime('now'), skipped_at = NULL
                    "#,
                )
                .bind(video_id)
                .execute(&mut *tx)
                .await?;
                watched_delta = 1;
            }
        }
        ActivityAction::ToggleSkipped => {
            if video.status == "skipped" {
                sqlx::query(
                    r#"
                    INSERT INTO user_activity (video_id, status, skipped_at)
                    VALUES (?1, 'unwatched', NULL)
                    ON CONFLICT(video_id) DO UPDATE SET status = 'unwatched', skipped_at = NULL
                    "#,
                )
                .bind(video_id)
                .execute(&mut *tx)
                .await?;
                skipped_delta = -1;
            } else {
                if video.status == "watched" {
                    watched_delta = -1;
                }
                sqlx::query(
                    r#"
                    INSERT INTO user_activity (video_id, status, skipped_at, watched_at)
                    VALUES (?1, 'skipped', datetime('now'), NULL)
                    ON CONFLICT(video_id) DO UPDATE SET status = 'skipped', skipped_at = datetime('now'), watched_at = NULL
                    "#,
                )
                .bind(video_id)
                .execute(&mut *tx)
                .await?;
                skipped_delta = 1;
            }
        }
        ActivityAction::ToggleFavorite => {
            let next_fav = if video.favorited != 0 { 0 } else { 1 };
            favorited_delta = if next_fav == 1 { 1 } else { -1 };
            sqlx::query(
                r#"
                INSERT INTO user_activity (video_id, favorited)
                VALUES (?1, ?2)
                ON CONFLICT(video_id) DO UPDATE SET favorited = ?2
                "#,
            )
            .bind(video_id)
            .bind(next_fav)
            .execute(&mut *tx)
            .await?;
        }
        ActivityAction::ToggleQueue => {
            if video.in_queue != 0 {
                sqlx::query(
                    r#"
                    INSERT INTO user_activity (video_id, added_to_queue_at)
                    VALUES (?1, NULL)
                    ON CONFLICT(video_id) DO UPDATE SET added_to_queue_at = NULL
                    "#,
                )
                .bind(video_id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(
                    r#"
                    INSERT INTO user_activity (video_id, added_to_queue_at)
                    VALUES (?1, datetime('now'))
                    ON CONFLICT(video_id) DO UPDATE SET added_to_queue_at = datetime('now')
                    "#,
                )
                .bind(video_id)
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    if watched_delta != 0 || skipped_delta != 0 || favorited_delta != 0 {
        bump_preference(&mut tx, "category", &video.category, watched_delta, skipped_delta, favorited_delta).await?;
        bump_preference(&mut tx, "subgenre", &video.subgenre, watched_delta, skipped_delta, favorited_delta).await?;
        bump_preference(&mut tx, "duration", &video.watch_time_bucket, watched_delta, skipped_delta, favorited_delta).await?;
        bump_preference(&mut tx, "language", &video.language, watched_delta, skipped_delta, favorited_delta).await?;
    }

    tx.commit().await?;
    get_video_card(pool, video_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool;
    use crate::import::import_csv;
    use std::fs::File;
    use std::io::Write;

    async fn setup_test_db() -> (SqlitePool, std::path::PathBuf) {
        let tmp = std::env::temp_dir().join(format!(
            "watchlist_disc_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("test.db");
        let csv_path = tmp.join("sample.csv");

        let mut f = File::create(&csv_path).unwrap();
        writeln!(
            f,
            "rank,importanceScore,language,watchTimeBucket,category,subgenre,isGuitar,isShort,classifiedBy,videoId,title,channel,duration,url"
        )
        .unwrap();
        writeln!(
            f,
            "1,98,en,\"Deep Dive (20-60m)\",\"Neuroscience & Cognitive Neuroscience\",\"Neural Circuits\",false,false,swarm,vid11111111,\"Synaptic Plasticity Lecture\",\"MITCBMM\",42:10,https://www.youtube.com/watch?v=vid11111111"
        )
        .unwrap();
        writeln!(
            f,
            "2,92,ar,\"Medium (5-20m)\",\"El Da7ee7, في الحضارة & Arabic Explainers\",\"Egyptian History\",false,false,rules,vid22222222,\"الدحيح - في الحضارة\",\"AJ+ كبريت\",18:20,https://www.youtube.com/watch?v=vid22222222"
        )
        .unwrap();
        writeln!(
            f,
            "3,88,en,\"Quick (<5m)\",\"Guitar Lessons & Practice\",\"Chord Voicings\",true,true,rules,vid33333333,\"Quick Guitar Chord Trick\",\"Paul Davids\",0:55,https://www.youtube.com/watch?v=vid33333333"
        )
        .unwrap();

        let pool = init_pool(&db_path).await.unwrap();
        import_csv(&pool, &csv_path, None).await.unwrap();
        (pool, tmp)
    }

    #[test]
    fn sanitizes_fts_queries_safely() {
        assert_eq!(sanitize_fts_query("C++ neural!"), Some("\"C\"* AND \"neural\"*".to_string()));
        assert_eq!(sanitize_fts_query("الدحيح"), Some("\"الدحيح\"*".to_string()));
        assert_eq!(sanitize_fts_query("   !!!  "), None);
    }

    #[tokio::test]
    async fn filters_recommends_and_persists_activity_without_mutating_metadata() {
        let (pool, tmp) = setup_test_db().await;

        // 1. FTS5 + category + duration filtering
        let mut p = FilterParams::default();
        p.q = "Plasticity".to_string();
        p.category = "Neuroscience & Cognitive Neuroscience".to_string();
        p.bucket = "Deep Dive (20-60m)".to_string();
        let (cnt, rows, _) = query_videos(&pool, &p).await.unwrap();
        assert_eq!(cnt, 1);
        assert_eq!(rows[0].video_id, "vid11111111");

        // 2. Choose for me picks top unwatched video and records last_shown_at
        let rec1 = choose_for_me(&pool, &FilterParams::default()).await.unwrap().unwrap();
        assert_eq!(rec1.video.video_id, "vid11111111");

        // Next Choose for me diversifies away from recently shown video/category
        let rec2 = choose_for_me(&pool, &FilterParams::default()).await.unwrap().unwrap();
        assert_ne!(rec2.video.video_id, "vid11111111");

        // 3. Activity actions: mark watched, favorite, queue, skip
        let w = apply_activity_action(&pool, "vid11111111", ActivityAction::ToggleWatched)
            .await
            .unwrap()
            .unwrap();
        assert!(w.is_watched());

        let fav = apply_activity_action(&pool, "vid22222222", ActivityAction::ToggleFavorite)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fav.favorited, 1);

        let q = apply_activity_action(&pool, "vid33333333", ActivityAction::ToggleQueue)
            .await
            .unwrap()
            .unwrap();
        assert!(q.is_in_queue());

    let counts = get_nav_counts(&pool).await.unwrap();
    assert_eq!(counts.total, 3);
    assert_eq!(counts.queue, 1);
    assert_eq!(counts.unwatched, 2);
    assert_eq!(counts.guitar, 1);

        // Original importance_score and rank remain untouched
        let (orig_score, orig_rank): (i64, i64) =
            sqlx::query_as("SELECT importance_score, rank FROM videos WHERE video_id = 'vid11111111'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(orig_score, 98);
        assert_eq!(orig_rank, 1);

        // Choose for me never picks watched video
        let rec3 = choose_for_me(&pool, &FilterParams::default()).await.unwrap().unwrap();
        assert_ne!(rec3.video.video_id, "vid11111111");

        pool.close().await;
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
