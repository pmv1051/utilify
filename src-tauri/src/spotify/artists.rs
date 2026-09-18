//! Artist search, artist albums and album tracks.

use serde::{Deserialize, Serialize};

use super::client::SpotifyClient;
use super::models::{Artist, Image, Paging, Track};
use crate::error::Result;

#[derive(Debug, Clone, Deserialize)]
pub struct Followers {
    #[serde(default)]
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct ArtistObject {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub images: Vec<Image>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default, skip_serializing)]
    pub followers: Option<Followers>,
}

#[derive(Debug, Deserialize)]
struct SeveralArtistsResponse {
    #[serde(default)]
    artists: Vec<Option<ArtistObject>>,
}

/// Ids per `GET /artists?ids=` request. Documented max is 50; kept lower in
/// case the 2026 API caps it like it caps `limit`.
const ARTISTS_BATCH: usize = 20;

/// `GET /artists?ids=…` for any number of ids (batched). Unknown ids are dropped.
pub async fn get_artists(c: &SpotifyClient, ids: &[String]) -> Result<Vec<ArtistObject>> {
    let mut out = Vec::with_capacity(ids.len());
    for (i, chunk) in ids.chunks(ARTISTS_BATCH).enumerate() {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
        let resp: Option<SeveralArtistsResponse> = c.get("/artists", &[("ids", chunk.join(","))]).await?;
        if let Some(r) = resp {
            out.extend(r.artists.into_iter().flatten());
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
struct ArtistSearchResponse {
    #[serde(default)]
    artists: Option<Paging<ArtistObject>>,
}

/// `GET /search?type=artist`. Development mode caps results at 10.
pub async fn search_artists(c: &SpotifyClient, query: &str) -> Result<Vec<ArtistObject>> {
    let resp: Option<ArtistSearchResponse> = c
        .get(
            "/search",
            &[
                ("q", query.to_string()),
                ("type", "artist".to_string()),
                ("limit", "10".to_string()),
            ],
        )
        .await?;
    Ok(resp.and_then(|r| r.artists).map(|p| p.items).unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct SimplifiedAlbum {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub album_type: Option<String>,
    #[serde(default)]
    pub album_group: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub total_tracks: Option<u32>,
    #[serde(default)]
    pub images: Vec<Image>,
    #[serde(default)]
    pub artists: Vec<Artist>,
}

/// Per-page size for artist/album listings. The Feb 2026 API rejected 50 and
/// 20 with "Invalid limit" on live runs; 10 is the user-confirmed value.
/// `get_all_pages` still adapts downwards if this is ever rejected.
const PAGE_LIMIT: &str = "10";

/// `GET /artists/{id}/albums`. `include_groups` is a comma list of
/// `album`, `single`, `compilation`, `appears_on`. `market=from_token`
/// collapses the per-market duplicates Spotify otherwise returns.
pub async fn artist_albums(c: &SpotifyClient, artist_id: &str, include_groups: &str) -> Result<Vec<SimplifiedAlbum>> {
    c.get_all_pages(
        &format!("/artists/{artist_id}/albums"),
        &[
            ("include_groups", include_groups.to_string()),
            ("market", "from_token".to_string()),
            ("limit", PAGE_LIMIT.to_string()),
        ],
    )
    .await
}

/// `GET /albums/{id}/tracks`. Simplified tracks carry no album object.
pub async fn album_tracks(c: &SpotifyClient, album_id: &str) -> Result<Vec<Track>> {
    c.get_all_pages(
        &format!("/albums/{album_id}/tracks"),
        &[("market", "from_token".to_string()), ("limit", PAGE_LIMIT.to_string())],
    )
    .await
}
