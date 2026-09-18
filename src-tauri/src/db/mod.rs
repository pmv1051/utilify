pub mod bench;
pub mod config;
pub mod discovery;
pub mod genres;
pub mod migrations;
pub mod playback_log;
pub mod playlists;
pub mod randomizer;
pub mod stats;

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{AppError, Result};

/// Thread-safe handle to the SQLite database.
///
/// All access goes through short, synchronous closures so no lock is ever
/// held across an `.await`.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        migrations::run(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let guard = self
            .conn
            .lock()
            .map_err(|_| AppError::other("database lock poisoned"))?;
        Ok(f(&guard)?)
    }

    pub fn with_mut<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| AppError::other("database lock poisoned"))?;
        Ok(f(&mut guard)?)
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
