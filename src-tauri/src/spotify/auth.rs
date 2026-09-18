//! Spotify OAuth 2.0 Authorization Code flow with PKCE.
//!
//! The user's own Client ID is used; there is no client secret. The callback
//! is received on a loopback HTTP listener (`http://127.0.0.1:8377/callback`)
//! which the user must register verbatim as a Redirect URI in their Spotify app.

use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::distr::Alphanumeric;
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::ACCOUNTS_BASE;
use crate::error::{AppError, Result};

pub const REDIRECT_PORT: u16 = 8377;
pub const CALLBACK_PATH: &str = "/callback";

pub const SCOPES: &[&str] = &[
    "playlist-read-private",
    "playlist-read-collaborative",
    "playlist-modify-public",
    "playlist-modify-private",
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-read-currently-playing",
    "user-library-read",
    "user-follow-read",
    "user-read-private",
];

pub fn redirect_uri() -> String {
    format!("http://127.0.0.1:{REDIRECT_PORT}{CALLBACK_PATH}")
}

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

pub fn generate_pkce() -> Pkce {
    let verifier = random_string(64);
    let digest = Sha256::digest(verifier.as_bytes());
    Pkce {
        challenge: URL_SAFE_NO_PAD.encode(digest),
        verifier,
        state: random_string(24),
    }
}

fn random_string(len: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

pub fn authorize_url(client_id: &str, pkce: &Pkce) -> Result<String> {
    let url = url::Url::parse_with_params(
        &format!("{ACCOUNTS_BASE}/authorize"),
        &[
            ("client_id", client_id),
            ("response_type", "code"),
            ("redirect_uri", &redirect_uri()),
            ("code_challenge_method", "S256"),
            ("code_challenge", &pkce.challenge),
            ("state", &pkce.state),
            ("scope", &SCOPES.join(" ")),
        ],
    )
    .map_err(|e| AppError::other(format!("bad authorize URL: {e}")))?;
    Ok(url.into())
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    pub expires_in: u64,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

async fn token_request(http: &reqwest::Client, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let resp = http
        .post(format!("{ACCOUNTS_BASE}/api/token"))
        .form(form)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    if status.is_success() {
        return Ok(serde_json::from_str(&text)?);
    }
    if let Ok(err) = serde_json::from_str::<TokenError>(&text) {
        // invalid_grant on refresh means the refresh token was revoked/expired.
        if err.error == "invalid_grant" {
            return Err(AppError::AuthExpired);
        }
        return Err(AppError::Auth(format!(
            "{}: {}",
            err.error,
            err.error_description.unwrap_or_default()
        )));
    }
    Err(AppError::Auth(format!("token endpoint returned {status}: {text}")))
}

pub async fn exchange_code(
    http: &reqwest::Client,
    client_id: &str,
    code: &str,
    verifier: &str,
) -> Result<TokenResponse> {
    let redirect = redirect_uri();
    token_request(
        http,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &redirect),
            ("client_id", client_id),
            ("code_verifier", verifier),
        ],
    )
    .await
}

pub async fn refresh_access_token(
    http: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenResponse> {
    token_request(
        http,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ],
    )
    .await
}

/// Loopback listener that waits for Spotify to redirect the browser back.
pub struct CallbackListener {
    server: tiny_http::Server,
}

impl CallbackListener {
    pub fn bind() -> Result<Self> {
        let server = tiny_http::Server::http(("127.0.0.1", REDIRECT_PORT)).map_err(|e| {
            AppError::Auth(format!(
                "Could not listen on 127.0.0.1:{REDIRECT_PORT} for the Spotify callback ({e}). \
                 Is another copy of Utilify running?"
            ))
        })?;
        Ok(Self { server })
    }

    /// Blocks until the callback arrives (or `timeout` passes). Returns the
    /// authorization code. Call from a blocking thread.
    pub fn wait_for_code(self, expected_state: &str, timeout: Duration) -> Result<String> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(AppError::Auth(
                    "Timed out waiting for Spotify to redirect back. Try connecting again.".into(),
                ));
            }
            let request = match self.server.recv_timeout(remaining.min(Duration::from_secs(2))) {
                Ok(Some(r)) => r,
                Ok(None) => continue,
                Err(e) => return Err(AppError::Auth(format!("callback listener failed: {e}"))),
            };

            let url = format!("http://127.0.0.1{}", request.url());
            let parsed = match url::Url::parse(&url) {
                Ok(u) => u,
                Err(_) => {
                    let _ = request.respond(html_response(400, "Bad request"));
                    continue;
                }
            };
            if parsed.path() != CALLBACK_PATH {
                let _ = request.respond(html_response(404, "Not found"));
                continue;
            }

            let mut code = None;
            let mut state = None;
            let mut error = None;
            for (k, v) in parsed.query_pairs() {
                match k.as_ref() {
                    "code" => code = Some(v.into_owned()),
                    "state" => state = Some(v.into_owned()),
                    "error" => error = Some(v.into_owned()),
                    _ => {}
                }
            }

            if let Some(err) = error {
                let _ = request.respond(html_response(
                    200,
                    "Spotify authorization was denied. You can close this tab and try again in Utilify.",
                ));
                return Err(AppError::Auth(format!("Spotify authorization denied: {err}")));
            }
            if state.as_deref() != Some(expected_state) {
                let _ = request.respond(html_response(400, "State mismatch. Close this tab and try again."));
                return Err(AppError::Auth(
                    "OAuth state mismatch. Close any old Spotify tabs and try again.".into(),
                ));
            }
            match code {
                Some(code) => {
                    let _ = request.respond(html_response(
                        200,
                        "Utilify is connected to Spotify. You can close this tab.",
                    ));
                    return Ok(code);
                }
                None => {
                    let _ = request.respond(html_response(400, "Missing authorization code."));
                    continue;
                }
            }
        }
    }
}

fn html_response(status: u16, message: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Utilify</title>\
         <style>body{{font-family:system-ui,sans-serif;background:#0f1115;color:#f4f4f5;\
         display:flex;align-items:center;justify-content:center;height:100vh;margin:0}}\
         .card{{background:#171a21;border:1px solid #2a2f3a;border-radius:12px;padding:32px 40px;max-width:480px}}\
         h1{{color:#1db954;font-size:20px;margin:0 0 8px}}</style></head>\
         <body><div class=\"card\"><h1>Utilify</h1><p>{message}</p></div></body></html>"
    );
    tiny_http::Response::from_string(body)
        .with_status_code(status)
        .with_header(tiny_http::Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap())
}
