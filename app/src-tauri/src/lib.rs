//! FLACtastic desktop shell (Tauri 2).

pub mod artists;
mod artwork;
pub mod commands;
pub mod dto;
pub mod downloads;
pub mod home;
pub mod import;
pub mod lucida;
pub mod lyrics;
pub mod media_controls;
pub mod organizer;
pub mod visualizer;
pub mod metadata;
pub mod player_actor;
pub mod playlists;
pub mod presence;
pub mod rebuild;
pub mod spotify_auth;
pub mod state;

use std::sync::Arc;

use tauri::Manager;

use crate::state::AppState;

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // A second launch carrying a flactastic:// link (Spotify login).
            if let Some(auth) = app.try_state::<Arc<spotify_auth::SpotifyAuth>>() {
                for a in &args {
                    auth.handle_url(a);
                }
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
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
            let lucida_dir = state.dirs.data.join("lucida-webview");
            app.manage(state);
            app.manage(lucida::Lucida::new(app.handle().clone(), lucida_dir));
            app.manage(downloads::Downloads::new(app.handle().clone()));
            app.manage(rebuild::Rebuild::new(app.handle().clone()));
            let auth = spotify_auth::SpotifyAuth::new(app.handle().clone());
            app.manage(auth.clone());
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let a = auth.clone();
                app.deep_link().on_open_url(move |e| {
                    for u in e.urls() {
                        a.handle_url(u.as_str());
                    }
                });
            }
            auth.restore();
            presence::spawn(app.handle().clone());
            media_controls::spawn(app.handle().clone());
            // The hidden Lucida window must not keep the app alive.
            if let Some(main) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                main.on_window_event(move |e| {
                    if let tauri::WindowEvent::Destroyed = e {
                        handle.exit(0);
                    }
                });
            }
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
            lyrics::visualizer_lyrics,
            organizer::organizer_plan,
            organizer::organizer_apply,
            organizer::organizer_examples,
            organizer::organizer_tokens,
            organizer::organizer_presets,
            organizer::organizer_new_level,
            organizer::organizer_duplicate,
            visualizer::visualizer_backdrop,
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
            lucida::lucida_state,
            lucida::lucida_log,
            lucida::lucida_clear_log,
            lucida::lucida_warm_up,
            lucida::lucida_reload,
            lucida::lucida_clear_site_data,
            lucida::lucida_show_webview,
            lucida::lucida_reveal_challenge,
            lucida::lucida_dismiss_challenge,
            lucida::download_resolve,
            lucida::lucida_debug_eval,
            downloads::download_jobs,
            downloads::download_enqueue,
            downloads::download_cancel,
            downloads::download_cancel_all,
            downloads::download_clear_completed,
            downloads::remote_artwork,
            rebuild::rebuild_state,
            rebuild::rebuild_start,
            rebuild::rebuild_cancel,
            rebuild::rebuild_dismiss,
            spotify_auth::spotify_state,
            spotify_auth::spotify_connect,
            spotify_auth::spotify_cancel_connect,
            spotify_auth::spotify_disconnect,
            spotify_auth::spotify_load_playlists,
            spotify_auth::spotify_resolve_playlist,
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
