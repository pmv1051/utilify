//! Updates via Tauri's updater plugin. Releases publish `latest.json` (signed
//! with the key whose public half is in `tauri.conf.json`).
//!
//! Nothing is ever installed automatically. Checking is opt-in: when the
//! user enables it in Settings, a background task checks shortly after
//! launch and then every few hours and raises an `update-available` event;
//! installing is always a button press.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::db::config;
use crate::error::{AppError, Result};
use crate::state::AppState;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);

pub fn check_enabled(state: &AppState) -> bool {
    state
        .db
        .with(|c| config::get(c, config::UPDATE_CHECK_ENABLED))
        .ok()
        .flatten()
        .map(|v| v == "1")
        .unwrap_or(false)
}

pub fn set_check_enabled(state: &AppState, enabled: bool) -> Result<()> {
    state
        .db
        .with(|c| config::set(c, config::UPDATE_CHECK_ENABLED, if enabled { "1" } else { "0" }))
}

/// Background task: periodic checks while the opt-in setting is on. Only
/// notifies; never installs.
pub fn start_periodic(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            let state = app.state::<AppState>();
            if check_enabled(&state) {
                match check(&app, &state).await {
                    Ok(Some(info)) => {
                        let _ = app.emit("update-available", &info);
                    }
                    Ok(None) => log::debug!("updater: up to date"),
                    Err(e) => log::debug!("updater: periodic check failed: {e}"),
                }
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Ask the release feed whether a newer version exists. The `Update` handle
/// is kept in state so `install` can proceed without a second check.
pub async fn check(app: &AppHandle, state: &AppState) -> Result<Option<UpdateInfo>> {
    let updater = app
        .updater()
        .map_err(|e| AppError::other(format!("Updater is not available in this build: {e}")))?;
    let update = updater.check().await.map_err(|e| match e {
        // The endpoint 404s until the first release (with latest.json) is published.
        tauri_plugin_updater::Error::ReleaseNotFound => AppError::other(
            "No published release found on GitHub yet. Updates become available once the first release \
             (with its latest.json) is published.",
        ),
        other => AppError::other(format!("Could not check for updates: {other}")),
    })?;

    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    });
    *state.pending_update.lock().unwrap_or_else(|e| e.into_inner()) = update;
    if let Some(i) = &info {
        log::info!("update available: {} (running {})", i.version, i.current_version);
    }
    Ok(info)
}

/// Download and install the update found by `check`, then restart.
pub async fn install(app: &AppHandle, state: &AppState) -> Result<()> {
    let update = state
        .pending_update
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| AppError::other("No update has been found yet. Check for updates first."))?;

    let app2 = app.clone();
    let mut downloaded: u64 = 0;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = app2.emit("update-progress", UpdateProgress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| AppError::other(format!("Update failed: {e}")))?;

    log::info!("update installed; restarting");
    // The relaunched copy must not find this process still holding the
    // single-instance lock, or it would hand off to us and exit while we exit
    // too, leaving nothing running. The lock is normally released on the exit
    // event, but Tauri skips that event when restart runs on the main thread,
    // so release it here rather than depend on which thread we are on.
    #[cfg(desktop)]
    tauri_plugin_single_instance::destroy(app);
    app.restart();
}
