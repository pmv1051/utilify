//! Custom Playlist Editor backend.
//!
//! The UI computes the desired final order; this module turns it into the
//! fewest Spotify reorder calls (one per contiguous block that has to move)
//! so `added_at` and Spotify's per-item ids survive. A replace would reset
//! both and confuse a player currently in the playlist.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, Result};
use crate::features::tracks::{fetch_playlist, TrackInfo};
use crate::spotify::{library, playlists};
use crate::state::AppState;

const MOVE_DELAY: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorTrack {
    #[serde(flatten)]
    pub track: TrackInfo,
    /// Saved in the user's library ("liked"). `None` for local files.
    pub liked: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorLoad {
    pub tracks: Vec<EditorTrack>,
    /// False when the liked check failed; `liked` is then `None` everywhere.
    pub liked_available: bool,
    pub liked_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorProgress {
    pub playlist_id: String,
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub moves: usize,
}

/// All items of a playlist with true positions and liked flags. The liked
/// lookup is best-effort: if Spotify refuses it (scope or dev-mode limits),
/// the playlist still loads without hearts.
pub async fn load(state: &AppState, playlist_id: &str) -> Result<EditorLoad> {
    let tracks = fetch_playlist(state, playlist_id).await?;
    let ids: Vec<String> = tracks
        .iter()
        .filter(|t| t.playable)
        .filter_map(|t| t.id.clone())
        .collect();

    let (flags, liked_error) = if ids.is_empty() {
        (Some(Vec::new()), None)
    } else {
        match library::contains_saved_tracks(&state.spotify, &ids).await {
            Ok(f) => (Some(f), None),
            Err(e) => {
                log::warn!("editor: liked lookup failed, continuing without hearts: {e}");
                (None, Some(e.to_string()))
            }
        }
    };
    let liked_available = flags.is_some();
    let mut flag_iter = flags.unwrap_or_default().into_iter();
    let tracks = tracks
        .into_iter()
        .map(|t| {
            let liked = if liked_available && t.playable && t.id.is_some() {
                flag_iter.next()
            } else {
                None
            };
            EditorTrack { track: t, liked }
        })
        .collect();
    Ok(EditorLoad {
        tracks,
        liked_available,
        liked_error,
    })
}

/// One Spotify reorder call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub range_start: usize,
    pub insert_before: usize,
    pub range_length: usize,
}

/// Plan the moves that transform identity order `0..n` into `target` (a
/// permutation of original positions). Walks left to right; whenever the
/// item that belongs at `i` is elsewhere, moves it (and the run of items
/// that follow it in both sequences) into place.
pub fn plan_moves(target: &[usize]) -> Vec<Move> {
    let n = target.len();
    let mut cur: Vec<usize> = (0..n).collect();
    let mut moves = Vec::new();
    let mut i = 0;
    while i < n {
        if cur[i] == target[i] {
            i += 1;
            continue;
        }
        let j = cur[i..].iter().position(|&x| x == target[i]).map(|p| p + i).expect("target is a permutation");
        // Extend the block while the following items also line up.
        let mut len = 1;
        while j + len < n && i + len < n && cur[j + len] == target[i + len] {
            len += 1;
        }
        moves.push(Move {
            range_start: j,
            insert_before: i,
            range_length: len,
        });
        let block: Vec<usize> = cur.drain(j..j + len).collect();
        for (k, v) in block.into_iter().enumerate() {
            cur.insert(i + k, v);
        }
        i += len;
    }
    moves
}

fn validate_permutation(target: &[usize], n: usize) -> Result<()> {
    if target.len() != n {
        return Err(AppError::other(
            "The playlist changed since it was loaded. Reload and try again.",
        ));
    }
    let mut seen = vec![false; n];
    for &p in target {
        if p >= n || seen[p] {
            return Err(AppError::other("Invalid order: not a permutation of the playlist."));
        }
        seen[p] = true;
    }
    Ok(())
}

/// Apply `target` (desired order as a permutation of current positions).
pub async fn apply_order(app: &AppHandle, state: &AppState, playlist_id: &str, target: &[usize]) -> Result<ApplyResult> {
    // Verify against the live length so a stale view cannot scramble the list.
    let live = playlists::get_playlist_entries(&state.spotify, playlist_id).await?;
    validate_permutation(target, live.len())?;

    let moves = plan_moves(target);
    let total = moves.len();
    log::info!("editor: applying {total} move(s) to {playlist_id}");
    for (done, m) in moves.iter().enumerate() {
        if done > 0 {
            tokio::time::sleep(MOVE_DELAY).await;
        }
        playlists::reorder_items(&state.spotify, playlist_id, m.range_start, m.insert_before, m.range_length).await?;
        let _ = app.emit(
            "editor-progress",
            EditorProgress {
                playlist_id: playlist_id.to_string(),
                done: done + 1,
                total,
            },
        );
    }
    Ok(ApplyResult { moves: total })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulate(n: usize, moves: &[Move]) -> Vec<usize> {
        let mut cur: Vec<usize> = (0..n).collect();
        for m in moves {
            let block: Vec<usize> = cur.drain(m.range_start..m.range_start + m.range_length).collect();
            for (k, v) in block.into_iter().enumerate() {
                cur.insert(m.insert_before + k, v);
            }
        }
        cur
    }

    #[test]
    fn identity_needs_no_moves() {
        assert!(plan_moves(&[0, 1, 2, 3]).is_empty());
    }

    #[test]
    fn move_to_top_is_one_call() {
        let target = vec![3, 0, 1, 2];
        let moves = plan_moves(&target);
        assert_eq!(moves, vec![Move { range_start: 3, insert_before: 0, range_length: 1 }]);
        assert_eq!(simulate(4, &moves), target);
    }

    #[test]
    fn move_block_to_bottom_is_one_call() {
        let target = vec![0, 3, 4, 1, 2]; // items 1,2 moved to the end
        let moves = plan_moves(&target);
        assert_eq!(moves.len(), 1);
        assert_eq!(simulate(5, &moves), target);
    }

    #[test]
    fn arbitrary_permutations_are_reproduced() {
        let cases: Vec<Vec<usize>> = vec![
            vec![2, 0, 1],
            vec![4, 3, 2, 1, 0],
            vec![1, 0, 3, 2, 5, 4],
            vec![5, 1, 4, 0, 3, 2, 6],
        ];
        for target in cases {
            let moves = plan_moves(&target);
            assert_eq!(simulate(target.len(), &moves), target, "target {target:?}");
        }
    }

    #[test]
    fn rejects_bad_permutations() {
        assert!(validate_permutation(&[0, 0, 1], 3).is_err());
        assert!(validate_permutation(&[0, 1], 3).is_err());
        assert!(validate_permutation(&[0, 1, 2], 3).is_ok());
    }
}
