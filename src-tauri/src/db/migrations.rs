use rusqlite::Connection;

/// Ordered list of schema migrations. Each entry runs once, tracked by
/// SQLite's `user_version` pragma. Never edit an existing entry; append.
const MIGRATIONS: &[&str] = &[
    // v1: Phase 1 schema
    r#"
    CREATE TABLE IF NOT EXISTS config (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS playlists (
        id              TEXT PRIMARY KEY,
        name            TEXT NOT NULL,
        owner_id        TEXT,
        owner_name      TEXT,
        track_count     INTEGER NOT NULL DEFAULT 0,
        snapshot_id     TEXT,
        image_url       TEXT,
        uri             TEXT,
        is_public       INTEGER,
        is_collaborative INTEGER,
        position        INTEGER NOT NULL DEFAULT 0,
        updated_at      INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS benched_tracks (
        id                INTEGER PRIMARY KEY AUTOINCREMENT,
        track_uri         TEXT NOT NULL,
        track_name        TEXT,
        artist_name       TEXT,
        playlist_id       TEXT NOT NULL,
        playlist_name     TEXT,
        original_position INTEGER,
        benched_at        INTEGER NOT NULL,
        restore_at        INTEGER NOT NULL,
        restored_at       INTEGER
    );
    CREATE INDEX IF NOT EXISTS idx_benched_pending
        ON benched_tracks(restore_at) WHERE restored_at IS NULL;

    CREATE TABLE IF NOT EXISTS playback_log (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        track_uri    TEXT NOT NULL,
        track_name   TEXT,
        artist_name  TEXT,
        artist_id    TEXT,
        album_name   TEXT,
        context_uri  TEXT,
        started_at   INTEGER NOT NULL,
        ended_at     INTEGER,
        listened_ms  INTEGER NOT NULL DEFAULT 0,
        duration_ms  INTEGER,
        skipped      INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX IF NOT EXISTS idx_playback_track ON playback_log(track_uri);
    CREATE INDEX IF NOT EXISTS idx_playback_started ON playback_log(started_at);

    CREATE TABLE IF NOT EXISTS discovery_log (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        track_uri     TEXT NOT NULL UNIQUE,
        track_name    TEXT,
        artist_name   TEXT,
        source        TEXT,
        first_seen_at INTEGER NOT NULL,
        status        TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS randomizer_sessions (
        shadow_playlist_id TEXT PRIMARY KEY,
        source_playlist_id TEXT NOT NULL,
        source_name        TEXT NOT NULL,
        shadow_name        TEXT NOT NULL,
        track_count        INTEGER NOT NULL DEFAULT 0,
        last_track_uri     TEXT,
        last_shuffled_at   INTEGER NOT NULL,
        reshuffle_count    INTEGER NOT NULL DEFAULT 0,
        active             INTEGER NOT NULL DEFAULT 1
    );
    CREATE INDEX IF NOT EXISTS idx_sessions_source ON randomizer_sessions(source_playlist_id);
    "#,
    // v2: Randomizer keeps the shuffled order (JSON array of URIs) to locate
    // the playing track, plus the track pinned by the last re-shuffle so the
    // poller can verify Spotify followed the new order. Sessions created
    // before this have no order and are retired; re-randomizing recreates them.
    r#"
    ALTER TABLE randomizer_sessions ADD COLUMN track_order TEXT;
    ALTER TABLE randomizer_sessions ADD COLUMN pending_pin TEXT;
    UPDATE randomizer_sessions SET active = 0 WHERE track_order IS NULL;
    "#,
    // v3: tracks that could not be copied into the shadow playlist (JSON array
    // of {uri, name, artists, reason}), shown on the Randomizer page.
    r#"
    ALTER TABLE randomizer_sessions ADD COLUMN missing_tracks TEXT;
    "#,
    // v4: Phase 4. Playback log gains `updated_at` (last poll that touched
    // the row, used to close rows left open by a crash/quit). Library index
    // (every track in the user's playlists = "seen"), artist genre cache,
    // discovery sources + candidate pool.
    r#"
    ALTER TABLE playback_log ADD COLUMN updated_at INTEGER;

    CREATE TABLE IF NOT EXISTS library_index (
        playlist_id TEXT NOT NULL,
        track_uri   TEXT NOT NULL,
        PRIMARY KEY (playlist_id, track_uri)
    );
    CREATE INDEX IF NOT EXISTS idx_library_track ON library_index(track_uri);

    CREATE TABLE IF NOT EXISTS artist_genres (
        artist_id TEXT PRIMARY KEY,
        name      TEXT,
        genres    TEXT NOT NULL,
        cached_at INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS discovery_sources (
        key         TEXT PRIMARY KEY,
        kind        TEXT NOT NULL,
        label       TEXT NOT NULL,
        indexed_at  INTEGER NOT NULL,
        track_count INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS discovery_pool (
        track_uri  TEXT PRIMARY KEY,
        name       TEXT,
        artists    TEXT,
        artist_id  TEXT,
        album      TEXT,
        source_key TEXT NOT NULL,
        added_at   INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_pool_source ON discovery_pool(source_key);
    "#,
    // v5: Discovery playlists we created, with their track order, so the
    // poller can infer outcomes for tracks that played between two polls.
    r#"
    CREATE TABLE IF NOT EXISTS discovery_playlists (
        playlist_id TEXT PRIMARY KEY,
        name        TEXT NOT NULL,
        created_at  INTEGER NOT NULL,
        track_order TEXT NOT NULL
    );
    "#,
    // v6: Stats. Every track play from Spotify's "extended streaming history"
    // data export. The primary key makes re-importing the same export (or a
    // newer one that overlaps) a no-op, so imports are additive and safe to
    // repeat. `ip_addr` from the export is deliberately not stored.
    r#"
    CREATE TABLE IF NOT EXISTS stream_history (
        ts           INTEGER NOT NULL,
        track_uri    TEXT NOT NULL,
        ms_played    INTEGER NOT NULL,
        track_name   TEXT,
        artist_name  TEXT,
        album_name   TEXT,
        reason_start TEXT,
        reason_end   TEXT,
        shuffle      INTEGER,
        skipped      INTEGER,
        platform     TEXT,
        country      TEXT,
        offline      INTEGER,
        incognito    INTEGER,
        PRIMARY KEY (ts, track_uri, ms_played)
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS idx_stream_ts ON stream_history(ts);

    CREATE TABLE IF NOT EXISTS stream_imports (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        imported_at INTEGER NOT NULL,
        source      TEXT NOT NULL,
        files       INTEGER NOT NULL,
        rows_read   INTEGER NOT NULL,
        rows_added  INTEGER NOT NULL
    );
    "#,
    // v7: Discography listings kept on disk. `/artists/{id}/albums` and
    // `/albums/{id}/tracks` are capped at ten items a page in Development
    // Mode, so building one catalogue costs dozens of requests; caching them
    // means a rebuild costs none. An album's track list never changes, so it
    // has no expiry; an artist's release list does, so it carries a timestamp.
    r#"
    CREATE TABLE IF NOT EXISTS artist_albums_cache (
        artist_id TEXT NOT NULL,
        groups    TEXT NOT NULL,
        albums    TEXT NOT NULL,
        cached_at INTEGER NOT NULL,
        PRIMARY KEY (artist_id, groups)
    );

    CREATE TABLE IF NOT EXISTS album_tracks_cache (
        album_id  TEXT PRIMARY KEY,
        tracks    TEXT NOT NULL,
        cached_at INTEGER NOT NULL
    );
    "#,
];

pub fn run(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let version = idx as i64 + 1;
        if version <= current {
            continue;
        }
        log::info!("applying database migration v{version}");
        conn.execute_batch("BEGIN")?;
        if let Err(e) = conn.execute_batch(sql) {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
        conn.pragma_update(None, "user_version", version)?;
        conn.execute_batch("COMMIT")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::randomizer::{self, SessionRow};

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        conn
    }

    #[test]
    fn migrations_apply_to_latest_version() {
        let conn = fresh();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64);
        // Running again is a no-op.
        run(&conn).unwrap();
    }

    #[test]
    fn migrations_upgrade_from_v1() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("BEGIN").unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute_batch("COMMIT").unwrap();
        conn.execute(
            "INSERT INTO randomizer_sessions
             (shadow_playlist_id, source_playlist_id, source_name, shadow_name, track_count, last_shuffled_at)
             VALUES ('old', 'src', 'Old', 'Old-Utilify', 3, 0)",
            [],
        )
        .unwrap();

        run(&conn).unwrap();

        // Pre-v2 sessions carry no order and are retired.
        let active = randomizer::list_active(&conn).unwrap();
        assert!(active.is_empty());
        let old = randomizer::get_by_shadow(&conn, "old").unwrap().unwrap();
        assert!(!old.active);
        assert!(old.track_order.is_empty());
    }

    #[test]
    fn session_round_trips_order_and_pin() {
        let conn = fresh();
        let session = SessionRow {
            shadow_playlist_id: "shadow".into(),
            source_playlist_id: "src".into(),
            source_name: "Src".into(),
            shadow_name: "Src-Utilify".into(),
            track_count: 2,
            last_track_uri: Some("spotify:track:b".into()),
            last_shuffled_at: 1,
            reshuffle_count: 0,
            active: true,
            track_order: vec!["spotify:track:a".into(), "spotify:track:b".into()],
            pending_pin: Some("spotify:track:a".into()),
            missing_tracks: vec![crate::spotify::models::MissingTrack {
                uri: Some("spotify:track:c".into()),
                name: "Gone".into(),
                artists: "Nobody".into(),
                reason: "test".into(),
            }],
        };
        randomizer::upsert(&conn, &session).unwrap();
        let back = randomizer::get_by_shadow(&conn, "shadow").unwrap().unwrap();
        assert_eq!(back.track_order, session.track_order);
        assert_eq!(back.pending_pin.as_deref(), Some("spotify:track:a"));
        assert_eq!(back.missing_tracks, session.missing_tracks);

        randomizer::set_pending_pin(&conn, "shadow", None).unwrap();
        assert!(randomizer::get_by_shadow(&conn, "shadow").unwrap().unwrap().pending_pin.is_none());

        // A new shadow for the same source retires the old one.
        let newer = SessionRow {
            shadow_playlist_id: "shadow2".into(),
            ..session.clone()
        };
        randomizer::upsert(&conn, &newer).unwrap();
        randomizer::deactivate_others_for_source(&conn, "src", "shadow2").unwrap();
        let active = randomizer::list_active(&conn).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].shadow_playlist_id, "shadow2");

        // Missing from the library listing → retired.
        randomizer::deactivate_missing(&conn, &["something-else".to_string()]).unwrap();
        assert!(randomizer::list_active(&conn).unwrap().is_empty());
    }

    #[test]
    fn bench_lifecycle() {
        use crate::db::bench::{self, NewBench};
        let conn = fresh();
        let id = bench::insert(
            &conn,
            &NewBench {
                track_uri: "spotify:track:x",
                track_name: Some("X"),
                artist_name: Some("Y"),
                playlist_id: "pl",
                playlist_name: Some("PL"),
                original_position: Some(3),
                benched_at: 100,
                restore_at: 200,
            },
        )
        .unwrap();
        assert!(bench::is_active(&conn, "pl", "spotify:track:x").unwrap());
        assert!(!bench::is_active(&conn, "other", "spotify:track:x").unwrap());
        assert_eq!(bench::list_active(&conn).unwrap().len(), 1);
        assert!(bench::list_due(&conn, 150).unwrap().is_empty());
        assert_eq!(bench::list_due(&conn, 200).unwrap().len(), 1);
        assert_eq!(
            bench::active_benched_uris(&conn, &["pl", "shadow"]).unwrap(),
            vec!["spotify:track:x".to_string()]
        );
        assert!(bench::active_benched_uris(&conn, &["other"]).unwrap().is_empty());

        bench::mark_restored(&conn, id, 201).unwrap();
        assert!(bench::list_active(&conn).unwrap().is_empty());
        assert!(bench::list_due(&conn, 300).unwrap().is_empty());
        assert!(!bench::is_active(&conn, "pl", "spotify:track:x").unwrap());
        let row = bench::get(&conn, id).unwrap().unwrap();
        assert_eq!(row.restored_at, Some(201));
        assert_eq!(row.original_position, Some(3));
    }
}
