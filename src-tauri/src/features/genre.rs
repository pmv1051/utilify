//! Genre Filtering (light): artist-level genre tags applied to the tracks of
//! a playlist. A track carries the union of its credited artists' genres.
//! Genre tags are cached per artist for 30 days.
//!
//! Development Mode forbids `GET /artists?ids=` (403), so artists are looked
//! up one by one with `GET /artists/{id}`. Live runs also showed that endpoint
//! returning empty `genres` for every artist; Spotify has been stripping genre
//! data from the API. If a whole batch comes back untagged, a few artists are
//! probed through `GET /search?type=artist`, and if search still carries tags
//! the lookup switches to search for the session.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::genres::{self, ArtistGenres};
use crate::db::now;
use crate::error::Result;
use crate::features::tracks::{fetch_playlist, playlist_display_name, TrackInfo};
use crate::spotify::artists::{self, ArtistObject};
use crate::state::AppState;

const GENRE_CACHE_SECS: i64 = 30 * 24 * 3600;
const THROTTLE: Duration = Duration::from_millis(120);
const PROBE_COUNT: usize = 3;

/// Set once search proved to carry genres while single lookups did not.
static USE_SEARCH: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenreCount {
    pub genre: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenreTrack {
    #[serde(flatten)]
    pub track: TrackInfo,
    pub genres: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenreBreakdown {
    pub playlist_id: String,
    pub playlist_name: String,
    pub genres: Vec<GenreCount>,
    pub tracks: Vec<GenreTrack>,
    pub untagged: usize,
    pub artists_total: usize,
    pub artists_fetched: usize,
    /// `single` or `search`.
    pub lookup_strategy: String,
    /// True when Spotify returned no tags for any artist fetched this run.
    pub no_tags_from_spotify: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenreProgress {
    pub done: usize,
    pub total: usize,
}

async fn lookup_single(state: &AppState, id: &str) -> Result<Option<ArtistObject>> {
    artists::get_artist(&state.spotify, id).await
}

async fn lookup_search(state: &AppState, id: &str, name: &str) -> Result<Option<ArtistObject>> {
    let hits = artists::search_artists(&state.spotify, name).await?;
    let lower = name.to_lowercase();
    Ok(hits
        .iter()
        .find(|a| a.id == id)
        .or_else(|| hits.iter().find(|a| a.name.to_lowercase() == lower))
        .cloned())
}

/// Fetch artists one by one with the current strategy, reporting progress.
async fn fetch_all(
    app: &AppHandle,
    state: &AppState,
    ids: &[String],
    names: &HashMap<String, String>,
    use_search: bool,
) -> Result<Vec<ArtistObject>> {
    let total = ids.len();
    let mut out = Vec::with_capacity(total);
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(THROTTLE).await;
        }
        let found = if use_search {
            match names.get(id) {
                Some(name) => lookup_search(state, id, name).await?,
                None => None,
            }
        } else {
            lookup_single(state, id).await?
        };
        if let Some(a) = found {
            out.push(a);
        }
        let _ = app.emit("genre-progress", GenreProgress { done: i + 1, total });
    }
    Ok(out)
}

/// Genres for the given artist ids: cache (unless `force`), then lookups.
/// Returns (id → genres, number fetched, strategy, no-tags flag).
async fn genres_for(
    app: &AppHandle,
    state: &AppState,
    ids: &[String],
    names: &HashMap<String, String>,
    force: bool,
) -> Result<(HashMap<String, Vec<String>>, usize, &'static str, bool)> {
    let cutoff = if force { i64::MAX } else { now() - GENRE_CACHE_SECS };
    let cached = state.db.with(|c| genres::get_many(c, ids, cutoff))?;
    let mut map: HashMap<String, Vec<String>> = cached.into_iter().map(|(id, a)| (id, a.genres)).collect();

    let missing: Vec<String> = ids.iter().filter(|id| !map.contains_key(*id)).cloned().collect();
    let mut strategy = if USE_SEARCH.load(Ordering::Relaxed) { "search" } else { "single" };
    let mut no_tags = false;

    if !missing.is_empty() {
        let mut list = fetch_all(app, state, &missing, names, strategy == "search").await?;

        // Single lookups answered but nothing carried a tag: see whether search does.
        if strategy == "single" && !list.is_empty() && list.iter().all(|a| a.genres.is_empty()) {
            log::warn!("genre: {} artists returned with no genres via /artists/{{id}}; probing search", list.len());
            let mut probe_hits = 0;
            for a in list.iter().take(PROBE_COUNT) {
                tokio::time::sleep(THROTTLE).await;
                if let Some(s) = lookup_search(state, &a.id, &a.name).await? {
                    if !s.genres.is_empty() {
                        probe_hits += 1;
                    }
                }
            }
            if probe_hits > 0 {
                log::info!("genre: search carries genres ({probe_hits}/{PROBE_COUNT} probes); switching strategy");
                USE_SEARCH.store(true, Ordering::Relaxed);
                strategy = "search";
                list = fetch_all(app, state, &missing, names, true).await?;
            } else {
                log::warn!("genre: search returns no genres either; Spotify provides no tags for these artists");
                no_tags = true;
            }
        }
        if strategy == "search" && !list.is_empty() && list.iter().all(|a| a.genres.is_empty()) {
            no_tags = true;
        }

        let ts = now();
        let mut returned: HashSet<String> = HashSet::new();
        state.db.with(|c| {
            for a in &list {
                genres::put(
                    c,
                    &ArtistGenres {
                        artist_id: a.id.clone(),
                        name: Some(a.name.clone()),
                        genres: a.genres.clone(),
                    },
                    ts,
                )?;
            }
            // Ids Spotify did not return: cache as untagged so we do not retry every time.
            for id in missing.iter().filter(|id| !list.iter().any(|a| &a.id == *id)) {
                genres::put(c, &ArtistGenres { artist_id: id.clone(), name: names.get(id).cloned(), genres: vec![] }, ts)?;
            }
            Ok(())
        })?;
        for a in list {
            returned.insert(a.id.clone());
            map.insert(a.id, a.genres);
        }
    }
    Ok((map, missing.len(), strategy, no_tags))
}

pub async fn breakdown(app: &AppHandle, state: &AppState, playlist_id: &str, force: bool) -> Result<GenreBreakdown> {
    let tracks: Vec<TrackInfo> = fetch_playlist(state, playlist_id).await?.into_iter().filter(|t| t.playable).collect();
    let mut names: HashMap<String, String> = HashMap::new();
    for t in &tracks {
        for a in &t.artist_refs {
            if let Some(id) = &a.id {
                names.entry(id.clone()).or_insert_with(|| a.name.clone());
            }
        }
    }
    let mut ids: Vec<String> = names.keys().cloned().collect();
    ids.sort();
    let artists_total = ids.len();

    let (map, artists_fetched, strategy, no_tags) = genres_for(app, state, &ids, &names, force).await?;

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut untagged = 0usize;
    let mut out = Vec::with_capacity(tracks.len());
    for t in tracks {
        let mut genres: Vec<String> = Vec::new();
        for id in &t.artist_ids {
            if let Some(g) = map.get(id) {
                for genre in g {
                    if !genres.contains(genre) {
                        genres.push(genre.clone());
                    }
                }
            }
        }
        if genres.is_empty() {
            untagged += 1;
        }
        for g in &genres {
            *counts.entry(g.clone()).or_insert(0) += 1;
        }
        out.push(GenreTrack { track: t, genres });
    }

    let mut genres: Vec<GenreCount> = counts.into_iter().map(|(genre, count)| GenreCount { genre, count }).collect();
    genres.sort_by(|a, b| b.count.cmp(&a.count).then(a.genre.cmp(&b.genre)));

    Ok(GenreBreakdown {
        playlist_id: playlist_id.to_string(),
        playlist_name: playlist_display_name(state, playlist_id),
        genres,
        tracks: out,
        untagged,
        artists_total,
        artists_fetched,
        lookup_strategy: strategy.into(),
        no_tags_from_spotify: no_tags,
    })
}
