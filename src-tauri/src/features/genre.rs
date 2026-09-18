//! Genre Filtering (light): artist-level genre tags applied to the tracks of
//! a playlist. A track carries the union of its credited artists' genres.
//! Genre tags are cached per artist for 30 days.
//!
//! Development Mode forbids `GET /artists?ids=` (403, live 2026-09-18), so
//! lookups fall back to `GET /artists/{id}` and, if that is forbidden too, to
//! `GET /search?type=artist` by name (search is proven to work and returns
//! genres). The first strategy that works is remembered for the session.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::genres::{self, ArtistGenres};
use crate::db::now;
use crate::error::{AppError, Result};
use crate::features::tracks::{fetch_playlist, playlist_display_name, TrackInfo};
use crate::spotify::artists::{self, ArtistObject};
use crate::state::AppState;

const GENRE_CACHE_SECS: i64 = 30 * 24 * 3600;
const THROTTLE: Duration = Duration::from_millis(120);

/// 0 = try batch, 1 = single lookups, 2 = search by name.
static STRATEGY: AtomicU8 = AtomicU8::new(0);

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
    /// Which lookup worked: `batch`, `single` or `search`.
    pub lookup_strategy: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenreProgress {
    pub done: usize,
    pub total: usize,
}

fn is_forbidden(e: &AppError) -> bool {
    matches!(e, AppError::Spotify { status: 403, .. })
}

/// Fetch artist objects for `ids`, escalating strategies on 403.
async fn fetch_artists(
    app: &AppHandle,
    state: &AppState,
    ids: &[String],
    names: &HashMap<String, String>,
    done_so_far: usize,
    total: usize,
) -> Result<Vec<ArtistObject>> {
    let c = &state.spotify;
    let mut strategy = STRATEGY.load(Ordering::Relaxed);

    if strategy == 0 {
        match artists::get_artists(c, ids).await {
            Ok(list) => return Ok(list),
            Err(e) if is_forbidden(&e) => {
                log::warn!("genre: batch artist lookup forbidden; falling back to single lookups");
                strategy = 1;
                STRATEGY.store(1, Ordering::Relaxed);
            }
            Err(e) => return Err(e),
        }
    }

    let mut out = Vec::with_capacity(ids.len());
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(THROTTLE).await;
        }
        if strategy == 1 {
            match artists::get_artist(c, id).await {
                Ok(Some(a)) => {
                    out.push(a);
                }
                Ok(None) => {}
                Err(e) if is_forbidden(&e) => {
                    log::warn!("genre: single artist lookup forbidden; falling back to search by name");
                    strategy = 2;
                    STRATEGY.store(2, Ordering::Relaxed);
                }
                Err(e) => return Err(e),
            }
        }
        if strategy == 2 {
            let Some(name) = names.get(id) else { continue };
            let hits = artists::search_artists(c, name).await?;
            let lower = name.to_lowercase();
            if let Some(a) = hits
                .iter()
                .find(|a| &a.id == id)
                .or_else(|| hits.iter().find(|a| a.name.to_lowercase() == lower))
            {
                out.push(a.clone());
            }
        }
        let _ = app.emit(
            "genre-progress",
            GenreProgress {
                done: done_so_far + i + 1,
                total,
            },
        );
    }
    Ok(out)
}

/// Genres for the given artist ids, from cache where fresh, fetched otherwise.
async fn genres_for(
    app: &AppHandle,
    state: &AppState,
    ids: &[String],
    names: &HashMap<String, String>,
) -> Result<(HashMap<String, Vec<String>>, usize)> {
    let cutoff = now() - GENRE_CACHE_SECS;
    let cached = state.db.with(|c| genres::get_many(c, ids, cutoff))?;
    let mut map: HashMap<String, Vec<String>> = cached.into_iter().map(|(id, a)| (id, a.genres)).collect();

    let missing: Vec<String> = ids.iter().filter(|id| !map.contains_key(*id)).cloned().collect();
    let total = missing.len();
    let mut fetched = 0usize;
    for slice in missing.chunks(40) {
        let list = fetch_artists(app, state, slice, names, fetched, total).await?;
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
            Ok(())
        })?;
        for a in list {
            returned.insert(a.id.clone());
            map.insert(a.id, a.genres);
        }
        // Ids Spotify did not return: cache as untagged so we do not retry every time.
        state.db.with(|c| {
            for id in slice.iter().filter(|id| !returned.contains(*id)) {
                genres::put(c, &ArtistGenres { artist_id: id.clone(), name: names.get(id).cloned(), genres: vec![] }, ts)?;
            }
            Ok(())
        })?;
        fetched += slice.len();
        let _ = app.emit("genre-progress", GenreProgress { done: fetched, total });
    }
    Ok((map, total))
}

pub async fn breakdown(app: &AppHandle, state: &AppState, playlist_id: &str) -> Result<GenreBreakdown> {
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

    let (map, artists_fetched) = genres_for(app, state, &ids, &names).await?;

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
        lookup_strategy: match STRATEGY.load(Ordering::Relaxed) {
            1 => "single",
            2 => "search",
            _ => "batch",
        }
        .into(),
    })
}
