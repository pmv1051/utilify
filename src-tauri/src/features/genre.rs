//! Genre Filtering (light): artist-level genre tags applied to the tracks of
//! a playlist. A track carries the union of its credited artists' genres.
//! Genre tags are cached per artist for 30 days.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::genres::{self, ArtistGenres};
use crate::db::now;
use crate::error::Result;
use crate::features::tracks::{fetch_playlist, playlist_display_name, TrackInfo};
use crate::spotify::artists;
use crate::state::AppState;

const GENRE_CACHE_SECS: i64 = 30 * 24 * 3600;

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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenreProgress {
    pub done: usize,
    pub total: usize,
}

/// Genres for the given artist ids, from cache where fresh, fetched otherwise.
async fn genres_for(app: &AppHandle, state: &AppState, ids: &[String]) -> Result<(HashMap<String, Vec<String>>, usize)> {
    let cutoff = now() - GENRE_CACHE_SECS;
    let cached = state.db.with(|c| genres::get_many(c, ids, cutoff))?;
    let mut map: HashMap<String, Vec<String>> = cached.into_iter().map(|(id, a)| (id, a.genres)).collect();

    let missing: Vec<String> = ids.iter().filter(|id| !map.contains_key(*id)).cloned().collect();
    let total = missing.len();
    let mut fetched = 0usize;
    // Fetch in slices so progress is visible on big playlists.
    for slice in missing.chunks(60) {
        let list = artists::get_artists(&state.spotify, slice).await?;
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
                genres::put(c, &ArtistGenres { artist_id: id.clone(), name: None, genres: vec![] }, ts)?;
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
    let mut ids: Vec<String> = tracks.iter().flat_map(|t| t.artist_ids.iter().cloned()).collect();
    ids.sort();
    ids.dedup();
    let artists_total = ids.len();

    let (map, artists_fetched) = genres_for(app, state, &ids).await?;

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
    })
}
