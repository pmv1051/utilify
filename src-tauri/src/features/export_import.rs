//! Playlist Export (CSV / text) and Import ("Artist — Track" lines → search
//! → confirm → playlist).

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;

use crate::error::{AppError, Result};
use crate::features::matching::{normalize_title, similarity};
use crate::features::tracks::{fetch_playlist, playlist_display_name};
use crate::spotify::search;
use crate::state::AppState;

// ---- export ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportContent {
    pub file_name: String,
    pub content: String,
    pub tracks: usize,
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn mmss(ms: Option<u64>) -> String {
    match ms {
        Some(ms) => format!("{}:{:02}", ms / 60000, (ms / 1000) % 60),
        None => String::new(),
    }
}

fn safe_file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let stem = stem.trim().to_string();
    if stem.is_empty() {
        "playlist".into()
    } else {
        stem
    }
}

/// Build the export text. `format` is `csv` or `txt`.
pub async fn export(state: &AppState, playlist_id: &str, format: &str) -> Result<ExportContent> {
    let name = playlist_display_name(state, playlist_id);
    let tracks = fetch_playlist(state, playlist_id).await?;
    let mut out = String::new();
    match format {
        "csv" => {
            out.push_str("position,title,artists,album,duration,duration_ms,added_at,uri\n");
            for t in &tracks {
                out.push_str(&format!(
                    "{},{},{},{},{},{},{},{}\n",
                    t.position + 1,
                    csv_field(&t.name),
                    csv_field(&t.artists),
                    csv_field(t.album.as_deref().unwrap_or("")),
                    mmss(t.duration_ms),
                    t.duration_ms.map(|d| d.to_string()).unwrap_or_default(),
                    t.added_at.as_deref().unwrap_or(""),
                    t.uri
                ));
            }
        }
        "txt" => {
            for t in &tracks {
                out.push_str(&format!("{} — {}\n", t.artists, t.name));
            }
        }
        other => return Err(AppError::other(format!("unknown export format '{other}'"))),
    }
    Ok(ExportContent {
        file_name: format!("{}.{}", safe_file_stem(&name), format),
        tracks: tracks.len(),
        content: out,
    })
}

/// Native save dialog + write. Returns the chosen path, or `None` if cancelled.
pub async fn save_text_file(app: &AppHandle, file_name: &str, content: &str) -> Result<Option<String>> {
    let ext = file_name.rsplit('.').next().unwrap_or("txt").to_string();
    let app2 = app.clone();
    let name = file_name.to_string();
    let picked = tokio::task::spawn_blocking(move || {
        app2.dialog()
            .file()
            .set_file_name(&name)
            .add_filter(ext.to_uppercase(), &[ext.as_str()])
            .blocking_save_file()
    })
    .await
    .map_err(|e| AppError::other(format!("dialog task failed: {e}")))?;

    let Some(file_path) = picked else {
        return Ok(None);
    };
    let path = file_path
        .into_path()
        .map_err(|e| AppError::other(format!("unsupported save location: {e}")))?;
    std::fs::write(&path, content)?;
    log::info!("exported to {}", path.display());
    Ok(Some(path.display().to_string()))
}

// ---- import ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub uri: String,
    pub name: String,
    pub artists: String,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    /// 0..=1 combined title/artist similarity.
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportMatch {
    pub line: String,
    pub query_artist: Option<String>,
    pub query_title: String,
    pub candidates: Vec<Candidate>,
    /// Index into `candidates` of the best match, if any.
    pub best: Option<usize>,
    /// `exact` (≥0.9), `fuzzy` (≥0.6), `unsure` (<0.6), `none`.
    pub status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub done: usize,
    pub total: usize,
}

/// Split "Artist — Track" (em dash, en dash, hyphen, or tab). Lines with no
/// separator become a free-text title query.
pub fn parse_line(line: &str) -> Option<(Option<String>, String)> {
    let l = line.trim();
    if l.is_empty() || l.starts_with('#') {
        return None;
    }
    for sep in [" — ", " – ", "\t", " - "] {
        if let Some((a, b)) = l.split_once(sep) {
            let (a, b) = (a.trim(), b.trim());
            if !a.is_empty() && !b.is_empty() {
                return Some((Some(a.to_string()), b.to_string()));
            }
        }
    }
    Some((None, l.to_string()))
}

fn score(artist_q: Option<&str>, title_q: &str, name: &str, artists: &str) -> f64 {
    let title_sim = similarity(&normalize_title(title_q), &normalize_title(name));
    match artist_q {
        Some(aq) => {
            let aq = aq.to_lowercase();
            // Best similarity against any credited artist.
            let artist_sim = artists
                .split(',')
                .map(|a| similarity(&aq, &a.trim().to_lowercase()))
                .fold(0.0_f64, f64::max);
            0.6 * title_sim + 0.4 * artist_sim
        }
        None => title_sim,
    }
}

pub async fn import_search(app: &AppHandle, state: &AppState, lines: &[String]) -> Result<Vec<ImportMatch>> {
    let parsed: Vec<(String, Option<String>, String)> = lines
        .iter()
        .filter_map(|l| parse_line(l).map(|(a, t)| (l.trim().to_string(), a, t)))
        .collect();
    if parsed.is_empty() {
        return Err(AppError::other("Paste at least one line like: Artist — Track"));
    }
    if parsed.len() > 500 {
        return Err(AppError::other("Import at most 500 lines at a time."));
    }

    let total = parsed.len();
    let mut out = Vec::with_capacity(total);
    for (i, (line, artist, title)) in parsed.into_iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
        let structured = match &artist {
            Some(a) => format!("track:\"{title}\" artist:\"{a}\""),
            None => format!("track:\"{title}\""),
        };
        let mut found = search::search_tracks(&state.spotify, &structured, 5).await?;
        if found.is_empty() {
            let plain = match &artist {
                Some(a) => format!("{a} {title}"),
                None => title.clone(),
            };
            found = search::search_tracks(&state.spotify, &plain, 5).await?;
        }

        let mut candidates: Vec<Candidate> = found
            .into_iter()
            .filter(|t| t.is_playable_catalog_track())
            .filter_map(|t| {
                let artists = t.artist_names();
                Some(Candidate {
                    score: score(artist.as_deref(), &title, &t.name, &artists),
                    uri: t.uri.clone()?,
                    name: t.name,
                    artists,
                    album: t.album.as_ref().map(|a| a.name.clone()),
                    duration_ms: t.duration_ms,
                })
            })
            .collect();
        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        let best_score = candidates.first().map(|c| c.score).unwrap_or(0.0);
        let status = if candidates.is_empty() {
            "none"
        } else if best_score >= 0.9 {
            "exact"
        } else if best_score >= 0.6 {
            "fuzzy"
        } else {
            "unsure"
        };
        out.push(ImportMatch {
            line,
            query_artist: artist,
            query_title: title,
            best: if candidates.is_empty() { None } else { Some(0) },
            candidates,
            status,
        });
        let _ = app.emit("import-progress", ImportProgress { done: i + 1, total });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_separators() {
        assert_eq!(parse_line("Daft Punk — One More Time"), Some((Some("Daft Punk".into()), "One More Time".into())));
        assert_eq!(parse_line("Daft Punk - One More Time"), Some((Some("Daft Punk".into()), "One More Time".into())));
        assert_eq!(parse_line("Daft Punk\tOne More Time"), Some((Some("Daft Punk".into()), "One More Time".into())));
        assert_eq!(parse_line("One More Time"), Some((None, "One More Time".into())));
        assert_eq!(parse_line("   "), None);
        assert_eq!(parse_line("# comment"), None);
    }

    #[test]
    fn csv_quoting() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(mmss(Some(125_000)), "2:05");
        assert_eq!(safe_file_stem("My / List: 2024"), "My _ List_ 2024");
    }

    #[test]
    fn scoring_prefers_right_artist() {
        let good = score(Some("Adele"), "Hello", "Hello", "Adele");
        let wrong_artist = score(Some("Adele"), "Hello", "Hello", "Lionel Richie");
        assert!(good > 0.95);
        assert!(wrong_artist < good);
        assert!(score(None, "Hello - Remastered", "Hello", "Anyone") > 0.95);
    }
}
