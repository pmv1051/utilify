//! Generic playlist actions shared by the tools: create a `Utilify: …`
//! playlist from a list of URIs, append tracks, remove tracks.

use serde::Serialize;

use crate::db;
use crate::error::{AppError, Result};
use crate::features::randomizer::fisher_yates;
use crate::spotify::playlists;
use crate::state::AppState;

pub const GENERATED_PREFIX: &str = "Utilify: ";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPlaylist {
    pub id: String,
    pub name: String,
    /// Tracks actually in the playlist after writing (read back).
    pub track_count: usize,
    /// Tracks we asked Spotify to add.
    pub requested: usize,
}

/// `Utilify: {description}`, unless the description already carries the prefix.
pub fn generated_name(description: &str) -> String {
    let d = description.trim();
    if d.starts_with(GENERATED_PREFIX.trim_end()) {
        d.to_string()
    } else {
        format!("{GENERATED_PREFIX}{d}")
    }
}

/// Create a new private playlist holding `uris` (optionally Fisher-Yates
/// shuffled first) and report how many tracks Spotify actually kept.
pub async fn create_from_uris(
    state: &AppState,
    description: &str,
    mut uris: Vec<String>,
    randomize: bool,
) -> Result<GeneratedPlaylist> {
    if uris.is_empty() {
        return Err(AppError::EmptyPlaylist);
    }
    if description.trim().is_empty() {
        return Err(AppError::other("Give the new playlist a name."));
    }
    if randomize {
        fisher_yates(&mut uris);
    }
    let name = generated_name(description);
    let created = playlists::create_playlist(&state.spotify, &name, "Created by Utilify.", false).await?;
    playlists::set_playlist_items(&state.spotify, &created.id, &uris).await?;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let actual = playlists::get_playlist_tracks(&state.spotify, &created.id).await?.len();
    log::info!("created '{}' ({}) with {actual}/{} tracks", name, created.id, uris.len());
    Ok(GeneratedPlaylist {
        id: created.id,
        name,
        track_count: actual,
        requested: uris.len(),
    })
}

/// Append tracks to an existing playlist.
pub async fn add_tracks(state: &AppState, playlist_id: &str, uris: &[String]) -> Result<usize> {
    if uris.is_empty() {
        return Ok(0);
    }
    playlists::add_items(&state.spotify, playlist_id, uris, None).await?;
    log::info!("added {} tracks to {playlist_id}", uris.len());
    Ok(uris.len())
}

/// Remove every copy of each URI from a playlist. If the playlist is a
/// tracked `-Utilify` shadow, its stored order is pruned too.
pub async fn remove_tracks(state: &AppState, playlist_id: &str, uris: &[String]) -> Result<usize> {
    if uris.is_empty() {
        return Ok(0);
    }
    playlists::remove_items(&state.spotify, playlist_id, uris, None).await?;
    prune_shadow_order(state, playlist_id, uris)?;
    log::info!("removed {} tracks from {playlist_id}", uris.len());
    Ok(uris.len())
}

/// Keep a tracked shadow's stored order in sync after URIs were removed.
pub fn prune_shadow_order(state: &AppState, playlist_id: &str, uris: &[String]) -> Result<()> {
    let session = state.db.with(|c| db::randomizer::get_by_shadow(c, playlist_id))?;
    if let Some(mut s) = session.filter(|s| s.active) {
        let before = s.track_order.len();
        s.track_order.retain(|u| !uris.contains(u));
        if s.track_order.len() != before {
            s.track_count = s.track_order.len() as i64;
            s.last_track_uri = s.track_order.last().cloned();
            state.db.with(|c| db::randomizer::upsert(c, &s))?;
            log::info!("pruned {} tracks from shadow '{}'", before - s.track_order.len(), s.shadow_name);
        }
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naming() {
        assert_eq!(generated_name("Road trip"), "Utilify: Road trip");
        assert_eq!(generated_name("  Utilify: Already  "), "Utilify: Already");
    }
}
