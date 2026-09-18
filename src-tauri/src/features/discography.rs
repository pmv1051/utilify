//! Artist Discography: search an artist, list their releases, build a
//! deduplicated playlist from the chosen releases.

use std::collections::HashSet;
use std::time::Duration;

use serde::Serialize;

use crate::error::{AppError, Result};
use crate::features::generated::{self, GeneratedPlaylist};
use crate::features::matching::name_key;
use crate::spotify::artists;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistHit {
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
    pub genres: Vec<String>,
    pub followers: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumInfo {
    pub id: String,
    pub name: String,
    /// `album`, `single`, `compilation`, or `appears_on`.
    pub group: String,
    pub release_date: Option<String>,
    pub total_tracks: u32,
    pub image_url: Option<String>,
    pub artists: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscographyResult {
    pub playlist: GeneratedPlaylist,
    pub albums: usize,
    pub tracks_seen: usize,
    pub duplicates_skipped: usize,
    pub other_artist_skipped: usize,
}

pub async fn search(state: &AppState, query: &str) -> Result<Vec<ArtistHit>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    Ok(artists::search_artists(&state.spotify, q)
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

pub async fn albums(
    state: &AppState,
    artist_id: &str,
    include_compilations: bool,
    include_appears_on: bool,
) -> Result<Vec<AlbumInfo>> {
    let mut groups = vec!["album", "single"];
    if include_compilations {
        groups.push("compilation");
    }
    if include_appears_on {
        groups.push("appears_on");
    }
    let mut out: Vec<AlbumInfo> = artists::artist_albums(&state.spotify, artist_id, &groups.join(","))
        .await?
        .into_iter()
        .map(|a| AlbumInfo {
            group: a
                .album_group
                .clone()
                .or_else(|| a.album_type.clone())
                .unwrap_or_else(|| "album".into()),
            image_url: a.images.iter().min_by_key(|i| i.width.unwrap_or(u32::MAX)).map(|i| i.url.clone()),
            artists: a.artists.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", "),
            total_tracks: a.total_tracks.unwrap_or(0),
            release_date: a.release_date,
            id: a.id,
            name: a.name,
        })
        .collect();
    // Oldest first so the playlist reads chronologically.
    out.sort_by(|x, y| x.release_date.cmp(&y.release_date).then(x.name.cmp(&y.name)));
    Ok(out)
}

pub struct BuildRequest<'a> {
    pub artist_id: &'a str,
    pub artist_name: &'a str,
    pub album_ids: &'a [String],
    pub name: &'a str,
    pub only_this_artist: bool,
    pub dedupe_by_name: bool,
    pub randomize: bool,
}

pub async fn build(state: &AppState, req: BuildRequest<'_>) -> Result<DiscographyResult> {
    if req.album_ids.is_empty() {
        return Err(AppError::other("Pick at least one release."));
    }
    let description = if req.name.trim().is_empty() {
        format!("{} discography", req.artist_name)
    } else {
        req.name.trim().to_string()
    };

    let mut seen_uri: HashSet<String> = HashSet::new();
    let mut seen_name: HashSet<String> = HashSet::new();
    let mut uris = Vec::new();
    let mut tracks_seen = 0usize;
    let mut duplicates = 0usize;
    let mut other_artist = 0usize;

    for (i, album_id) in req.album_ids.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        for t in artists::album_tracks(&state.spotify, album_id).await? {
            if !t.is_playable_catalog_track() {
                continue;
            }
            tracks_seen += 1;
            if req.only_this_artist && !t.artists.iter().any(|a| a.id.as_deref() == Some(req.artist_id)) {
                other_artist += 1;
                continue;
            }
            let Some(uri) = t.uri.clone() else { continue };
            if !seen_uri.insert(uri.clone()) {
                duplicates += 1;
                continue;
            }
            if req.dedupe_by_name && !seen_name.insert(name_key(&t.name, &t.artist_names())) {
                duplicates += 1;
                continue;
            }
            uris.push(uri);
        }
    }

    let playlist = generated::create_from_uris(state, &description, uris, req.randomize).await?;
    Ok(DiscographyResult {
        playlist,
        albums: req.album_ids.len(),
        tracks_seen,
        duplicates_skipped: duplicates,
        other_artist_skipped: other_artist,
    })
}
