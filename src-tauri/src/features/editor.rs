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
use crate::spotify::playlists;
use crate::state::AppState;

const MOVE_DELAY: Duration = Duration::from_millis(120);

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

/// All items of a playlist (local files included, so positions are exact).
pub async fn load(state: &AppState, playlist_id: &str) -> Result<Vec<TrackInfo>> {
    fetch_playlist(state, playlist_id).await
}

/// One Spotify reorder call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub range_start: usize,
    pub insert_before: usize,
    pub range_length: usize,
}

/// Spotify rejects large reorder ranges (live: moving one track to the end of
/// a long playlist as a 500-item block → 400). Keep every range at most this.
const MAX_RANGE: usize = 100;

/// Apply a reorder with Spotify's semantics: `insert_before` refers to
/// positions *before* the range is removed.
fn apply_move(cur: &mut Vec<usize>, m: &Move) {
    let block: Vec<usize> = cur.drain(m.range_start..m.range_start + m.range_length).collect();
    let at = if m.insert_before > m.range_start {
        m.insert_before - m.range_length
    } else {
        m.insert_before
    };
    for (k, v) in block.into_iter().enumerate() {
        cur.insert(at + k, v);
    }
}

/// Plan the moves that transform identity order `0..n` into `target` (a
/// permutation of original positions). Walks left to right; whenever the
/// item that belongs at `i` sits at `j > i`, either the block starting at `j`
/// moves before `i`, or the items in between (the gap) move after the block,
/// whichever is smaller. Ranges are chunked to `MAX_RANGE`.
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
        let gap = j - i;
        if gap <= len {
            // Move the gap after the block, in chunks. The block's end index
            // (pre-removal) shrinks by what has already been moved out.
            let mut moved = 0;
            while moved < gap {
                let chunk = (gap - moved).min(MAX_RANGE);
                let m = Move {
                    range_start: i,
                    insert_before: j + len - moved,
                    range_length: chunk,
                };
                apply_move(&mut cur, &m);
                moves.push(m);
                moved += chunk;
            }
        } else {
            // Move the block before i, in chunks. Each chunk inserted before
            // i shifts the block's remainder right by the chunk size.
            let mut moved = 0;
            while moved < len {
                let chunk = (len - moved).min(MAX_RANGE);
                let m = Move {
                    range_start: j + moved,
                    insert_before: i + moved,
                    range_length: chunk,
                };
                apply_move(&mut cur, &m);
                moves.push(m);
                moved += chunk;
            }
        }
        i += len;
    }
    debug_assert_eq!(cur, target);
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
    // Verify against the live item count (one request) so a stale view cannot
    // scramble the list.
    let live_count = playlists::get_playlist(&state.spotify, playlist_id).await?.track_count() as usize;
    validate_permutation(target, live_count)?;

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
            assert!(m.range_length <= MAX_RANGE, "range too large: {m:?}");
            assert!(m.range_start + m.range_length <= n && m.insert_before <= n, "out of bounds: {m:?}");
            apply_move(&mut cur, m);
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
    fn move_one_to_bottom_is_one_small_call() {
        // The live failure: item 0 to the end of a long list must not move
        // the other n-1 items as a block.
        let n = 600;
        let mut target: Vec<usize> = (1..n).collect();
        target.push(0);
        let moves = plan_moves(&target);
        assert_eq!(moves, vec![Move { range_start: 0, insert_before: n, range_length: 1 }]);
        assert_eq!(simulate(n, &moves), target);
    }

    #[test]
    fn move_block_to_bottom_is_one_call() {
        let target = vec![0, 3, 4, 1, 2]; // items 1,2 moved to the end
        let moves = plan_moves(&target);
        assert_eq!(moves.len(), 1);
        assert_eq!(simulate(5, &moves), target);
    }

    #[test]
    fn large_blocks_are_chunked() {
        // Reverse halves of a 450-item list: a 225-block must move in 100-chunks.
        let n = 450;
        let target: Vec<usize> = (225..n).chain(0..225).collect();
        let moves = plan_moves(&target);
        assert!(moves.iter().all(|m| m.range_length <= MAX_RANGE));
        assert_eq!(simulate(n, &moves), target);
    }

    #[test]
    fn arbitrary_permutations_are_reproduced() {
        let mut cases: Vec<Vec<usize>> = vec![
            vec![2, 0, 1],
            vec![4, 3, 2, 1, 0],
            vec![1, 0, 3, 2, 5, 4],
            vec![5, 1, 4, 0, 3, 2, 6],
        ];
        // A deterministic pseudo-random shuffle of 300 items.
        let mut v: Vec<usize> = (0..300).collect();
        let mut seed = 12345u64;
        for k in (1..v.len()).rev() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let r = (seed >> 33) as usize % (k + 1);
            v.swap(k, r);
        }
        cases.push(v);
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
