//! lucida.to wire types (`LucidaWebProvider`, `LucidaOptions`). The HTTP
//! itself runs inside the app's Lucida webview; this module only builds the
//! request bodies and maps the JSON it hands back.

use serde::{Deserialize, Serialize};

use crate::remote::*;

/// Hosts the lucida.to frontend accepts as paste targets (exact match after
/// stripping `www.`).
pub const HOSTNAMES: &[&str] = &[
    "open.spotify.com",
    "spotify.com",
    "tidal.com",
    "listen.tidal.com",
    "qobuz.com",
    "play.qobuz.com",
    "open.qobuz.com",
    "deezer.com",
    "soundcloud.com",
    "on.soundcloud.com",
    "music.apple.com",
    "music.amazon.com",
    "music.amazon.co.uk",
    "music.youtube.com",
    "lucida.to",
];

/// `StreamerRegistry.provider(for:)`: case-insensitive, `www.`-tolerant.
pub fn claims(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    let h = h.strip_prefix("www.").unwrap_or(&h);
    HOSTNAMES.iter().any(|t| t.strip_prefix("www.").unwrap_or(t) == h)
}

// MARK: - Options

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Format {
    #[serde(rename = "original")]
    Original,
    #[serde(rename = "flac")]
    Flac,
    #[serde(rename = "mp3")]
    Mp3,
    #[serde(rename = "ogg-vorbis")]
    OggVorbis,
    #[serde(rename = "opus")]
    Opus,
    #[serde(rename = "m4a-aac")]
    M4aAac,
    #[serde(rename = "wav")]
    Wav,
    #[serde(rename = "bitcrush")]
    Bitcrush,
}

impl Format {
    pub fn raw(self) -> &'static str {
        match self {
            Format::Original => "original",
            Format::Flac => "flac",
            Format::Mp3 => "mp3",
            Format::OggVorbis => "ogg-vorbis",
            Format::Opus => "opus",
            Format::M4aAac => "m4a-aac",
            Format::Wav => "wav",
            Format::Bitcrush => "bitcrush",
        }
    }

    pub fn requires_quality(self) -> bool {
        !matches!(self, Format::Original | Format::Wav | Format::Bitcrush)
    }
}

/// Per-track download knobs (`LucidaOptions`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub region: String,
    pub add_metadata: bool,
    pub compatibility: bool,
    pub format: Format,
    #[serde(default)]
    pub quality: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options { region: "auto".into(), add_metadata: true, compatibility: false, format: Format::Original, quality: None }
    }
}

impl Options {
    /// The `downscale` field: bare format unless a quality applies, so it
    /// never reads `"flac-"`.
    pub fn downscale(&self) -> String {
        match &self.quality {
            Some(q) if self.format.requires_quality() && !q.is_empty() => format!("{}-{q}", self.format.raw()),
            _ => self.format.raw().to_owned(),
        }
    }
}

/// Body for `POST /api/fetch/stream/v2` (`LucidaStreamRequest`).
pub fn stream_request(url: &str, o: &Options) -> serde_json::Value {
    serde_json::json!({
        "url": url,
        "metadata": o.add_metadata,
        "compat": o.compatibility,
        "private": true,
        "handoff": true,
        "account": { "id": o.region, "type": "country" },
        "upload": { "enabled": false, "service": "pixeldrain" },
        "downscale": o.downscale(),
    })
}

#[derive(Debug, Deserialize)]
pub struct InitiateResponse {
    pub handoff: Option<String>,
    /// Server that owns the job; sent back as `force=`.
    pub name: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PollResponse {
    pub status: Option<String>,
    pub message: Option<String>,
}

/// `LucidaDownloadWriter.guessExt`: fallback when the response has no name.
pub fn guess_ext(mime: &str) -> &'static str {
    let m = mime.to_ascii_lowercase();
    if m.contains("flac") {
        "flac"
    } else if m.contains("mpeg") || m.contains("mp3") {
        "mp3"
    } else if m.contains("mp4") || m.contains("m4a") || m.contains("aac") {
        "m4a"
    } else if m.contains("opus") {
        "opus"
    } else if m.contains("ogg") || m.contains("vorbis") {
        "ogg"
    } else if m.contains("wav") {
        "wav"
    } else {
        "bin"
    }
}

// MARK: - Metadata

/// Loose decoder for `/api/fetch/metadata`: a track, album or playlist over a
/// `{success, error?}` envelope. Almost everything is optional.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Metadata {
    pub success: Option<bool>,
    pub error: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub title: Option<String>,
    pub artists: Option<Vec<MArtist>>,
    pub album: Option<MAlbum>,
    pub duration_ms: Option<f64>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub isrc: Option<String>,
    pub url: Option<String>,
    pub cover_artwork: Option<Vec<Artwork>>,
    pub release_date: Option<String>,
    pub genres: Option<Vec<String>>,
    pub tracks: Option<Vec<MTrack>>,
    pub creator: Option<String>,
    pub owner: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct MArtist {
    pub name: Option<String>,
    pub url: Option<String>,
    pub pictures: Option<Vec<Artwork>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MAlbum {
    pub title: Option<String>,
    pub release_date: Option<String>,
    pub release_year: Option<i64>,
    pub track_count: Option<i64>,
    pub url: Option<String>,
    pub cover_artwork: Option<Vec<Artwork>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MTrack {
    pub title: Option<String>,
    pub artists: Option<Vec<MArtist>>,
    pub duration_ms: Option<f64>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub isrc: Option<String>,
    pub url: Option<String>,
    pub release_date: Option<String>,
}

/// Artwork arrives as `{url, width?, height?}` or as a bare URL string.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Artwork {
    Bare(String),
    Full {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        width: Option<i64>,
        #[serde(default)]
        height: Option<i64>,
    },
}

impl Artwork {
    fn cover(&self) -> Option<RemoteCoverArt> {
        match self {
            Artwork::Bare(s) => Some(RemoteCoverArt { url: s.clone(), width: None, height: None }),
            Artwork::Full { url, width, height } => {
                url.clone().map(|url| RemoteCoverArt { url, width: *width, height: *height })
            }
        }
    }

    fn url(&self) -> Option<&str> {
        match self {
            Artwork::Bare(s) => Some(s),
            Artwork::Full { url, .. } => url.as_deref(),
        }
    }
}

fn covers(a: &Option<Vec<Artwork>>) -> Vec<RemoteCoverArt> {
    a.iter().flatten().filter_map(Artwork::cover).collect()
}

/// The year from `2024`, `2024-08`, `2024-08-15` or a full ISO timestamp.
pub fn year_from(raw: Option<&str>) -> Option<i64> {
    let raw = raw?;
    if raw.chars().count() < 4 {
        return None;
    }
    raw.chars().take(4).collect::<String>().parse().ok()
}

fn last_path_component(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let path = path.split_once("://").map_or(path, |(_, rest)| rest.split_once('/').map_or("", |(_, p)| p));
    path.trim_end_matches('/').rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("/").to_owned()
}

fn plain_artists(a: &[MArtist]) -> Vec<RemoteArtist> {
    a.iter()
        .map(|a| RemoteArtist {
            id: a.name.clone().unwrap_or_default(),
            name: a.name.clone().unwrap_or_else(|| "Unknown Artist".into()),
            url: None,
            picture_url: None,
        })
        .collect()
}

impl Metadata {
    /// An explicit `success: false` (region-locked, removed) as an error.
    pub fn failure(&self) -> Option<String> {
        (self.success == Some(false)).then(|| self.error.clone().unwrap_or_else(|| "lucida metadata unavailable".into()))
    }

    pub fn to_remote(&self, original_url: &str) -> RemoteResolve {
        match self.kind.as_deref() {
            Some("album") => RemoteResolve::Album { album: self.build_album(original_url) },
            Some("playlist") => RemoteResolve::Playlist { playlist: self.build_playlist(original_url) },
            _ => RemoteResolve::Track { track: self.build_track(original_url) },
        }
    }

    fn build_track(&self, original_url: &str) -> RemoteTrack {
        let artists = self
            .artists
            .iter()
            .flatten()
            .map(|a| RemoteArtist {
                id: a.name.clone().unwrap_or_default(),
                name: a.name.clone().unwrap_or_else(|| "Unknown Artist".into()),
                url: a.url.clone(),
                picture_url: a.pictures.as_ref().and_then(|p| p.first()).and_then(|p| p.url()).map(Into::into),
            })
            .collect();
        let title = self.title.clone().unwrap_or_else(|| last_path_component(original_url));
        // Services without albums (SoundCloud): synthesize a single named
        // after the track rather than a nameless bucket.
        let album = match &self.album {
            Some(al) if al.title.as_deref().is_some_and(|t| !t.is_empty()) => {
                let t = al.title.clone().unwrap_or_default();
                RemoteAlbumRef {
                    id: t.clone(),
                    title: t,
                    url: al.url.clone(),
                    cover_art: covers(&al.cover_artwork),
                    release_year: al.release_year.or_else(|| year_from(al.release_date.as_deref())),
                    track_count: al.track_count,
                }
            }
            _ => RemoteAlbumRef {
                id: format!("single:{original_url}"),
                title: title.clone(),
                url: None,
                cover_art: covers(&self.cover_artwork),
                release_year: year_from(self.release_date.as_deref()),
                track_count: Some(1),
            },
        };
        RemoteTrack {
            id: original_url.to_owned(),
            title,
            artists,
            album: Some(album),
            track_number: self.track_number,
            disc_number: self.disc_number,
            duration_seconds: self.duration_ms.map(|d| d / 1000.0),
            cover_art: covers(&self.cover_artwork),
            url: Some(self.url.clone().unwrap_or_else(|| original_url.to_owned())),
            service_id: "lucida".into(),
            is_lossless: true,
        }
    }

    fn build_album(&self, original_url: &str) -> RemoteAlbum {
        let album_arts = match self.album.as_ref().and_then(|a| a.cover_artwork.as_ref()) {
            Some(a) => covers(&Some(a.clone())),
            None => covers(&self.cover_artwork),
        };
        let main_artists = plain_artists(self.artists.as_deref().unwrap_or(&[]));
        let album_title = self
            .album
            .as_ref()
            .and_then(|a| a.title.clone())
            .or_else(|| self.title.clone())
            .unwrap_or_else(|| last_path_component(original_url));
        let album_year = self.album.as_ref().and_then(|a| a.release_year).or_else(|| {
            year_from(self.album.as_ref().and_then(|a| a.release_date.as_deref()).or(self.release_date.as_deref()))
        });
        let track_count = self.album.as_ref().and_then(|a| a.track_count);
        let tracks: Vec<RemoteTrack> = self
            .tracks
            .iter()
            .flatten()
            .enumerate()
            .map(|(idx, t)| RemoteTrack {
                id: format!("{original_url}#{idx}"),
                title: t.title.clone().unwrap_or_else(|| format!("Track {}", idx + 1)),
                artists: plain_artists(t.artists.as_deref().or(self.artists.as_deref()).unwrap_or(&[])),
                album: Some(RemoteAlbumRef {
                    id: album_title.clone(),
                    title: album_title.clone(),
                    url: Some(original_url.to_owned()),
                    cover_art: album_arts.clone(),
                    release_year: album_year,
                    track_count,
                }),
                track_number: t.track_number.or(Some(idx as i64 + 1)),
                disc_number: t.disc_number,
                duration_seconds: t.duration_ms.map(|d| d / 1000.0),
                cover_art: album_arts.clone(),
                url: Some(t.url.clone().unwrap_or_else(|| original_url.to_owned())),
                service_id: "lucida".into(),
                is_lossless: true,
            })
            .collect();
        RemoteAlbum {
            id: original_url.to_owned(),
            title: album_title,
            artists: main_artists,
            release_year: album_year,
            cover_art: album_arts,
            url: Some(original_url.to_owned()),
            track_count: track_count.or(Some(tracks.len() as i64)),
            tracks,
            service_id: "lucida".into(),
        }
    }

    /// Playlist entries keep their own service URL and each synthesize a
    /// single, so they aren't lumped into one fake album folder.
    fn build_playlist(&self, original_url: &str) -> RemotePlaylist {
        let tracks = self
            .tracks
            .iter()
            .flatten()
            .enumerate()
            .map(|(idx, t)| {
                let artists = t
                    .artists
                    .iter()
                    .flatten()
                    .map(|a| RemoteArtist {
                        id: a.name.clone().unwrap_or_default(),
                        name: a.name.clone().unwrap_or_else(|| "Unknown Artist".into()),
                        url: a.url.clone(),
                        picture_url: None,
                    })
                    .collect();
                let title = t.title.clone().unwrap_or_else(|| format!("Track {}", idx + 1));
                let entry_url = t.url.clone().unwrap_or_else(|| original_url.to_owned());
                RemoteTrack {
                    id: format!("{original_url}#{idx}"),
                    title: title.clone(),
                    artists,
                    album: Some(RemoteAlbumRef {
                        id: format!("single:{entry_url}"),
                        title,
                        url: None,
                        cover_art: vec![],
                        release_year: year_from(t.release_date.as_deref()),
                        track_count: Some(1),
                    }),
                    track_number: t.track_number,
                    disc_number: t.disc_number,
                    duration_seconds: t.duration_ms.map(|d| d / 1000.0),
                    cover_art: vec![],
                    url: Some(entry_url),
                    service_id: "lucida".into(),
                    is_lossless: true,
                }
            })
            .collect();
        RemotePlaylist {
            id: original_url.to_owned(),
            title: self.title.clone().unwrap_or_else(|| last_path_component(original_url)),
            creator: self
                .creator
                .clone()
                .or_else(|| self.owner.clone())
                .or_else(|| self.artists.as_ref().and_then(|a| a.first()).and_then(|a| a.name.clone())),
            cover_art: covers(&self.cover_artwork),
            url: Some(original_url.to_owned()),
            tracks,
            service_id: "lucida".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscale_and_body() {
        let mut o = Options::default();
        assert_eq!(o.downscale(), "original");
        o.format = Format::Mp3;
        assert_eq!(o.downscale(), "mp3");
        o.quality = Some("320".into());
        assert_eq!(o.downscale(), "mp3-320");
        o.format = Format::Wav;
        assert_eq!(o.downscale(), "wav");
        let b = stream_request("https://x", &Options::default());
        assert_eq!(b["account"]["type"], "country");
        assert_eq!(b["handoff"], true);
    }

    #[test]
    fn host_claims() {
        assert!(claims("www.qobuz.com"));
        assert!(claims("OPEN.SPOTIFY.COM"));
        assert!(!claims("example.com"));
    }

    #[test]
    fn track_without_album_becomes_single() {
        let m: Metadata = serde_json::from_str(
            r#"{"success":true,"type":"track","title":"Song","artists":[{"name":"A","pictures":["https://p"]}],
                "durationMs":61000,"coverArtwork":["https://c1",{"url":"https://c2","width":600,"height":600}],
                "releaseDate":"2021-05-01T00:00:00Z"}"#,
        )
        .unwrap();
        let RemoteResolve::Track { track } = m.to_remote("https://soundcloud.com/a/song") else { panic!() };
        let al = track.album.unwrap();
        assert_eq!((al.title.as_str(), al.release_year, al.track_count), ("Song", Some(2021), Some(1)));
        assert_eq!(al.id, "single:https://soundcloud.com/a/song");
        assert_eq!(track.duration_seconds, Some(61.0));
        assert_eq!(track.artists[0].picture_url.as_deref(), Some("https://p"));
        assert_eq!(RemoteCoverArt::best(&track.cover_art).unwrap().url, "https://c2");
    }

    #[test]
    fn album_tracks_inherit() {
        let m: Metadata = serde_json::from_str(
            r#"{"type":"album","title":"LP","artists":[{"name":"Band"}],"album":{"title":"LP","releaseYear":1999},
                "tracks":[{"title":"One","durationMs":1000},{"artists":[{"name":"Guest"}],"trackNumber":7}]}"#,
        )
        .unwrap();
        assert!(m.failure().is_none());
        let RemoteResolve::Album { album } = m.to_remote("https://tidal.com/album/1") else { panic!() };
        assert_eq!(album.track_count, Some(2));
        assert_eq!(album.tracks[0].artists[0].name, "Band");
        assert_eq!(album.tracks[0].track_number, Some(1));
        assert_eq!(album.tracks[1].title, "Track 2");
        assert_eq!(album.tracks[1].track_number, Some(7));
        assert_eq!(album.tracks[1].artists[0].name, "Guest");
        assert_eq!(album.tracks[1].album.as_ref().unwrap().release_year, Some(1999));
        assert_eq!(album.tracks[1].id, "https://tidal.com/album/1#1");
    }

    #[test]
    fn failure_envelope() {
        let m: Metadata = serde_json::from_str(r#"{"success":false,"error":"region locked"}"#).unwrap();
        assert_eq!(m.failure().as_deref(), Some("region locked"));
        assert_eq!(last_path_component("https://a.com/x/y/?q=1"), "y");
        assert_eq!(year_from(Some("20")), None);
    }
}
