//! Playlist Diff: three-way breakdown of two playlists.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::error::{AppError, Result};
use crate::features::matching::name_key;
use crate::features::tracks::{fetch_playlist, playlist_display_name, TrackInfo};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffPair {
    pub a: TrackInfo,
    pub b: TrackInfo,
    /// Same song under different URIs (only possible with name matching).
    pub different_uri: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffResult {
    pub a: PlaylistRef,
    pub b: PlaylistRef,
    pub match_by_name: bool,
    pub only_a: Vec<TrackInfo>,
    pub only_b: Vec<TrackInfo>,
    pub both: Vec<DiffPair>,
}

fn key_of(t: &TrackInfo, by_name: bool) -> String {
    if by_name {
        name_key(&t.name, &t.artists)
    } else {
        t.uri.clone()
    }
}

pub async fn diff(state: &AppState, a_id: &str, b_id: &str, match_by_name: bool) -> Result<DiffResult> {
    if a_id == b_id {
        return Err(AppError::other("Pick two different playlists."));
    }
    let a_tracks: Vec<TrackInfo> = fetch_playlist(state, a_id).await?.into_iter().filter(|t| t.playable).collect();
    let b_tracks: Vec<TrackInfo> = fetch_playlist(state, b_id).await?.into_iter().filter(|t| t.playable).collect();

    let mut b_by_key: HashMap<String, &TrackInfo> = HashMap::new();
    for t in &b_tracks {
        b_by_key.entry(key_of(t, match_by_name)).or_insert(t);
    }
    let mut a_keys: HashSet<String> = HashSet::new();

    let mut only_a = Vec::new();
    let mut both = Vec::new();
    for t in &a_tracks {
        let k = key_of(t, match_by_name);
        a_keys.insert(k.clone());
        match b_by_key.get(&k) {
            Some(bt) => both.push(DiffPair {
                a: t.clone(),
                b: (*bt).clone(),
                different_uri: bt.uri != t.uri,
            }),
            None => only_a.push(t.clone()),
        }
    }
    let only_b: Vec<TrackInfo> = b_tracks
        .iter()
        .filter(|t| !a_keys.contains(&key_of(t, match_by_name)))
        .cloned()
        .collect();

    Ok(DiffResult {
        a: PlaylistRef {
            id: a_id.to_string(),
            name: playlist_display_name(state, a_id),
        },
        b: PlaylistRef {
            id: b_id.to_string(),
            name: playlist_display_name(state, b_id),
        },
        match_by_name,
        only_a,
        only_b,
        both,
    })
}
