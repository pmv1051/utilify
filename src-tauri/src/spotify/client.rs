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
        let mut page: Option<Paging<T>> = self.get(path, query).await?;
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
                log::warn!(
                    "{method} {url} -> {status}: {message}{}",
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
                        return Err(AppError::QuotaExceeded);
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
