use std::path::PathBuf;
use std::sync::Mutex;

use tokio::sync::Notify;

use crate::db::{config, Db};
use crate::spotify::client::SpotifyClient;
use crate::spotify::models::PlaybackState;

/// Shared application state managed by Tauri.
pub struct AppState {
    pub db: Db,
    pub spotify: SpotifyClient,
    pub db_path: PathBuf,
    /// Most recent playback state observed by the polling loop.
    pub last_playback: Mutex<Option<PlaybackState>>,
    /// Unix time since which polls have failed with network errors, if any.
    pub offline_since: Mutex<Option<i64>>,
    /// Update found by the last check, ready to install.
    pub pending_update: Mutex<Option<tauri_plugin_updater::Update>>,
    /// Last time the poller probed Spotify during a quota pause.
    /// Signal the polling loop to run immediately (e.g. right after a randomize).
    pub poll_now: Notify,
}

impl AppState {
    pub fn new(db: Db, spotify: SpotifyClient, db_path: PathBuf) -> Self {
        Self {
            db,
            spotify,
            db_path,
            last_playback: Mutex::new(None),
            offline_since: Mutex::new(None),
            pending_update: Mutex::new(None),
            poll_now: Notify::new(),
        }
    }

    /// Record connectivity from the poller. Returns `Some(now_online)` when
    /// the state flipped, so callers can notify the UI once per transition.
    pub fn set_online(&self, online: bool, now: i64) -> Option<bool> {
        let mut guard = self.offline_since.lock().unwrap_or_else(|e| e.into_inner());
        match (online, *guard) {
            (true, Some(_)) => {
                *guard = None;
                Some(true)
            }
            (false, None) => {
                *guard = Some(now);
                Some(false)
            }
            _ => None,
        }
    }

    pub fn offline_since(&self) -> Option<i64> {
        *self.offline_since.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn minimize_to_tray(&self) -> bool {
        self.db
            .with(|c| config::get(c, config::MINIMIZE_TO_TRAY))
            .ok()
            .flatten()
            .map(|v| v != "0" && v != "false")
            .unwrap_or(true)
    }

    pub fn set_last_playback(&self, state: Option<PlaybackState>) -> Option<PlaybackState> {
        let mut guard = self.last_playback.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::replace(&mut *guard, state)
    }

    pub fn last_playback(&self) -> Option<PlaybackState> {
        self.last_playback
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
