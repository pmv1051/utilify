mod commands;
mod db;
mod error;
mod features;
mod polling;
mod spotify;
mod state;
mod tray;

use tauri::{Manager, WindowEvent};

use crate::db::Db;
use crate::spotify::client::SpotifyClient;
use crate::state::AppState;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .level_for("utilify_lib", log::LevelFilter::Debug)
                .build(),
        )
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("utilify.db");
            log::info!("opening database at {}", db_path.display());
            let db = Db::open(&db_path)?;
            let spotify = SpotifyClient::new(db.clone());
            app.manage(AppState::new(db, spotify, db_path));
            features::stats::close_stale(app.state::<AppState>().inner());

            tray::setup(app.handle())?;
            polling::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.minimize_to_tray() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_setup_state,
            commands::save_client_id,
            commands::start_auth,
            commands::disconnect,
            commands::open_external,
            commands::get_settings,
            commands::set_minimize_to_tray,
            commands::get_playlists,
            commands::randomize_playlist,
            commands::reshuffle_session,
            commands::get_randomizer_sessions,
            commands::stop_randomizer_session,
            commands::get_playback_state,
            commands::player_command,
            commands::get_playlist_tracks,
            commands::bench_track,
            commands::get_benched_tracks,
            commands::unbench_track,
            commands::scan_duplicates,
            commands::remove_duplicates,
            commands::create_playlist_from_tracks,
            commands::add_tracks_to_playlist,
            commands::remove_tracks_from_playlist,
            commands::diff_playlists,
            commands::merge_playlists,
            commands::search_artists,
            commands::get_artist_albums,
            commands::create_discography,
            commands::load_playlist_for_editor,
            commands::apply_playlist_order,
            commands::export_playlist,
            commands::save_text_file,
            commands::import_search,
            commands::set_play_threshold,
            commands::get_discovery_status,
            commands::rebuild_library_index,
            commands::list_followed_artists,
            commands::index_discovery_artists,
            commands::add_seed_playlist,
            commands::remove_discovery_source,
            commands::discover,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Utilify");
}
