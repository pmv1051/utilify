//! Spotify Web API response types (February 2026 revision).
//!
//! Everything is lenient: fields Spotify may omit are `Option` or defaulted so
//! a small schema change does not break deserialization. Fields deserialize
//! from Spotify's snake_case and serialize to the frontend as camelCase.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paging<T> {
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    pub url: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Owner {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountRef {
    #[serde(default)]
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct SimplifiedPlaylist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub snapshot_id: Option<String>,
    #[serde(default)]
    pub owner: Option<Owner>,
    #[serde(default)]
    pub images: Option<Vec<Image>>,
    #[serde(default)]
    pub public: Option<bool>,
    #[serde(default)]
    pub collaborative: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
    /// The item-count object. Pre-2026 payloads call it `tracks`; the renamed
    /// API may call it `items`.
    #[serde(default, alias = "items")]
    pub tracks: Option<CountRef>,
}

impl SimplifiedPlaylist {
    pub fn track_count(&self) -> u64 {
        self.tracks.as_ref().map(|t| t.total).unwrap_or(0)
    }

    pub fn owner_id(&self) -> Option<&str> {
        self.owner.as_ref().and_then(|o| o.id.as_deref())
    }

    /// Smallest image, good enough for thumbnails.
    pub fn thumbnail(&self) -> Option<String> {
        self.images.as_ref().and_then(|imgs| {
            imgs.iter()
                .min_by_key(|i| i.width.unwrap_or(u32::MAX))
                .map(|i| i.url.clone())
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub images: Vec<Image>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Track {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub is_local: bool,
    #[serde(default)]
    pub artists: Vec<Artist>,
    #[serde(default)]
    pub album: Option<Album>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Present when Spotify relinked the track to a market-specific copy; the
    /// playlist still holds the original URI.
    #[serde(default)]
    pub linked_from: Option<LinkedFrom>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedFrom {
    #[serde(default)]
    pub uri: Option<String>,
}

impl Track {
    /// A Spotify-catalog track we can add to playlists (not local, not an episode).
    pub fn is_playable_catalog_track(&self) -> bool {
        !self.is_local
            && self
                .uri
                .as_deref()
                .map(|u| u.starts_with("spotify:track:"))
                .unwrap_or(false)
    }

    pub fn is_episode(&self) -> bool {
        self.kind.as_deref() == Some("episode")
            || self
                .uri
                .as_deref()
                .map(|u| u.starts_with("spotify:episode:"))
                .unwrap_or(false)
    }

    pub fn artist_names(&self) -> String {
        self.artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// One entry of `GET /playlists/{id}/items`. The nested object is `track` in
/// pre-2026 payloads and `item` after the rename; accept both.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct PlaylistEntry {
    #[serde(default)]
    pub added_at: Option<String>,
    #[serde(default, alias = "item")]
    pub track: Option<Track>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Device {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub is_restricted: bool,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DevicesResponse {
    #[serde(default)]
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Context {
    pub uri: String,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct PlaybackState {
    #[serde(default)]
    pub device: Option<Device>,
    #[serde(default)]
    pub is_playing: bool,
    #[serde(default)]
    pub progress_ms: Option<u64>,
    #[serde(default)]
    pub shuffle_state: bool,
    #[serde(default)]
    pub repeat_state: Option<String>,
    #[serde(default)]
    pub context: Option<Context>,
    #[serde(default)]
    pub item: Option<Track>,
    #[serde(default)]
    pub currently_playing_type: Option<String>,
    #[serde(default)]
    pub timestamp: u64,
}

impl PlaybackState {
    pub fn context_uri(&self) -> Option<&str> {
        self.context.as_ref().map(|c| c.uri.as_str())
    }

    pub fn track_uri(&self) -> Option<&str> {
        self.item.as_ref().and_then(|t| t.uri.as_deref())
    }

    /// The URI as it appears in the playlist: the relink origin if any.
    pub fn original_track_uri(&self) -> Option<&str> {
        self.item
            .as_ref()
            .and_then(|t| t.linked_from.as_ref())
            .and_then(|l| l.uri.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct UserProfile {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

/// A source entry that did not make it into a generated playlist, and why.
/// Our own type (stored as JSON, sent to the UI), hence camelCase both ways.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MissingTrack {
    #[serde(default)]
    pub uri: Option<String>,
    pub name: String,
    #[serde(default)]
    pub artists: String,
    pub reason: String,
}

/// Standard Spotify error envelope: `{"error": {"status": 429, "message": "...", "reason": "..."}}`.
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ErrorBody {
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}
