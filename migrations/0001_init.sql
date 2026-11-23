-- Original catalog metadata (never overwritten by user behavior)
CREATE TABLE IF NOT EXISTS videos (
    video_id          TEXT PRIMARY KEY NOT NULL,
    rank              INTEGER NOT NULL,
    importance_score  INTEGER NOT NULL,
    language          TEXT NOT NULL,
    watch_time_bucket TEXT NOT NULL,
    category          TEXT NOT NULL,
    subgenre          TEXT NOT NULL DEFAULT '',
    is_guitar         INTEGER NOT NULL DEFAULT 0,
    is_short          INTEGER NOT NULL DEFAULT 0,
    classified_by     TEXT NOT NULL DEFAULT '',
    title             TEXT NOT NULL,
    channel           TEXT NOT NULL DEFAULT '',
    duration          TEXT NOT NULL DEFAULT '',
    url               TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_videos_rank ON videos (rank);
CREATE INDEX IF NOT EXISTS idx_videos_importance ON videos (importance_score DESC, rank ASC);
CREATE INDEX IF NOT EXISTS idx_videos_category ON videos (category);
CREATE INDEX IF NOT EXISTS idx_videos_subgenre ON videos (subgenre);
CREATE INDEX IF NOT EXISTS idx_videos_language ON videos (language);
CREATE INDEX IF NOT EXISTS idx_videos_bucket ON videos (watch_time_bucket);
CREATE INDEX IF NOT EXISTS idx_videos_guitar ON videos (is_guitar);
CREATE INDEX IF NOT EXISTS idx_videos_short ON videos (is_short);

-- Personal user activity (stored separately from original catalog)
CREATE TABLE IF NOT EXISTS user_activity (
    video_id          TEXT PRIMARY KEY NOT NULL REFERENCES videos(video_id) ON DELETE CASCADE,
    status            TEXT NOT NULL DEFAULT 'unwatched' CHECK (status IN ('unwatched', 'watched', 'skipped')),
    watched_at        TEXT,
    skipped_at        TEXT,
    favorited         INTEGER NOT NULL DEFAULT 0,
    added_to_queue_at TEXT,
    notes             TEXT NOT NULL DEFAULT '',
    last_shown_at     TEXT
);

CREATE INDEX IF NOT EXISTS idx_activity_status ON user_activity (status);
CREATE INDEX IF NOT EXISTS idx_activity_favorited ON user_activity (favorited);
CREATE INDEX IF NOT EXISTS idx_activity_queue ON user_activity (added_to_queue_at);

-- Learned user preferences (separate from original importance_score & rank)
CREATE TABLE IF NOT EXISTS user_preferences (
    pref_type       TEXT NOT NULL CHECK (pref_type IN ('category', 'subgenre', 'duration', 'language')),
    pref_key        TEXT NOT NULL,
    watched_count   INTEGER NOT NULL DEFAULT 0,
    skipped_count   INTEGER NOT NULL DEFAULT 0,
    favorited_count INTEGER NOT NULL DEFAULT 0,
    weight          REAL NOT NULL DEFAULT 0.0,
    PRIMARY KEY (pref_type, pref_key)
);

-- FTS5 full-text search index over title, channel, category, subgenre
CREATE VIRTUAL TABLE IF NOT EXISTS videos_fts USING fts5(
    title,
    channel,
    category,
    subgenre,
    content='videos',
    content_rowid='rowid'
);

CREATE TRIGGER IF NOT EXISTS videos_ai AFTER INSERT ON videos BEGIN
    INSERT INTO videos_fts(rowid, title, channel, category, subgenre)
    VALUES (new.rowid, new.title, new.channel, new.category, new.subgenre);
END;

CREATE TRIGGER IF NOT EXISTS videos_ad AFTER DELETE ON videos BEGIN
    INSERT INTO videos_fts(videos_fts, rowid, title, channel, category, subgenre)
    VALUES ('delete', old.rowid, old.title, old.channel, old.category, old.subgenre);
END;

CREATE TRIGGER IF NOT EXISTS videos_au AFTER UPDATE ON videos BEGIN
    INSERT INTO videos_fts(videos_fts, rowid, title, channel, category, subgenre)
    VALUES ('delete', old.rowid, old.title, old.channel, old.category, old.subgenre);
    INSERT INTO videos_fts(rowid, title, channel, category, subgenre)
    VALUES (new.rowid, new.title, new.channel, new.category, new.subgenre);
END;
