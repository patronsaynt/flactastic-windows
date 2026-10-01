//! `SpotifyAuthController`: the user's Spotify login (Authorization Code +
//! PKCE) for listing and downloading their own playlists. The login page
//! opens in the system browser and returns via `flactastic://spotify-callback`
//! (deep link, handed to the running instance). Tokens live in the OS
//! keyring, never in settings files.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fl_net::remote::RemotePlaylist;
use fl_net::spotify::{self, PlaylistSummary, SpotifyError, TokenResponse};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

const SECRET: &str = "spotify.session";
/// A login left unanswered in the browser stops showing "Connecting…".
const CONNECT_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Connection {
    Disconnected,
    Connecting,
    #[serde(rename_all = "camelCase")]
    Connected { display_name: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyState {
    pub connection: Connection,
    pub playlists: Vec<PlaylistSummary>,
    /// A load failed and nothing is cached to show.
    pub playlists_error: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    refresh: Option<String>,
    access: Option<String>,
    /// Unix seconds.
    expiry: Option<f64>,
    name: Option<String>,
}

struct Pending {
    state: String,
    verifier: String,
    started: std::time::Instant,
}

struct Inner {
    connection: Connection,
    playlists: Vec<PlaylistSummary>,
    playlists_error: Option<String>,
    stored: Stored,
    pending: Option<Pending>,
}

pub struct SpotifyAuth {
    app: AppHandle,
    inner: Mutex<Inner>,
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn load_stored() -> Stored {
    fl_platform::secrets::get(SECRET)
        .ok()
        .flatten()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

impl SpotifyAuth {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(SpotifyAuth {
            app,
            inner: Mutex::new(Inner {
                connection: Connection::Disconnected,
                playlists: vec![],
                playlists_error: None,
                stored: Stored::default(),
                pending: None,
            }),
        })
    }

    pub fn state(&self) -> SpotifyState {
        let i = self.inner.lock();
        SpotifyState {
            connection: i.connection.clone(),
            playlists: i.playlists.clone(),
            playlists_error: i.playlists_error.clone(),
        }
    }

    fn emit(&self) {
        let _ = self.app.emit("spotify://changed", self.state());
    }

    fn set_connection(&self, c: Connection) {
        self.inner.lock().connection = c;
        self.emit();
    }

    fn persist(&self) {
        let bytes = serde_json::to_vec(&self.inner.lock().stored).unwrap_or_default();
        if let Err(e) = fl_platform::secrets::set(SECRET, &bytes) {
            log::warn!("[Spotify] couldn't save the session to the keyring: {e}");
        }
    }

    /// At launch: with a stored refresh token, show connected (cached name)
    /// at once, then verify via `/v1/me`.
    pub fn restore(self: &Arc<Self>) {
        let me = self.clone();
        std::thread::spawn(move || {
            let stored = load_stored();
            if stored.refresh.is_none() {
                return;
            }
            let cached = stored.name.clone();
            {
                let mut i = me.inner.lock();
                i.stored = stored;
                i.connection = match &cached {
                    Some(n) => Connection::Connected { display_name: n.clone() },
                    None => Connection::Connecting,
                };
            }
            me.emit();
            let verified = me.valid_access_token().and_then(|t| spotify::display_name(&t).map_err(|e| e.to_string()));
            match verified {
                Ok(name) => {
                    me.inner.lock().stored.name = Some(name.clone());
                    me.persist();
                    me.set_connection(Connection::Connected { display_name: name });
                    me.load_playlists();
                }
                // Offline or expired: keep showing a cached connection so a
                // later action can retry.
                Err(_) if cached.is_some() => {}
                Err(_) => me.set_connection(Connection::Disconnected),
            }
        });
    }

    /// Opens Spotify's consent page in the browser; the deep-link callback
    /// finishes the login.
    pub fn connect(self: &Arc<Self>) -> Result<(), String> {
        #[cfg(any(windows, target_os = "linux"))]
        {
            use tauri_plugin_deep_link::DeepLinkExt;
            // Installed builds register the scheme at install time; dev runs
            // (and portable ones) need it pointed at this executable.
            if !self.app.deep_link().is_registered("flactastic").unwrap_or(false) || cfg!(debug_assertions) {
                if let Err(e) = self.app.deep_link().register("flactastic") {
                    log::warn!("[Spotify] couldn't register flactastic:// links: {e}");
                }
            }
        }
        let (verifier, challenge) = spotify::pkce_pair();
        let state = fl_core::Uid::new_v4().to_string().to_uppercase();
        let url = spotify::authorize_url(&challenge, &state);
        {
            let mut i = self.inner.lock();
            i.pending = Some(Pending { state, verifier, started: std::time::Instant::now() });
            i.connection = Connection::Connecting;
        }
        self.emit();
        if let Err(e) = tauri_plugin_opener::open_url(&url, None::<&str>) {
            self.inner.lock().pending = None;
            self.set_connection(Connection::Disconnected);
            return Err(format!("Couldn't start the Spotify login window. ({e})"));
        }
        let me = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(CONNECT_TIMEOUT);
            let expired = {
                let mut i = me.inner.lock();
                let stale = i.pending.as_ref().is_some_and(|p| p.started.elapsed() >= CONNECT_TIMEOUT);
                if stale {
                    i.pending = None;
                }
                stale && i.connection == Connection::Connecting
            };
            if expired {
                me.set_connection(Connection::Disconnected);
            }
        });
        Ok(())
    }

    /// Abandons a login waiting on the browser.
    pub fn cancel_connect(&self) {
        let was = {
            let mut i = self.inner.lock();
            i.pending.take().is_some() && i.connection == Connection::Connecting
        };
        if was {
            self.set_connection(Connection::Disconnected);
        }
    }

    /// Handles any `flactastic://spotify-callback?...` URL delivered to the app.
    pub fn handle_url(self: &Arc<Self>, url: &str) {
        if !url.starts_with("flactastic://spotify-callback") {
            return;
        }
        let Some(pending) = self.inner.lock().pending.take() else { return };
        let me = self.clone();
        let url = url.to_owned();
        std::thread::spawn(move || {
            let result = (|| -> Result<String, String> {
                let code = spotify::query_param(&url, "code")
                    .ok_or_else(|| "Spotify didn't return an authorization code.".to_owned())?;
                if spotify::query_param(&url, "state").as_deref() != Some(pending.state.as_str()) {
                    return Err("Login state mismatch — please try again.".into());
                }
                let token = spotify::exchange_code(&code, &pending.verifier).map_err(|e| e.to_string())?;
                me.apply_tokens(token);
                let name = spotify::display_name(&me.valid_access_token()?).map_err(|e| e.to_string())?;
                me.inner.lock().stored.name = Some(name.clone());
                me.persist();
                Ok(name)
            })();
            match result {
                Ok(name) => {
                    me.set_connection(Connection::Connected { display_name: name });
                    me.load_playlists();
                }
                Err(e) => {
                    log::warn!("[Spotify] connect failed: {e}");
                    me.set_connection(Connection::Disconnected);
                }
            }
        });
    }

    pub fn disconnect(&self) {
        {
            let mut i = self.inner.lock();
            i.stored = Stored::default();
            i.playlists.clear();
            i.playlists_error = None;
            i.connection = Connection::Disconnected;
        }
        let _ = fl_platform::secrets::delete(SECRET);
        self.emit();
    }

    fn apply_tokens(&self, t: TokenResponse) {
        {
            let mut i = self.inner.lock();
            i.stored.access = Some(t.access_token);
            if let Some(r) = t.refresh_token {
                i.stored.refresh = Some(r);
            }
            i.stored.expiry = Some(now() + t.expires_in.unwrap_or(3600) as f64);
        }
        self.persist();
    }

    /// A bearer token, refreshed when within a minute of expiry. A rejected
    /// refresh token forces a reconnect.
    pub fn valid_access_token(&self) -> Result<String, String> {
        let (refresh, access, expiry) = {
            let i = self.inner.lock();
            (i.stored.refresh.clone(), i.stored.access.clone(), i.stored.expiry)
        };
        let Some(refresh) = refresh else { return Err("Connect your Spotify account first.".into()) };
        if let (Some(a), Some(e)) = (access, expiry) {
            if e - now() > 60.0 {
                return Ok(a);
            }
        }
        match spotify::refresh(&refresh) {
            Ok(t) => {
                let access = t.access_token.clone();
                self.apply_tokens(t);
                Ok(access)
            }
            Err(_) => {
                self.disconnect();
                Err("Your Spotify session expired. Reconnect to continue.".into())
            }
        }
    }

    pub fn load_playlists(&self) {
        let result = self.valid_access_token().and_then(|t| match spotify::my_playlists(&t) {
            Ok(p) => Ok(p),
            Err(SpotifyError::AuthFailed(_)) => {
                self.disconnect();
                Err("Your Spotify session expired. Reconnect to continue.".into())
            }
            Err(e) => Err(e.to_string()),
        });
        {
            let mut i = self.inner.lock();
            match result {
                Ok(p) => {
                    i.playlists = p;
                    i.playlists_error = None;
                }
                // Keep whatever we already have.
                Err(e) => i.playlists_error = i.playlists.is_empty().then_some(e),
            }
        }
        self.emit();
    }
}

// MARK: - Commands

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPlaylist {
    pub playlist: RemotePlaylist,
    pub was_truncated: bool,
}

type Auth<'a> = State<'a, Arc<SpotifyAuth>>;

#[tauri::command]
pub fn spotify_state(auth: Auth) -> SpotifyState {
    auth.state()
}

#[tauri::command]
pub fn spotify_connect(auth: Auth) -> Result<(), String> {
    auth.connect()
}

#[tauri::command]
pub fn spotify_cancel_connect(auth: Auth) {
    auth.cancel_connect();
}

#[tauri::command]
pub fn spotify_disconnect(auth: Auth) {
    auth.disconnect();
}

#[tauri::command]
pub async fn spotify_load_playlists(auth: Auth<'_>) -> Result<(), String> {
    let a = auth.inner().clone();
    tauri::async_runtime::spawn_blocking(move || a.load_playlists()).await.map_err(|e| e.to_string())
}

/// A playlist by link: the full tracklist with the user's token when `authed`,
/// else the no-auth embed preview (capped at 100). `liked` resolves Liked Songs.
#[tauri::command]
pub async fn spotify_resolve_playlist(
    auth: Auth<'_>,
    url: String,
    authed: bool,
    liked: bool,
) -> Result<ResolvedPlaylist, String> {
    let a = auth.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = if authed || liked {
            let token = a.valid_access_token()?;
            if liked {
                spotify::resolve_liked_songs(&token)
            } else {
                spotify::resolve_api(&url, &token)
            }
        } else {
            spotify::resolve_embed(&url)
        };
        r.map(|r| ResolvedPlaylist { playlist: r.playlist, was_truncated: r.was_truncated }).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
