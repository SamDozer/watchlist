use askama::Template;
use axum::{
    extract::{Form, Path, Query, State},
    http::{header, StatusCode},
    middleware,
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower_http::services::ServeDir;

use crate::auth::{require_auth, AuthConfig};
use crate::discovery::{
    apply_activity_action, choose_for_me, get_bucket_counts, get_category_groups, get_nav_counts,
    get_subgenres, query_videos, ActivityAction,
};
use crate::models::{
    BucketCounts, CategoryGroup, CountOption, FilterParams, NavCounts, Recommendation, VideoCardRow,
};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    #[allow(dead_code)]
    pub fts5_enabled: bool,
    pub auth: AuthConfig,
    pub login_failures: Arc<Mutex<Vec<Instant>>>,
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    error: Option<String>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    tab: String,
    auth_enabled: bool,
    counts: NavCounts,
    buckets: BucketCounts,
    params: FilterParams,
    category_groups: Vec<CategoryGroup>,
    subgenres: Vec<CountOption>,
    filtered_total: i64,
    videos: Vec<VideoCardRow>,
    next_offset: Option<i64>,
}

#[derive(Template)]
#[template(path = "partials/video_list.html")]
struct VideoListTemplate {
    tab: String,
    params: FilterParams,
    filtered_total: i64,
    videos: Vec<VideoCardRow>,
    next_offset: Option<i64>,
    subgenres: Vec<CountOption>,
    selected_subgenre: String,
    include_subgenre_oob: bool,
}

#[derive(Template)]
#[template(path = "partials/video_cards.html")]
struct VideoCardsTemplate {
    tab: String,
    videos: Vec<VideoCardRow>,
    next_offset: Option<i64>,
}

#[derive(Template)]
#[template(path = "partials/card_response.html")]
struct CardResponseTemplate {
    v: VideoCardRow,
    counts: NavCounts,
    tab: String,
    auth_enabled: bool,
}

#[derive(Template)]
#[template(path = "partials/choose_spotlight.html")]
struct ChooseSpotlightTemplate {
    rec: Option<Recommendation>,
    counts: Option<NavCounts>,
    tab: String,
    auth_enabled: bool,
    include_nav_oob: bool,
}

#[derive(Deserialize)]
pub struct ActionQuery {
    kind: String,
    #[serde(default)]
    spotlight: String,
    #[serde(default)]
    tab: String,
}

#[derive(Deserialize)]
pub struct LoginForm {
    password: String,
}

fn render_html<T: Template>(tmpl: T) -> impl IntoResponse {
    match tmpl.render() {
        Ok(body) => (StatusCode::OK, Html(body)).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template render error: {err}"),
        )
            .into_response(),
    }
}

async fn login_get_handler(State(state): State<AppState>) -> impl IntoResponse {
    if !state.auth.is_enabled() {
        return Redirect::to("/").into_response();
    }
    render_html(LoginTemplate { error: None }).into_response()
}

async fn login_post_handler(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    if !state.auth.is_enabled() {
        return Redirect::to("/").into_response();
    }

    // Rate limit: max 5 failed attempts per 60 seconds
    {
        let mut attempts = state.login_failures.lock().unwrap_or_else(|e| e.into_inner());
        let cutoff = Instant::now() - Duration::from_secs(60);
        attempts.retain(|&t| t > cutoff);
        if attempts.len() >= 5 {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [("Retry-After", "60")],
                render_html(LoginTemplate {
                    error: Some("Too many failed attempts. Please wait 60 seconds.".to_string()),
                }),
            )
                .into_response();
        }
    }

    if state.auth.verify_password(&form.password) {
        if let Ok(mut attempts) = state.login_failures.lock() {
            attempts.clear();
        }
        let cookie = state.auth.issue_session_cookie();
        return (
            [(header::SET_COOKIE, cookie)],
            Redirect::to("/"),
        )
            .into_response();
    }

    if let Ok(mut attempts) = state.login_failures.lock() {
        attempts.push(Instant::now());
    }
    (
        StatusCode::UNAUTHORIZED,
        render_html(LoginTemplate {
            error: Some("Incorrect password".to_string()),
        }),
    )
        .into_response()
}

async fn logout_handler(State(state): State<AppState>) -> impl IntoResponse {
    let cookie = state.auth.clear_session_cookie();
    ([(header::SET_COOKIE, cookie)], Redirect::to("/login")).into_response()
}

async fn render_page(state: AppState, mut params: FilterParams, forced_tab: Option<&str>) -> impl IntoResponse {
    if let Some(tab) = forced_tab {
        params.tab = tab.to_string();
    } else {
        params.tab = params.normalized_tab().to_string();
    }

    let counts = match get_nav_counts(&state.pool).await {
        Ok(c) => c,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {err}")).into_response(),
    };
    let buckets = match get_bucket_counts(&state.pool).await {
        Ok(b) => b,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {err}")).into_response(),
    };
    let category_groups = match get_category_groups(&state.pool).await {
        Ok(c) => c,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {err}")).into_response(),
    };
    let subgenres = match get_subgenres(&state.pool, &params.category).await {
        Ok(s) => s,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {err}")).into_response(),
    };

    let auth_enabled = state.auth.is_enabled();
    match query_videos(&state.pool, &params).await {
        Ok((filtered_total, videos, next_offset)) => render_html(IndexTemplate {
            tab: params.tab.clone(),
            auth_enabled,
            counts,
            buckets,
            params,
            category_groups,
            subgenres,
            filtered_total,
            videos,
            next_offset,
        })
        .into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {err}")).into_response(),
    }
}

async fn all_handler(
    State(state): State<AppState>,
    Query(params): Query<FilterParams>,
) -> impl IntoResponse {
    render_page(state, params, None).await
}

async fn queue_handler(
    State(state): State<AppState>,
    Query(params): Query<FilterParams>,
) -> impl IntoResponse {
    render_page(state, params, Some("queue")).await
}

async fn videos_partial_handler(
    State(state): State<AppState>,
    Query(mut params): Query<FilterParams>,
) -> impl IntoResponse {
    params.tab = params.normalized_tab().to_string();

    if params.offset > 0 {
        return match query_videos(&state.pool, &params).await {
            Ok((_total, videos, next_offset)) => render_html(VideoCardsTemplate {
                tab: params.tab.clone(),
                videos,
                next_offset,
            })
            .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {err}")).into_response(),
        };
    }

    let subgenres = match get_subgenres(&state.pool, &params.category).await {
        Ok(s) => s,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {err}")).into_response(),
    };
    if !params.subgenre.is_empty() && !subgenres.iter().any(|s| s.name == params.subgenre) {
        params.subgenre.clear();
    }

    match query_videos(&state.pool, &params).await {
        Ok((filtered_total, videos, next_offset)) => render_html(VideoListTemplate {
            tab: params.tab.clone(),
            selected_subgenre: params.subgenre.clone(),
            params,
            filtered_total,
            videos,
            next_offset,
            subgenres,
            include_subgenre_oob: true,
        })
        .into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {err}")).into_response(),
    }
}

async fn choose_handler(
    State(state): State<AppState>,
    Form(params): Form<FilterParams>,
) -> impl IntoResponse {
    let tab = params.normalized_tab().to_string();
    let auth_enabled = state.auth.is_enabled();
    match choose_for_me(&state.pool, &params).await {
        Ok(rec) => render_html(ChooseSpotlightTemplate {
            rec,
            counts: None,
            tab,
            auth_enabled,
            include_nav_oob: false,
        })
        .into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {err}")).into_response(),
    }
}

async fn action_handler(
    State(state): State<AppState>,
    Path(video_id): Path<String>,
    Query(query): Query<ActionQuery>,
) -> impl IntoResponse {
    let Some(action) = ActivityAction::from_str(&query.kind) else {
        return (StatusCode::BAD_REQUEST, "Unknown action").into_response();
    };

    let updated = match apply_activity_action(&state.pool, &video_id, action).await {
        Ok(Some(v)) => v,
        Ok(None) => return (StatusCode::NOT_FOUND, "Video not found").into_response(),
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {err}")).into_response(),
    };

    let counts = match get_nav_counts(&state.pool).await {
        Ok(c) => c,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {err}")).into_response(),
    };

    let tab = if query.tab == "queue" {
        "queue".to_string()
    } else {
        "all".to_string()
    };
    let auth_enabled = state.auth.is_enabled();

    if query.spotlight == "1" {
        let status_label = if updated.is_watched() {
            "Marked as watched"
        } else if updated.is_skipped() {
            "Skipped"
        } else if updated.is_in_queue() {
            "Saved to Queue"
        } else {
            "Updated"
        };
        return render_html(ChooseSpotlightTemplate {
            rec: Some(crate::models::Recommendation {
                video: updated,
                reason: status_label.to_string(),
                computed_score: 0,
            }),
            counts: Some(counts),
            tab,
            auth_enabled,
            include_nav_oob: true,
        })
        .into_response();
    }

    render_html(CardResponseTemplate {
        v: updated,
        counts,
        tab,
        auth_enabled,
    })
    .into_response()
}

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let ok = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();
    if ok {
        (StatusCode::OK, "ok").into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "db error").into_response()
    }
}

pub fn build_router(state: AppState) -> Router {
    let auth_cfg = state.auth.clone();
    Router::new()
        .route("/login", get(login_get_handler).post(login_post_handler))
        .route("/logout", post(logout_handler))
        .route("/", get(all_handler))
        .route("/queue", get(queue_handler))
        .route("/videos/partial", get(videos_partial_handler))
        .route("/choose", post(choose_handler))
        .route("/videos/{video_id}/action", post(action_handler))
        .route("/health", get(health_handler))
        .nest_service("/static", ServeDir::new("static"))
        .layer(middleware::from_fn_with_state(auth_cfg, require_auth))
        .with_state(state)
}
