//! The single shared playback polling loop.
//!
//! Every 30 seconds (never faster, to stay well inside Spotify's rate limits)
//! it fetches `/me/player`, stores the result, emits `playback-state` to the
//! frontend and hands the state to each subscriber: the Randomizer, then
//! Bench restores.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::error::AppError;
use crate::features::{bench, randomizer};
use crate::spotify::playback;
use crate::state::AppState;

pub const POLL_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connectivity {
    pub online: bool,
    pub since: Option<i64>,
}
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
    // Only the player family gates the poll; a catalog pause must not stop
    // bench restores. Nothing is sent while it is paused: the budget resets
    // daily, so probing before then only spends a request on a refusal.
    if state.spotify.quota_status().is_paused(crate::spotify::client::QuotaScope::Player) {
        return;
    }
    match playback::get_playback_state(&state.spotify).await {
        Ok(current) => {
            if state.set_online(true, crate::db::now()).is_some() {
                log::info!("polling: Spotify reachable again");
                let _ = app.emit("connectivity", Connectivity { online: true, since: None });
            }
            let previous = state.set_last_playback(current.clone());
            let _ = app.emit("playback-state", &current);
            randomizer::on_poll(app, &state, previous.as_ref(), current.as_ref()).await;
        }
        Err(AppError::AuthExpired) | Err(AppError::NotAuthenticated) => {
            log::warn!("polling: session expired");
            let _ = app.emit("auth-expired", ());
            return;
        }
        Err(e @ AppError::Http(_)) => {
            log::warn!("polling: playback fetch failed: {e}");
            let now = crate::db::now();
            if state.set_online(false, now).is_some() {
                let _ = app.emit("connectivity", Connectivity { online: false, since: Some(now) });
            }
            // No point running scheduled work without connectivity.
            return;
        }
        Err(AppError::QuotaExceeded { .. }) | Err(AppError::QuotaCooldown { .. }) => {
            log::debug!("polling: quota still exhausted");
            return;
        }
        Err(e) => log::warn!("polling: playback fetch failed: {e}"),
    }
    // Scheduled work that does not depend on playback state.
    bench::on_tick(app, &state).await;
}
