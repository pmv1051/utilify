//! Personal stats, built from Spotify's "extended streaming history" data
//! export (Account → Privacy settings → Download your data).
//!
//! Nothing here calls the Web API: the export is the only source that knows
//! what was played before Utilify existed, and the API has no history
//! endpoint at all. Every command in this module is local, so the Stats page
//! keeps working while the API quota is exhausted.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;

use crate::db::stats::{self, Bucket, NamedStat, PlayRow, SongStat, StatsStatus, Totals};
use crate::db::{now, Db};
use crate::error::{AppError, Result};

/// Rows returned per ranked table. The UI shows the first few and lets the
/// reader scroll the rest; one query is cheaper than pagination round-trips.
const TOP_LIMIT: usize = 100;

// ---- the export's record shape ---------------------------------------------

/// One row of `Streaming_History_Audio_*.json`. Unknown fields are ignored so
/// a future export revision still imports; `ip_addr` is one we never read.
#[derive(Debug, Deserialize)]
struct RawPlay {
    ts: String,
    #[serde(default)]
    ms_played: Option<i64>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    conn_country: Option<String>,
    #[serde(default)]
    master_metadata_track_name: Option<String>,
    #[serde(default)]
    master_metadata_album_artist_name: Option<String>,
    #[serde(default)]
    master_metadata_album_album_name: Option<String>,
    #[serde(default)]
    spotify_track_uri: Option<String>,
    #[serde(default)]
    reason_start: Option<String>,
    #[serde(default)]
    reason_end: Option<String>,
    #[serde(default)]
    shuffle: Option<bool>,
    #[serde(default)]
    skipped: Option<bool>,
    #[serde(default)]
    offline: Option<bool>,
    #[serde(default)]
    incognito_mode: Option<bool>,
}

/// The *other* export: "account data" ships `StreamingHistory_music_0.json`,
/// a year of plays with no URIs, skips or shuffle flags. Recognising it lets
/// us say so instead of reporting an empty import.
#[derive(Debug, Deserialize)]
struct BasicPlay {
    #[serde(rename = "endTime")]
    _end_time: String,
    #[serde(rename = "trackName")]
    _track_name: Option<String>,
}

fn parse_ts(raw: &str) -> Option<i64> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(dt.timestamp());
    }
    // Some older files use "2021-01-31 15:17" in UTC.
    chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M")
        .ok()
        .map(|dt| dt.and_utc().timestamp())
}

impl RawPlay {
    fn into_row(self) -> Option<PlayRow> {
        let track_uri = self.spotify_track_uri?;
        let ts = parse_ts(&self.ts)?;
        Some(PlayRow {
            ts,
            track_uri,
            ms_played: self.ms_played.unwrap_or(0).max(0),
            track_name: self.master_metadata_track_name,
            artist_name: self.master_metadata_album_artist_name,
            album_name: self.master_metadata_album_album_name,
            reason_start: self.reason_start,
            reason_end: self.reason_end,
            shuffle: self.shuffle,
            skipped: self.skipped,
            platform: self.platform,
            country: self.conn_country,
            offline: self.offline,
            incognito: self.incognito_mode,
        })
    }
}

// ---- import ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub source: String,
    pub files: usize,
    /// Track plays found in the files.
    pub plays_read: usize,
    /// How many of those were not already stored.
    pub plays_added: usize,
    /// Podcast episodes and audiobook chapters, which Utilify does not chart.
    pub other_rows: usize,
    /// Files that were read but held no streaming history.
    pub ignored_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub file: String,
    pub done: usize,
    pub total: usize,
    pub plays: usize,
}

/// `Streaming_History_Audio_2024.json`, `Streaming_History_Video_2024.json`
/// (which also carries music), `endsong_0.json` from older exports, and the
/// short `StreamingHistory_music_0.json` we only recognise to explain it.
fn is_history_file(name: &str) -> bool {
    let path = name.to_ascii_lowercase();
    // Zips repacked on macOS carry a `__MACOSX/._name` resource fork beside
    // every real entry; those are not JSON.
    if path.contains("__macosx") {
        return false;
    }
    let file = path.rsplit('/').next().unwrap_or(&path);
    if !file.ends_with(".json") || file.starts_with("._") {
        return false;
    }
    let squashed: String = file.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    squashed.contains("streaminghistory") || squashed.contains("endsong")
}

fn file_name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

enum Parsed {
    Extended(Vec<RawPlay>),
    /// The short "account data" history, which we cannot chart.
    Basic,
    Unrecognized,
}

fn parse_bytes(bytes: &[u8]) -> Parsed {
    match serde_json::from_slice::<Vec<RawPlay>>(bytes) {
        Ok(rows) => Parsed::Extended(rows),
        Err(_) => match serde_json::from_slice::<Vec<BasicPlay>>(bytes) {
            Ok(_) => Parsed::Basic,
            Err(_) => Parsed::Unrecognized,
        },
    }
}

/// Native picker (the zip Spotify mails, or the JSON files inside it), then
/// parse and store. `None` means the picker was cancelled.
pub async fn import(app: &AppHandle, db: &Db) -> Result<Option<ImportSummary>> {
    let app2 = app.clone();
    let picked = tokio::task::spawn_blocking(move || {
        app2.dialog()
            .file()
            .set_title("Choose my_spotify_data.zip or its Streaming_History JSON files")
            .add_filter("Spotify data export", &["zip", "json"])
            .blocking_pick_files()
    })
    .await
    .map_err(|e| AppError::other(format!("dialog task failed: {e}")))?;

    let Some(files) = picked else {
        return Ok(None);
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for f in files {
        paths.push(
            f.into_path()
                .map_err(|e| AppError::other(format!("unsupported file location: {e}")))?,
        );
    }
    if paths.is_empty() {
        return Ok(None);
    }

    let app2 = app.clone();
    let db = db.clone();
    let summary = tokio::task::spawn_blocking(move || {
        import_paths(&db, &paths, &|p| {
            let _ = app2.emit("stats-import-progress", p);
        })
    })
    .await
    .map_err(|e| AppError::other(format!("import task failed: {e}")))??;
    Ok(Some(summary))
}

/// Read every selected file (unpacking zips as we go) and store the plays.
/// Parsing 80 MB of JSON takes a few seconds, so this runs off the async
/// runtime and reports progress per file.
fn import_paths(db: &Db, paths: &[PathBuf], progress: &dyn Fn(ImportProgress)) -> Result<ImportSummary> {
    // Expand the selection into a work list first so progress has a total.
    enum Source {
        Loose(PathBuf),
        Zipped { archive: PathBuf, index: usize, name: String },
    }
    let mut work: Vec<Source> = Vec::new();
    for path in paths {
        let is_zip = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("zip"))
            .unwrap_or(false);
        if is_zip {
            let mut zip = zip::ZipArchive::new(File::open(path)?)
                .map_err(|e| AppError::other(format!("{}: not a readable zip ({e})", file_name_of(path))))?;
            for i in 0..zip.len() {
                let entry = zip
                    .by_index(i)
                    .map_err(|e| AppError::other(format!("{}: unreadable entry ({e})", file_name_of(path))))?;
                if entry.is_file() && is_history_file(entry.name()) {
                    work.push(Source::Zipped {
                        archive: path.clone(),
                        index: i,
                        name: file_name_of(Path::new(entry.name())),
                    });
                }
            }
        } else {
            work.push(Source::Loose(path.clone()));
        }
    }

    if work.is_empty() {
        return Err(AppError::other(
            "No streaming history in that file. Pick the zip Spotify emailed you (it holds \
             Streaming_History_Audio_*.json), or those JSON files directly.",
        ));
    }

    let total = work.len();
    let mut plays_read = 0usize;
    let mut plays_added = 0usize;
    let mut other_rows = 0usize;
    let mut ignored_files: Vec<String> = Vec::new();
    let mut basic_files = 0usize;
    let mut files_used = 0usize;

    for (done, source) in work.iter().enumerate() {
        let (name, bytes) = match source {
            Source::Loose(path) => {
                let mut buf = Vec::new();
                File::open(path)?.read_to_end(&mut buf)?;
                (file_name_of(path), buf)
            }
            Source::Zipped { archive, index, name } => {
                let mut zip = zip::ZipArchive::new(File::open(archive)?)
                    .map_err(|e| AppError::other(format!("{}: not a readable zip ({e})", file_name_of(archive))))?;
                let mut entry = zip
                    .by_index(*index)
                    .map_err(|e| AppError::other(format!("{name}: unreadable entry ({e})")))?;
                let mut buf = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut buf)?;
                (name.clone(), buf)
            }
        };

        let rows = match parse_bytes(&bytes) {
            Parsed::Extended(rows) => rows,
            Parsed::Basic => {
                basic_files += 1;
                ignored_files.push(name.clone());
                continue;
            }
            Parsed::Unrecognized => {
                log::warn!("stats import: {name} is not streaming history");
                ignored_files.push(name.clone());
                continue;
            }
        };
        drop(bytes);

        let mut batch: Vec<PlayRow> = Vec::with_capacity(rows.len());
        for raw in rows {
            match raw.into_row() {
                Some(row) => batch.push(row),
                None => other_rows += 1,
            }
        }
        plays_read += batch.len();
        files_used += 1;

        let added = db.with_mut(|c| {
            let tx = c.transaction()?;
            let added = stats::insert_plays(&tx, &batch)?;
            tx.commit()?;
            Ok(added)
        })?;
        plays_added += added;
        log::info!("stats import: {name} → {} plays ({added} new)", batch.len());

        progress(ImportProgress {
            file: name,
            done: done + 1,
            total,
            plays: plays_read,
        });
    }

    if files_used == 0 {
        if basic_files > 0 {
            return Err(AppError::other(
                "Those files are the short \"account data\" history, which has no track IDs, \
                 skips or shuffle flags. Request \"Extended streaming history\" on Spotify's \
                 privacy page and import that zip instead.",
            ));
        }
        return Err(AppError::other("No streaming history could be read from the selected files."));
    }

    let source = if paths.len() == 1 {
        file_name_of(&paths[0])
    } else {
        format!("{} files", paths.len())
    };
    db.with(|c| stats::record_import(c, now(), &source, files_used, plays_read, plays_added))?;

    Ok(ImportSummary {
        source,
        files: files_used,
        plays_read,
        plays_added,
        other_rows,
        ignored_files,
    })
}

// ---- summary ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsSummary {
    /// The range actually used, after filling in open ends.
    pub from: i64,
    pub to: i64,
    pub threshold_ms: i64,
    pub status: StatsStatus,
    pub totals: Totals,
    pub top_songs: Vec<SongStat>,
    pub top_artists: Vec<NamedStat>,
    pub top_albums: Vec<NamedStat>,
    pub most_skipped: Vec<SongStat>,
    pub by_hour: Vec<Bucket>,
    pub by_weekday: Vec<Bucket>,
    pub by_month: Vec<Bucket>,
    pub by_platform: Vec<Bucket>,
}

/// Fold "iOS 14.8.1 (iPhone11,8)", "ios" and the rest into device families.
/// Order matters: a web player string also names its operating system.
fn platform_family(raw: &str) -> &'static str {
    let p = raw.to_ascii_lowercase();
    if p.is_empty() {
        return "Unknown";
    }
    if p.contains("web_player") || p.contains("webplayer") {
        return "Web player";
    }
    if p.contains("ios") || p.contains("iphone") || p.contains("ipad") {
        return "iPhone / iPad";
    }
    if p.contains("android") {
        return "Android";
    }
    if p.contains("windows") || p.contains("win32") || p.contains("winrt") {
        return "Windows";
    }
    if p.contains("osx") || p.contains("os x") || p.contains("macos") || p.contains("darwin") {
        return "macOS";
    }
    if p.contains("linux") {
        return "Linux";
    }
    if p.contains("cast") {
        return "Cast";
    }
    if p.contains("partner") || p.contains("sonos") || p.contains("xbox") || p.contains("playstation") {
        return "Speakers & consoles";
    }
    "Other"
}

fn fold_platforms(raw: Vec<Bucket>) -> Vec<Bucket> {
    let mut out: Vec<Bucket> = Vec::new();
    for b in raw {
        let family = platform_family(&b.label);
        match out.iter_mut().find(|x| x.label == family) {
            Some(existing) => {
                existing.plays += b.plays;
                existing.ms += b.ms;
            }
            None => out.push(Bucket {
                label: family.to_string(),
                plays: b.plays,
                ms: b.ms,
            }),
        }
    }
    out.sort_by(|a, b| b.ms.cmp(&a.ms));
    out
}

pub fn status(db: &Db) -> Result<StatsStatus> {
    db.with(stats::status)
}

/// Every number on the Stats page, for one range and one play threshold.
/// Open ends default to the full history.
pub fn summary(db: &Db, from: Option<i64>, to: Option<i64>, threshold_secs: i64) -> Result<StatsSummary> {
    let threshold_ms = threshold_secs.clamp(0, 3600) * 1000;
    db.with(|c| {
        let status = stats::status(c)?;
        let from = from.or(status.first_ts).unwrap_or(0);
        let to = to.or(status.last_ts).unwrap_or(0).max(from);
        let (top_songs, most_skipped) = stats::song_rankings(c, from, to, threshold_ms, TOP_LIMIT)?;
        Ok(StatsSummary {
            from,
            to,
            threshold_ms,
            totals: stats::totals(c, from, to, threshold_ms)?,
            top_songs,
            most_skipped,
            top_artists: stats::top_artists(c, from, to, threshold_ms, TOP_LIMIT as i64)?,
            top_albums: stats::top_albums(c, from, to, threshold_ms, TOP_LIMIT as i64)?,
            by_hour: stats::by_hour(c, from, to, threshold_ms)?,
            by_weekday: stats::by_weekday(c, from, to, threshold_ms)?,
            by_month: stats::by_month(c, from, to, threshold_ms)?,
            by_platform: fold_platforms(stats::by_platform(c, from, to, threshold_ms)?),
            status,
        })
    })
}

pub fn clear(db: &Db) -> Result<()> {
    db.with(stats::clear)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_history_file_names() {
        assert!(is_history_file("Spotify Extended Streaming History/Streaming_History_Audio_2024.json"));
        assert!(is_history_file("Streaming_History_Video_2026.json"));
        assert!(is_history_file("endsong_0.json"));
        assert!(is_history_file("StreamingHistory_music_0.json"));
        assert!(!is_history_file("Playlist1.json"));
        assert!(!is_history_file("ReadMeFirst_ExtendedStreamingHistory.pdf"));
        assert!(!is_history_file("__MACOSX/._Streaming_History_Audio_2024.json"));
        assert!(!is_history_file("Spotify Extended Streaming History/._Streaming_History_Audio_2024.json"));
    }

    #[test]
    fn parses_an_export_record() {
        let json = br#"[{
            "ts": "2021-01-31T15:17:35Z",
            "platform": "iOS 14.3 (iPhone11,8)",
            "ms_played": 120554,
            "conn_country": "US",
            "ip_addr": "10.0.0.1",
            "master_metadata_track_name": "HOLIDAY",
            "master_metadata_album_artist_name": "Lil Nas X",
            "master_metadata_album_album_name": "HOLIDAY",
            "spotify_track_uri": "spotify:track:6zFMeegAMYQo0mt8rXtrli",
            "episode_name": null,
            "reason_start": "playbtn",
            "reason_end": "fwdbtn",
            "shuffle": false,
            "skipped": false,
            "offline": false,
            "incognito_mode": false
        }]"#;
        let Parsed::Extended(rows) = parse_bytes(json) else {
            panic!("should parse as extended history");
        };
        let row = rows.into_iter().next().unwrap().into_row().unwrap();
        assert_eq!(row.ts, 1_612_106_255);
        assert_eq!(row.ms_played, 120_554);
        assert_eq!(row.artist_name.as_deref(), Some("Lil Nas X"));
        assert_eq!(row.track_uri, "spotify:track:6zFMeegAMYQo0mt8rXtrli");
    }

    #[test]
    fn podcast_rows_have_no_track_and_are_dropped() {
        let json = br#"[{
            "ts": "2023-04-05T10:00:00Z",
            "ms_played": 900000,
            "master_metadata_track_name": null,
            "spotify_track_uri": null,
            "episode_name": "Some episode",
            "spotify_episode_uri": "spotify:episode:abc"
        }]"#;
        let Parsed::Extended(rows) = parse_bytes(json) else {
            panic!("should parse as extended history");
        };
        assert!(rows.into_iter().next().unwrap().into_row().is_none());
    }

    #[test]
    fn the_short_account_export_is_recognised_separately() {
        let json = br#"[{"endTime":"2024-01-01 12:00","artistName":"X","trackName":"Y","msPlayed":1000}]"#;
        assert!(matches!(parse_bytes(json), Parsed::Basic));
        assert!(matches!(parse_bytes(b"{\"not\":\"history\"}"), Parsed::Unrecognized));
    }

    const ONE_TRACK: &str = r#"[
        {"ts":"2024-03-01T10:00:00Z","ms_played":200000,"platform":"ios",
         "master_metadata_track_name":"Song","master_metadata_album_artist_name":"Band",
         "master_metadata_album_album_name":"Album","spotify_track_uri":"spotify:track:1"},
        {"ts":"2024-03-01T10:05:00Z","ms_played":900000,"platform":"ios",
         "episode_name":"Pod","spotify_episode_uri":"spotify:episode:9"}
    ]"#;
    const OVERLAPPING: &str = r#"[
        {"ts":"2024-03-01T10:00:00Z","ms_played":200000,"platform":"ios",
         "master_metadata_track_name":"Song","master_metadata_album_artist_name":"Band",
         "spotify_track_uri":"spotify:track:1"},
        {"ts":"2024-03-02T11:00:00Z","ms_played":150000,"platform":"windows",
         "master_metadata_track_name":"Other","master_metadata_album_artist_name":"Band",
         "spotify_track_uri":"spotify:track:2"}
    ]"#;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("utilify-stats-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    /// The real export is a zip of per-year JSON files; the yearly files
    /// overlap when a user imports a newer export over an older one.
    #[test]
    fn imports_a_zip_then_a_loose_file_without_double_counting() {
        use std::io::Write;

        let dir = temp_dir("zip");
        let zip_path = dir.0.join("my_spotify_data.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("Spotify Extended Streaming History/Streaming_History_Audio_2024.json", opts)
                .unwrap();
            zip.write_all(ONE_TRACK.as_bytes()).unwrap();
            // Resource forks and non-history files must be passed over.
            zip.start_file("__MACOSX/._Streaming_History_Audio_2024.json", opts).unwrap();
            zip.write_all(b"not json at all").unwrap();
            zip.start_file("Playlist1.json", opts).unwrap();
            zip.write_all(b"[]").unwrap();
            zip.finish().unwrap();
        }

        let db = Db::open(&dir.0.join("test.db")).unwrap();
        let first = import_paths(&db, &[zip_path.clone()], &|_| {}).unwrap();
        assert_eq!(first.files, 1, "only the history file is read");
        assert_eq!(first.plays_read, 1);
        assert_eq!(first.plays_added, 1);
        assert_eq!(first.other_rows, 1, "the podcast row is counted, not stored");

        // Importing the same zip again changes nothing.
        let again = import_paths(&db, &[zip_path], &|_| {}).unwrap();
        assert_eq!(again.plays_added, 0);
        assert_eq!(status(&db).unwrap().plays, 1);

        // A later export repeats old rows and adds new ones.
        let loose = dir.0.join("Streaming_History_Audio_2024_1.json");
        std::fs::write(&loose, OVERLAPPING).unwrap();
        let third = import_paths(&db, &[loose], &|_| {}).unwrap();
        assert_eq!(third.plays_read, 2);
        assert_eq!(third.plays_added, 1);
        assert_eq!(status(&db).unwrap().plays, 2);

        let s = summary(&db, None, None, 30).unwrap();
        assert_eq!(s.totals.plays, 2);
        assert_eq!(s.totals.artists, 1);
        assert_eq!(s.top_artists[0].name, "Band");
        assert_eq!(s.by_platform.len(), 2);
    }

    #[test]
    fn a_selection_with_no_history_explains_itself() {
        let dir = temp_dir("empty");
        let db = Db::open(&dir.0.join("test.db")).unwrap();

        let basic = dir.0.join("StreamingHistory_music_0.json");
        std::fs::write(
            &basic,
            br#"[{"endTime":"2024-01-01 12:00","artistName":"X","trackName":"Y","msPlayed":1000}]"#,
        )
        .unwrap();
        let err = import_paths(&db, &[basic], &|_| {}).unwrap_err().to_string();
        assert!(err.contains("Extended streaming history"), "{err}");

        let zip_path = dir.0.join("empty.zip");
        {
            let zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            zip.finish().unwrap();
        }
        let err = import_paths(&db, &[zip_path], &|_| {}).unwrap_err().to_string();
        assert!(err.contains("No streaming history"), "{err}");
    }

    #[test]
    fn platforms_fold_into_device_families() {
        assert_eq!(platform_family("iOS 14.8.1 (iPhone11,8)"), "iPhone / iPad");
        assert_eq!(platform_family("ios"), "iPhone / iPad");
        assert_eq!(platform_family("windows"), "Windows");
        assert_eq!(platform_family("Android OS 13 API 33 (Google, Pixel 7)"), "Android");
        // A web player names its OS too; the browser wins.
        assert_eq!(platform_family("web_player windows 10;chrome 102.0;desktop"), "Web player");
        assert_eq!(platform_family(""), "Unknown");

        let folded = fold_platforms(vec![
            Bucket { label: "ios".into(), plays: 2, ms: 200 },
            Bucket { label: "iOS 15.6 (iPhone11,8)".into(), plays: 1, ms: 100 },
            Bucket { label: "windows".into(), plays: 5, ms: 500 },
        ]);
        assert_eq!(folded.len(), 2);
        assert_eq!(folded[0].label, "Windows");
        assert_eq!(folded[1].label, "iPhone / iPad");
        assert_eq!(folded[1].plays, 3);
        assert_eq!(folded[1].ms, 300);
    }
}
