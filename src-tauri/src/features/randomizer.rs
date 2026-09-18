//! Shadow Playlist Randomizer.
//!
//! A source playlist is copied into `{name}-Utilify` in a Fisher-Yates order
//! and played sequentially with Spotify shuffle off. The shuffled order is
//! kept in the session so the polling loop can tell where playback is.
//!
//! Spotify identifies the playing item by an internal per-item id, not by
//! track URI. A full replace of the playlist discards every id, after which
//! the player falls back to its numeric position (observed: it stays on the
//! last index and skipping goes nowhere). So while the shadow is playing, a
//! re-shuffle is done *incrementally*: remove every track except the playing
//! one, then append a fresh shuffle. The playing item keeps its id and the
//! player flows straight into the new order, like songs added to a playing
//! playlist. A full replace is only used when the shadow is not playing.
//!
//! Every 30 seconds [`on_poll`] checks each active session:
//! 1. After an incremental re-shuffle, confirm the hand-over: if Spotify
//!    stopped after the pivot track, or jumped to the final index, restart
//!    the shadow playlist at position 1.
//! 2. If Spotify parked at 0:00 on the final track, the context ran out
//!    between polls: full re-shuffle and restart from the top.
//! 3. If playback is within the last [`END_WINDOW`] tracks, re-shuffle
//!    incrementally around the playing track.
//! 4. If playback jumped back to the start of the list (repeat wrapped, or
//!    the tail was skipped through between polls), same incremental re-shuffle.

use std::collections::HashMap;

use rand::Rng;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::randomizer::SessionRow;
use crate::db::{self, now};
use crate::error::{AppError, Result};
use crate::spotify::models::{MissingTrack, PlaybackState};
use crate::spotify::{playback, playlists};
use crate::state::AppState;

pub const SHADOW_SUFFIX: &str = "-Utilify";
/// Re-shuffle once playback is within this many tracks of the end. 1 means
/// only while the final track is playing; if that track is skipped before a
/// poll sees it, the wrap-around and parked-at-end checks take over.
const END_WINDOW: usize = 1;

pub fn shadow_name(source_name: &str) -> String {
    format!("{source_name}{SHADOW_SUFFIX}")
}

pub fn shadow_context_uri(shadow_id: &str) -> String {
    format!("spotify:playlist:{shadow_id}")
}

/// In-place Fisher-Yates (Durstenfeld) shuffle: every permutation equally likely.
pub fn fisher_yates<T>(items: &mut [T]) {
    let mut rng = rand::rng();
    for i in (1..items.len()).rev() {
        let j = rng.random_range(0..=i);
        items.swap(i, j);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RandomizeResult {
    pub session: SessionRow,
    pub playback_started: bool,
    pub warning: Option<String>,
}

/// A shuffled order ready to write, plus bookkeeping for reporting.
struct BuiltOrder {
    uris: Vec<String>,
    /// Source entries left out on purpose or because they cannot be copied.
    missing: Vec<MissingTrack>,
    /// URI → (name, artists) for everything seen in the source.
    names: HashMap<String, (String, String)>,
}

/// Fetch the source tracks minus benched ones, shuffled. If `exclude` is
/// given, that URI is left out (the caller places it itself).
async fn build_order(
    state: &AppState,
    source_id: &str,
    shadow_id: Option<&str>,
    exclude: Option<&str>,
) -> Result<BuiltOrder> {
    let items = playlists::get_playlist_items(&state.spotify, source_id).await?;

    let mut ids = vec![source_id];
    if let Some(s) = shadow_id {
        ids.push(s);
    }
    let benched = state.db.with(|c| db::bench::active_benched_uris(c, &ids))?;

    let mut missing = items.skipped;
    let mut names = HashMap::new();
    let mut uris = Vec::with_capacity(items.tracks.len());
    for t in items.tracks {
        let Some(uri) = t.uri.clone() else { continue };
        names.insert(uri.clone(), (t.name.clone(), t.artist_names()));
        if Some(uri.as_str()) == exclude {
            continue;
        }
        if benched.contains(&uri) {
            missing.push(MissingTrack {
                uri: Some(uri),
                artists: t.artist_names(),
                name: t.name,
                reason: "Benched; returns on the re-shuffle after the bench expires".into(),
            });
            continue;
        }
        uris.push(uri);
    }
    if uris.is_empty() && exclude.is_none() {
        return Err(AppError::EmptyPlaylist);
    }
    fisher_yates(&mut uris);
    Ok(BuiltOrder { uris, missing, names })
}

/// Read the shadow playlist back and compare with what was written. Any URI
/// Spotify silently dropped is removed from `order` (so end-of-playlist
/// detection never waits for a track that is not there) and reported.
async fn verify_written(
    state: &AppState,
    shadow_id: &str,
    order: &mut Vec<String>,
    names: &HashMap<String, (String, String)>,
    missing: &mut Vec<MissingTrack>,
) -> Result<()> {
    // Give Spotify a moment to settle before reading back.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let actual = playlists::get_playlist_tracks(&state.spotify, shadow_id).await?;

    let deficits: Vec<(String, usize)> = {
        let mut have: HashMap<&str, usize> = HashMap::new();
        for t in &actual {
            if let Some(u) = t.uri.as_deref() {
                *have.entry(u).or_insert(0) += 1;
            }
        }
        let mut want: HashMap<&str, usize> = HashMap::new();
        for u in order.iter() {
            *want.entry(u.as_str()).or_insert(0) += 1;
        }
        want.iter()
            .filter_map(|(u, w)| {
                let h = have.get(u).copied().unwrap_or(0);
                (h < *w).then(|| ((*u).to_string(), w - h))
            })
            .collect()
    };

    for (uri, mut k) in deficits {
        let mut i = order.len();
        while i > 0 && k > 0 {
            i -= 1;
            if order[i] == uri {
                order.remove(i);
                k -= 1;
            }
        }
        let (name, artists) = names
            .get(&uri)
            .cloned()
            .unwrap_or_else(|| (uri.clone(), String::new()));
        log::warn!("randomizer: Spotify did not add {uri} ({name}) to {shadow_id}");
        missing.push(MissingTrack {
            uri: Some(uri),
            name,
            artists,
            reason: "Spotify accepted the request but did not add it; the track is probably unavailable or removed".into(),
        });
    }
    Ok(())
}

/// Create or refresh the shadow playlist for `source_id` with a full replace,
/// optionally starting playback from the top.
pub async fn randomize(state: &AppState, source_id: &str, start_playback: bool) -> Result<RandomizeResult> {
    let me = state.db.with(|c| db::config::get(c, db::config::USER_ID))?;

    let all = playlists::list_user_playlists(&state.spotify).await?;
    crate::features::cache_playlists(&state.db, &all)?;

    let source = all
        .iter()
        .find(|p| p.id == source_id)
        .ok_or_else(|| AppError::other("Playlist not found in your library. Refresh and try again."))?;
    if source.name.ends_with(SHADOW_SUFFIX) {
        return Err(AppError::other(
            "That is already a Utilify shadow playlist. Randomize the original playlist instead.",
        ));
    }

    let name = shadow_name(&source.name);
    let existing = all
        .iter()
        .find(|p| p.name == name && (me.is_none() || p.owner_id() == me.as_deref()));
    let shadow_id = match existing {
        Some(p) => {
            log::info!("reusing shadow playlist '{}' ({})", p.name, p.id);
            p.id.clone()
        }
        None => {
            log::info!("creating shadow playlist '{name}'");
            let description = format!("Randomized copy of \"{}\" managed by Utilify.", source.name);
            let created =
                playlists::create_playlist(&state.spotify, &name, &description, source.public.unwrap_or(false))
                    .await?;
            created.id
        }
    };

    let mut built = build_order(state, source_id, Some(&shadow_id), None).await?;
    playlists::set_playlist_items(&state.spotify, &shadow_id, &built.uris).await?;
    verify_written(state, &shadow_id, &mut built.uris, &built.names, &mut built.missing).await?;
    let uris = built.uris;

    let previous = state.db.with(|c| db::randomizer::get_by_shadow(c, &shadow_id))?;
    let session = SessionRow {
        shadow_playlist_id: shadow_id.clone(),
        source_playlist_id: source_id.to_string(),
        source_name: source.name.clone(),
        shadow_name: name,
        track_count: uris.len() as i64,
        last_track_uri: uris.last().cloned(),
        last_shuffled_at: now(),
        reshuffle_count: previous.map(|p| p.reshuffle_count).unwrap_or(0),
        active: true,
        track_order: uris.clone(),
        pending_pin: None,
        missing_tracks: built.missing,
    };
    let source_id_owned = source_id.to_string();
    state.db.with(|c| {
        db::randomizer::upsert(c, &session)?;
        db::randomizer::deactivate_others_for_source(c, &source_id_owned, &session.shadow_playlist_id)?;
        Ok(())
    })?;
    log::info!("wrote {} tracks to shadow playlist {}", uris.len(), shadow_id);

    // From here on the shadow is written and tracked; a playback problem is a
    // warning, not a failed randomize.
    let (playback_started, warning) = if start_playback {
        match playback::play_playlist_unshuffled(&state.spotify, &shadow_id).await {
            Ok(()) => (true, None),
            Err(AppError::NoActiveDevice) => (
                false,
                Some(format!(
                    "\"{}\" is ready, but no Spotify device is available. Open Spotify and play it from there with shuffle off.",
                    session.shadow_name
                )),
            ),
            Err(AppError::PremiumRequired) => (
                false,
                Some(format!(
                    "\"{}\" is ready. Spotify Premium is required to start playback automatically.",
                    session.shadow_name
                )),
            ),
            Err(e) => {
                log::warn!("randomizer: playback start failed: {e}");
                (
                    false,
                    Some(format!(
                        "\"{}\" is ready, but playback could not be started: {e}",
                        session.shadow_name
                    )),
                )
            }
        }
    } else {
        (false, None)
    };

    state.poll_now.notify_one();
    Ok(RandomizeResult {
        session,
        playback_started,
        warning,
    })
}

/// Re-fetch the source, re-shuffle and rewrite the shadow playlist.
///
/// With `pivot` (the URI playing right now) the rewrite is incremental so the
/// playing item survives: everything else is removed, then the fresh order is
/// appended. The result is `[pivot] + shuffle(rest)` and `pending_pin` is set
/// so the poller can confirm the hand-over. Without a pivot the playlist is
/// replaced outright.
pub async fn reshuffle(
    state: &AppState,
    session: &SessionRow,
    pivot: Option<&str>,
    automatic: bool,
) -> Result<SessionRow> {
    let shadow = &session.shadow_playlist_id;
    let mut built = build_order(state, &session.source_playlist_id, Some(shadow), pivot).await?;
    let fresh = std::mem::take(&mut built.uris);

    let (mut order, pinned) = match pivot {
        Some(p) => {
            // Everything currently in the shadow except the pivot. Remove by
            // URI takes out every occurrence, so it must run before the append.
            let mut to_remove: Vec<String> = session
                .track_order
                .iter()
                .filter(|u| u.as_str() != p)
                .cloned()
                .collect();
            to_remove.sort();
            to_remove.dedup();
            playlists::remove_items(&state.spotify, shadow, &to_remove, None).await?;
            playlists::add_items(&state.spotify, shadow, &fresh, None).await?;

            let mut order = Vec::with_capacity(fresh.len() + 1);
            order.push(p.to_string());
            order.extend(fresh);
            (order, Some(p.to_string()))
        }
        None => {
            playlists::set_playlist_items(&state.spotify, shadow, &fresh).await?;
            (fresh, None)
        }
    };
    verify_written(state, shadow, &mut order, &built.names, &mut built.missing).await?;

    let updated = SessionRow {
        track_count: order.len() as i64,
        last_track_uri: order.last().cloned(),
        last_shuffled_at: now(),
        reshuffle_count: session.reshuffle_count + automatic as i64,
        track_order: order,
        pending_pin: pinned,
        missing_tracks: built.missing,
        ..session.clone()
    };
    state.db.with(|c| db::randomizer::upsert(c, &updated))?;
    log::info!(
        "re-shuffled '{}' ({} tracks, {})",
        updated.shadow_name,
        updated.track_count,
        if updated.pending_pin.is_some() { "incremental around playing track" } else { "full replace" }
    );
    Ok(updated)
}

/// Index of the playing track within the session's order, matching the
/// playlist URI or, for relinked tracks, the original URI.
fn position_of(session: &SessionRow, state: &PlaybackState) -> Option<usize> {
    let uri = state.track_uri()?;
    let original = state.original_track_uri();
    session
        .track_order
        .iter()
        .position(|u| u == uri || Some(u.as_str()) == original)
}

/// Called by the shared polling loop with the previous and latest playback state.
pub async fn on_poll(
    app: &AppHandle,
    state: &AppState,
    previous: Option<&PlaybackState>,
    current: Option<&PlaybackState>,
) {
    let sessions = match state.db.with(db::randomizer::list_active) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("randomizer: could not load sessions: {e}");
            return;
        }
    };
    for session in sessions {
        if let Err(e) = check_session(app, state, &session, previous, current).await {
            log::warn!("randomizer: '{}' failed: {e}", session.shadow_name);
            let _ = app.emit(
                "randomizer-error",
                format!("Re-shuffle of \"{}\" failed: {e}", session.shadow_name),
            );
            if matches!(e, AppError::Spotify { status: 404, .. }) {
                // Source or shadow playlist is gone; stop tracking it.
                let _ = state
                    .db
                    .with(|c| db::randomizer::set_active(c, &session.shadow_playlist_id, false));
            }
        }
    }
}

async fn check_session(
    app: &AppHandle,
    state: &AppState,
    session: &SessionRow,
    previous: Option<&PlaybackState>,
    current: Option<&PlaybackState>,
) -> Result<()> {
    let n = session.track_order.len();
    if n < 2 {
        return Ok(());
    }
    let shadow_uri = shadow_context_uri(&session.shadow_playlist_id);
    let in_shadow = |p: &PlaybackState| p.context_uri() == Some(shadow_uri.as_str());

    let Some(cur) = current.filter(|p| in_shadow(p)) else {
        // Not playing the shadow playlist right now; a pending hand-over check
        // no longer applies.
        if session.pending_pin.is_some() {
            clear_pin(state, session)?;
        }
        return Ok(());
    };
    let Some(cur_idx) = position_of(session, cur) else {
        return Ok(());
    };
    let cur_uri = session.track_order[cur_idx].clone();
    let parked = !cur.is_playing && cur.progress_ms.unwrap_or(0) == 0;
    let device_id = cur.device.as_ref().and_then(|d| d.id.clone());

    // 1. Confirm the hand-over after an incremental re-shuffle.
    if let Some(pin) = session.pending_pin.as_deref() {
        if cur_uri == pin {
            if parked {
                log::info!("randomizer: '{}' stopped after the pivot track; resuming at 1", session.shadow_name);
                clear_pin(state, session)?;
                restart_at(state, session, 1, device_id.as_deref()).await?;
            }
            return Ok(());
        }
        clear_pin(state, session)?;
        if cur_idx + 1 >= n {
            // The failure mode seen with full replaces: player kept its old
            // numeric position (the end) instead of following the item.
            log::info!(
                "randomizer: Spotify jumped to the end of '{}' after the re-shuffle; restarting at 1",
                session.shadow_name
            );
            restart_at(state, session, 1, device_id.as_deref()).await?;
        }
        return Ok(());
    }

    // 2. Context ran out between polls: Spotify parks at 0:00 on the final track.
    if parked && cur_idx + 1 >= n {
        log::info!("randomizer: '{}' ran off the end; restarting", session.shadow_name);
        let updated = reshuffle(state, session, None, true).await?;
        match playback::play_playlist_unshuffled(&state.spotify, &session.shadow_playlist_id).await {
            Ok(()) | Err(AppError::NoActiveDevice) => {}
            Err(e) => return Err(e),
        }
        app.emit("randomizer-reshuffled", &updated)?;
        return Ok(());
    }

    // 3. Near the end.
    let near_end = cur_idx + END_WINDOW >= n;

    // 4. Jumped back to the start of the list from further in: repeat wrapped
    //    around, or the tail was skipped through between two polls. Picking an
    //    early track by hand also lands here; the chosen track keeps playing
    //    and a fresh order follows it, which is harmless.
    let wrapped = previous
        .filter(|p| in_shadow(p) && p.is_playing)
        .and_then(|p| position_of(session, p))
        .map(|prev_idx| cur_idx <= 1 && prev_idx >= 2 && prev_idx > cur_idx)
        .unwrap_or(false);

    if near_end || wrapped {
        log::info!(
            "randomizer: '{}' at track {}/{} ({}); re-shuffling",
            session.shadow_name,
            cur_idx + 1,
            n,
            if wrapped { "wrapped around" } else { "near the end" }
        );
        let updated = reshuffle(state, session, Some(&cur_uri), true).await?;
        app.emit("randomizer-reshuffled", &updated)?;
    }
    Ok(())
}

fn clear_pin(state: &AppState, session: &SessionRow) -> Result<()> {
    state
        .db
        .with(|c| db::randomizer::set_pending_pin(c, &session.shadow_playlist_id, None))
}

async fn restart_at(state: &AppState, session: &SessionRow, position: usize, device_id: Option<&str>) -> Result<()> {
    let context = shadow_context_uri(&session.shadow_playlist_id);
    match playback::start_playback(&state.spotify, &context, position, device_id).await {
        Ok(()) => playback::set_shuffle(&state.spotify, false, device_id).await,
        Err(AppError::NoActiveDevice) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_keeps_all_elements() {
        let mut v: Vec<u32> = (0..500).collect();
        fisher_yates(&mut v);
        let mut sorted = v.clone();
        sorted.sort();
        assert_eq!(sorted, (0..500).collect::<Vec<_>>());
    }

    #[test]
    fn shuffle_changes_order() {
        let mut v: Vec<u32> = (0..500).collect();
        fisher_yates(&mut v);
        assert_ne!(v, (0..500).collect::<Vec<_>>());
    }

    #[test]
    fn shuffle_handles_edge_sizes() {
        let mut empty: Vec<u32> = vec![];
        fisher_yates(&mut empty);
        let mut one = vec![1];
        fisher_yates(&mut one);
        assert_eq!(one, vec![1]);
    }

    #[test]
    fn shadow_naming() {
        assert_eq!(shadow_name("Chill"), "Chill-Utilify");
    }
}
