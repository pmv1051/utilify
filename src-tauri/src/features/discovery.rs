//! Discovery Mode: queue tracks the user has genuinely never encountered.
//!
//! "Seen" (see `db::discovery::UNSEEN_FILTER`) means: ever in the playback log
//! (skipped or listened ≥10 s), ever offered by Discovery, or present in any
//! playlist of the user's library (the library index, rebuilt on demand).
//! Candidates come from indexed sources: followed artists' releases, seed
//! artists, seed playlists. Outcomes are recorded by the playback logger.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::discovery::{self as ddb, Candidate, DiscoveryLogRow, DiscoveryTotals, PoolTrack, SourceRow};
use crate::db::{self, config, now};
use crate::error::{AppError, Result};
use crate::features::discography::ArtistHit;
use crate::features::generated::{self, GeneratedPlaylist};
use crate::features::tracks::fetch_playlist;
use crate::spotify::models::PlaybackState;
use crate::spotify::{artists, following, playback, playlists};
use crate::state::AppState;

const LIBRARY_INDEX_UPDATED_AT: &str = "library_index_updated_at";
/// Sources younger than this are skipped by a non-forced re-index.
const SOURCE_FRESH_SECS: i64 = 7 * 24 * 3600;
const THROTTLE: Duration = Duration::from_millis(150);
pub const MAX_QUEUE: usize = 20;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryIndexInfo {
    pub playlists: i64,
    pub tracks: i64,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryStatus {
    pub library: LibraryIndexInfo,
    pub pool_total: i64,
    pub pool_unseen: i64,
    pub sources: Vec<SourceRow>,
    pub totals: DiscoveryTotals,
    pub recent: Vec<DiscoveryLogRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryProgress {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub label: String,
}

fn progress(app: &AppHandle, phase: &str, done: usize, total: usize, label: &str) {
    let _ = app.emit(
        "discovery-progress",
        DiscoveryProgress {
            phase: phase.into(),
            done,
            total,
            label: label.into(),
        },
    );
}

pub fn status(state: &AppState) -> Result<DiscoveryStatus> {
    state.db.with(|c| {
        let (playlists, tracks) = ddb::library_index_size(c)?;
        let updated_at = config::get(c, LIBRARY_INDEX_UPDATED_AT)?.and_then(|s| s.parse().ok());
        let (pool_total, pool_unseen) = ddb::pool_counts(c)?;
        Ok(DiscoveryStatus {
            library: LibraryIndexInfo {
                playlists,
                tracks,
                updated_at,
            },
            pool_total,
            pool_unseen,
            sources: ddb::list_sources(c)?,
            totals: ddb::totals(c)?,
            recent: ddb::recent_log(c, 50)?,
        })
    })
}

/// Re-read every playlist in the library into the index. One request per 50
/// tracks per playlist, so this is deliberately manual.
pub async fn rebuild_library_index(app: &AppHandle, state: &AppState) -> Result<LibraryIndexInfo> {
    let mut list = state.db.with(|c| db::playlists::list(c, None))?;
    if list.is_empty() {
        let fresh = playlists::list_user_playlists(&state.spotify).await?;
        crate::features::cache_playlists(&state.db, &fresh)?;
        list = state.db.with(|c| db::playlists::list(c, None))?;
    }
    let total = list.len();
    for (i, p) in list.iter().enumerate() {
        progress(app, "library", i, total, &p.name);
        let uris: Vec<String> = fetch_playlist(state, &p.id)
            .await?
            .into_iter()
            .filter(|t| t.playable)
            .map(|t| t.uri)
            .collect();
        state.db.with_mut(|c| ddb::replace_library_playlist(c, &p.id, &uris))?;
        tokio::time::sleep(THROTTLE).await;
    }
    let ts = now();
    state.db.with(|c| config::set(c, LIBRARY_INDEX_UPDATED_AT, &ts.to_string()))?;
    progress(app, "library", total, total, "done");
    let (playlists, tracks) = state.db.with(ddb::library_index_size)?;
    log::info!("discovery: library index rebuilt: {playlists} playlists, {tracks} tracks");
    Ok(LibraryIndexInfo {
        playlists,
        tracks,
        updated_at: Some(ts),
    })
}

pub async fn followed_artists(state: &AppState) -> Result<Vec<ArtistHit>> {
    Ok(following::followed_artists(&state.spotify)
        .await?
        .into_iter()
        .map(|a| ArtistHit {
            image_url: a.images.iter().min_by_key(|i| i.width.unwrap_or(u32::MAX)).map(|i| i.url.clone()),
            followers: a.followers.as_ref().and_then(|f| f.total),
            id: a.id,
            name: a.name,
            genres: a.genres,
        })
        .collect())
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRef {
    pub id: String,
    pub name: String,
}

/// Index the newest `max_releases` albums/singles of each artist into the pool.
/// Cost per artist: one release listing plus one request per release.
pub async fn index_artists(
    app: &AppHandle,
    state: &AppState,
    list: &[ArtistRef],
    kind: &str,
    max_releases: usize,
    force: bool,
) -> Result<usize> {
    if !matches!(kind, "followed_artist" | "seed_artist") {
        return Err(AppError::other("unknown source kind"));
    }
    let max_releases = max_releases.clamp(1, 50);
    let total = list.len();
    let mut indexed = 0usize;
    for (i, a) in list.iter().enumerate() {
        let key = format!("{kind}:{}", a.id);
        progress(app, "artists", i, total, &a.name);
        if !force {
            if let Some(existing) = state.db.with(|c| ddb::get_source(c, &key))? {
                if now() - existing.indexed_at < SOURCE_FRESH_SECS {
                    continue;
                }
            }
        }

        let mut releases = artists::artist_albums(&state.spotify, &a.id, "album,single").await?;
        releases.sort_by(|x, y| y.release_date.cmp(&x.release_date));
        releases.truncate(max_releases);

        let mut tracks: Vec<(String, String, String, Option<String>, String)> = Vec::new();
        for r in &releases {
            tokio::time::sleep(THROTTLE).await;
            for t in artists::album_tracks(&state.spotify, &r.id).await? {
                if !t.is_playable_catalog_track() || !t.artists.iter().any(|x| x.id.as_deref() == Some(a.id.as_str())) {
                    continue;
                }
                let Some(uri) = t.uri.clone() else { continue };
                tracks.push((uri, t.name.clone(), t.artist_names(), Some(a.id.clone()), r.name.clone()));
            }
        }
        let rows: Vec<PoolTrack<'_>> = tracks
            .iter()
            .map(|(uri, name, artists, artist_id, album)| PoolTrack {
                track_uri: uri,
                name: Some(name),
                artists: Some(artists),
                artist_id: artist_id.as_deref(),
                album: Some(album),
            })
            .collect();
        let source = SourceRow {
            key,
            kind: kind.into(),
            label: a.name.clone(),
            indexed_at: now(),
            track_count: rows.len() as i64,
        };
        state.db.with_mut(|c| ddb::replace_source(c, &source, &rows))?;
        indexed += 1;
        log::info!("discovery: indexed {} ({} releases, {} tracks)", a.name, releases.len(), rows.len());
        tokio::time::sleep(THROTTLE).await;
    }
    progress(app, "artists", total, total, "done");
    Ok(indexed)
}

/// Accepts a playlist id, `spotify:playlist:…` URI, or open.spotify.com URL.
pub fn parse_playlist_ref(input: &str) -> Option<String> {
    let s = input.trim();
    if let Some(rest) = s.strip_prefix("spotify:playlist:") {
        return Some(rest.to_string());
    }
    if let Some(idx) = s.find("/playlist/") {
        let tail = &s[idx + "/playlist/".len()..];
        let id: String = tail.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        return (!id.is_empty()).then_some(id);
    }
    (s.len() >= 16 && s.chars().all(|c| c.is_ascii_alphanumeric())).then(|| s.to_string())
}

pub async fn add_seed_playlist(app: &AppHandle, state: &AppState, reference: &str) -> Result<SourceRow> {
    let id = parse_playlist_ref(reference)
        .ok_or_else(|| AppError::other("Paste a Spotify playlist link, URI, or id."))?;
    let meta = playlists::get_playlist(&state.spotify, &id).await?;
    progress(app, "seed", 0, 1, &meta.name);
    let tracks = fetch_playlist(state, &id).await?;
    let rows: Vec<PoolTrack<'_>> = tracks
        .iter()
        .filter(|t| t.playable)
        .map(|t| PoolTrack {
            track_uri: &t.uri,
            name: Some(&t.name),
            artists: Some(&t.artists),
            artist_id: t.artist_ids.first().map(String::as_str),
            album: t.album.as_deref(),
        })
        .collect();
    let source = SourceRow {
        key: format!("seed_playlist:{id}"),
        kind: "seed_playlist".into(),
        label: meta.name.clone(),
        indexed_at: now(),
        track_count: rows.len() as i64,
    };
    state.db.with_mut(|c| ddb::replace_source(c, &source, &rows))?;
    progress(app, "seed", 1, 1, "done");
    Ok(source)
}

pub fn remove_source(state: &AppState, key: &str) -> Result<()> {
    state.db.with_mut(|c| ddb::remove_source(c, key))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverResult {
    pub mode: String,
    pub offered: Vec<Candidate>,
    pub playlist: Option<GeneratedPlaylist>,
}

/// Pick `n` unseen tracks and either add them to the Spotify queue or build a
/// `Utilify: Discovery …` playlist and start it. Either way they are logged as
/// offered, so they will not come up again.
pub async fn discover(state: &AppState, n: usize, mode: &str) -> Result<DiscoverResult> {
    let n = n.clamp(1, if mode == "queue" { MAX_QUEUE } else { 100 });
    let picks = state.db.with(|c| ddb::sample_unseen(c, n))?;
    if picks.is_empty() {
        return Err(AppError::other(
            "No unseen tracks left in the pool. Add or re-index sources on this page.",
        ));
    }

    let playlist = match mode {
        "queue" => {
            let device = state.last_playback().and_then(|p| p.device.and_then(|d| d.id));
            for (i, c) in picks.iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(THROTTLE).await;
                }
                playback::add_to_queue(&state.spotify, &c.track_uri, device.as_deref()).await?;
            }
            None
        }
        "playlist" => {
            let name = format!("Discovery {}", chrono::Local::now().format("%Y-%m-%d %H:%M"));
            let uris: Vec<String> = picks.iter().map(|c| c.track_uri.clone()).collect();
            let created = generated::create_from_uris(state, &name, uris.clone(), false).await?;
            state
                .db
                .with(|c| ddb::save_playlist(c, &created.id, &created.name, &uris, now()))?;
            match playback::play_playlist_unshuffled(&state.spotify, &created.id).await {
                Ok(()) | Err(AppError::NoActiveDevice) => {}
                Err(e) => log::warn!("discovery: could not start playlist: {e}"),
            }
            Some(created)
        }
        other => return Err(AppError::other(format!("unknown discovery mode '{other}'"))),
    };

    let ts = now();
    state.db.with(|c| {
        for p in &picks {
            ddb::log_offered(c, p, ts)?;
        }
        Ok(())
    })?;
    log::info!("discovery: offered {} tracks via {mode}", picks.len());
    Ok(DiscoverResult {
        mode: mode.into(),
        offered: picks,
        playlist,
    })
}

/// Positional inference for Discovery playlists (called every poll).
///
/// Polls are 30 s apart, so a track can start and finish unobserved. If the
/// previous poll was at index `i` of a Discovery playlist we built and this
/// poll is at index `j > i` of the same playlist, every track strictly between
/// them was played through: mark those listened. The two endpoints were
/// observed, so the playback logger's own 10-second verdict stands for them.
pub fn on_poll(state: &AppState, previous: Option<&PlaybackState>, current: Option<&PlaybackState>) {
    let (Some(prev), Some(cur)) = (previous, current) else { return };
    let (Some(ctx), Some(prev_ctx)) = (cur.context_uri(), prev.context_uri()) else { return };
    if ctx != prev_ctx {
        return;
    }
    let Some(playlist_id) = ctx.strip_prefix("spotify:playlist:") else { return };
    let order = match state.db.with(|c| ddb::playlist_order(c, playlist_id)) {
        Ok(Some(o)) => o,
        Ok(None) => return,
        Err(e) => {
            log::warn!("discovery: could not read playlist order: {e}");
            return;
        }
    };
    let idx = |p: &PlaybackState| {
        let uri = p.track_uri()?;
        let original = p.original_track_uri();
        order.iter().position(|u| u == uri || Some(u.as_str()) == original)
    };
    let (Some(i), Some(j)) = (idx(prev), idx(cur)) else { return };
    if j <= i + 1 {
        return;
    }
    let between: Vec<String> = order[i + 1..j].to_vec();
    match state.db.with(|c| ddb::mark_listened(c, &between)) {
        Ok(n) if n > 0 => log::info!("discovery: {n} track(s) played between polls marked listened"),
        Ok(_) => {}
        Err(e) => log::warn!("discovery: mark_listened failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_playlist_ref;

    #[test]
    fn parses_playlist_references() {
        assert_eq!(parse_playlist_ref("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"), Some("37i9dQZF1DXcBWIGoYBM5M".into()));
        assert_eq!(
            parse_playlist_ref("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc"),
            Some("37i9dQZF1DXcBWIGoYBM5M".into())
        );
        assert_eq!(parse_playlist_ref("  37i9dQZF1DXcBWIGoYBM5M "), Some("37i9dQZF1DXcBWIGoYBM5M".into()));
        assert_eq!(parse_playlist_ref("not a playlist"), None);
    }
}
