//! User library endpoints.

use std::time::Duration;

use super::client::SpotifyClient;
use crate::error::Result;

const CONTAINS_BATCH: usize = 50;

/// `GET /me/tracks/contains` for up to any number of track ids (batched by
/// 50). Result aligns with `ids`.
pub async fn contains_saved_tracks(c: &SpotifyClient, ids: &[String]) -> Result<Vec<bool>> {
    let mut out = Vec::with_capacity(ids.len());
    for (i, chunk) in ids.chunks(CONTAINS_BATCH).enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
        let flags: Vec<bool> = c
            .get("/me/tracks/contains", &[("ids", chunk.join(","))])
            .await?
            .unwrap_or_default();
        if flags.len() == chunk.len() {
            out.extend(flags);
        } else {
            // Unexpected shape: treat as unknown rather than fail the whole load.
            log::warn!("contains_saved_tracks: got {} flags for {} ids", flags.len(), chunk.len());
            out.extend(std::iter::repeat_n(false, chunk.len()));
        }
    }
    Ok(out)
}
