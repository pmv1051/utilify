//! Tauri command handlers: the bridge between the React frontend and the Rust core.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::db::bench::BenchRow;
use crate::db::playlists::PlaylistRow;
use crate::db::randomizer::SessionRow;
use crate::db::{self, config};
use crate::error::{AppError, Result};
use crate::features::diff::{self, DiffResult};
use crate::features::discography::{self, AlbumInfo, ArtistHit, DiscographyResult};
use crate::features::duplicates::{self, DuplicateReport, RemovalRequest, RemovalSummary};
use crate::features::generated::{self, GeneratedPlaylist};
use crate::features::merge::{self, MergeResult};
use crate::features::randomizer::{self, RandomizeResult};
use crate::features::tracks::{self, TrackInfo};
use crate::features::{self, bench};
use crate::spotify::models::PlaybackState;
use crate::spotify::{auth, playback, playlists};
use crate::state::AppState;
use crate::tray;

const AUTH_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupState {
    pub has_client_id: bool,
    pub is_authenticated: bool,
    pub user_display_name: Option<String>,
    pub user_product: Option<String>,
    pub redirect_uri: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub client_id: Option<String>,
    pub minimize_to_tray: bool,
    pub redirect_uri: String,
    pub user_display_name: Option<String>,
    pub user_id: Option<String>,
    pub db_path: String,
}

fn setup_state(state: &AppState) -> Result<SetupState> {
    let (display_name, product) = state.db.with(|c| {
        Ok((
            config::get(c, config::USER_DISPLAY_NAME)?,
            config::get(c, config::USER_PRODUCT)?,
        ))
    })?;
    Ok(SetupState {
        has_client_id: state.spotify.client_id().is_ok(),
        is_authenticated: state.spotify.is_authenticated(),
        user_display_name: display_name,
        user_product: product,
        redirect_uri: auth::redirect_uri(),
    })
}

fn user_id(state: &AppState) -> Result<Option<String>> {
    state.db.with(|c| config::get(c, config::USER_ID))
}

// ---- setup & auth ----------------------------------------------------------

#[tauri::command]
pub fn get_setup_state(state: State<'_, AppState>) -> Result<SetupState> {
    setup_state(&state)
}

#[tauri::command]
pub fn save_client_id(state: State<'_, AppState>, client_id: String) -> Result<()> {
    let trimmed = client_id.trim();
    if trimmed.is_empty() {
        state.spotify.clear_tokens()?;
        state.db.with(|c| config::delete(c, config::CLIENT_ID))?;
        return Ok(());
    }
    if trimmed.len() != 32 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::other("A Spotify Client ID is 32 hexadecimal characters."));
    }
    let current = state.spotify.client_id().ok();
    if current.as_deref() != Some(trimmed) {
        // Tokens belong to the old app; they are useless with a new Client ID.
        state.spotify.clear_tokens()?;
    }
    state.db.with(|c| config::set(c, config::CLIENT_ID, trimmed))
}

/// Runs the full PKCE flow: opens the browser, waits for the loopback
/// callback, exchanges the code, stores tokens and the user's profile.
#[tauri::command]
pub async fn start_auth(app: AppHandle, state: State<'_, AppState>) -> Result<SetupState> {
    let client_id = state.spotify.client_id()?;
    let pkce = auth::generate_pkce();
    let url = auth::authorize_url(&client_id, &pkce)?;

    // Bind before opening the browser so the redirect can never race us.
    let listener = auth::CallbackListener::bind()?;
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| AppError::Auth(format!("Could not open your browser: {e}")))?;

    let expected_state = pkce.state.clone();
    let code = tokio::task::spawn_blocking(move || listener.wait_for_code(&expected_state, AUTH_TIMEOUT))
        .await
        .map_err(|e| AppError::other(format!("callback task failed: {e}")))??;

    let tokens = auth::exchange_code(state.spotify.http(), &client_id, &code, &pkce.verifier).await?;
    state.spotify.store_tokens(&tokens)?;

    let me = playlists::current_user(&state.spotify).await?;
    state.db.with(|c| {
        config::set(c, config::USER_ID, &me.id)?;
        config::set(c, config::USER_DISPLAY_NAME, me.display_name.as_deref().unwrap_or(&me.id))?;
        config::set(c, config::USER_PRODUCT, me.product.as_deref().unwrap_or(""))?;
        Ok(())
    })?;
    log::info!("connected to Spotify as {} ({:?})", me.id, me.product);

    state.poll_now.notify_one();
    tray::show_main_window(&app);
    setup_state(&state)
}

#[tauri::command]
pub fn disconnect(state: State<'_, AppState>) -> Result<()> {
    state.spotify.clear_tokens()?;
    state.set_last_playback(None);
    Ok(())
}

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(AppError::other("Only http(s) links can be opened."));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| AppError::other(format!("Could not open link: {e}")))
}

// ---- settings --------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings> {
    let (client_id, display_name, uid) = state.db.with(|c| {
        Ok((
            config::get(c, config::CLIENT_ID)?,
            config::get(c, config::USER_DISPLAY_NAME)?,
            config::get(c, config::USER_ID)?,
        ))
    })?;
    Ok(Settings {
        client_id: client_id.filter(|s| !s.is_empty()),
        minimize_to_tray: state.minimize_to_tray(),
        redirect_uri: auth::redirect_uri(),
        user_display_name: display_name,
        user_id: uid,
        db_path: state.db_path.display().to_string(),
    })
}

#[tauri::command]
pub fn set_minimize_to_tray(state: State<'_, AppState>, enabled: bool) -> Result<()> {
    state
        .db
        .with(|c| config::set(c, config::MINIMIZE_TO_TRAY, if enabled { "1" } else { "0" }))
}

// ---- playlists -------------------------------------------------------------

#[tauri::command]
pub async fn get_playlists(state: State<'_, AppState>, refresh: bool) -> Result<Vec<PlaylistRow>> {
    let me = user_id(&state)?;
    let cached = state.db.with(db::playlists::count)?;
    if refresh || cached == 0 {
        let list = playlists::list_user_playlists(&state.spotify).await?;
        features::cache_playlists(&state.db, &list)?;
        // Shadow playlists deleted in Spotify should stop showing as tracked.
        let ids: Vec<String> = list.iter().map(|p| p.id.clone()).collect();
        let retired = state.db.with(|c| db::randomizer::deactivate_missing(c, &ids))?;
        if retired > 0 {
            log::info!("retired {retired} randomizer session(s) whose shadow playlist is gone");
        }
    }
    state.db.with(|c| db::playlists::list(c, me.as_deref()))
}

// ---- randomizer ------------------------------------------------------------

#[tauri::command]
pub async fn randomize_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    start_playback: bool,
) -> Result<RandomizeResult> {
    randomizer::randomize(&state, &playlist_id, start_playback).await
}

#[tauri::command]
pub async fn reshuffle_session(state: State<'_, AppState>, shadow_playlist_id: String) -> Result<SessionRow> {
    let session = state
        .db
        .with(|c| db::randomizer::get_by_shadow(c, &shadow_playlist_id))?
        .ok_or_else(|| AppError::other("Unknown randomizer session."))?;
    // If the shadow playlist is playing right now, keep the current track at
    // position 0 so playback flows into the new order without skipping.
    let shadow_uri = randomizer::shadow_context_uri(&shadow_playlist_id);
    let pin = state
        .last_playback()
        .filter(|p| p.context_uri() == Some(shadow_uri.as_str()))
        .and_then(|p| p.track_uri().map(String::from));
    randomizer::reshuffle(&state, &session, pin.as_deref(), false).await
}

#[tauri::command]
pub fn get_randomizer_sessions(state: State<'_, AppState>) -> Result<Vec<SessionRow>> {
    state.db.with(db::randomizer::list_active)
}

#[tauri::command]
pub fn stop_randomizer_session(state: State<'_, AppState>, shadow_playlist_id: String) -> Result<()> {
    state
        .db
        .with(|c| db::randomizer::set_active(c, &shadow_playlist_id, false))
}

// ---- bench -----------------------------------------------------------------

/// Copyable tracks of a playlist, in order, with true API positions.
#[tauri::command]
pub async fn get_playlist_tracks(state: State<'_, AppState>, playlist_id: String) -> Result<Vec<TrackInfo>> {
    Ok(tracks::fetch_playlist(&state, &playlist_id)
        .await?
        .into_iter()
        .filter(|t| t.playable)
        .collect())
}

// ---- tools: duplicates -----------------------------------------------------

#[tauri::command]
pub async fn scan_duplicates(
    state: State<'_, AppState>,
    playlist_ids: Vec<String>,
    match_by_name: bool,
) -> Result<DuplicateReport> {
    duplicates::scan(&state, &playlist_ids, match_by_name).await
}

#[tauri::command]
pub async fn remove_duplicates(
    state: State<'_, AppState>,
    removals: Vec<RemovalRequest>,
) -> Result<RemovalSummary> {
    duplicates::apply_removals(&state, removals).await
}

// ---- tools: generic playlist actions ---------------------------------------

#[tauri::command]
pub async fn create_playlist_from_tracks(
    state: State<'_, AppState>,
    name: String,
    uris: Vec<String>,
    randomize: bool,
) -> Result<GeneratedPlaylist> {
    generated::create_from_uris(&state, &name, uris, randomize).await
}

#[tauri::command]
pub async fn add_tracks_to_playlist(state: State<'_, AppState>, playlist_id: String, uris: Vec<String>) -> Result<usize> {
    generated::add_tracks(&state, &playlist_id, &uris).await
}

#[tauri::command]
pub async fn remove_tracks_from_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    uris: Vec<String>,
) -> Result<usize> {
    generated::remove_tracks(&state, &playlist_id, &uris).await
}

// ---- tools: diff & merge ---------------------------------------------------

#[tauri::command]
pub async fn diff_playlists(
    state: State<'_, AppState>,
    a: String,
    b: String,
    match_by_name: bool,
) -> Result<DiffResult> {
    diff::diff(&state, &a, &b, match_by_name).await
}

// ---- tools: discography ----------------------------------------------------

#[tauri::command]
pub async fn search_artists(state: State<'_, AppState>, query: String) -> Result<Vec<ArtistHit>> {
    discography::search(&state, &query).await
}

#[tauri::command]
pub async fn get_artist_albums(
    state: State<'_, AppState>,
    artist_id: String,
    include_compilations: bool,
    include_appears_on: bool,
) -> Result<Vec<AlbumInfo>> {
    discography::albums(&state, &artist_id, include_compilations, include_appears_on).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn create_discography(
    state: State<'_, AppState>,
    artist_id: String,
    artist_name: String,
    album_ids: Vec<String>,
    name: String,
    only_this_artist: bool,
    dedupe_by_name: bool,
    randomize: bool,
) -> Result<DiscographyResult> {
    discography::build(
        &state,
        discography::BuildRequest {
            artist_id: &artist_id,
            artist_name: &artist_name,
            album_ids: &album_ids,
            name: &name,
            only_this_artist,
            dedupe_by_name,
            randomize,
        },
    )
    .await
}

#[tauri::command]
pub async fn merge_playlists(
    state: State<'_, AppState>,
    playlist_ids: Vec<String>,
    name: String,
    dedupe_by_name: bool,
    randomize: bool,
) -> Result<MergeResult> {
    merge::merge(&state, &playlist_ids, &name, dedupe_by_name, randomize).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn bench_track(
    state: State<'_, AppState>,
    playlist_id: String,
    track_uri: String,
    track_name: Option<String>,
    artist_name: Option<String>,
    position: Option<i64>,
    duration_secs: i64,
) -> Result<BenchRow> {
    bench::bench_track(
        &state,
        bench::BenchRequest {
            playlist_id: &playlist_id,
            track_uri: &track_uri,
            track_name: track_name.as_deref(),
            artist_name: artist_name.as_deref(),
            position,
            duration_secs,
        },
    )
    .await
}

#[tauri::command]
pub fn get_benched_tracks(state: State<'_, AppState>) -> Result<Vec<BenchRow>> {
    state.db.with(db::bench::list_active)
}

#[tauri::command]
pub async fn unbench_track(state: State<'_, AppState>, id: i64) -> Result<BenchRow> {
    bench::unbench(&state, id).await
}

// ---- playback --------------------------------------------------------------

/// Transport control for the Now Playing bar. `action` is one of `play`,
/// `pause`, `next`, `previous`, `seek` (value = ms), `shuffle` (value =
/// true/false), `repeat` (value = off/context/track). Returns the refreshed
/// playback state and broadcasts it like a poll would.
#[tauri::command]
pub async fn player_command(
    app: AppHandle,
    state: State<'_, AppState>,
    action: String,
    value: Option<String>,
) -> Result<Option<PlaybackState>> {
    let device = state
        .last_playback()
        .and_then(|p| p.device.and_then(|d| d.id));
    let dev = device.as_deref();
    let c = &state.spotify;
    match action.as_str() {
        "play" => playback::resume(c, dev).await?,
        "pause" => playback::pause(c, dev).await?,
        "next" => playback::next(c, dev).await?,
        "previous" => playback::previous(c, dev).await?,
        "seek" => {
            let ms = value
                .as_deref()
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(|| AppError::other("seek needs a position in ms"))?;
            playback::seek(c, ms, dev).await?
        }
        "shuffle" => playback::set_shuffle(c, value.as_deref() == Some("true"), dev).await?,
        "repeat" => {
            let mode = value.as_deref().unwrap_or("off");
            if !matches!(mode, "off" | "context" | "track") {
                return Err(AppError::other("repeat must be off, context or track"));
            }
            playback::set_repeat(c, mode, dev).await?
        }
        other => return Err(AppError::other(format!("unknown player action '{other}'"))),
    }
    // Spotify needs a moment before the state endpoint reflects the change.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let current = playback::get_playback_state(c).await?;
    state.set_last_playback(current.clone());
    let _ = tauri::Emitter::emit(&app, "playback-state", &current);
    Ok(current)
}

#[tauri::command]
pub async fn get_playback_state(state: State<'_, AppState>, refresh: bool) -> Result<Option<PlaybackState>> {
    if refresh && state.spotify.is_authenticated() {
        let current = playback::get_playback_state(&state.spotify).await?;
        state.set_last_playback(current.clone());
        Ok(current)
    } else {
        Ok(state.last_playback())
    }
}
