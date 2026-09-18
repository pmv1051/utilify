//! Playlist endpoints (Feb 2026 paths: `/playlists/{id}/items`, `POST /me/playlists`).

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use super::client::SpotifyClient;
use super::models::{MissingTrack, PlaylistEntry, SimplifiedPlaylist, Track, UserProfile};
use crate::error::{AppError, Result};

/// Spotify caps add/replace/remove at 100 items per request.
pub const BATCH_SIZE: usize = 100;
const BATCH_DELAY: Duration = Duration::from_millis(200);

pub async fn current_user(c: &SpotifyClient) -> Result<UserProfile> {
    c.get("/me", &[])
        .await?
        .ok_or_else(|| AppError::other("empty response from /me"))
}

/// All playlists the user owns or follows, in library order.
pub async fn list_user_playlists(c: &SpotifyClient) -> Result<Vec<SimplifiedPlaylist>> {
    // Spotify occasionally returns `null` entries in this listing; tolerate them.
    let items: Vec<Option<SimplifiedPlaylist>> = c
        .get_all_pages("/me/playlists", &[("limit", "50".into())])
        .await?;
    Ok(items.into_iter().flatten().collect())
}

#[allow(dead_code)]
pub async fn get_playlist(c: &SpotifyClient, id: &str) -> Result<SimplifiedPlaylist> {
    c.get(&format!("/playlists/{id}"), &[])
        .await?
        .ok_or_else(|| AppError::other("playlist not found"))
}

/// The copyable tracks of a playlist plus the entries that cannot be copied
/// through the API (local files, episodes, entries with no track data).
pub struct PlaylistItems {
    pub tracks: Vec<Track>,
    pub skipped: Vec<MissingTrack>,
}

/// Every entry of a playlist as Spotify returns it, including local files,
/// episodes and null tracks. Index in this Vec == API position.
pub async fn get_playlist_entries(c: &SpotifyClient, id: &str) -> Result<Vec<PlaylistEntry>> {
    c.get_all_pages(&format!("/playlists/{id}/items"), &[("limit", "50".into())])
        .await
}

pub async fn get_playlist_items(c: &SpotifyClient, id: &str) -> Result<PlaylistItems> {
    let entries = get_playlist_entries(c, id).await?;
    let mut tracks = Vec::with_capacity(entries.len());
    let mut skipped = Vec::new();
    for entry in entries {
        match entry.track {
            None => skipped.push(MissingTrack {
                uri: None,
                name: "Unknown track".into(),
                artists: String::new(),
                reason: "Spotify returned no track data; it was probably removed from the catalog".into(),
            }),
            Some(t) if t.is_playable_catalog_track() => tracks.push(t),
            Some(t) => {
                let reason = if t.is_local {
                    "Local file; the Spotify API cannot add it to playlists"
                } else if t.is_episode() {
                    "Podcast episode; only tracks are copied"
                } else {
                    "No Spotify track URI"
                };
                skipped.push(MissingTrack {
                    uri: t.uri.clone(),
                    name: t.name.clone(),
                    artists: t.artist_names(),
                    reason: reason.into(),
                });
            }
        }
    }
    Ok(PlaylistItems { tracks, skipped })
}

/// Every catalog track in a playlist, in playlist order.
pub async fn get_playlist_tracks(c: &SpotifyClient, id: &str) -> Result<Vec<Track>> {
    Ok(get_playlist_items(c, id).await?.tracks)
}

pub async fn create_playlist(
    c: &SpotifyClient,
    name: &str,
    description: &str,
    public: bool,
) -> Result<SimplifiedPlaylist> {
    let body = json!({ "name": name, "description": description, "public": public });
    c.post_json("/me/playlists", &body)
        .await?
        .ok_or_else(|| AppError::other("Spotify returned no playlist after create"))
}

/// Overwrite a playlist's contents with `uris`, in order. Replace handles the
/// first 100; subsequent chunks are appended. An empty slice clears the playlist.
pub async fn set_playlist_items(c: &SpotifyClient, id: &str, uris: &[String]) -> Result<()> {
    let path = format!("/playlists/{id}/items");
    let first: Vec<&String> = uris.iter().take(BATCH_SIZE).collect();
    c.put(&path, &[], Some(&json!({ "uris": first }))).await?;
    for chunk in uris.iter().skip(BATCH_SIZE).collect::<Vec<_>>().chunks(BATCH_SIZE) {
        tokio::time::sleep(BATCH_DELAY).await;
        c.post(&path, &json!({ "uris": chunk })).await?;
    }
    Ok(())
}

pub async fn add_items(c: &SpotifyClient, id: &str, uris: &[String], position: Option<usize>) -> Result<()> {
    let path = format!("/playlists/{id}/items");
    let mut pos = position;
    for chunk in uris.chunks(BATCH_SIZE) {
        let mut body = json!({ "uris": chunk });
        if let Some(p) = pos {
            body["position"] = json!(p);
            pos = Some(p + chunk.len());
        }
        c.post(&path, &body).await?;
        tokio::time::sleep(BATCH_DELAY).await;
    }
    Ok(())
}

/// Which body key the remove endpoint accepts: 0 unknown, 1 `tracks` (pre-2026
/// shape), 2 `items` (renamed). Learned from the first successful call.
static REMOVE_KEY: AtomicU8 = AtomicU8::new(0);

/// Remove every occurrence of the given URIs. `snapshot_id` guards against
/// concurrent edits from another Spotify client.
pub async fn remove_items(
    c: &SpotifyClient,
    id: &str,
    uris: &[String],
    snapshot_id: Option<&str>,
) -> Result<()> {
    let path = format!("/playlists/{id}/items");
    for chunk in uris.chunks(BATCH_SIZE) {
        let entries: Vec<Value> = chunk.iter().map(|u| json!({ "uri": u })).collect();
        // Live runs showed Spotify accept both shapes at different times; try
        // the Feb-2026 `items` shape first.
        let keys: &[(&str, u8)] = match REMOVE_KEY.load(Ordering::Relaxed) {
            1 => &[("tracks", 1)],
            2 => &[("items", 2)],
            _ => &[("items", 2), ("tracks", 1)],
        };
        let mut last_err = None;
        for (key, code) in keys {
            let mut body = serde_json::Map::new();
            body.insert((*key).to_string(), Value::Array(entries.clone()));
            if let Some(s) = snapshot_id {
                body.insert("snapshot_id".into(), json!(s));
            }
            match c.delete(&path, &Value::Object(body)).await {
                Ok(()) => {
                    REMOVE_KEY.store(*code, Ordering::Relaxed);
                    last_err = None;
                    break;
                }
                Err(e) => match e {
                    AppError::Spotify { status: 400, .. } if keys.len() > 1 => {
                        log::info!("remove items: key '{key}' rejected, trying the other shape");
                        last_err = Some(e);
                    }
                    other => return Err(other),
                },
            }
        }
        if let Some(e) = last_err {
            return Err(e);
        }
        tokio::time::sleep(BATCH_DELAY).await;
    }
    Ok(())
}
