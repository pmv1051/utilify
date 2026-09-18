//! Playback log: one row per play, filled in progressively by the poller.
//!
//! `listened_ms` is the source of truth. The `skipped` flag is derived from it
//! against the user's play threshold at finish time; the Stats queries derive
//! it again at read time so a changed threshold re-classifies history.

use rusqlite::{params, Connection};

pub struct NewPlay<'a> {
    pub track_uri: &'a str,
    pub track_name: Option<&'a str>,
    pub artist_name: Option<&'a str>,
    pub artist_id: Option<&'a str>,
    pub album_name: Option<&'a str>,
    pub context_uri: Option<&'a str>,
    pub started_at: i64,
    pub duration_ms: Option<i64>,
    pub listened_ms: i64,
}

pub fn start(conn: &Connection, p: &NewPlay<'_>) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO playback_log
            (track_uri, track_name, artist_name, artist_id, album_name, context_uri,
             started_at, listened_ms, duration_ms, skipped, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?7)",
        params![
            p.track_uri,
            p.track_name,
            p.artist_name,
            p.artist_id,
            p.album_name,
            p.context_uri,
            p.started_at,
            p.listened_ms,
            p.duration_ms,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, listened_ms: i64, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE playback_log SET listened_ms = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, listened_ms, now],
    )?;
    Ok(())
}

pub fn finish(conn: &Connection, id: i64, listened_ms: i64, now: i64, threshold_ms: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE playback_log
         SET listened_ms = ?2, ended_at = ?3, updated_at = ?3, skipped = CASE WHEN ?2 < ?4 THEN 1 ELSE 0 END
         WHERE id = ?1",
        params![id, listened_ms, now, threshold_ms],
    )?;
    Ok(())
}

/// Close rows left open by a quit or crash, using the last poll time.
pub fn close_stale(conn: &Connection, threshold_ms: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE playback_log
         SET ended_at = COALESCE(updated_at, started_at),
             skipped = CASE WHEN listened_ms < ?1 THEN 1 ELSE 0 END
         WHERE ended_at IS NULL",
        [threshold_ms],
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::migrations;

    pub(crate) fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    pub(crate) fn play(conn: &Connection, uri: &str, artist: &str, started: i64, listened: i64, finish_it: bool) -> i64 {
        let id = start(
            conn,
            &NewPlay {
                track_uri: uri,
                track_name: Some("T"),
                artist_name: Some(artist),
                artist_id: Some(artist),
                album_name: None,
                context_uri: Some("spotify:playlist:p1"),
                started_at: started,
                duration_ms: Some(200_000),
                listened_ms: 0,
            },
        )
        .unwrap();
        if finish_it {
            finish(conn, id, listened, started + listened / 1000, 10_000).unwrap();
        } else {
            update(conn, id, listened, started + 30).unwrap();
        }
        id
    }

    #[test]
    fn lifecycle_and_stale_rows() {
        let conn = fresh();
        let t0 = 1_800_000_000;
        play(&conn, "spotify:track:a", "A", t0, 120_000, true);
        play(&conn, "spotify:track:a", "A", t0 + 400, 5_000, true); // skip
        play(&conn, "spotify:track:c", "C", t0 + 1200, 40_000, false); // left open

        let skipped: i64 = conn
            .query_row("SELECT SUM(skipped) FROM playback_log WHERE ended_at IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(skipped, 1);

        // Open row gets closed at "launch" and classified.
        assert_eq!(close_stale(&conn, 10_000).unwrap(), 1);
        assert_eq!(close_stale(&conn, 10_000).unwrap(), 0);
        let (ended, skipped_c): (Option<i64>, i64) = conn
            .query_row(
                "SELECT ended_at, skipped FROM playback_log WHERE track_uri = 'spotify:track:c'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(ended, Some(t0 + 1200 + 30));
        assert_eq!(skipped_c, 0);
    }
}
