//! Bench table: tracks temporarily removed from a playlist with a restore time.

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchRow {
    pub id: i64,
    pub track_uri: String,
    pub track_name: Option<String>,
    pub artist_name: Option<String>,
    pub playlist_id: String,
    pub playlist_name: Option<String>,
    pub original_position: Option<i64>,
    pub benched_at: i64,
    pub restore_at: i64,
    pub restored_at: Option<i64>,
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<BenchRow> {
    Ok(BenchRow {
        id: row.get("id")?,
        track_uri: row.get("track_uri")?,
        track_name: row.get("track_name")?,
        artist_name: row.get("artist_name")?,
        playlist_id: row.get("playlist_id")?,
        playlist_name: row.get("playlist_name")?,
        original_position: row.get("original_position")?,
        benched_at: row.get("benched_at")?,
        restore_at: row.get("restore_at")?,
        restored_at: row.get("restored_at")?,
    })
}

pub struct NewBench<'a> {
    pub track_uri: &'a str,
    pub track_name: Option<&'a str>,
    pub artist_name: Option<&'a str>,
    pub playlist_id: &'a str,
    pub playlist_name: Option<&'a str>,
    pub original_position: Option<i64>,
    pub benched_at: i64,
    pub restore_at: i64,
}

pub fn insert(conn: &Connection, b: &NewBench<'_>) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO benched_tracks
            (track_uri, track_name, artist_name, playlist_id, playlist_name,
             original_position, benched_at, restore_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            b.track_uri,
            b.track_name,
            b.artist_name,
            b.playlist_id,
            b.playlist_name,
            b.original_position,
            b.benched_at,
            b.restore_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<BenchRow>> {
    conn.query_row("SELECT * FROM benched_tracks WHERE id = ?1", [id], from_row)
        .optional()
}

/// Benches not yet restored, soonest restore first.
pub fn list_active(conn: &Connection) -> rusqlite::Result<Vec<BenchRow>> {
    let mut stmt =
        conn.prepare("SELECT * FROM benched_tracks WHERE restored_at IS NULL ORDER BY restore_at ASC")?;
    let rows = stmt.query_map([], from_row)?;
    rows.collect()
}

/// Benches whose restore time has passed.
pub fn list_due(conn: &Connection, now: i64) -> rusqlite::Result<Vec<BenchRow>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM benched_tracks WHERE restored_at IS NULL AND restore_at <= ?1 ORDER BY restore_at ASC",
    )?;
    let rows = stmt.query_map([now], from_row)?;
    rows.collect()
}

pub fn mark_restored(conn: &Connection, id: i64, at: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE benched_tracks SET restored_at = ?2 WHERE id = ?1 AND restored_at IS NULL",
        params![id, at],
    )?;
    Ok(())
}

/// True if this exact track is already benched from this playlist.
pub fn is_active(conn: &Connection, playlist_id: &str, track_uri: &str) -> rusqlite::Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM benched_tracks
         WHERE restored_at IS NULL AND playlist_id = ?1 AND track_uri = ?2",
        params![playlist_id, track_uri],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// URIs of tracks currently benched from any of the given playlists.
pub fn active_benched_uris(conn: &Connection, playlist_ids: &[&str]) -> rusqlite::Result<Vec<String>> {
    if playlist_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = playlist_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT DISTINCT track_uri FROM benched_tracks
         WHERE restored_at IS NULL AND playlist_id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(playlist_ids.iter()), |r| r.get(0))?;
    rows.collect()
}
