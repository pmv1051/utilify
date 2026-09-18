//! Playlist Merge: combine several playlists into a new deduplicated one.

use std::collections::HashSet;

use serde::Serialize;

use crate::error::{AppError, Result};
use crate::features::generated::{self, GeneratedPlaylist};
use crate::features::matching::name_key;
use crate::features::tracks::{fetch_playlist, playlist_display_name};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
    pub playlist: GeneratedPlaylist,
    pub sources: usize,
    pub tracks_seen: usize,
    pub duplicates_skipped: usize,
}

pub async fn merge(
    state: &AppState,
    playlist_ids: &[String],
    name: &str,
    dedupe_by_name: bool,
    randomize: bool,
) -> Result<MergeResult> {
    if playlist_ids.len() < 2 {
        return Err(AppError::other("Pick at least two playlists to merge."));
    }
    let description = if name.trim().is_empty() {
        let names: Vec<String> = playlist_ids.iter().map(|id| playlist_display_name(state, id)).collect();
        format!("Merge of {}", names.join(", "))
    } else {
        name.trim().to_string()
    };

    let mut seen_uri: HashSet<String> = HashSet::new();
    let mut seen_name: HashSet<String> = HashSet::new();
    let mut uris = Vec::new();
    let mut tracks_seen = 0usize;
    let mut skipped = 0usize;

    for id in playlist_ids {
        for t in fetch_playlist(state, id).await? {
            if !t.playable {
                continue;
            }
            tracks_seen += 1;
            if !seen_uri.insert(t.uri.clone()) {
                skipped += 1;
                continue;
            }
            if dedupe_by_name && !seen_name.insert(name_key(&t.name, &t.artists)) {
                skipped += 1;
                continue;
            }
            uris.push(t.uri);
        }
    }

    let playlist = generated::create_from_uris(state, &description, uris, randomize).await?;
    Ok(MergeResult {
        playlist,
        sources: playlist_ids.len(),
        tracks_seen,
        duplicates_skipped: skipped,
    })
}
