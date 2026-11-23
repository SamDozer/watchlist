# Watchlist

A minimal, fast, private video-discovery web app built with **Rust + Axum + SQLite (FTS5) + Askama + HTMX**.

Instead of endlessly scrolling through thousands of saved YouTube videos, Watchlist imports your categorized CSV into SQLite and helps you decide **what to watch next** in seconds—on desktop or mobile—without embedded players, JavaScript frameworks, or external AI APIs.

---

## Screenshots

*(Captured using the included public `demo/sample_watchlist.csv` dataset.)*

### Desktop
![Watchlist Desktop](docs/screenshots/desktop.png)

### Mobile
![Watchlist Mobile](docs/screenshots/mobile.png)

---

## Why Watchlist?

- **Tiny & Fast:** Single Rust binary, server-rendered HTML with Askama, and local HTMX (`50 KB`, zero npm/React/build pipeline).
- **Instant Full-Text Search:** Powered by SQLite **FTS5** across titles, channels, categories, and subgenres (supports English and Arabic RTL).
- **"Choose for me":** Lightweight, explainable recommendation scorer combining `importanceScore`, `rank`, personal category/subgenre/duration/language preferences, and anti-repeat diversity.
- **Metadata Integrity:** Original catalog rows (`videos`) stay untouched; all personal activity (`watched`, `skipped`, `favorited`, `Watch Later` queue) and learned preference weights live separately in `user_activity` and `user_preferences`.
- **Direct YouTube Links:** Opens original `youtube.com/watch?v=...` URLs in a new tab or the YouTube app—no embedded playback, tracking, or analytics.

---

## Architecture

```text
watchlist/
├── Cargo.toml
├── migrations/
│   └── 0001_init.sql          # videos, videos_fts (FTS5), user_activity, user_preferences
├── src/
│   ├── main.rs                # CLI (--import, --db, --port, --serve) & 127.0.0.1 server
│   ├── db.rs                  # SQLite WAL pool, migrations, FTS5 check, pre-import backup
│   ├── import.rs              # UTF-8 BOM-safe, idempotent CSV importer + import report
│   ├── discovery.rs           # FTS5 query builder, filters, "Choose for me", activity actions
│   ├── models.rs              # Typed SQLx rows, filter params, nav counts
│   └── routes.rs              # Axum routes + Askama HTML / HTMX partial responses
├── templates/                 # Askama server-side HTML templates
├── static/                    # Custom dark-first CSS + vendored htmx.min.js
└── demo/
    └── sample_watchlist.csv   # Public educational sample dataset
```

---

## Quick Start

### 1. Prerequisites
- [Rust](https://rustup.rs/) (stable toolchain)

### 2. Import the Demo CSV & Run
```bash
# Build and run tests
cargo test

# Import the included demo CSV (idempotent; backs up existing DB before re-import)
cargo run -- --import demo/sample_watchlist.csv --serve --port 3000
```

Open **http://127.0.0.1:3000** in your browser.

### 3. Import Your Own CSV
Your CSV file is never committed or modified. Pass its path via `--import`:

```bash
cargo run -- --db ./data/watchlist.db --import /path/to/your_videos.csv --serve
```

Required CSV columns:
`rank,importanceScore,language,watchTimeBucket,category,subgenre,isGuitar,isShort,classifiedBy,videoId,title,channel,duration,url`

---

## Navigation & Features

- **Discover:** Unwatched videos ranked by importance and personal preferences, plus **Choose for me**.
- **Explore:** Browse the full catalog with status filtering (`Unwatched`, `Watched`, `Skipped`, `All`).
- **Watch Later / Watched / Favorites:** Persistent personal views updated live via HTMX.
- **Filters:** FTS5 search, Category, dynamic Subgenre, Duration (`<5m`, `5–20m`, `20–60m`, `>60m`), Language, Shorts toggle, and Guitar-only toggle.
