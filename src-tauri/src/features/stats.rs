//! Playback logging (fed by the polling loop) and the Stats summary.
//!
//! Polls are 30 s apart, so listening time is credited from progress deltas
//! between polls: a delta larger than the wall-clock gap means a seek and is
//! capped; a track first seen mid-way is credited at most one poll interval.
//! A play is finalized when the track changes, playback stops, or (via
//! `close_stale` at launch) when the app was closed mid-song. A play with
//! under 10 s heard counts as a skip; either way the track is now "seen".

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::playback_log::{self, NewPlay};
use crate::db::stats::StatsSummary;
use crate::db::{self, config, now};
use crate::error::{AppError, Result};
use crate::spotify::models::PlaybackState;
use crate::state::AppState;

/// Poll interval plus slack: the most we credit for progress we did not watch.
const MAX_UNWATCHED_CREDIT_MS: i64 = 35_000;

pub const DEFAULT_PLAY_THRESHOLD_SECS: i64 = 10;
pub const MIN_PLAY_THRESHOLD_SECS: i64 = 1;
pub const MAX_PLAY_THRESHOLD_SECS: i64 = 600;

/// User-adjustable: how long a track must be heard to count as a play.
pub fn play_threshold_ms(state: &AppState) -> i64 {
    state
        .db
        .with(|c| config::get(c, config::PLAY_THRESHOLD_SECS))
        .ok()
        .flatten()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(DEFAULT_PLAY_THRESHOLD_SECS)
        .clamp(MIN_PLAY_THRESHOLD_SECS, MAX_PLAY_THRESHOLD_SECS)
        * 1000
}

pub fn set_play_threshold(state: &AppState, secs: i64) -> Result<()> {
    if !(MIN_PLAY_THRESHOLD_SECS..=MAX_PLAY_THRESHOLD_SECS).contains(&secs) {
        return Err(AppError::other(format!(
            "Play threshold must be between {MIN_PLAY_THRESHOLD_SECS} and {MAX_PLAY_THRESHOLD_SECS} seconds."
        )));
    }
    state
        .db
        .with(|c| config::set(c, config::PLAY_THRESHOLD_SECS, &secs.to_string()))
}

#[derive(Debug, Clone)]
pub struct CurrentPlay {
    pub log_id: i64,
    pub uri: String,
    pub last_progress_ms: i64,
    pub last_seen_at: i64,
    pub listened_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayFinished {
    pub track_uri: String,
    pub listened_ms: i64,
    pub skipped: bool,
}

/// Called every poll with the latest playback state.
pub fn on_tick(app: &AppHandle, state: &AppState, current: Option<&PlaybackState>) {
    let now = now();
    let item = current
        .and_then(|p| p.item.as_ref().map(|i| (p, i)))
        .filter(|(_, i)| i.uri.as_deref().map(|u| u.starts_with("spotify:track:")).unwrap_or(false));

    let mut guard = state.current_play.lock().unwrap_or_else(|e| e.into_inner());
    let previous = guard.take();

    match (previous, item) {
        (Some(mut play), Some((pb, track))) if track.uri.as_deref() == Some(play.uri.as_str()) && !restarted(&play, pb) => {
            let progress = pb.progress_ms.unwrap_or(0) as i64;
            let elapsed_ms = (now - play.last_seen_at).max(0) * 1000;
            let delta = progress - play.last_progress_ms;
            // Only forward progress counts, and never more than the time that passed.
            let credit = if delta > 0 { delta.min(elapsed_ms + 2_000) } else { 0 };
            play.listened_ms += credit;
            play.last_progress_ms = progress;
            play.last_seen_at = now;
            if let Err(e) = state.db.with(|c| playback_log::update(c, play.log_id, play.listened_ms, now)) {
                log::warn!("stats: could not update play: {e}");
            }
            *guard = Some(play);
        }
        (previous, item) => {
            if let Some(play) = previous {
                finish(app, state, &play, now);
            }
            if let Some((pb, track)) = item {
                let progress = pb.progress_ms.unwrap_or(0) as i64;
                let initial = progress.min(MAX_UNWATCHED_CREDIT_MS);
                let uri = track.uri.clone().unwrap_or_default();
                let artist = track.artists.first();
                let new = NewPlay {
                    track_uri: &uri,
                    track_name: Some(&track.name),
                    artist_name: Some(&track.artist_names()),
                    artist_id: artist.and_then(|a| a.id.as_deref()),
                    album_name: track.album.as_ref().map(|a| a.name.as_str()),
                    context_uri: pb.context_uri(),
                    started_at: now - progress / 1000,
                    duration_ms: track.duration_ms.map(|d| d as i64),
                    listened_ms: initial,
                };
                match state.db.with(|c| playback_log::start(c, &new)) {
                    Ok(id) => {
                        *guard = Some(CurrentPlay {
                            log_id: id,
                            uri,
                            last_progress_ms: progress,
                            last_seen_at: now,
                            listened_ms: initial,
                        });
                    }
                    Err(e) => log::warn!("stats: could not log play: {e}"),
                }
            }
        }
    }
}

/// Same track but progress jumped back to the start: a repeat, log it anew.
fn restarted(play: &CurrentPlay, pb: &PlaybackState) -> bool {
    let progress = pb.progress_ms.unwrap_or(0) as i64;
    progress + 5_000 < play.last_progress_ms && progress < 15_000
}

fn finish(app: &AppHandle, state: &AppState, play: &CurrentPlay, now: i64) {
    let threshold = play_threshold_ms(state);
    let skipped = play.listened_ms < threshold;
    let result = state.db.with(|c| {
        playback_log::finish(c, play.log_id, play.listened_ms, now, threshold)?;
        db::discovery::mark_outcome(c, &play.uri, skipped)
    });
    match result {
        Ok(was_discovery) => {
            log::debug!(
                "stats: finished {} ({} ms, skipped={skipped}, discovery={was_discovery})",
                play.uri,
                play.listened_ms
            );
            let _ = app.emit(
                "play-finished",
                PlayFinished {
                    track_uri: play.uri.clone(),
                    listened_ms: play.listened_ms,
                    skipped,
                },
            );
        }
        Err(e) => log::warn!("stats: could not finish play: {e}"),
    }
}

/// Close rows left open by a previous run.
pub fn close_stale(state: &AppState) {
    let threshold = play_threshold_ms(state);
    match state.db.with(|c| playback_log::close_stale(c, threshold)) {
        Ok(n) if n > 0 => log::info!("stats: closed {n} play(s) left open by the previous run"),
        Ok(_) => {}
        Err(e) => log::warn!("stats: close_stale failed: {e}"),
    }
}

pub fn summary(state: &AppState, range_days: Option<u32>) -> Result<StatsSummary> {
    let since = range_days.map(|d| now() - d as i64 * 86_400);
    let threshold = play_threshold_ms(state);
    state.db.with(|c| db::stats::summary(c, since, threshold))
}
