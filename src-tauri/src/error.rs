use serde::ser::{SerializeStruct, Serializer};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Tauri(#[from] tauri::Error),
    #[error("unexpected response from Spotify: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Spotify Client ID is not configured yet")]
    NoClientId,
    #[error("Not connected to Spotify")]
    NotAuthenticated,
    #[error("Spotify session expired. Please reconnect.")]
    AuthExpired,
    #[error("This Spotify app's API quota is exhausted. Artist lookups are paused for 24 hours.")]
    QuotaExceeded,
    #[error("Spotify's API quota was exceeded earlier; artist lookups are paused until it recovers (see the countdown).")]
    QuotaCooldown { until: i64 },
    #[error("Spotify is rate limiting requests. Wait a minute and try again.")]
    RateLimited,
    #[error("No Spotify device is available. Open Spotify on a device, then try again.")]
    NoActiveDevice,
    #[error("Spotify Premium is required to control playback.")]
    PremiumRequired,
    #[error("Playlist has no playable tracks.")]
    EmptyPlaylist,
    #[error("Spotify API error ({status}): {message}")]
    Spotify { status: u16, message: String },
    #[error("{0}")]
    Auth(String),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::Db(_) => "db",
            AppError::Http(_) => "network",
            AppError::Io(_) => "io",
            AppError::Tauri(_) => "tauri",
            AppError::Json(_) => "json",
            AppError::NoClientId => "no_client_id",
            AppError::NotAuthenticated => "not_authenticated",
            AppError::AuthExpired => "auth_expired",
            AppError::QuotaExceeded => "quota_exceeded",
            AppError::QuotaCooldown { .. } => "quota_cooldown",
            AppError::RateLimited => "rate_limited",
            AppError::NoActiveDevice => "no_active_device",
            AppError::PremiumRequired => "premium_required",
            AppError::EmptyPlaylist => "empty_playlist",
            AppError::Spotify { .. } => "spotify",
            AppError::Auth(_) => "auth",
            AppError::Other(_) => "other",
        }
    }

    pub fn other(msg: impl Into<String>) -> Self {
        AppError::Other(msg.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
