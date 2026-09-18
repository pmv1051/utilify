//! Rich, UI-ready track rows shared by the playlist tools.

use serde::Serialize;

use crate::error::Result;
use crate::spotify::models::PlaylistEntry;
use crate::spotify::playlists;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRef {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    pub uri: String,
    pub id: Option<String>,
    pub name: String,
    pub artists: String,
    pub artist_ids: Vec<String>,
    pub artist_refs: Vec<ArtistRef>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub added_at: Option<String>,
    /// Index among *all* playlist items (local files and episodes included),
    /// which is how the API counts positions.
    pub position: i64,
    pub is_local: bool,
    /// A catalog track the API can add/remove (not local, not an episode).
    pub playable: bool,
}

impl TrackInfo {
    pub fn from_entry(entry: &PlaylistEntry, position: usize) -> Option<Self> {
        let t = entry.track.as_ref()?;
        Some(Self {
            uri: t.uri.clone()?,
            id: t.id.clone(),
            name: t.name.clone(),
            artists: t.artist_names(),
            artist_ids: t.artists.iter().filter_map(|a| a.id.clone()).collect(),
            artist_refs: t
                .artists
                .iter()
                .map(|a| ArtistRef {
                    id: a.id.clone(),
                    name: a.name.clone(),
                })
                .collect(),
            album: t.album.as_ref().map(|a| a.name.clone()),
            duration_ms: t.duration_ms,
            added_at: entry.added_at.clone(),
            position: position as i64,
            is_local: t.is_local,
            playable: t.is_playable_catalog_track(),
        })
    }
}

/// All entries of a playlist with correct API positions. Entries with no
/// track data are skipped (their positions are still accounted for).
pub async fn fetch_playlist(state: &AppState, playlist_id: &str) -> Result<Vec<TrackInfo>> {
    let entries = playlists::get_playlist_entries(&state.spotify, playlist_id).await?;
    Ok(entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| TrackInfo::from_entry(e, i))
        .collect())
}

/// Display name for a playlist from the local cache, falling back to its id.
pub fn playlist_display_name(state: &AppState, playlist_id: &str) -> String {
    state
        .db
        .with(|c| crate::db::playlists::get(c, playlist_id, None))
        .ok()
        .flatten()
        .map(|p| p.name)
        .unwrap_or_else(|| playlist_id.to_string())
}
