//! Track search (`GET /search?type=track`). Development mode caps at 10 per page.

use serde::Deserialize;

use super::client::SpotifyClient;
use super::models::{Paging, Track};
use crate::error::Result;

#[derive(Debug, Deserialize)]
struct TrackSearchResponse {
    #[serde(default)]
    tracks: Option<Paging<Track>>,
}

pub async fn search_tracks(c: &SpotifyClient, query: &str, limit: u8) -> Result<Vec<Track>> {
    let resp: Option<TrackSearchResponse> = c
        .get(
            "/search",
            &[
                ("q", query.to_string()),
                ("type", "track".to_string()),
                ("limit", limit.min(10).to_string()),
            ],
        )
        .await?;
    Ok(resp.and_then(|r| r.tracks).map(|p| p.items).unwrap_or_default())
}
