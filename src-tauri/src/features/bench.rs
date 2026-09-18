//! Bench: temporarily remove a track from a playlist and bring it back later.
//!
//! Restores run from the shared polling loop ([`on_tick`]), so a bench that
//! expired while the app was closed is restored on the first tick after
//! launch. A track benched from (or whose source feeds) a tracked `-Utilify`
//! shadow playlist is not re-inserted mid-playback; it re-enters the pool on
//! the next re-shuffle because it is no longer excluded.

use tauri::{AppHandle, Emitter};

use crate::db::bench::{BenchRow, NewBench};
use crate::db::{self, now};
use crate::error::{AppError, Result};
use crate::features::randomizer;
use crate::spotify::playlists;
use crate::state::AppState;

pub const MIN_DURATION_SECS: i64 = 60;
pub const MAX_DURATION_SECS: i64 = 366 * 24 * 3600;

pub struct BenchRequest<'a> {
    pub playlist_id: &'a str,
    pub track_uri: &'a str,
    pub track_name: Option<&'a str>,
    pub artist_name: Option<&'a str>,
    pub position: Option<i64>,
    pub duration_secs: i64,
}

/// Remove the track from the playlist now and schedule its return.
pub async fn bench_track(state: &AppState, req: BenchRequest<'_>) -> Result<BenchRow> {
    if !(MIN_DURATION_SECS..=MAX_DURATION_SECS).contains(&req.duration_secs) {
        return Err(AppError::other("Bench duration must be between 1 minute and 1 year."));
    }
    if !req.track_uri.starts_with("spotify:track:") {
        return Err(AppError::other("Only Spotify tracks can be benched."));
    }
    let already = state
        .db
        .with(|c| db::bench::is_active(c, req.playlist_id, req.track_uri))?;
    if already {
        return Err(AppError::other("That track is already benched from this playlist."));
    }

    let playlist_name = state
        .db
        .with(|c| db::playlists::get(c, req.playlist_id, None))?
        .map(|p| p.name);

    // Remove from the chosen playlist.
    let uris = vec![req.track_uri.to_string()];
    playlists::remove_items(&state.spotify, req.playlist_id, &uris, None).await?;

    // Keep tracked shadow playlists consistent: if this playlist *is* a shadow,
    // or is the source of one, drop the track there too so it stops playing
    // now rather than after the next re-shuffle.
    let shadows = state.db.with(|c| {
        let mut out = Vec::new();
        if let Some(s) = db::randomizer::get_by_shadow(c, req.playlist_id)? {
            if s.active {
                out.push(s);
            }
        }
        for s in db::randomizer::list_active(c)? {
            if s.source_playlist_id == req.playlist_id && s.shadow_playlist_id != req.playlist_id {
                out.push(s);
            }
        }
        Ok(out)
    })?;
    for mut session in shadows {
        if session.shadow_playlist_id != req.playlist_id {
            if let Err(e) = playlists::remove_items(&state.spotify, &session.shadow_playlist_id, &uris, None).await {
                log::warn!("bench: could not remove from shadow '{}': {e}", session.shadow_name);
                continue;
            }
        }
        session.track_order.retain(|u| u != req.track_uri);
        session.track_count = session.track_order.len() as i64;
        session.last_track_uri = session.track_order.last().cloned();
        state.db.with(|c| db::randomizer::upsert(c, &session))?;
        log::info!("bench: pruned {} from shadow '{}'", req.track_uri, session.shadow_name);
    }

    let benched_at = now();
    let new = NewBench {
        track_uri: req.track_uri,
        track_name: req.track_name,
        artist_name: req.artist_name,
        playlist_id: req.playlist_id,
        playlist_name: playlist_name.as_deref(),
        original_position: req.position,
        benched_at,
        restore_at: benched_at + req.duration_secs,
    };
    let id = state.db.with(|c| db::bench::insert(c, &new))?;
    let row = state
        .db
        .with(|c| db::bench::get(c, id))?
        .ok_or_else(|| AppError::other("bench row vanished"))?;
    log::info!(
        "benched {} from '{}' until {}",
        row.track_uri,
        row.playlist_name.as_deref().unwrap_or(&row.playlist_id),
        row.restore_at
    );
    Ok(row)
}

/// Put a benched track back. Tracked shadow playlists are left alone; the
/// track re-enters on the next re-shuffle.
pub async fn restore(state: &AppState, row: &BenchRow) -> Result<()> {
    let is_tracked_shadow = state
        .db
        .with(|c| db::randomizer::get_by_shadow(c, &row.playlist_id))?
        .map(|s| s.active)
        .unwrap_or(false);

    if is_tracked_shadow {
        log::info!(
            "bench: {} returns to '{}' on its next re-shuffle",
            row.track_uri,
            row.playlist_name.as_deref().unwrap_or(&row.playlist_id)
        );
    } else {
        let uris = vec![row.track_uri.clone()];
        let position = row.original_position.filter(|p| *p >= 0).map(|p| p as usize);
        let result = playlists::add_items(&state.spotify, &row.playlist_id, &uris, position).await;
        match result {
            Ok(()) => {}
            // Position no longer valid (playlist shrank): append instead.
            Err(AppError::Spotify { status: 400 | 403, .. }) if position.is_some() => {
                playlists::add_items(&state.spotify, &row.playlist_id, &uris, None).await?;
            }
            Err(AppError::Spotify { status: 404, .. }) => {
                log::warn!("bench: playlist {} is gone; nothing to restore into", row.playlist_id);
            }
            Err(e) => return Err(e),
        }
        log::info!(
            "bench: restored {} to '{}'",
            row.track_uri,
            row.playlist_name.as_deref().unwrap_or(&row.playlist_id)
        );
    }
    state.db.with(|c| db::bench::mark_restored(c, row.id, now()))?;
    Ok(())
}

/// Manual early restore.
pub async fn unbench(state: &AppState, id: i64) -> Result<BenchRow> {
    let row = state
        .db
        .with(|c| db::bench::get(c, id))?
        .ok_or_else(|| AppError::other("Unknown bench entry."))?;
    if row.restored_at.is_some() {
        return Err(AppError::other("That track has already been restored."));
    }
    restore(state, &row).await?;
    Ok(row)
}

/// Called by the shared polling loop: restore everything that is due.
pub async fn on_tick(app: &AppHandle, state: &AppState) {
    let due = match state.db.with(|c| db::bench::list_due(c, now())) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("bench: could not query due benches: {e}");
            return;
        }
    };
    for row in due {
        match restore(state, &row).await {
            Ok(()) => {
                let _ = app.emit("bench-restored", &row);
            }
            Err(e) => {
                log::warn!("bench: restore of {} failed: {e}", row.track_uri);
                let _ = app.emit(
                    "bench-error",
                    format!(
                        "Could not restore \"{}\" to \"{}\": {e}",
                        row.track_name.as_deref().unwrap_or(&row.track_uri),
                        row.playlist_name.as_deref().unwrap_or(&row.playlist_id)
                    ),
                );
            }
        }
    }
}

/// Convenience for the UI: which shadow (if any) a playlist maps to.
#[allow(dead_code)]
pub fn shadow_uri_for(playlist_id: &str) -> String {
    randomizer::shadow_context_uri(playlist_id)
}
