//! Aggregations over `playback_log` for the Stats dashboard.

use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackStat {
    pub uri: String,
    pub name: Option<String>,
    pub artist: Option<String>,
    pub plays: i64,
    pub listened_ms: i64,
    pub skips: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistStat {
    pub artist: String,
    pub artist_id: Option<String>,
    pub plays: i64,
    pub listened_ms: i64,
    pub skips: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextStat {
    pub context_uri: String,
    pub plays: i64,
    pub listened_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    pub date: String,
    pub plays: i64,
    pub listened_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsSummary {
    pub since: Option<i64>,
    pub plays: i64,
    pub listened_ms: i64,
    pub unique_tracks: i64,
    pub unique_artists: i64,
    pub skips: i64,
    pub first_logged_at: Option<i64>,
    pub top_tracks: Vec<TrackStat>,
    pub top_artists: Vec<ArtistStat>,
    pub contexts: Vec<ContextStat>,
    /// Listened ms per local hour of day, index 0..24.
    pub hours: Vec<i64>,
    pub days: Vec<DayStat>,
    pub most_skipped: Vec<TrackStat>,
}

const TOP_N: i64 = 25;

fn track_stats(conn: &Connection, since: i64, order: &str, min_plays: i64) -> rusqlite::Result<Vec<TrackStat>> {
    let sql = format!(
        "SELECT track_uri, MAX(track_name), MAX(artist_name), COUNT(*), SUM(listened_ms), SUM(skipped)
         FROM playback_log WHERE started_at >= ?1
         GROUP BY track_uri HAVING COUNT(*) >= ?2
         ORDER BY {order} LIMIT ?3"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![since, min_plays, TOP_N], |r| {
        Ok(TrackStat {
            uri: r.get(0)?,
            name: r.get(1)?,
            artist: r.get(2)?,
            plays: r.get(3)?,
            listened_ms: r.get(4)?,
            skips: r.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn summary(conn: &Connection, since: Option<i64>) -> rusqlite::Result<StatsSummary> {
    let s = since.unwrap_or(0);

    let (plays, listened_ms, unique_tracks, unique_artists, skips): (i64, i64, i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(listened_ms), 0), COUNT(DISTINCT track_uri),
                COUNT(DISTINCT COALESCE(artist_id, artist_name)), COALESCE(SUM(skipped), 0)
         FROM playback_log WHERE started_at >= ?1",
        [s],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let first_logged_at: Option<i64> = conn.query_row("SELECT MIN(started_at) FROM playback_log", [], |r| r.get(0))?;

    let top_tracks = track_stats(conn, s, "SUM(listened_ms) DESC, COUNT(*) DESC", 1)?;
    let most_skipped = track_stats(conn, s, "SUM(skipped) DESC, COUNT(*) DESC", 2)?
        .into_iter()
        .filter(|t| t.skips > 0)
        .collect();

    let top_artists = {
        let mut stmt = conn.prepare(
            "SELECT COALESCE(artist_name, '?'), MAX(artist_id), COUNT(*), SUM(listened_ms), SUM(skipped)
             FROM playback_log WHERE started_at >= ?1
             GROUP BY COALESCE(artist_id, artist_name)
             ORDER BY SUM(listened_ms) DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![s, TOP_N], |r| {
            Ok(ArtistStat {
                artist: r.get(0)?,
                artist_id: r.get(1)?,
                plays: r.get(2)?,
                listened_ms: r.get(3)?,
                skips: r.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let contexts = {
        let mut stmt = conn.prepare(
            "SELECT context_uri, COUNT(*), SUM(listened_ms)
             FROM playback_log WHERE started_at >= ?1 AND context_uri IS NOT NULL
             GROUP BY context_uri ORDER BY SUM(listened_ms) DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![s, TOP_N], |r| {
            Ok(ContextStat {
                context_uri: r.get(0)?,
                plays: r.get(1)?,
                listened_ms: r.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let mut hours = vec![0i64; 24];
    {
        let mut stmt = conn.prepare(
            "SELECT CAST(strftime('%H', started_at, 'unixepoch', 'localtime') AS INTEGER), SUM(listened_ms)
             FROM playback_log WHERE started_at >= ?1 GROUP BY 1",
        )?;
        let rows = stmt.query_map([s], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (h, ms) = row?;
            if (0..24).contains(&h) {
                hours[h as usize] = ms;
            }
        }
    }

    let days = {
        let mut stmt = conn.prepare(
            "SELECT date(started_at, 'unixepoch', 'localtime'), COUNT(*), SUM(listened_ms)
             FROM playback_log WHERE started_at >= ?1 GROUP BY 1 ORDER BY 1 DESC LIMIT 90",
        )?;
        let rows = stmt.query_map([s], |r| {
            Ok(DayStat {
                date: r.get(0)?,
                plays: r.get(1)?,
                listened_ms: r.get(2)?,
            })
        })?;
        let mut v = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        v.reverse();
        v
    };

    Ok(StatsSummary {
        since,
        plays,
        listened_ms,
        unique_tracks,
        unique_artists,
        skips,
        first_logged_at,
        top_tracks,
        top_artists,
        contexts,
        hours,
        days,
        most_skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::playback_log::{self, NewPlay};
    use crate::db::{discovery, migrations};

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn play(conn: &Connection, uri: &str, artist: &str, started: i64, listened: i64, finish: bool) -> i64 {
        let id = playback_log::start(
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
        if finish {
            playback_log::finish(conn, id, listened, started + listened / 1000).unwrap();
        } else {
            playback_log::update(conn, id, listened, started + 30).unwrap();
        }
        id
    }

    #[test]
    fn log_lifecycle_and_summary() {
        let conn = fresh();
        let t0 = 1_800_000_000;
        play(&conn, "spotify:track:a", "A", t0, 120_000, true);
        play(&conn, "spotify:track:a", "A", t0 + 400, 5_000, true); // skip
        play(&conn, "spotify:track:b", "B", t0 + 800, 60_000, true);
        play(&conn, "spotify:track:c", "C", t0 + 1200, 40_000, false); // left open


        // Open row gets closed at "launch" and classified.
        assert_eq!(playback_log::close_stale(&conn).unwrap(), 1);
        assert_eq!(playback_log::close_stale(&conn).unwrap(), 0);

        let s = summary(&conn, None).unwrap();
        assert_eq!(s.plays, 4);
        assert_eq!(s.skips, 1);
        assert_eq!(s.unique_tracks, 3);
        assert_eq!(s.unique_artists, 3);
        assert_eq!(s.listened_ms, 225_000);
        assert_eq!(s.top_tracks[0].uri, "spotify:track:a");
        assert_eq!(s.top_tracks[0].plays, 2);
        assert_eq!(s.contexts.len(), 1);
        assert_eq!(s.hours.iter().sum::<i64>(), 225_000);
        assert_eq!(s.most_skipped.len(), 1);

        // Range filter excludes older plays.
        let recent = summary(&conn, Some(t0 + 1000)).unwrap();
        assert_eq!(recent.plays, 1);
    }

    #[test]
    fn discovery_seen_rules() {
        let mut conn = fresh();
        let src = discovery::SourceRow {
            key: "seed_artist:x".into(),
            kind: "seed_artist".into(),
            label: "X".into(),
            indexed_at: 1,
            track_count: 0,
        };
        let pool = ["spotify:track:p1", "spotify:track:p2", "spotify:track:p3", "spotify:track:p4"];
        let rows: Vec<discovery::PoolTrack<'_>> = pool
            .iter()
            .enumerate()
            .map(|(i, u)| discovery::PoolTrack {
                track_uri: u,
                name: Some("n"),
                artists: Some(if i < 2 { "Same Artist" } else { "Other" }),
                artist_id: None,
                album: None,
            })
            .collect();
        discovery::replace_source(&mut conn, &src, &rows).unwrap();
        assert_eq!(discovery::pool_counts(&conn).unwrap(), (4, 4));

        // In a library playlist → seen.
        discovery::replace_library_playlist(&mut conn, "pl", &["spotify:track:p1".to_string()]).unwrap();
        // Skipped once → seen.
        play(&conn, "spotify:track:p2", "Same Artist", 10, 3_000, true);
        assert_eq!(discovery::pool_counts(&conn).unwrap(), (4, 2));

        let sample = discovery::sample_unseen(&conn, 10).unwrap();
        let uris: Vec<&str> = sample.iter().map(|c| c.track_uri.as_str()).collect();
        assert_eq!(sample.len(), 2);
        assert!(uris.contains(&"spotify:track:p3") && uris.contains(&"spotify:track:p4"));

        // Offering logs it as queued and it is no longer unseen; outcome updates the row.
        discovery::log_offered(&conn, &sample[0], 20).unwrap();
        assert_eq!(discovery::pool_counts(&conn).unwrap().1, 1);
        assert!(discovery::mark_outcome(&conn, &sample[0].track_uri, false).unwrap());
        assert!(!discovery::mark_outcome(&conn, &sample[0].track_uri, false).unwrap());
        let totals = discovery::totals(&conn).unwrap();
        assert_eq!((totals.offered, totals.listened, totals.skipped, totals.pending), (1, 1, 0, 0));

        discovery::remove_source(&mut conn, &src.key).unwrap();
        assert_eq!(discovery::pool_counts(&conn).unwrap(), (0, 0));
    }
}
