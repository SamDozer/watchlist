use axum::{
    body::Body,
    extract::State,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

const COOKIE_NAME: &str = "watchlist_session";
const SESSION_TTL_SECS: u64 = 60 * 60 * 24 * 30; // 30 days

#[derive(Clone)]
pub struct AuthConfig {
    pub password: Option<String>,
    pub secret: Vec<u8>,
    pub secure_cookie: bool,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        let password = std::env::var("WATCHLIST_PASSWORD")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let secret = std::env::var("WATCHLIST_SESSION_SECRET")
            .ok()
            .filter(|s| s.len() >= 16)
            .map(|s| s.into_bytes())
            .unwrap_or_else(generate_ephemeral_secret);

        let secure_cookie = matches!(
            std::env::var("WATCHLIST_SECURE_COOKIE").as_deref(),
            Ok("1") | Ok("true")
        );

        Self {
            password,
            secret,
            secure_cookie,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.password.is_some()
    }

    pub fn verify_password(&self, candidate: &str) -> bool {
        let Some(expected) = &self.password else {
            return true;
        };
        let a = Sha256::digest(expected.as_bytes());
        let b = Sha256::digest(candidate.as_bytes());
        a.ct_eq(&b).into()
    }

    pub fn issue_session_cookie(&self) -> String {
        let exp = now_secs() + SESSION_TTL_SECS;
        let payload = format!("v1.{exp}");
        let sig = self.sign(&payload);
        let token = format!("{payload}.{sig}");
        let secure_attr = if self.secure_cookie { "; Secure" } else { "" };
        format!(
            "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECS}{secure_attr}"
        )
    }

    pub fn clear_session_cookie(&self) -> String {
        let secure_attr = if self.secure_cookie { "; Secure" } else { "" };
        format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_attr}")
    }

    pub fn verify_cookie_header(&self, cookie_header: Option<&str>) -> bool {
        if !self.is_enabled() {
            return true;
        }
        let Some(raw) = cookie_header else {
            return false;
        };
        for part in raw.split(';') {
            let trimmed = part.trim();
            if let Some(val) = trimmed.strip_prefix(&format!("{COOKIE_NAME}=")) {
                return self.verify_token(val);
            }
        }
        false
    }

    fn verify_token(&self, token: &str) -> bool {
        let mut parts = token.split('.');
        let (Some(ver), Some(exp_str), Some(sig_hex), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        if ver != "v1" {
            return false;
        }
        let Ok(exp) = exp_str.parse::<u64>() else {
            return false;
        };
        if exp < now_secs() {
            return false;
        }
        let payload = format!("{ver}.{exp_str}");
        let expected_sig = self.sign(&payload);
        expected_sig
            .as_bytes()
            .ct_eq(sig_hex.as_bytes())
            .into()
    }

    fn sign(&self, payload: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC key length valid");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn generate_ephemeral_secret() -> Vec<u8> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(pid.to_le_bytes());
    hasher.finalize().to_vec()
}

pub async fn require_auth(
    State(auth): State<AuthConfig>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if !auth.is_enabled() {
        return next.run(req).await;
    }

    let path = req.uri().path();
    let is_public = path == "/login"
        || path == "/health"
        || path == "/static/style.css"
        || path == "/static/manifest.json"
        || path == "/static/sw.js"
        || path.starts_with("/static/icon-");

    if is_public {
        return next.run(req).await;
    }

    let cookie_hdr = req
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok());

    if auth.verify_cookie_header(cookie_hdr) {
        return next.run(req).await;
    }

    let is_htmx = req.headers().contains_key("hx-request");
    if is_htmx {
        return (
            StatusCode::UNAUTHORIZED,
            [("HX-Redirect", "/login")],
            "Unauthorized",
        )
            .into_response();
    }

    Redirect::to("/login").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_verifies_session_cookies() {
        let cfg = AuthConfig {
            password: Some("my-secret-pass".to_string()),
            secret: b"test-secret-key-at-least-16-bytes".to_vec(),
            secure_cookie: false,
        };
        assert!(cfg.verify_password("my-secret-pass"));
        assert!(!cfg.verify_password("wrong-pass"));

        let cookie = cfg.issue_session_cookie();
        let pair = cookie.split(';').next().unwrap();
        assert!(cfg.verify_cookie_header(Some(pair)));
        assert!(!cfg.verify_cookie_header(Some("watchlist_session=v1.9999999999.deadbeef")));
        assert!(!cfg.verify_cookie_header(None));
    }
}
