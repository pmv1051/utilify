//! The single shared playback polling loop.
//!
//! Every 30 seconds (never faster, to stay well inside Spotify's rate limits)
//! it fetches `/me/player`, stores the result, emits `playback-state` to the
//! frontend and hands the state to each subscriber: playback logging (Stats
//! and Discovery outcomes), the Randomizer, then Bench restores.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::error::AppError;
use crate::features::{bench, discovery, randomizer, stats};
use crate::spotify::playback;
use crate::state::AppState;

pub const POLL_INTERVAL: Duration = Duration::from_secs(30);
const STARTUP_DELAY: Duration = Duration::from_secs(3);
/// Minimum gap after an explicit `poll_now` nudge, so a burst of user actions
/// cannot turn into a burst of requests.
const NUDGE_DELAY: Duration = Duration::from_secs(2);

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            tick(&app).await;
            let state = app.state::<AppState>();
            tokio::select! {
                _ = tokio::time::sleep(POLL_INTERVAL) => {}
                _ = state.poll_now.notified() => {
                    tokio::time::sleep(NUDGE_DELAY).await;
                }
            }
        }
    });
}

async fn tick(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.spotify.is_authenticated() {
        return;
    }
    match playback::get_playback_state(&state.spotify).await {
        Ok(current) => {
            let previous = state.set_last_playback(current.clone());
            let _ = app.emit("playback-state", &current);
            stats::on_tick(app, &state, current.as_ref());
            discovery::on_poll(&state, previous.as_ref(), current.as_ref());
            randomizer::on_poll(app, &state, previous.as_ref(), current.as_ref()).await;
        }
        Err(AppError::AuthExpired) | Err(AppError::NotAuthenticated) => {
            log::warn!("polling: session expired");
            let _ = app.emit("auth-expired", ());
            return;
        }
        Err(e) => log::warn!("polling: playback fetch failed: {e}"),
    }
    // Scheduled work that does not depend on playback state.
    bench::on_tick(app, &state).await;
}
