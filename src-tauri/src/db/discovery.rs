//! Discovery: library index ("in a playlist = seen"), candidate pool,
//! sources, and the discovery log.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

// ---- library index ---------------------------------------------------------

pub fn replace_library_playlist(conn: &mut Connection, playlist_id: &str, uris: &[String]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM library_index WHERE playlist_id = ?1", [playlist_id])?;
    {
        let mut stmt = tx.prepare("INSERT OR IGNORE INTO library_index (playlist_id, track_uri) VALUES (?1, ?2)")?;
        for u in uris {
            stmt.execute(params![playlist_id, u])?;
        }
    }
    tx.commit()
}

pub fn library_index_size(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    conn.query_row(
        "SELECT COUNT(DISTINCT playlist_id), COUNT(DISTINCT track_uri) FROM library_index",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}

// ---- sources & pool --------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRow {
    pub key: String,
    /// `followed_artist`, `seed_artist`, `seed_playlist`
    pub kind: String,
    pub label: String,
    pub indexed_at: i64,
    pub track_count: i64,
}

fn source_from_row(r: &Row<'_>) -> rusqlite::Result<SourceRow> {
    Ok(SourceRow {
        key: r.get("key")?,
        kind: r.get("kind")?,
        label: r.get("label")?,
        indexed_at: r.get("indexed_at")?,
        track_count: r.get("track_count")?,
    })
}

pub fn list_sources(conn: &Connection) -> rusqlite::Result<Vec<SourceRow>> {
    let mut stmt = conn.prepare("SELECT * FROM discovery_sources ORDER BY indexed_at DESC")?;
    let rows = stmt.query_map([], source_from_row)?;
    rows.collect()
}

pub fn get_source(conn: &Connection, key: &str) -> rusqlite::Result<Option<SourceRow>> {
    conn.query_row("SELECT * FROM discovery_sources WHERE key = ?1", [key], source_from_row)
        .optional()
}

pub struct PoolTrack<'a> {
    pub track_uri: &'a str,
    pub name: Option<&'a str>,
    pub artists: Option<&'a str>,
    pub artist_id: Option<&'a str>,
    pub album: Option<&'a str>,
}

/// Replace a source's candidates and record it.
pub fn replace_source(
    conn: &mut Connection,
    source: &SourceRow,
    tracks: &[PoolTrack<'_>],
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM discovery_pool WHERE source_key = ?1", [&source.key])?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO discovery_pool (track_uri, name, artists, artist_id, album, source_key, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for t in tracks {
            stmt.execute(params![
                t.track_uri,
                t.name,
                t.artists,
                t.artist_id,
                t.album,
                source.key,
                source.indexed_at
            ])?;
        }
    }
    tx.execute(
        "INSERT INTO discovery_sources (key, kind, label, indexed_at, track_count) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(key) DO UPDATE SET kind = excluded.kind, label = excluded.label,
             indexed_at = excluded.indexed_at, track_count = excluded.track_count",
        params![source.key, source.kind, source.label, source.indexed_at, tracks.len() as i64],
    )?;
    tx.commit()
}

pub fn remove_source(conn: &mut Connection, key: &str) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM discovery_pool WHERE source_key = ?1", [key])?;
    tx.execute("DELETE FROM discovery_sources WHERE key = ?1", [key])?;
    tx.commit()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub track_uri: String,
    pub name: Option<String>,
    pub artists: Option<String>,
    pub album: Option<String>,
    pub source_key: String,
}

/// Unseen = never in the playback log, never offered by Discovery, and not in
/// any playlist of the user's library.
const UNSEEN_FILTER: &str = "NOT EXISTS (SELECT 1 FROM playback_log l WHERE l.track_uri = p.track_uri)
     AND NOT EXISTS (SELECT 1 FROM discovery_log d WHERE d.track_uri = p.track_uri)
     AND NOT EXISTS (SELECT 1 FROM library_index i WHERE i.track_uri = p.track_uri)";

pub fn pool_counts(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM discovery_pool", [], |r| r.get(0))?;
    let unseen: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM discovery_pool p WHERE {UNSEEN_FILTER}"),
        [],
        |r| r.get(0),
    )?;
    Ok((total, unseen))
}

/// Random unseen candidates, at most one per artist so a batch is varied.
pub fn sample_unseen(conn: &Connection, n: usize) -> rusqlite::Result<Vec<Candidate>> {
    let sql = format!(
        "SELECT track_uri, name, artists, album, source_key FROM discovery_pool p
         WHERE {UNSEEN_FILTER} ORDER BY RANDOM() LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    // Over-sample, then thin to one per artist.
    let rows = stmt.query_map([(n * 4).max(20) as i64], |r| {
        Ok((
            Candidate {
                track_uri: r.get(0)?,
                name: r.get(1)?,
                artists: r.get(2)?,
                album: r.get(3)?,
                source_key: r.get(4)?,
            },
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut seen_artists = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut spare = Vec::new();
    for row in rows {
        let (c, artists) = row?;
        let key = artists.unwrap_or_default().to_lowercase();
        if seen_artists.insert(key) {
            out.push(c);
        } else {
            spare.push(c);
        }
        if out.len() >= n {
            break;
        }
    }
    for c in spare {
        if out.len() >= n {
            break;
        }
        out.push(c);
    }
    Ok(out)
}

// ---- discovery log ---------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryLogRow {
    pub id: i64,
    pub track_uri: String,
    pub track_name: Option<String>,
    pub artist_name: Option<String>,
    pub source: Option<String>,
    pub first_seen_at: i64,
    /// `queued`, `listened`, `skipped`
    pub status: String,
}

pub fn log_offered(conn: &Connection, c: &Candidate, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO discovery_log (track_uri, track_name, artist_name, source, first_seen_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, 'queued')",
        params![c.track_uri, c.name, c.artists, c.source_key, now],
    )?;
    Ok(())
}

/// Called when a play finishes: upgrade a queued entry to its outcome.
pub fn mark_outcome(conn: &Connection, track_uri: &str, skipped: bool) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "UPDATE discovery_log SET status = ?2 WHERE track_uri = ?1 AND status = 'queued'",
        params![track_uri, if skipped { "skipped" } else { "listened" }],
    )?;
    Ok(n > 0)
}

pub fn recent_log(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<DiscoveryLogRow>> {
    let mut stmt = conn.prepare("SELECT * FROM discovery_log ORDER BY first_seen_at DESC, id DESC LIMIT ?1")?;
    let rows = stmt.query_map([limit], |r| {
        Ok(DiscoveryLogRow {
            id: r.get("id")?,
            track_uri: r.get("track_uri")?,
            track_name: r.get("track_name")?,
            artist_name: r.get("artist_name")?,
            source: r.get("source")?,
            first_seen_at: r.get("first_seen_at")?,
            status: r.get("status")?,
        })
    })?;
    rows.collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryTotals {
    pub offered: i64,
    pub listened: i64,
    pub skipped: i64,
    pub pending: i64,
}

pub fn totals(conn: &Connection) -> rusqlite::Result<DiscoveryTotals> {
    conn.query_row(
        "SELECT COUNT(*),
                SUM(CASE WHEN status = 'listened' THEN 1 ELSE 0 END),
                SUM(CASE WHEN status = 'skipped' THEN 1 ELSE 0 END),
                SUM(CASE WHEN status = 'queued' THEN 1 ELSE 0 END)
         FROM discovery_log",
        [],
        |r| {
            Ok(DiscoveryTotals {
                offered: r.get(0)?,
                listened: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                skipped: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                pending: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            })
        },
    )
}
