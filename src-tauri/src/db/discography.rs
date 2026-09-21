//! On-disk cache for the two listings the Discography tool depends on.
//!
//! In Development Mode `/artists/{id}/albums` and `/albums/{id}/tracks` return
//! ten items a page, so one catalogue costs dozens of requests and a rebuild
//! used to cost them again. Both listings are stored as JSON here.
//!
//! An album's track list is fixed once released, so it never expires. An
//! artist's release list gains entries, so it carries a timestamp and the
//! caller decides how old is too old.

use rusqlite::{params, Connection, OptionalExtension};

/// Release listings older than this are refetched. A week is short enough to
/// pick up new releases without making the cache pointless.
pub const ALBUMS_MAX_AGE_SECS: i64 = 7 * 24 * 3600;

pub fn get_albums(conn: &Connection, artist_id: &str, groups: &str, now: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT albums FROM artist_albums_cache
         WHERE artist_id = ?1 AND groups = ?2 AND cached_at > ?3",
        params![artist_id, groups, now - ALBUMS_MAX_AGE_SECS],
        |r| r.get::<_, String>(0),
    )
    .optional()
}

pub fn put_albums(
    conn: &Connection,
    artist_id: &str,
    groups: &str,
    albums: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO artist_albums_cache (artist_id, groups, albums, cached_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(artist_id, groups) DO UPDATE SET albums = excluded.albums, cached_at = excluded.cached_at",
        params![artist_id, groups, albums, now],
    )?;
    Ok(())
}

/// Forget an artist's release listings so the next look refetches them.
pub fn forget_artist(conn: &Connection, artist_id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM artist_albums_cache WHERE artist_id = ?1", params![artist_id])?;
    Ok(())
}

pub fn get_album_tracks(conn: &Connection, album_id: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT tracks FROM album_tracks_cache WHERE album_id = ?1",
        params![album_id],
        |r| r.get::<_, String>(0),
    )
    .optional()
}

pub fn put_album_tracks(conn: &Connection, album_id: &str, tracks: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO album_tracks_cache (album_id, tracks, cached_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(album_id) DO UPDATE SET tracks = excluded.tracks, cached_at = excluded.cached_at",
        params![album_id, tracks, now],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheSize {
    pub artists: i64,
    pub albums: i64,
    pub bytes: i64,
}

pub fn size(conn: &Connection) -> rusqlite::Result<CacheSize> {
    let artists = conn.query_row(
        "SELECT COUNT(DISTINCT artist_id) FROM artist_albums_cache",
        [],
        |r| r.get(0),
    )?;
    let albums = conn.query_row("SELECT COUNT(*) FROM album_tracks_cache", [], |r| r.get(0))?;
    let bytes = conn.query_row(
        "SELECT COALESCE((SELECT SUM(LENGTH(albums)) FROM artist_albums_cache), 0)
              + COALESCE((SELECT SUM(LENGTH(tracks)) FROM album_tracks_cache), 0)",
        [],
        |r| r.get(0),
    )?;
    Ok(CacheSize {
        artists,
        albums,
        bytes,
    })
}

pub fn clear(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("DELETE FROM artist_albums_cache; DELETE FROM album_tracks_cache;")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    #[test]
    fn album_listings_round_trip_and_expire() {
        let conn = fresh();
        let now = 1_700_000_000;
        put_albums(&conn, "artist", "album,single", "[1,2]", now).unwrap();
        assert_eq!(get_albums(&conn, "artist", "album,single", now).unwrap().as_deref(), Some("[1,2]"));

        // A different group set is a different entry.
        assert!(get_albums(&conn, "artist", "appears_on", now).unwrap().is_none());

        // Too old to trust.
        let later = now + ALBUMS_MAX_AGE_SECS + 1;
        assert!(get_albums(&conn, "artist", "album,single", later).unwrap().is_none());

        // Refetching replaces rather than duplicating.
        put_albums(&conn, "artist", "album,single", "[3]", later).unwrap();
        assert_eq!(get_albums(&conn, "artist", "album,single", later).unwrap().as_deref(), Some("[3]"));
    }

    #[test]
    fn album_tracks_never_expire_but_can_be_forgotten() {
        let conn = fresh();
        put_album_tracks(&conn, "album", "[\"a\"]", 1).unwrap();
        assert_eq!(get_album_tracks(&conn, "album").unwrap().as_deref(), Some("[\"a\"]"));
        clear(&conn).unwrap();
        assert!(get_album_tracks(&conn, "album").unwrap().is_none());
    }

    #[test]
    fn forgetting_an_artist_leaves_other_artists_alone() {
        let conn = fresh();
        let now = 1_700_000_000;
        put_albums(&conn, "a", "album", "[1]", now).unwrap();
        put_albums(&conn, "a", "appears_on", "[2]", now).unwrap();
        put_albums(&conn, "b", "album", "[3]", now).unwrap();
        forget_artist(&conn, "a").unwrap();
        assert!(get_albums(&conn, "a", "album", now).unwrap().is_none());
        assert!(get_albums(&conn, "a", "appears_on", now).unwrap().is_none());
        assert!(get_albums(&conn, "b", "album", now).unwrap().is_some());
    }

    #[test]
    fn size_counts_artists_albums_and_bytes() {
        let conn = fresh();
        assert_eq!(size(&conn).unwrap().bytes, 0);
        put_albums(&conn, "a", "album", "[1]", 1).unwrap();
        put_albums(&conn, "a", "appears_on", "[2]", 1).unwrap();
        put_album_tracks(&conn, "x", "[\"t\"]", 1).unwrap();
        let s = size(&conn).unwrap();
        assert_eq!(s.artists, 1, "two group sets for one artist is one artist");
        assert_eq!(s.albums, 1);
        assert_eq!(s.bytes, 3 + 3 + 5);
    }
}
