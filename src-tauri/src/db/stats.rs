//! Streaming-history storage and the aggregate queries behind the Stats page.
//!
//! Every row here comes from Spotify's "extended streaming history" data
//! export, never from the Web API, so nothing on the Stats page costs a
//! request or is affected by the quota cooldown.

use rusqlite::{params, Connection, Transaction};
use serde::Serialize;

/// One track play, as stored. Podcast episodes and audiobooks are dropped at
/// import; `ip_addr` from the export is deliberately never stored.
#[derive(Debug, Clone)]
pub struct PlayRow {
    pub ts: i64,
    pub track_uri: String,
    pub ms_played: i64,
    pub track_name: Option<String>,
    pub artist_name: Option<String>,
    pub album_name: Option<String>,
    pub reason_start: Option<String>,
    pub reason_end: Option<String>,
    pub shuffle: Option<bool>,
    pub skipped: Option<bool>,
    pub platform: Option<String>,
    pub country: Option<String>,
    pub offline: Option<bool>,
    pub incognito: Option<bool>,
}

/// Songs are grouped by name + artist, so a single, an album cut and a
/// remaster of the same song count as one song rather than three.
const SONG_KEY: &str = "lower(coalesce(track_name,'')) || char(1) || lower(coalesce(artist_name,''))";

/// A song needs this many skips before it can be called one you skip past.
const MIN_SKIPS: i64 = 3;

/// Insert a batch inside the caller's transaction, returning how many rows
/// were new. Exports overlap each other (and themselves, across the yearly
/// files), so duplicates are ignored rather than counted twice.
pub fn insert_plays(tx: &Transaction, rows: &[PlayRow]) -> rusqlite::Result<usize> {
    let mut stmt = tx.prepare_cached(
        "INSERT OR IGNORE INTO stream_history
           (ts, track_uri, ms_played, track_name, artist_name, album_name,
            reason_start, reason_end, shuffle, skipped, platform, country, offline, incognito)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    let mut added = 0;
    for r in rows {
        added += stmt.execute(params![
            r.ts,
            r.track_uri,
            r.ms_played,
            r.track_name,
            r.artist_name,
            r.album_name,
            r.reason_start,
            r.reason_end,
            r.shuffle,
            r.skipped,
            r.platform,
            r.country,
            r.offline,
            r.incognito,
        ])?;
    }
    Ok(added)
}

pub fn record_import(
    conn: &Connection,
    imported_at: i64,
    source: &str,
    files: usize,
    rows_read: usize,
    rows_added: usize,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO stream_imports (imported_at, source, files, rows_read, rows_added)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![imported_at, source, files as i64, rows_read as i64, rows_added as i64],
    )?;
    Ok(())
}

pub fn clear(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("DELETE FROM stream_history; DELETE FROM stream_imports;")?;
    Ok(())
}

// ---- status ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastImport {
    pub imported_at: i64,
    pub source: String,
    pub rows_added: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsStatus {
    pub plays: i64,
    pub first_ts: Option<i64>,
    pub last_ts: Option<i64>,
    /// Years present in the history, newest first, for the range presets.
    pub years: Vec<i32>,
    pub last_import: Option<LastImport>,
}

pub fn status(conn: &Connection) -> rusqlite::Result<StatsStatus> {
    let (plays, first_ts, last_ts) = conn.query_row(
        "SELECT COUNT(*), MIN(ts), MAX(ts) FROM stream_history",
        [],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, Option<i64>>(2)?)),
    )?;

    let mut years = Vec::new();
    if plays > 0 {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT CAST(strftime('%Y', ts, 'unixepoch', 'localtime') AS INTEGER) AS y
             FROM stream_history ORDER BY y DESC",
        )?;
        for row in stmt.query_map([], |r| r.get::<_, i32>(0))? {
            years.push(row?);
        }
    }

    let last_import = conn
        .query_row(
            "SELECT imported_at, source, rows_added FROM stream_imports ORDER BY id DESC LIMIT 1",
            [],
            |r| {
                Ok(LastImport {
                    imported_at: r.get(0)?,
                    source: r.get(1)?,
                    rows_added: r.get(2)?,
                })
            },
        )
        .ok();

    Ok(StatsStatus {
        plays,
        first_ts,
        last_ts,
        years,
        last_import,
    })
}

// ---- summary ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    /// Every row in range, including the ones too short to count as a play.
    pub streams: i64,
    /// Rows that reached the play threshold.
    pub plays: i64,
    /// Rows that did not.
    pub skips: i64,
    /// Milliseconds played, counting partial plays.
    pub ms: i64,
    pub shuffle_streams: i64,
    pub songs: i64,
    pub artists: i64,
    pub albums: i64,
    /// Calendar days (local time) with at least one play.
    pub days: i64,
    pub first_ts: Option<i64>,
    pub last_ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongStat {
    pub name: String,
    pub artist: String,
    pub uri: Option<String>,
    pub plays: i64,
    pub skips: i64,
    pub ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedStat {
    pub name: String,
    /// Artist for an album row; empty for artist rows.
    pub secondary: String,
    pub plays: i64,
    pub ms: i64,
    pub songs: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub label: String,
    pub plays: i64,
    pub ms: i64,
}

pub fn totals(conn: &Connection, from: i64, to: i64, threshold_ms: i64) -> rusqlite::Result<Totals> {
    // One scan: the distinct counts describe plays only, so they filter with a
    // CASE rather than a second pass over the same rows.
    let sql = format!(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN ms_played >= ?3 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(ms_played), 0),
                COALESCE(SUM(CASE WHEN shuffle = 1 THEN 1 ELSE 0 END), 0),
                MIN(ts), MAX(ts),
                COUNT(DISTINCT CASE WHEN ms_played >= ?3 THEN {SONG_KEY} END),
                COUNT(DISTINCT CASE WHEN ms_played >= ?3 THEN lower(artist_name) END),
                COUNT(DISTINCT CASE WHEN ms_played >= ?3
                      THEN lower(coalesce(album_name,'')) || char(1) || lower(coalesce(artist_name,'')) END),
                COUNT(DISTINCT CASE WHEN ms_played >= ?3
                      THEN date(ts, 'unixepoch', 'localtime') END)
         FROM stream_history WHERE ts >= ?1 AND ts <= ?2"
    );
    conn.query_row(&sql, params![from, to, threshold_ms], |r| {
        let streams: i64 = r.get(0)?;
        let plays: i64 = r.get(1)?;
        Ok(Totals {
            streams,
            plays,
            skips: streams - plays,
            ms: r.get(2)?,
            shuffle_streams: r.get(3)?,
            first_ts: r.get(4)?,
            last_ts: r.get(5)?,
            songs: r.get(6)?,
            artists: r.get(7)?,
            albums: r.get(8)?,
            days: r.get(9)?,
        })
    })
}

/// Both song rankings in one pass: "most played" and "most skipped past" are
/// the same grouping with different orders, and each scan over the history
/// costs about as much as all the sorting put together.
pub fn song_rankings(
    conn: &Connection,
    from: i64,
    to: i64,
    threshold_ms: i64,
    limit: usize,
) -> rusqlite::Result<(Vec<SongStat>, Vec<SongStat>)> {
    let sql = format!(
        "SELECT track_name, artist_name, track_uri,
                COALESCE(SUM(CASE WHEN ms_played >= ?3 THEN 1 ELSE 0 END), 0) AS plays,
                COALESCE(SUM(CASE WHEN ms_played <  ?3 THEN 1 ELSE 0 END), 0) AS skips,
                COALESCE(SUM(ms_played), 0) AS ms
         FROM stream_history
         WHERE ts >= ?1 AND ts <= ?2
         GROUP BY {SONG_KEY}
         HAVING plays > 0 OR skips >= {MIN_SKIPS}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![from, to, threshold_ms], |r| {
        Ok(SongStat {
            name: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            artist: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            uri: r.get(2)?,
            plays: r.get(3)?,
            skips: r.get(4)?,
            ms: r.get(5)?,
        })
    })?;
    let all: Vec<SongStat> = rows.collect::<rusqlite::Result<_>>()?;

    let mut played: Vec<SongStat> = all.iter().filter(|s| s.plays > 0).cloned().collect();
    played.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.ms.cmp(&a.ms)));
    played.truncate(limit);

    let mut skipped: Vec<SongStat> = all.into_iter().filter(|s| s.skips >= MIN_SKIPS).collect();
    skipped.sort_by(|a, b| {
        b.skips
            .cmp(&a.skips)
            .then((b.skips + b.plays).cmp(&(a.skips + a.plays)))
    });
    skipped.truncate(limit);

    Ok((played, skipped))
}

pub fn top_artists(conn: &Connection, from: i64, to: i64, threshold_ms: i64, limit: i64) -> rusqlite::Result<Vec<NamedStat>> {
    let sql = "SELECT artist_name,
                      COALESCE(SUM(CASE WHEN ms_played >= ?3 THEN 1 ELSE 0 END), 0) AS plays,
                      COALESCE(SUM(ms_played), 0) AS ms,
                      COUNT(DISTINCT lower(coalesce(track_name,''))) AS songs
               FROM stream_history
               WHERE ts >= ?1 AND ts <= ?2 AND artist_name IS NOT NULL AND artist_name <> ''
               GROUP BY lower(artist_name)
               HAVING plays > 0
               ORDER BY ms DESC
               LIMIT ?4";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![from, to, threshold_ms, limit], |r| {
        Ok(NamedStat {
            name: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            secondary: String::new(),
            plays: r.get(1)?,
            ms: r.get(2)?,
            songs: r.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn top_albums(conn: &Connection, from: i64, to: i64, threshold_ms: i64, limit: i64) -> rusqlite::Result<Vec<NamedStat>> {
    let sql = "SELECT album_name, artist_name,
                      COALESCE(SUM(CASE WHEN ms_played >= ?3 THEN 1 ELSE 0 END), 0) AS plays,
                      COALESCE(SUM(ms_played), 0) AS ms,
                      COUNT(DISTINCT lower(coalesce(track_name,''))) AS songs
               FROM stream_history
               WHERE ts >= ?1 AND ts <= ?2 AND album_name IS NOT NULL AND album_name <> ''
               GROUP BY lower(album_name) || char(1) || lower(coalesce(artist_name,''))
               HAVING plays > 0
               ORDER BY ms DESC
               LIMIT ?4";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![from, to, threshold_ms, limit], |r| {
        Ok(NamedStat {
            name: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            secondary: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            plays: r.get(2)?,
            ms: r.get(3)?,
            songs: r.get(4)?,
        })
    })?;
    rows.collect()
}

/// Bucket by a `strftime` pattern evaluated in the user's local timezone.
fn by_format(
    conn: &Connection,
    from: i64,
    to: i64,
    threshold_ms: i64,
    format: &str,
) -> rusqlite::Result<Vec<Bucket>> {
    let sql = format!(
        "SELECT strftime('{format}', ts, 'unixepoch', 'localtime') AS bucket,
                COALESCE(SUM(CASE WHEN ms_played >= ?3 THEN 1 ELSE 0 END), 0) AS plays,
                COALESCE(SUM(ms_played), 0) AS ms
         FROM stream_history
         WHERE ts >= ?1 AND ts <= ?2
         GROUP BY bucket
         ORDER BY bucket"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![from, to, threshold_ms], |r| {
        Ok(Bucket {
            label: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            plays: r.get(1)?,
            ms: r.get(2)?,
        })
    })?;
    rows.collect()
}

/// 24 buckets, "00".."23", always all present so the chart keeps its shape.
pub fn by_hour(conn: &Connection, from: i64, to: i64, threshold_ms: i64) -> rusqlite::Result<Vec<Bucket>> {
    let found = by_format(conn, from, to, threshold_ms, "%H")?;
    Ok(fill(24, |i| format!("{i:02}"), &found))
}

/// 7 buckets, "0" (Sunday) .. "6".
pub fn by_weekday(conn: &Connection, from: i64, to: i64, threshold_ms: i64) -> rusqlite::Result<Vec<Bucket>> {
    let found = by_format(conn, from, to, threshold_ms, "%w")?;
    Ok(fill(7, |i| i.to_string(), &found))
}

/// One bucket per calendar month between the first and last month in range,
/// including months with no listening (a gap is part of the story).
pub fn by_month(conn: &Connection, from: i64, to: i64, threshold_ms: i64) -> rusqlite::Result<Vec<Bucket>> {
    let found = by_format(conn, from, to, threshold_ms, "%Y-%m")?;
    let (Some(first), Some(last)) = (found.first(), found.last()) else {
        return Ok(found);
    };
    let Some((first, last)) = parse_month(&first.label).zip(parse_month(&last.label)) else {
        return Ok(found);
    };
    let mut months = Vec::new();
    let (mut y, mut m) = first;
    while (y, m) <= last {
        months.push(format!("{y:04}-{m:02}"));
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }
    Ok(months
        .into_iter()
        .map(|label| match found.iter().find(|b| b.label == label) {
            Some(b) => b.clone(),
            None => Bucket {
                label,
                plays: 0,
                ms: 0,
            },
        })
        .collect())
}

fn parse_month(label: &str) -> Option<(i32, u32)> {
    let (y, m) = label.split_once('-')?;
    Some((y.parse().ok()?, m.parse().ok()?))
}

fn fill(n: usize, label: impl Fn(usize) -> String, found: &[Bucket]) -> Vec<Bucket> {
    (0..n)
        .map(|i| {
            let label = label(i);
            match found.iter().find(|b| b.label == label) {
                Some(b) => b.clone(),
                None => Bucket {
                    label,
                    plays: 0,
                    ms: 0,
                },
            }
        })
        .collect()
}

/// Raw `platform` strings with their totals; the caller folds them into
/// families ("iOS 14.8.1 (iPhone11,8)" and "ios" are one device).
pub fn by_platform(conn: &Connection, from: i64, to: i64, threshold_ms: i64) -> rusqlite::Result<Vec<Bucket>> {
    let sql = "SELECT COALESCE(platform, ''),
                      COALESCE(SUM(CASE WHEN ms_played >= ?3 THEN 1 ELSE 0 END), 0),
                      COALESCE(SUM(ms_played), 0)
               FROM stream_history
               WHERE ts >= ?1 AND ts <= ?2
               GROUP BY platform";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![from, to, threshold_ms], |r| {
        Ok(Bucket {
            label: r.get(0)?,
            plays: r.get(1)?,
            ms: r.get(2)?,
        })
    })?;
    rows.collect()
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

    fn play(ts: i64, name: &str, artist: &str, ms: i64) -> PlayRow {
        PlayRow {
            ts,
            track_uri: format!("spotify:track:{name}"),
            ms_played: ms,
            track_name: Some(name.into()),
            artist_name: Some(artist.into()),
            album_name: Some(format!("{name} EP")),
            reason_start: Some("playbtn".into()),
            reason_end: Some("trackdone".into()),
            shuffle: Some(true),
            skipped: Some(false),
            platform: Some("ios".into()),
            country: Some("US".into()),
            offline: Some(false),
            incognito: Some(false),
        }
    }

    fn insert(conn: &mut Connection, rows: &[PlayRow]) -> usize {
        let tx = conn.transaction().unwrap();
        let n = insert_plays(&tx, rows).unwrap();
        tx.commit().unwrap();
        n
    }

    #[test]
    fn reimporting_the_same_rows_adds_nothing() {
        let mut conn = fresh();
        let rows = vec![play(1_700_000_000, "A", "X", 200_000), play(1_700_000_300, "B", "Y", 1_000)];
        assert_eq!(insert(&mut conn, &rows), 2);
        assert_eq!(insert(&mut conn, &rows), 0);
        assert_eq!(status(&conn).unwrap().plays, 2);
    }

    #[test]
    fn totals_split_plays_from_skips() {
        let mut conn = fresh();
        insert(
            &mut conn,
            &[
                play(1_700_000_000, "A", "X", 200_000),
                play(1_700_000_300, "A", "X", 1_000),
                play(1_700_000_600, "B", "Y", 60_000),
            ],
        );
        let t = totals(&conn, 0, i64::MAX, 30_000).unwrap();
        assert_eq!(t.streams, 3);
        assert_eq!(t.plays, 2);
        assert_eq!(t.skips, 1);
        assert_eq!(t.ms, 261_000);
        assert_eq!(t.songs, 2);
        assert_eq!(t.artists, 2);
    }

    #[test]
    fn editions_of_one_song_count_together() {
        let mut conn = fresh();
        let mut remaster = play(1_700_000_900, "A", "X", 190_000);
        remaster.track_uri = "spotify:track:A-remaster".into();
        remaster.track_name = Some("a".into()); // different casing, same song
        insert(&mut conn, &[play(1_700_000_000, "A", "X", 200_000), remaster]);

        let (top, _) = song_rankings(&conn, 0, i64::MAX, 30_000, 10).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].plays, 2);
        assert_eq!(top[0].ms, 390_000);
    }

    #[test]
    fn range_filter_excludes_outside_rows() {
        let mut conn = fresh();
        insert(
            &mut conn,
            &[play(1_000, "A", "X", 200_000), play(9_000, "B", "Y", 200_000)],
        );
        let t = totals(&conn, 5_000, 10_000, 30_000).unwrap();
        assert_eq!(t.plays, 1);
        assert_eq!(song_rankings(&conn, 5_000, 10_000, 30_000, 10).unwrap().0[0].name, "B");
    }

    #[test]
    fn skipped_ranking_needs_repeat_skips() {
        let mut conn = fresh();
        let mut rows = vec![play(1_700_000_000, "Keeper", "X", 200_000)];
        for i in 0..4 {
            rows.push(play(1_700_001_000 + i, "Skipper", "X", 900));
        }
        insert(&mut conn, &rows);
        let (_, skipped) = song_rankings(&conn, 0, i64::MAX, 30_000, 10).unwrap();
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "Skipper");
        assert_eq!(skipped[0].skips, 4);
        assert_eq!(skipped[0].plays, 0);
    }

    #[test]
    fn hour_and_weekday_buckets_are_complete() {
        let mut conn = fresh();
        insert(&mut conn, &[play(1_700_000_000, "A", "X", 200_000)]);
        assert_eq!(by_hour(&conn, 0, i64::MAX, 30_000).unwrap().len(), 24);
        assert_eq!(by_weekday(&conn, 0, i64::MAX, 30_000).unwrap().len(), 7);
    }

    #[test]
    fn month_buckets_include_silent_months() {
        let mut conn = fresh();
        // 2023-01-15 and 2023-04-15 UTC, three months apart.
        insert(
            &mut conn,
            &[
                play(1_673_784_000, "A", "X", 200_000),
                play(1_681_560_000, "B", "Y", 200_000),
            ],
        );
        let months = by_month(&conn, 0, i64::MAX, 30_000).unwrap();
        assert_eq!(months.len(), 4, "{months:?}");
        assert!(months[1].ms == 0 && months[2].ms == 0);
    }

    #[test]
    fn clearing_removes_everything() {
        let mut conn = fresh();
        insert(&mut conn, &[play(1_700_000_000, "A", "X", 200_000)]);
        record_import(&conn, 1, "export.zip", 1, 1, 1).unwrap();
        assert!(status(&conn).unwrap().last_import.is_some());
        clear(&conn).unwrap();
        let s = status(&conn).unwrap();
        assert_eq!(s.plays, 0);
        assert!(s.last_import.is_none());
        assert!(s.years.is_empty());
    }
}
