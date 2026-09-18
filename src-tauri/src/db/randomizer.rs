use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::spotify::models::MissingTrack;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub shadow_playlist_id: String,
    pub source_playlist_id: String,
    pub source_name: String,
    pub shadow_name: String,
    pub track_count: i64,
    pub last_track_uri: Option<String>,
    pub last_shuffled_at: i64,
    pub reshuffle_count: i64,
    pub active: bool,
    /// The shuffled order currently written to the shadow playlist. Not sent
    /// to the frontend (can be hundreds of URIs).
    #[serde(skip)]
    pub track_order: Vec<String>,
    /// Track pinned to position 0 by the last re-shuffle, until the poller has
    /// confirmed that playback moved on into the new order.
    pub pending_pin: Option<String>,
    /// Source entries that did not make it into the shadow playlist.
    pub missing_tracks: Vec<MissingTrack>,
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<SessionRow> {
    let order_json: Option<String> = row.get("track_order")?;
    let missing_json: Option<String> = row.get("missing_tracks")?;
    Ok(SessionRow {
        shadow_playlist_id: row.get("shadow_playlist_id")?,
        source_playlist_id: row.get("source_playlist_id")?,
        source_name: row.get("source_name")?,
        shadow_name: row.get("shadow_name")?,
        track_count: row.get("track_count")?,
        last_track_uri: row.get("last_track_uri")?,
        last_shuffled_at: row.get("last_shuffled_at")?,
        reshuffle_count: row.get("reshuffle_count")?,
        active: row.get::<_, i64>("active")? != 0,
        track_order: order_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        pending_pin: row.get("pending_pin")?,
        missing_tracks: missing_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
    })
}

pub fn upsert(conn: &Connection, s: &SessionRow) -> rusqlite::Result<()> {
    let order_json = serde_json::to_string(&s.track_order)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let missing_json = serde_json::to_string(&s.missing_tracks)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "INSERT INTO randomizer_sessions
            (shadow_playlist_id, source_playlist_id, source_name, shadow_name, track_count,
             last_track_uri, last_shuffled_at, reshuffle_count, active, track_order, pending_pin,
             missing_tracks)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(shadow_playlist_id) DO UPDATE SET
            source_playlist_id = excluded.source_playlist_id,
            source_name        = excluded.source_name,
            shadow_name        = excluded.shadow_name,
            track_count        = excluded.track_count,
            last_track_uri     = excluded.last_track_uri,
            last_shuffled_at   = excluded.last_shuffled_at,
            reshuffle_count    = excluded.reshuffle_count,
            active             = excluded.active,
            track_order        = excluded.track_order,
            pending_pin        = excluded.pending_pin,
            missing_tracks     = excluded.missing_tracks",
        params![
            s.shadow_playlist_id,
            s.source_playlist_id,
            s.source_name,
            s.shadow_name,
            s.track_count,
            s.last_track_uri,
            s.last_shuffled_at,
            s.reshuffle_count,
            s.active as i64,
            order_json,
            s.pending_pin,
            missing_json,
        ],
    )?;
    Ok(())
}

pub fn list_active(conn: &Connection) -> rusqlite::Result<Vec<SessionRow>> {
    let mut stmt =
        conn.prepare("SELECT * FROM randomizer_sessions WHERE active = 1 ORDER BY last_shuffled_at DESC")?;
    let rows = stmt.query_map([], from_row)?;
    rows.collect()
}

pub fn get_by_shadow(conn: &Connection, shadow_id: &str) -> rusqlite::Result<Option<SessionRow>> {
    conn.query_row(
        "SELECT * FROM randomizer_sessions WHERE shadow_playlist_id = ?1",
        [shadow_id],
        from_row,
    )
    .optional()
}

pub fn set_active(conn: &Connection, shadow_id: &str, active: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE randomizer_sessions SET active = ?2 WHERE shadow_playlist_id = ?1",
        params![shadow_id, active as i64],
    )?;
    Ok(())
}

pub fn set_pending_pin(conn: &Connection, shadow_id: &str, pin: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE randomizer_sessions SET pending_pin = ?2 WHERE shadow_playlist_id = ?1",
        params![shadow_id, pin],
    )?;
    Ok(())
}

/// A source playlist has exactly one live shadow. When a new shadow is created
/// (for example after the user deleted the old one in Spotify), retire the rest.
pub fn deactivate_others_for_source(conn: &Connection, source_id: &str, keep_shadow_id: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE randomizer_sessions SET active = 0
         WHERE active = 1 AND source_playlist_id = ?1 AND shadow_playlist_id <> ?2",
        params![source_id, keep_shadow_id],
    )
}

/// Retire active sessions whose shadow playlist is no longer in the user's
/// library (deleted or unfollowed in Spotify). `existing_ids` is a fresh
/// `/me/playlists` listing.
pub fn deactivate_missing(conn: &Connection, existing_ids: &[String]) -> rusqlite::Result<usize> {
    if existing_ids.is_empty() {
        // An empty listing is more likely an API hiccup than a truly empty library.
        return Ok(0);
    }
    let placeholders = existing_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "UPDATE randomizer_sessions SET active = 0
         WHERE active = 1 AND shadow_playlist_id NOT IN ({placeholders})"
    );
    conn.execute(&sql, params_from_iter(existing_ids.iter()))
}
