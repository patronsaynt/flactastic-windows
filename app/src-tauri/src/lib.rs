//! FLACtastic desktop shell (Tauri 2).

pub mod artists;
mod artwork;
pub mod commands;
pub mod dto;
pub mod home;
pub mod import;
pub mod metadata;
pub mod player_actor;
pub mod playlists;
pub mod state;

use std::sync::Arc;

use tauri::Manager;

use crate::state::AppState;

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol("flart", |ctx, req| {
            let st = ctx.app_handle().state::<Arc<AppState>>();
            let (status, mime, body) = artwork::respond(&st.artwork, &req.uri().to_string());
            tauri::http::Response::builder()
                .status(status)
                .header("Content-Type", mime)
                .header("Cache-Control", "max-age=31536000, immutable")
                .header("Access-Control-Allow-Origin", "*")
                .body(body)
                .unwrap()
        })
        .setup(|app| {
            let state = AppState::new(app.handle());
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_ready,
            commands::app_version,
            commands::get_settings,
            commands::set_setting,
            commands::open_library,
            commands::bootstrap_library,
            commands::create_default_music_folder,
            commands::refresh_library,
            commands::library_snapshot,
            commands::remove_tracks,
            commands::play_tracks,
            commands::play_next,
            commands::add_to_queue,
            commands::transport,
            commands::seek,
            commands::set_volume,
            commands::set_repeat,
            commands::jump_to,
            commands::remove_from_queue,
            commands::move_queue_track,
            commands::set_queue_visible,
            commands::player_snapshot,
            commands::set_spectrum,
            commands::output_status,
            commands::select_output_device,
            commands::select_output_sample_rate,
            commands::select_output_bit_depth,
            commands::set_exclusive_output,
            commands::artists,
            commands::artist_detail,
            commands::ensure_artist_image,
            commands::artist_override,
            commands::save_artist_override,
            commands::reset_artist_override,
            commands::load_image_file,
            commands::crop_image,
            home::home_metrics,
            import::import_load,
            import::import_commit,
            metadata::write_track_metadata,
            metadata::write_tracks_metadata,
            metadata::read_track_lyrics,
            metadata::write_track_lyrics,
            metadata::parse_lyrics_for_sync,
            metadata::serialize_lrc,
            metadata::read_track_markers,
            metadata::write_track_markers,
            metadata::parse_marker_timestamp,
            home::home_highlight,
            home::toggle_highlight_pin,
            playlists::playlists,
            playlists::create_playlist,
            playlists::delete_playlist,
            playlists::rename_playlist,
            playlists::update_playlist_metadata,
            playlists::playlist_duplicate_count,
            playlists::add_to_playlist,
            playlists::create_playlist_and_add,
            playlists::remove_playlist_entries,
            playlists::move_playlist_entry,
            playlists::record_playlist_play,
            playlists::record_album_play,
        ])
        .build(tauri::generate_context!())
        .expect("error while building FLACtastic")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(st) = app.try_state::<Arc<AppState>>() {
                    st.player.send(player_actor::Cmd::FlushPending);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    st.listening.lock().save();
                    st.save_settings();
                }
            }
        });
}
