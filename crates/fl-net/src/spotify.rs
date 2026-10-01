//! Spotify: `SpotifyPlaylistService` (playlist → `RemotePlaylist`, via the Web
//! API with a user token or the no-auth embed preview) and the token side of
//! `SpotifyAuthController` (Authorization Code + PKCE, no client secret).

use std::time::Duration;

use base64::Engine;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::remote::*;

/// FLACtastic's Spotify app (public PKCE client; not a secret).
pub const CLIENT_ID: &str = "d4b4f9bd568c45a1a89c597a2792f53a";
pub const REDIRECT_URI: &str = "flactastic://spotify-callback";
pub const SCOPES: &str = "playlist-read-private playlist-read-collaborative user-library-read";
/// The embed preview caps at this many entries.
pub const TRACK_CAP: usize = 100;
pub const LIKED_SONGS_URL: &str = "https://open.spotify.com/collection/tracks";

const EMBED_UA: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

#[derive(Debug, thiserror::Error)]
pub enum SpotifyError {
    #[error("That doesn't look like a Spotify playlist link.")]
    NotASpotifyPlaylist,
    #[error("Couldn't reach Spotify (HTTP {0}).")]
    FetchFailed(i32),
    #[error("Couldn't read the playlist from Spotify. It may be private.")]
    ParseFailed,
    #[error("Spotify rejected the request (HTTP {0}). Try reconnecting your Spotify account in Settings.")]
    AuthFailed(u16),
    #[error("Spotify refused the request (HTTP {code}): {message}")]
    Api { code: u16, message: String },
    #[error("{0}")]
    Network(String),
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub playlist: RemotePlaylist,
    /// The tracklist hit the embed preview's 100-entry cap.
    pub was_truncated: bool,
}

fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .build()
            .into()
    })
}

fn get(url: &str, query: &[(&str, &str)], headers: &[(&str, &str)]) -> Result<(u16, String), SpotifyError> {
    let mut req = agent().get(url);
    for (k, v) in query {
        req = req.query(*k, *v);
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut resp = req.call().map_err(|e| SpotifyError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    let body = resp
        .body_mut()
        .with_config()
        .limit(32 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| SpotifyError::Network(e.to_string()))?;
    Ok((status, body))
}

// MARK: - URLs

/// `(scheme, host, path)` of an absolute URL.
pub fn split_url(s: &str) -> Option<(String, String, String)> {
    let (scheme, rest) = s.trim().split_once("://")?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let host = authority.rsplit('@').next().unwrap_or(authority).split(':').next().unwrap_or("");
    if scheme.is_empty() || host.is_empty() {
        return None;
    }
    Some((scheme.to_ascii_lowercase(), host.to_ascii_lowercase(), format!("/{path}")))
}

/// The playlist id from an https URL or a `spotify:playlist:<id>` URI.
pub fn playlist_id(url: &str) -> Option<String> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("spotify:") {
        let parts: Vec<&str> = rest.split(':').collect();
        return (parts.len() == 2 && parts[0] == "playlist" && !parts[1].is_empty()).then(|| parts[1].to_owned());
    }
    let (_, host, path) = split_url(url)?;
    if host != "open.spotify.com" && host != "spotify.com" {
        return None;
    }
    let comps: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    let i = comps.iter().position(|c| *c == "playlist")?;
    comps.get(i + 1).map(|s| (*s).to_owned())
}

fn track_url_from_uri(uri: &str) -> Option<String> {
    let parts: Vec<&str> = uri.split(':').collect();
    (parts.len() == 3 && parts[1] == "track").then(|| format!("https://open.spotify.com/track/{}", parts[2]))
}

fn unknown_artist() -> RemoteArtist {
    RemoteArtist { id: "unknown".into(), name: "Unknown Artist".into(), url: None, picture_url: None }
}

// MARK: - Embed path

/// No-auth resolve via the `embed/playlist/<id>` page's `__NEXT_DATA__`.
pub fn resolve_embed(url: &str) -> Result<Resolved, SpotifyError> {
    let id = playlist_id(url).ok_or(SpotifyError::NotASpotifyPlaylist)?;
    let (status, html) = get(
        &format!("https://open.spotify.com/embed/playlist/{id}"),
        &[],
        &[("User-Agent", EMBED_UA), ("Accept", "text/html")],
    )?;
    if !(200..300).contains(&status) {
        return Err(SpotifyError::FetchFailed(status as i32));
    }
    let json = extract_next_data(&html).ok_or(SpotifyError::ParseFailed)?;
    build_embed(&json, url)
}

fn extract_next_data(html: &str) -> Option<Value> {
    const OPEN: &str = r#"<script id="__NEXT_DATA__" type="application/json">"#;
    let start = html.find(OPEN)? + OPEN.len();
    let end = html[start..].find("</script>")? + start;
    serde_json::from_str(&html[start..end]).ok()
}

fn cover_sources(raw: &Value) -> Vec<RemoteCoverArt> {
    raw.get("sources")
        .and_then(Value::as_array)
        .map(|srcs| {
            srcs.iter()
                .filter_map(|s| {
                    Some(RemoteCoverArt {
                        url: s.get("url")?.as_str()?.to_owned(),
                        width: s.get("width").and_then(Value::as_f64).map(|v| v as i64),
                        height: s.get("height").and_then(Value::as_f64).map(|v| v as i64),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn build_embed(next: &Value, source_url: &str) -> Result<Resolved, SpotifyError> {
    let e = next
        .pointer("/props/pageProps/state/data/entity")
        .filter(|v| v.is_object())
        .ok_or(SpotifyError::ParseFailed)?;
    let title = e.get("name").or_else(|| e.get("title")).and_then(Value::as_str).unwrap_or("Playlist").to_owned();
    let creator = e.get("subtitle").and_then(Value::as_str).map(Into::into);
    let cover = e.get("coverArt").map(cover_sources).unwrap_or_default();
    let raw = e.get("trackList").and_then(Value::as_array).cloned().unwrap_or_default();
    let was_truncated = raw.len() >= TRACK_CAP;
    let tracks = raw
        .iter()
        .enumerate()
        .filter_map(|(idx, t)| {
            let url = track_url_from_uri(t.get("uri")?.as_str()?)?;
            let artists: Vec<RemoteArtist> = t
                .get("subtitle")
                .and_then(Value::as_str)
                .unwrap_or("")
                .split(", ")
                .filter(|s| !s.is_empty())
                .map(RemoteArtist::named)
                .collect();
            Some(RemoteTrack {
                id: url.clone(),
                title: t.get("title").and_then(Value::as_str).map(Into::into).unwrap_or_else(|| format!("Track {}", idx + 1)),
                artists: if artists.is_empty() { vec![unknown_artist()] } else { artists },
                album: None,
                track_number: None,
                disc_number: None,
                duration_seconds: t.get("duration").and_then(Value::as_f64).map(|d| d / 1000.0),
                cover_art: vec![],
                url: Some(url),
                service_id: "lucida".into(),
                is_lossless: true,
            })
        })
        .collect();
    Ok(Resolved {
        playlist: RemotePlaylist {
            id: source_url.to_owned(),
            title,
            creator,
            cover_art: cover,
            url: Some(source_url.to_owned()),
            tracks,
            service_id: "lucida".into(),
        },
        was_truncated,
    })
}

// MARK: - Web API path

#[derive(Deserialize)]
struct ApiImage {
    url: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
}

#[derive(Deserialize)]
struct ApiTrack {
    name: Option<String>,
    duration_ms: Option<f64>,
    is_local: Option<bool>,
    external_urls: Option<ExternalUrls>,
    artists: Option<Vec<NameOnly>>,
    album: Option<ApiAlbum>,
}

#[derive(Deserialize)]
struct ApiAlbum {
    images: Option<Vec<ApiImage>>,
}

#[derive(Deserialize)]
struct ExternalUrls {
    spotify: Option<String>,
}

#[derive(Deserialize)]
struct NameOnly {
    name: Option<String>,
}

#[derive(Deserialize)]
struct PlaylistHeader {
    name: Option<String>,
    owner: Option<Owner>,
    images: Option<Vec<ApiImage>>,
}

#[derive(Deserialize)]
struct Owner {
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct TrackPage {
    items: Option<Vec<TrackItem>>,
    next: Option<String>,
}

/// `/items` nests the track under `item`; legacy `/tracks` used `track`.
#[derive(Deserialize)]
struct TrackItem {
    item: Option<ApiTrack>,
    track: Option<ApiTrack>,
}

fn images(i: Option<Vec<ApiImage>>) -> Vec<RemoteCoverArt> {
    i.into_iter()
        .flatten()
        .filter_map(|img| Some(RemoteCoverArt { url: img.url?, width: img.width, height: img.height }))
        .collect()
}

fn remote_track(t: Option<ApiTrack>, index: usize) -> Option<RemoteTrack> {
    let t = t?;
    if t.is_local == Some(true) {
        return None;
    }
    let url = t.external_urls?.spotify?;
    let artists: Vec<RemoteArtist> =
        t.artists.into_iter().flatten().filter_map(|a| a.name).map(|n| RemoteArtist::named(&n)).collect();
    Some(RemoteTrack {
        id: url.clone(),
        title: t.name.unwrap_or_else(|| format!("Track {}", index + 1)),
        artists: if artists.is_empty() { vec![unknown_artist()] } else { artists },
        album: None,
        track_number: None,
        disc_number: None,
        duration_seconds: t.duration_ms.map(|d| d / 1000.0),
        cover_art: vec![],
        url: Some(url),
        service_id: "lucida".into(),
        is_lossless: true,
    })
}

/// Spotify's `{ "error": { "message" } }` text, else a raw snippet.
fn spotify_message(body: &str) -> String {
    if let Some(m) = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_owned))
        .filter(|m| !m.is_empty())
    {
        return m;
    }
    if body.is_empty() {
        "no details".into()
    } else {
        body.chars().take(200).collect()
    }
}

/// `SpotifyPlaylistService.apiGet`.
fn api_get<T: DeserializeOwned>(url: &str, query: &[(&str, &str)], token: &str) -> Result<T, SpotifyError> {
    let auth = format!("Bearer {token}");
    let (status, body) = get(url, query, &[("Authorization", &auth)])?;
    if status == 401 {
        return Err(SpotifyError::AuthFailed(401));
    }
    if !(200..300).contains(&status) {
        return Err(SpotifyError::Api { code: status, message: spotify_message(&body) });
    }
    serde_json::from_str(&body).map_err(|_| SpotifyError::ParseFailed)
}

/// Full, uncapped tracklist with a user token (private playlists too).
pub fn resolve_api(url: &str, token: &str) -> Result<Resolved, SpotifyError> {
    let id = playlist_id(url).ok_or(SpotifyError::NotASpotifyPlaylist)?;
    let header: PlaylistHeader = api_get(
        &format!("https://api.spotify.com/v1/playlists/{id}"),
        &[("fields", "name,owner(display_name),images")],
        token,
    )?;
    let mut tracks = Vec::new();
    let mut offset = 0usize;
    const LIMIT: usize = 100;
    loop {
        let (o, l) = (offset.to_string(), LIMIT.to_string());
        let page: TrackPage = api_get(
            &format!("https://api.spotify.com/v1/playlists/{id}/items"),
            &[
                ("offset", &o),
                ("limit", &l),
                ("fields", "next,items(item(name,duration_ms,is_local,external_urls(spotify),artists(name)))"),
            ],
            token,
        )?;
        let got = page.items.as_ref().map_or(0, Vec::len);
        for entry in page.items.into_iter().flatten() {
            if let Some(t) = remote_track(entry.item.or(entry.track), tracks.len()) {
                tracks.push(t);
            }
        }
        if page.next.is_none() || got < LIMIT {
            break;
        }
        offset += LIMIT;
    }
    Ok(Resolved {
        playlist: RemotePlaylist {
            id: url.to_owned(),
            title: header.name.unwrap_or_else(|| "Playlist".into()),
            creator: header.owner.and_then(|o| o.display_name),
            cover_art: images(header.images),
            url: Some(url.to_owned()),
            tracks,
            service_id: "lucida".into(),
        },
        was_truncated: false,
    })
}

#[derive(Deserialize)]
struct SavedPage {
    items: Option<Vec<SavedItem>>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct SavedItem {
    track: Option<ApiTrack>,
}

/// The user's Liked Songs (`/v1/me/tracks`), 50 a page; the cover is the
/// first track's album art.
pub fn resolve_liked_songs(token: &str) -> Result<Resolved, SpotifyError> {
    let mut tracks = Vec::new();
    let mut cover = Vec::new();
    let mut offset = 0usize;
    const LIMIT: usize = 50;
    loop {
        let (o, l) = (offset.to_string(), LIMIT.to_string());
        let page: SavedPage = api_get(
            "https://api.spotify.com/v1/me/tracks",
            &[
                ("offset", &o),
                ("limit", &l),
                ("fields", "next,items(track(name,duration_ms,is_local,external_urls(spotify),artists(name),album(images)))"),
            ],
            token,
        )?;
        let got = page.items.as_ref().map_or(0, Vec::len);
        for mut entry in page.items.into_iter().flatten() {
            if cover.is_empty() {
                if let Some(imgs) = entry.track.as_mut().and_then(|t| t.album.as_mut()).and_then(|a| a.images.take()) {
                    cover = images(Some(imgs));
                }
            }
            if let Some(t) = remote_track(entry.track, tracks.len()) {
                tracks.push(t);
            }
        }
        if page.next.is_none() || got < LIMIT {
            break;
        }
        offset += LIMIT;
    }
    Ok(Resolved {
        playlist: RemotePlaylist {
            id: LIKED_SONGS_URL.into(),
            title: "Liked Songs".into(),
            creator: None,
            cover_art: cover,
            url: Some(LIKED_SONGS_URL.into()),
            tracks,
            service_id: "lucida".into(),
        },
        was_truncated: false,
    })
}

// MARK: - Account (SpotifyAuthController)

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
}

/// A 64-byte random verifier and its S256 challenge.
pub fn pkce_pair() -> (String, String) {
    let mut bytes = [0u8; 64];
    getrandom::fill(&mut bytes).expect("system randomness");
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    (verifier.clone(), code_challenge(&verifier))
}

pub fn code_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn form_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Always shows the consent screen so a reconnect re-grants current scopes.
pub fn authorize_url(challenge: &str, state: &str) -> String {
    let q = [
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT_URI),
        ("code_challenge_method", "S256"),
        ("code_challenge", challenge),
        ("scope", SCOPES),
        ("state", state),
        ("show_dialog", "true"),
    ];
    let qs: Vec<String> = q.iter().map(|(k, v)| format!("{k}={}", form_encode(v))).collect();
    format!("https://accounts.spotify.com/authorize?{}", qs.join("&"))
}

/// A query parameter from a callback URL.
pub fn query_param(url: &str, name: &str) -> Option<String> {
    let q = url.split_once('?')?.1.split('#').next()?;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == name).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn token_request(form: &[(&str, &str)]) -> Result<TokenResponse, SpotifyError> {
    let body: Vec<String> = form.iter().map(|(k, v)| format!("{k}={}", form_encode(v))).collect();
    let mut resp = agent()
        .post("https://accounts.spotify.com/api/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(body.join("&"))
        .map_err(|_| SpotifyError::Network("No response from Spotify.".into()))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(SpotifyError::Network(format!("Spotify token request failed (HTTP {status}).")));
    }
    resp.body_mut().read_json().map_err(|_| SpotifyError::ParseFailed)
}

pub fn exchange_code(code: &str, verifier: &str) -> Result<TokenResponse, SpotifyError> {
    token_request(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", CLIENT_ID),
        ("code_verifier", verifier),
    ])
}

pub fn refresh(refresh_token: &str) -> Result<TokenResponse, SpotifyError> {
    token_request(&[("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("client_id", CLIENT_ID)])
}

/// `SpotifyAuthController.apiGet`: 401 means the session is gone.
fn account_get<T: DeserializeOwned>(url: &str, token: &str) -> Result<T, SpotifyError> {
    let auth = format!("Bearer {token}");
    let (status, body) = get(url, &[], &[("Authorization", &auth)])?;
    if status == 401 {
        return Err(SpotifyError::AuthFailed(401));
    }
    if !(200..300).contains(&status) {
        return Err(SpotifyError::Network(format!("Spotify request failed (HTTP {status}).")));
    }
    serde_json::from_str(&body).map_err(|_| SpotifyError::ParseFailed)
}

#[derive(Deserialize)]
struct Me {
    display_name: Option<String>,
    id: Option<String>,
}

pub fn display_name(token: &str) -> Result<String, SpotifyError> {
    let me: Me = account_get("https://api.spotify.com/v1/me", token)?;
    Ok(me.display_name.or(me.id).unwrap_or_else(|| "Spotify".into()))
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSummary {
    pub id: String,
    pub name: String,
    pub owner: Option<String>,
    /// -1 for the synthetic Liked Songs card.
    pub track_count: i64,
    pub cover_art_url: Option<String>,
    pub external_url: String,
}

pub const LIKED_SONGS_ID: &str = "__liked_songs__";

pub fn liked_songs_summary() -> PlaylistSummary {
    PlaylistSummary {
        id: LIKED_SONGS_ID.into(),
        name: "Liked Songs".into(),
        owner: None,
        track_count: -1,
        cover_art_url: None,
        external_url: LIKED_SONGS_URL.into(),
    }
}

#[derive(Deserialize)]
struct PlaylistPage {
    items: Option<Vec<PlaylistItem>>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct PlaylistItem {
    id: Option<String>,
    name: Option<String>,
    owner: Option<Owner>,
    images: Option<Vec<ApiImage>>,
    tracks: Option<Count>,
    items: Option<Count>,
    external_urls: Option<ExternalUrls>,
}

#[derive(Deserialize)]
struct Count {
    total: Option<i64>,
}

/// `/v1/me/playlists`, every page.
pub fn my_playlists(token: &str) -> Result<Vec<PlaylistSummary>, SpotifyError> {
    let mut out = Vec::new();
    let mut next = Some("https://api.spotify.com/v1/me/playlists?limit=50&offset=0".to_owned());
    while let Some(url) = next {
        let page: PlaylistPage = account_get(&url, token)?;
        for item in page.items.into_iter().flatten() {
            let (Some(id), Some(name), Some(external)) = (item.id, item.name, item.external_urls.and_then(|e| e.spotify))
            else {
                continue;
            };
            out.push(PlaylistSummary {
                id,
                name,
                owner: item.owner.and_then(|o| o.display_name),
                track_count: item.items.and_then(|c| c.total).or(item.tracks.and_then(|c| c.total)).unwrap_or(0),
                cover_art_url: item.images.and_then(|i| i.into_iter().next()).and_then(|i| i.url),
                external_url: external,
            });
        }
        next = page.next;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_ids() {
        assert_eq!(playlist_id("https://open.spotify.com/playlist/abc123?si=x").as_deref(), Some("abc123"));
        assert_eq!(playlist_id("https://open.spotify.com/intl-de/playlist/xyz").as_deref(), Some("xyz"));
        assert_eq!(playlist_id("spotify:playlist:q1").as_deref(), Some("q1"));
        assert_eq!(playlist_id("spotify:track:q1"), None);
        assert_eq!(playlist_id("https://open.spotify.com/album/abc"), None);
        assert_eq!(playlist_id("open.spotify.com/playlist/abc"), None);
        assert_eq!(playlist_id("https://example.com/playlist/abc"), None);
    }

    #[test]
    fn embed_parse() {
        let html = r#"<html><script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":{
            "name":"Mix","subtitle":"Someone","coverArt":{"sources":[{"url":"https://i","width":300,"height":300}]},
            "trackList":[{"uri":"spotify:track:t1","title":"One","subtitle":"A, B","duration":200000},
                         {"uri":"spotify:episode:e1","title":"Pod"},
                         {"uri":"spotify:track:t2","title":"Two","subtitle":""}]}}}}}}</script></html>"#;
        let json = extract_next_data(html).unwrap();
        let r = build_embed(&json, "https://open.spotify.com/playlist/p").unwrap();
        assert!(!r.was_truncated);
        let p = r.playlist;
        assert_eq!((p.title.as_str(), p.creator.as_deref()), ("Mix", Some("Someone")));
        assert_eq!(p.tracks.len(), 2);
        assert_eq!(p.tracks[0].url.as_deref(), Some("https://open.spotify.com/track/t1"));
        assert_eq!(p.tracks[0].artists.len(), 2);
        assert_eq!(p.tracks[0].duration_seconds, Some(200.0));
        assert_eq!(p.tracks[1].artists[0].name, "Unknown Artist");
        assert_eq!(p.cover_art[0].width, Some(300));
    }

    #[test]
    fn pkce_and_urls() {
        // Checked against Python hashlib + urlsafe_b64encode.
        assert_eq!(code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r7wW1gFWFOEjXk"), "bwWFMyPfdG9qreDhH2lmftFx_dFeLDalzcT1gb_j68g");
        let (v, c) = pkce_pair();
        assert_eq!(v.len(), 86);
        assert_eq!(code_challenge(&v), c);
        let u = authorize_url("ch", "st");
        assert!(u.contains("redirect_uri=flactastic%3A%2F%2Fspotify-callback"));
        assert!(u.contains("scope=playlist-read-private%20playlist-read-collaborative%20user-library-read"));
        assert_eq!(query_param("flactastic://spotify-callback?code=a%2Fb&state=s1", "code").as_deref(), Some("a/b"));
        assert_eq!(query_param("flactastic://spotify-callback?code=a&state=s1", "state").as_deref(), Some("s1"));
        assert_eq!(query_param("flactastic://spotify-callback?error=access_denied", "code"), None);
        assert_eq!(spotify_message(r#"{"error":{"status":403,"message":"Forbidden"}}"#), "Forbidden");
    }
}
