use serde::Deserialize;
use sqlx::FromRow;

#[allow(dead_code)]
#[derive(Debug, Clone, FromRow)]
pub struct VideoCardRow {
    pub video_id: String,
    pub rank: i64,
    pub importance_score: i64,
    pub language: String,
    pub watch_time_bucket: String,
    pub category: String,
    pub subgenre: String,
    pub is_guitar: i64,
    pub is_short: i64,
    pub classified_by: String,
    pub title: String,
    pub channel: String,
    pub duration: String,
    pub url: String,
    pub status: String,
    pub favorited: i64,
    pub in_queue: i64,
}

impl VideoCardRow {
    pub fn thumbnail_url(&self) -> String {
        format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", self.video_id)
    }

    pub fn display_channel(&self) -> &str {
        if self.channel.trim().is_empty() {
            "Unknown channel"
        } else {
            self.channel.as_str()
        }
    }

    pub fn text_dir(&self) -> &str {
        if self.language == "ar" {
            "rtl"
        } else {
            "ltr"
        }
    }

    pub fn is_watched(&self) -> bool {
        self.status == "watched"
    }

    pub fn is_skipped(&self) -> bool {
        self.status == "skipped"
    }

    pub fn is_in_queue(&self) -> bool {
        self.in_queue != 0
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FilterParams {
    #[serde(default)]
    pub tab: String,
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub subgenre: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub guitar: String,
    #[serde(default)]
    pub short: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub offset: i64,
}

impl FilterParams {
    pub fn normalized_tab(&self) -> &str {
        match self.tab.as_str() {
            "queue" => "queue",
            _ => "all",
        }
    }

    pub fn effective_status(&self) -> &str {
        if !self.status.is_empty() {
            return self.status.as_str();
        }
        match self.normalized_tab() {
            "all" => "unwatched",
            _ => "all",
        }
    }

    pub fn has_active_filters(&self) -> bool {
        !self.q.trim().is_empty()
            || !self.category.is_empty()
            || !self.subgenre.is_empty()
            || !self.language.is_empty()
            || !self.bucket.is_empty()
            || self.guitar == "1"
            || !self.short.is_empty()
            || !self.status.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct NavCounts {
    pub total: i64,
    pub unwatched: i64,
    pub queue: i64,
    pub guitar: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct CountOption {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone)]
pub struct CategoryGroup {
    pub label: String,
    pub items: Vec<CountOption>,
}

#[derive(Debug, Clone, Default)]
pub struct BucketCounts {
    pub quick: i64,
    pub medium: i64,
    pub deep_dive: i64,
    pub long_form: i64,
}

#[derive(Debug, Clone)]
pub struct Recommendation {
    pub video: VideoCardRow,
    pub reason: String,
    pub computed_score: i64,
}
