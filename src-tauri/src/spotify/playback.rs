//! Player endpoints. All of these require Spotify Premium.

use serde_json::json;

use super::client::SpotifyClient;
use super::models::{Device, DevicesResponse, PlaybackState};
use crate::error::{AppError, Result};

/// `GET /me/player`. Returns `None` when nothing is playing (204).
pub async fn get_playback_state(c: &SpotifyClient) -> Result<Option<PlaybackState>> {
    c.get("/me/player", &[("additional_types", "track,episode".into())])
        .await
}

pub async fn get_devices(c: &SpotifyClient) -> Result<Vec<Device>> {
    Ok(c.get::<DevicesResponse>("/me/player/devices", &[])
        .await?
        .map(|d| d.devices)
        .unwrap_or_default())
}

pub async fn set_shuffle(c: &SpotifyClient, enabled: bool, device_id: Option<&str>) -> Result<()> {
    let mut query = vec![("state", enabled.to_string())];
    if let Some(d) = device_id {
        query.push(("device_id", d.to_string()));
    }
    c.put("/me/player/shuffle", &query, None).await
}

pub async fn start_playback(
    c: &SpotifyClient,
    context_uri: &str,
    position: usize,
    device_id: Option<&str>,
) -> Result<()> {
    let mut query = Vec::new();
    if let Some(d) = device_id {
        query.push(("device_id", d.to_string()));
    }
    let body = json!({ "context_uri": context_uri, "offset": { "position": position } });
    c.put("/me/player/play", &query, Some(&body)).await
}

fn device_query(device_id: Option<&str>) -> Vec<(&'static str, String)> {
    device_id.map(|d| vec![("device_id", d.to_string())]).unwrap_or_default()
}

pub async fn resume(c: &SpotifyClient, device_id: Option<&str>) -> Result<()> {
    c.put("/me/player/play", &device_query(device_id), None).await
}

pub async fn pause(c: &SpotifyClient, device_id: Option<&str>) -> Result<()> {
    c.put("/me/player/pause", &device_query(device_id), None).await
}

pub async fn next(c: &SpotifyClient, device_id: Option<&str>) -> Result<()> {
    c.post_empty("/me/player/next", &device_query(device_id)).await
}

pub async fn previous(c: &SpotifyClient, device_id: Option<&str>) -> Result<()> {
    c.post_empty("/me/player/previous", &device_query(device_id)).await
}

pub async fn seek(c: &SpotifyClient, position_ms: u64, device_id: Option<&str>) -> Result<()> {
    let mut query = device_query(device_id);
    query.push(("position_ms", position_ms.to_string()));
    c.put("/me/player/seek", &query, None).await
}

/// `POST /me/player/queue?uri=` — needs an active device.
pub async fn add_to_queue(c: &SpotifyClient, uri: &str, device_id: Option<&str>) -> Result<()> {
    let mut query = device_query(device_id);
    query.push(("uri", uri.to_string()));
    c.post_empty("/me/player/queue", &query).await
}

/// `state` is one of `off`, `context`, `track`.
pub async fn set_repeat(c: &SpotifyClient, state: &str, device_id: Option<&str>) -> Result<()> {
    let mut query = device_query(device_id);
    query.push(("state", state.to_string()));
    c.put("/me/player/repeat", &query, None).await
}

/// Play a playlist from the top with shuffle off, targeting the active device
/// or, failing that, the first available one.
pub async fn play_playlist_unshuffled(c: &SpotifyClient, playlist_id: &str) -> Result<()> {
    let devices = get_devices(c).await?;
    let active = devices.iter().find(|d| d.is_active && !d.is_restricted);
    let fallback = devices.iter().find(|d| !d.is_restricted && d.id.is_some());
    let context = format!("spotify:playlist:{playlist_id}");

    match (active, fallback) {
        (Some(dev), _) => {
            let id = dev.id.as_deref();
            set_shuffle(c, false, id).await?;
            start_playback(c, &context, 0, id).await?;
        }
        (None, Some(dev)) => {
            // Inactive device: starting playback there activates it; shuffle
            // can only be set once it is active.
            let id = dev.id.as_deref();
            start_playback(c, &context, 0, id).await?;
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            set_shuffle(c, false, id).await?;
        }
        (None, None) => return Err(AppError::NoActiveDevice),
    }
    Ok(())
}
