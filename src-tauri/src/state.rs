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
            poll_now: Notify::new(),
        }
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
