//! Duplicate Scanner: find the same track more than once in one playlist, or
//! across several, by URI and optionally by normalized title + artist. Removal
//! is by URI (the only shape known to work), re-adding the copies the user
//! chose to keep at their simulated positions.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::features::matching::name_key;
use crate::features::tracks::{fetch_playlist, playlist_display_name, TrackInfo};
use crate::spotify::playlists;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub playlist_id: String,
    pub playlist_name: String,
    pub track: TrackInfo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub key: String,
    /// `uri` = identical Spotify track; `name` = same title + primary artist under different URIs.
    pub match_kind: &'static str,
    pub name: String,
    pub artists: String,
    pub occurrences: Vec<Occurrence>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateReport {
    pub mode: &'static str,
    pub playlists_scanned: usize,
    pub tracks_scanned: usize,
    pub groups: Vec<DuplicateGroup>,
}

pub async fn scan(state: &AppState, playlist_ids: &[String], match_by_name: bool) -> Result<DuplicateReport> {
    if playlist_ids.is_empty() {
        return Err(AppError::other("Pick at least one playlist."));
    }
    let cross = playlist_ids.len() > 1;

    let mut occurrences: Vec<Occurrence> = Vec::new();
    for id in playlist_ids {
        let name = playlist_display_name(state, id);
        for t in fetch_playlist(state, id).await? {
            if t.playable {
                occurrences.push(Occurrence {
                    playlist_id: id.clone(),
                    playlist_name: name.clone(),
                    track: t,
                });
            }
        }
    }
    let tracks_scanned = occurrences.len();

    let qualifies = |idxs: &[usize]| -> bool {
        if cross {
            let playlists: HashSet<&str> = idxs.iter().map(|&i| occurrences[i].playlist_id.as_str()).collect();
            playlists.len() >= 2
        } else {
            idxs.len() >= 2
        }
    };

    let mut groups = Vec::new();

    let mut by_uri: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, o) in occurrences.iter().enumerate() {
        by_uri.entry(o.track.uri.as_str()).or_default().push(i);
    }
    for (uri, idxs) in &by_uri {
        if qualifies(idxs) {
            groups.push(make_group(uri.to_string(), "uri", idxs, &occurrences));
        }
    }

    if match_by_name {
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, o) in occurrences.iter().enumerate() {
            by_name
                .entry(name_key(&o.track.name, &o.track.artists))
                .or_default()
                .push(i);
        }
        for (key, idxs) in by_name {
            let distinct_uris: HashSet<&str> = idxs.iter().map(|&i| occurrences[i].track.uri.as_str()).collect();
            // A name group that is really one URI is already a URI group.
            if distinct_uris.len() >= 2 && qualifies(&idxs) {
                groups.push(make_group(key, "name", &idxs, &occurrences));
            }
        }
    }

    groups.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.match_kind.cmp(b.match_kind))
    });

    Ok(DuplicateReport {
        mode: if cross { "cross" } else { "single" },
        playlists_scanned: playlist_ids.len(),
        tracks_scanned,
        groups,
    })
}

fn make_group(key: String, kind: &'static str, idxs: &[usize], all: &[Occurrence]) -> DuplicateGroup {
    let mut occ: Vec<Occurrence> = idxs.iter().map(|&i| all[i].clone()).collect();
    occ.sort_by(|a, b| {
        a.playlist_name
            .cmp(&b.playlist_name)
            .then(a.track.position.cmp(&b.track.position))
    });
    DuplicateGroup {
        key,
        match_kind: kind,
        name: occ[0].track.name.clone(),
        artists: occ[0].track.artists.clone(),
        occurrences: occ,
    }
}

/// One playlist × one URI: which API positions to remove.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalRequest {
    pub playlist_id: String,
    pub uri: String,
    pub positions: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalSummary {
    pub removed: usize,
    pub re_added: usize,
    pub playlists: usize,
}

/// Remove the requested copies. Spotify's remove-by-URI takes out every copy,
/// so copies the user kept are re-added at the position they would have had.
pub async fn apply_removals(state: &AppState, requests: Vec<RemovalRequest>) -> Result<RemovalSummary> {
    let mut per_playlist: HashMap<String, HashMap<String, HashSet<i64>>> = HashMap::new();
    for r in requests {
        per_playlist
            .entry(r.playlist_id)
            .or_default()
            .entry(r.uri)
            .or_default()
            .extend(r.positions);
    }

    let mut summary = RemovalSummary {
        removed: 0,
        re_added: 0,
        playlists: per_playlist.len(),
    };

    for (playlist_id, wanted) in per_playlist {
        // Fresh positions; the playlist may have changed since the scan.
        let entries = fetch_playlist(state, &playlist_id).await?;
        let uris: Vec<String> = wanted.keys().cloned().collect();

        // Copies to keep: every current occurrence not marked for removal.
        let mut kept: Vec<(i64, String)> = Vec::new();
        for e in &entries {
            if let Some(remove_positions) = wanted.get(&e.uri) {
                if remove_positions.contains(&e.position) {
                    summary.removed += 1;
                } else {
                    kept.push((e.position, e.uri.clone()));
                }
            }
        }

        playlists::remove_items(&state.spotify, &playlist_id, &uris, None).await?;

        // Simulate the playlist after removal, then insert kept copies in
        // ascending original order so each insertion index is exact.
        let mut remaining: Vec<i64> = entries
            .iter()
            .filter(|e| !wanted.contains_key(&e.uri))
            .map(|e| e.position)
            .collect();
        kept.sort();
        for (pos, uri) in kept {
            let idx = remaining.partition_point(|p| *p < pos);
            playlists::add_items(&state.spotify, &playlist_id, &[uri], Some(idx)).await?;
            remaining.insert(idx, pos);
            summary.re_added += 1;
        }
        log::info!(
            "duplicates: playlist {playlist_id}: removed {} copies, re-added {}",
            summary.removed,
            summary.re_added
        );
    }
    Ok(summary)
}
