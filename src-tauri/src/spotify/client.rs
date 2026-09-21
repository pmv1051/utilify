//! Centralized Spotify HTTP client.
//!
//! Responsibilities:
//! * Attach the access token, refreshing it proactively before expiry and
//!   reactively on 401.
//! * Respect 429 + `Retry-After`, distinguishing `QUOTA_EXCEEDED` (give up)
//!   from ordinary rate limiting (wait and retry).
//! * Retry transient network / 5xx failures with backoff.
//! * Follow `next` links for paginated endpoints.

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::auth::{self, TokenResponse};
use super::models::{ErrorEnvelope, Paging};
use super::API_BASE;
use crate::db::{config, now, Db};
use crate::error::{AppError, Result};

const MAX_ATTEMPTS: u32 = 5;
const MAX_RETRY_AFTER_SECS: u64 = 60;
/// Refresh the access token this many seconds before it actually expires.
const REFRESH_LEEWAY_SECS: i64 = 60;
/// How long to stop calling a family of endpoints after Spotify reports its
/// quota is gone. The next call after this is allowed through; if it fails the
/// same way the wait starts again.
pub const QUOTA_RETRY_SECS: i64 = 3600;

/// Where cooldown changes are broadcast to the UI (`quota-cooldown` event).
static EVENT_SINK: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

pub fn set_event_sink(app: tauri::AppHandle) {
    let _ = EVENT_SINK.set(app);
}

/// Development Mode reports `QUOTA_EXCEEDED` per family of endpoints, and a
/// family that still has room keeps answering while another is exhausted.
/// Pausing every call on one 429 took the whole app down for one tool's limit,
/// so each family is paused on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaScope {
    /// `/artists`, `/albums`, `/search` — what Discography leans on.
    Catalog,
    /// `/playlists`, `/me/playlists`, `/me/tracks`.
    Playlists,
    /// `/me/player…`
    Player,
}

impl QuotaScope {
    pub fn key(self) -> &'static str {
        match self {
            QuotaScope::Catalog => "catalog",
            QuotaScope::Playlists => "playlists",
            QuotaScope::Player => "player",
        }
    }

    /// What to call this family in a sentence aimed at the user.
    pub fn label(self) -> &'static str {
        match self {
            QuotaScope::Catalog => "artist and album lookups",
            QuotaScope::Playlists => "playlist requests",
            QuotaScope::Player => "playback control",
        }
    }

    fn of_url(url: &str) -> Self {
        let p = url.strip_prefix(API_BASE).unwrap_or(url);
        if p.starts_with("/me/player") {
            QuotaScope::Player
        } else if p.starts_with("/artists") || p.starts_with("/albums") || p.starts_with("/search") {
            QuotaScope::Catalog
        } else {
            QuotaScope::Playlists
        }
    }
}

/// The one call allowed during a pause: the player poll, which the polling
/// loop throttles to a probe cadence while paused.
fn is_probe(method: &Method, url: &str) -> bool {
    let p = url.strip_prefix(API_BASE).unwrap_or(url);
    *method == Method::GET && (p == "/me/player" || p.starts_with("/me/player?"))
}

/// Retry times per paused family, keyed by `QuotaScope::key`. Families that
/// are not paused are absent, so an empty map means everything works.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaStatus {
    pub scopes: std::collections::BTreeMap<String, i64>,
}

impl QuotaStatus {
    pub fn until(&self, scope: QuotaScope) -> Option<i64> {
        self.scopes.get(scope.key()).copied()
    }

    pub fn is_paused(&self, scope: QuotaScope) -> bool {
        self.until(scope).is_some()
    }
}

#[derive(Clone)]
pub struct SpotifyClient {
    http: reqwest::Client,
    db: Db,
    /// Serialises token refreshes so concurrent requests do not each refresh.
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
}

impl SpotifyClient {
    pub fn new(db: Db) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("Utilify/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        Self {
            http,
            db,
            refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    // ---- credentials -------------------------------------------------------

    pub fn client_id(&self) -> Result<String> {
        self.db
            .with(|c| config::get(c, config::CLIENT_ID))?
            .filter(|s| !s.is_empty())
            .ok_or(AppError::NoClientId)
    }

    pub fn is_authenticated(&self) -> bool {
        matches!(
            self.db.with(|c| config::get(c, config::REFRESH_TOKEN)),
            Ok(Some(ref t)) if !t.is_empty()
        )
    }

    pub fn store_tokens(&self, tokens: &TokenResponse) -> Result<()> {
        let expires_at = now() + tokens.expires_in as i64;
        self.db.with(|c| {
            config::set(c, config::ACCESS_TOKEN, &tokens.access_token)?;
            config::set(c, config::TOKEN_EXPIRES_AT, &expires_at.to_string())?;
            if let Some(rt) = &tokens.refresh_token {
                config::set(c, config::REFRESH_TOKEN, rt)?;
            }
            Ok(())
        })
    }

    pub fn clear_tokens(&self) -> Result<()> {
        self.db.with(|c| {
            for key in [
                config::ACCESS_TOKEN,
                config::REFRESH_TOKEN,
                config::TOKEN_EXPIRES_AT,
                config::USER_ID,
                config::USER_DISPLAY_NAME,
                config::USER_PRODUCT,
            ] {
                config::delete(c, key)?;
            }
            Ok(())
        })
    }

    /// Returns a valid access token, refreshing first if it is about to expire.
    async fn access_token(&self) -> Result<String> {
        let (token, expires_at) = self.db.with(|c| {
            Ok((
                config::get(c, config::ACCESS_TOKEN)?,
                config::get(c, config::TOKEN_EXPIRES_AT)?
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0),
            ))
        })?;
        match token {
            Some(t) if !t.is_empty() && expires_at - REFRESH_LEEWAY_SECS > now() => Ok(t),
            _ => self.refresh(None).await,
        }
    }

    /// Refresh the access token. `stale` is the token the caller just used; if
    /// another task already refreshed, the fresh token is returned without a
    /// second round-trip.
    async fn refresh(&self, stale: Option<&str>) -> Result<String> {
        let _guard = self.refresh_lock.lock().await;
        if let Some(stale) = stale {
            if let Some(current) = self.db.with(|c| config::get(c, config::ACCESS_TOKEN))? {
                if current != stale {
                    return Ok(current);
                }
            }
        }
        let refresh_token = self
            .db
            .with(|c| config::get(c, config::REFRESH_TOKEN))?
            .filter(|s| !s.is_empty())
            .ok_or(AppError::NotAuthenticated)?;
        let client_id = self.client_id()?;
        log::debug!("refreshing Spotify access token");
        match auth::refresh_access_token(&self.http, &client_id, &refresh_token).await {
            Ok(tokens) => {
                self.store_tokens(&tokens)?;
                Ok(tokens.access_token)
            }
            Err(AppError::AuthExpired) => {
                log::warn!("refresh token rejected; clearing session");
                self.clear_tokens()?;
                Err(AppError::AuthExpired)
            }
            Err(e) => Err(e),
        }
    }

    // ---- quota cooldown ----------------------------------------------------

    pub fn quota_status(&self) -> QuotaStatus {
        let now = now();
        let mut scopes = self.read_cooldowns();
        scopes.retain(|_, until| *until > now);
        QuotaStatus { scopes }
    }

    fn read_cooldowns(&self) -> std::collections::BTreeMap<String, i64> {
        self.db
            .with(|c| config::get(c, config::QUOTA_COOLDOWNS))
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write_cooldowns(&self, scopes: &std::collections::BTreeMap<String, i64>) {
        let result = if scopes.is_empty() {
            self.db.with(|c| config::delete(c, config::QUOTA_COOLDOWNS))
        } else {
            match serde_json::to_string(scopes) {
                Ok(json) => self.db.with(|c| config::set(c, config::QUOTA_COOLDOWNS, &json)),
                Err(e) => {
                    log::warn!("could not serialise quota cooldowns: {e}");
                    return;
                }
            }
        };
        if let Err(e) = result {
            log::warn!("could not record quota cooldowns: {e}");
        }
        if let Some(app) = EVENT_SINK.get() {
            let _ = tauri::Emitter::emit(app, "quota-cooldown", self.quota_status());
        }
    }

    fn start_quota_cooldown(&self, scope: QuotaScope) {
        let mut scopes = self.quota_status().scopes;
        let until = now() + QUOTA_RETRY_SECS;
        scopes.insert(scope.key().to_string(), until);
        log::warn!(
            "QUOTA_EXCEEDED for {}: paused until {until}; other endpoints keep working",
            scope.key()
        );
        self.write_cooldowns(&scopes);
    }

    /// A successful call proves that family is back: end its pause.
    fn end_quota_cooldown_if_active(&self, scope: QuotaScope) {
        let mut scopes = self.quota_status().scopes;
        if scopes.remove(scope.key()).is_some() {
            log::info!("quota recovered for {}", scope.key());
            self.write_cooldowns(&scopes);
        }
    }

    pub fn clear_quota_cooldown(&self) -> Result<()> {
        self.db.with(|c| config::delete(c, config::QUOTA_COOLDOWNS))?;
        if let Some(app) = EVENT_SINK.get() {
            let _ = tauri::Emitter::emit(app, "quota-cooldown", QuotaStatus::default());
        }
        Ok(())
    }

    // ---- request helpers ---------------------------------------------------

    fn url(path: &str) -> String {
        if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{API_BASE}{path}")
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<Option<T>> {
        self.request(Method::GET, &Self::url(path), query, None).await
    }

    pub async fn post_json<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<Option<T>> {
        self.request(Method::POST, &Self::url(path), &[], Some(body)).await
    }

    #[allow(dead_code)]
    pub async fn put_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Option<T>> {
        self.request(Method::PUT, &Self::url(path), query, body).await
    }

    #[allow(dead_code)]
    pub async fn delete_json<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<Option<T>> {
        self.request(Method::DELETE, &Self::url(path), &[], Some(body)).await
    }

    /// GET a paginated endpoint and follow `next` until exhausted.
    pub async fn get_all_pages<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Vec<T>> {
        let mut out = Vec::new();
        let mut page: Option<Paging<T>> = self.get_first_page(path, query).await?;
        let mut pages = 0usize;
        while let Some(p) = page {
            out.extend(p.items);
            pages += 1;
            match p.next {
                Some(next) => {
                    // Light throttle on long listings to stay well under rate limits.
                    if pages % 5 == 0 {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    page = self.get(&next, &[]).await?;
                }
                None => break,
            }
        }
        Ok(out)
    }

    /// PUT where the response body is irrelevant. Several player endpoints
    /// answer 200 with a non-JSON body, so never try to parse these.
    pub async fn put(&self, path: &str, query: &[(&str, String)], body: Option<&Value>) -> Result<()> {
        self.request_text(Method::PUT, &Self::url(path), query, body).await?;
        Ok(())
    }

    /// POST where the response body is irrelevant.
    pub async fn post(&self, path: &str, body: &Value) -> Result<()> {
        self.request_text(Method::POST, &Self::url(path), &[], Some(body)).await?;
        Ok(())
    }

    /// POST with no body (player transport endpoints).
    pub async fn post_empty(&self, path: &str, query: &[(&str, String)]) -> Result<()> {
        self.request_text(Method::POST, &Self::url(path), query, None).await?;
        Ok(())
    }

    /// DELETE where the response body is irrelevant.
    #[allow(dead_code)]
    pub async fn delete(&self, path: &str, body: &Value) -> Result<()> {
        self.request_text(Method::DELETE, &Self::url(path), &[], Some(body)).await?;
        Ok(())
    }

    /// Typed request: `None` for an empty/204 response, otherwise parsed JSON.
    /// First page of a listing. The Feb 2026 API caps `limit` differently per
    /// endpoint (and per app mode) and answers 400 "Invalid limit" above the
    /// cap, so on that error retry with smaller limits and finally with no
    /// `limit` at all (Spotify's default). `next` links then carry the
    /// accepted value.
    async fn get_first_page<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Option<Paging<T>>> {
        let first = self.get(path, query).await;
        let Some(limit_idx) = query.iter().position(|(k, _)| *k == "limit") else {
            return first;
        };
        let is_limit_error = matches!(
            &first,
            Err(AppError::Spotify { status: 400, message }) if message.to_lowercase().contains("limit")
        );
        if !is_limit_error {
            return first;
        }
        let current: u32 = query[limit_idx].1.parse().unwrap_or(50);
        let mut candidates: Vec<Option<u32>> = [20u32, 10, 5]
            .into_iter()
            .filter(|l| *l < current)
            .map(Some)
            .collect();
        candidates.push(None);
        for cand in candidates {
            let mut q: Vec<(&str, String)> = query.to_vec();
            match cand {
                Some(l) => q[limit_idx].1 = l.to_string(),
                None => {
                    q.remove(limit_idx);
                }
            }
            log::info!(
                "{path}: limit {current} rejected; retrying with {}",
                cand.map(|l| l.to_string()).unwrap_or_else(|| "Spotify's default".into())
            );
            match self.get(path, &q).await {
                Err(AppError::Spotify { status: 400, message }) if message.to_lowercase().contains("limit") => continue,
                other => return other,
            }
        }
        first
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        url: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Option<T>> {
        match self.request_text(method, url, query, body).await? {
            None => Ok(None),
            Some(text) => serde_json::from_str::<T>(&text).map(Some).map_err(|e| {
                let preview: String = text.chars().take(400).collect();
                log::error!("failed to parse response from {url}: {e}\n{preview}");
                AppError::Json(e)
            }),
        }
    }

    /// Core request loop: auth, retries, rate limiting. Returns the raw body
    /// text of a successful response (`None` when empty).
    async fn request_text(
        &self,
        method: Method,
        url: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Option<String>> {
        let scope = QuotaScope::of_url(url);
        if !is_probe(&method, url) {
            if let Some(until) = self.quota_status().until(scope) {
                return Err(AppError::QuotaCooldown {
                    until,
                    what: scope.label().to_string(),
                });
            }
        }
        let mut refreshed_after_401 = false;
        for attempt in 1..=MAX_ATTEMPTS {
            let token = self.access_token().await?;
            let mut req = self.http.request(method.clone(), url).bearer_auth(&token);
            if !query.is_empty() {
                req = req.query(query);
            }
            if let Some(b) = body {
                req = req.json(b);
            } else if matches!(method, Method::PUT | Method::POST) {
                // Spotify wants a Content-Length even for bodiless PUT/POST.
                req = req.header(reqwest::header::CONTENT_LENGTH, "0");
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) if attempt < MAX_ATTEMPTS && (e.is_connect() || e.is_timeout() || e.is_request()) => {
                    log::warn!("{method} {url}: network error ({e}); retrying");
                    tokio::time::sleep(backoff(attempt)).await;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };

            let status = resp.status();
            let retry_after = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            let text = resp.text().await.unwrap_or_default();

            if status.is_success() {
                self.end_quota_cooldown_if_active(scope);
                if status == StatusCode::NO_CONTENT || text.trim().is_empty() {
                    return Ok(None);
                }
                return Ok(Some(text));
            }

            let envelope = serde_json::from_str::<ErrorEnvelope>(&text).ok();
            let message = envelope
                .as_ref()
                .and_then(|e| e.error.message.clone())
                .unwrap_or_else(|| text.chars().take(200).collect());
            let reason = envelope.as_ref().and_then(|e| e.error.reason.clone());
            if status != StatusCode::UNAUTHORIZED {
                let qs = if query.is_empty() {
                    String::new()
                } else {
                    format!(
                        "?{}",
                        query.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
                    )
                };
                log::warn!(
                    "{method} {url}{qs} -> {status}: {message}{}",
                    reason.as_deref().map(|r| format!(" (reason {r})")).unwrap_or_default()
                );
            }

            match status {
                StatusCode::UNAUTHORIZED => {
                    if refreshed_after_401 {
                        return Err(AppError::AuthExpired);
                    }
                    log::debug!("401 from {url}; refreshing token and retrying");
                    self.refresh(Some(&token)).await?;
                    refreshed_after_401 = true;
                    continue;
                }
                StatusCode::TOO_MANY_REQUESTS => {
                    if reason.as_deref() == Some("QUOTA_EXCEEDED") || text.contains("QUOTA_EXCEEDED") {
                        self.start_quota_cooldown(scope);
                        return Err(AppError::QuotaExceeded {
                            what: scope.label().to_string(),
                        });
                    }
                    if attempt >= MAX_ATTEMPTS {
                        return Err(AppError::RateLimited);
                    }
                    let wait = retry_after.unwrap_or(2).clamp(1, MAX_RETRY_AFTER_SECS);
                    log::warn!("429 from {url}; waiting {wait}s (attempt {attempt})");
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    continue;
                }
                StatusCode::FORBIDDEN => {
                    if reason.as_deref() == Some("PREMIUM_REQUIRED") || message.contains("PREMIUM_REQUIRED") {
                        return Err(AppError::PremiumRequired);
                    }
                    return Err(AppError::Spotify { status: 403, message });
                }
                StatusCode::NOT_FOUND => {
                    if reason.as_deref() == Some("NO_ACTIVE_DEVICE") || message.contains("No active device") {
                        return Err(AppError::NoActiveDevice);
                    }
                    return Err(AppError::Spotify { status: 404, message });
                }
                s if s.is_server_error() && attempt < MAX_ATTEMPTS => {
                    log::warn!("{s} from {url}; retrying");
                    tokio::time::sleep(backoff(attempt)).await;
                    continue;
                }
                s => {
                    return Err(AppError::Spotify {
                        status: s.as_u16(),
                        message,
                    })
                }
            }
        }
        Err(AppError::RateLimited)
    }
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500 * 2u64.pow(attempt.min(5)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_land_in_the_right_family() {
        let f = |p: &str| QuotaScope::of_url(&format!("{API_BASE}{p}"));
        assert_eq!(f("/artists/abc/albums"), QuotaScope::Catalog);
        assert_eq!(f("/albums/abc/tracks"), QuotaScope::Catalog);
        assert_eq!(f("/search?q=x"), QuotaScope::Catalog);
        assert_eq!(f("/playlists/abc/items"), QuotaScope::Playlists);
        assert_eq!(f("/me/playlists"), QuotaScope::Playlists);
        assert_eq!(f("/me/tracks"), QuotaScope::Playlists);
        assert_eq!(f("/me/player"), QuotaScope::Player);
        assert_eq!(f("/me/player/play"), QuotaScope::Player);
        // Anything unrecognised is treated as a playlist call rather than
        // silently escaping the pause.
        assert_eq!(f("/something/new"), QuotaScope::Playlists);
    }

    fn client() -> (SpotifyClient, tempdir::Guard) {
        let guard = tempdir::make();
        let db = Db::open(&guard.path.join("t.db")).unwrap();
        (SpotifyClient::new(db), guard)
    }

    /// The whole point of the change: one exhausted family must not stop the
    /// others.
    #[test]
    fn pausing_one_family_leaves_the_others_alone() {
        let (c, _g) = client();
        c.start_quota_cooldown(QuotaScope::Catalog);

        let s = c.quota_status();
        assert!(s.is_paused(QuotaScope::Catalog));
        assert!(!s.is_paused(QuotaScope::Playlists));
        assert!(!s.is_paused(QuotaScope::Player));

        c.start_quota_cooldown(QuotaScope::Playlists);
        let s = c.quota_status();
        assert!(s.is_paused(QuotaScope::Catalog));
        assert!(s.is_paused(QuotaScope::Playlists));
        assert!(!s.is_paused(QuotaScope::Player));
    }

    #[test]
    fn a_success_clears_only_its_own_family() {
        let (c, _g) = client();
        c.start_quota_cooldown(QuotaScope::Catalog);
        c.start_quota_cooldown(QuotaScope::Playlists);

        c.end_quota_cooldown_if_active(QuotaScope::Catalog);
        let s = c.quota_status();
        assert!(!s.is_paused(QuotaScope::Catalog));
        assert!(s.is_paused(QuotaScope::Playlists));
    }

    #[test]
    fn a_pause_lapses_so_the_next_attempt_goes_through() {
        let (c, _g) = client();
        c.start_quota_cooldown(QuotaScope::Catalog);
        let until = c.quota_status().until(QuotaScope::Catalog).unwrap();
        assert!(until > now() && until <= now() + QUOTA_RETRY_SECS);

        // Rewind the stored deadline past its end.
        let mut scopes = std::collections::BTreeMap::new();
        scopes.insert(QuotaScope::Catalog.key().to_string(), now() - 1);
        c.write_cooldowns(&scopes);
        assert!(!c.quota_status().is_paused(QuotaScope::Catalog));
    }

    #[test]
    fn clearing_removes_every_pause() {
        let (c, _g) = client();
        c.start_quota_cooldown(QuotaScope::Catalog);
        c.start_quota_cooldown(QuotaScope::Player);
        c.clear_quota_cooldown().unwrap();
        let s = c.quota_status();
        assert!(s.scopes.is_empty());
    }

    mod tempdir {
        use std::path::PathBuf;

        pub struct Guard {
            pub path: PathBuf,
        }

        impl Drop for Guard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }

        pub fn make() -> Guard {
            let path = std::env::temp_dir().join(format!(
                "utilify-quota-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Guard { path }
        }
    }
}
