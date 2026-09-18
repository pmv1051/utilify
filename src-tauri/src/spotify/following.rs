//! `GET /me/following?type=artist` (cursor-paged, unlike offset listings).

use serde::Deserialize;

use super::artists::ArtistObject;
use super::client::SpotifyClient;
use crate::error::{AppError, Result};

#[derive(Debug, Deserialize)]
struct CursorPage {
    #[serde(default)]
    items: Vec<ArtistObject>,
    #[serde(default)]
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FollowingResponse {
    #[serde(default)]
    artists: Option<CursorPage>,
}

pub async fn followed_artists(c: &SpotifyClient) -> Result<Vec<ArtistObject>> {
    let mut query = vec![("type", "artist".to_string()), ("limit", "20".to_string())];
    let mut first: Option<FollowingResponse> = match c.get("/me/following", &query).await {
        Err(AppError::Spotify { status: 400, message }) if message.to_lowercase().contains("limit") => {
            query.pop();
            c.get("/me/following", &query).await?
        }
        other => other?,
    };
    let mut out = Vec::new();
    let mut pages = 0;
    loop {
        let Some(page) = first.take().and_then(|r| r.artists) else { break };
        out.extend(page.items);
        pages += 1;
        match page.next {
            Some(next) => {
                if pages % 5 == 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                first = c.get(&next, &[]).await?;
            }
            None => break,
        }
    }
    Ok(out)
}
