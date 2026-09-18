//! Auto-update via Tauri's updater plugin. Releases publish `latest.json`
//! (signed with the key whose public half is in `tauri.conf.json`); the app
//! checks it on startup and on demand, then downloads, installs and restarts.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use crate::error::{AppError, Result};
use crate::state::AppState;

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
    let update = updater
        .check()
        .await
        .map_err(|e| AppError::other(format!("Could not check for updates: {e}")))?;

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
    app.restart();
}
