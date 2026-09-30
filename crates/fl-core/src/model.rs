//! `Track`, `Album`, `ArtistSummary`, `Playlist`, `PlaylistEntry`.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::apple_json::{b64, AppleDate, Uid};
use crate::format::AudioFileFormat;
use crate::text;

/// Embedded picture bytes. Shared so identical covers occupy one buffer.
pub type Artwork = Arc<[u8]>;

/// `ArtworkImageCache.contentID(for:)` — byte count plus three sampled bytes.
pub fn artwork_content_id(data: &[u8]) -> String {
    let n = data.len();
    if n == 0 {
        return "data:0".into();
    }
    format!("data:{}-{}-{}-{}", n, data[0], data[n / 2], data[n - 1])
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: Uid,
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<i64>,
    pub duration: Option<f64>,
    pub artwork: Option<Artwork>,
    pub file_format: AudioFileFormat,
    pub sample_rate: Option<f64>,
    pub bit_depth: Option<i64>,
    pub genre: Option<String>,
    /// Up to `GenreResolver::MAX_SECONDARY_COUNT` extra genres, packed into the
    /// same GENRE tag on disk.
    pub secondary_genres: Vec<String>,
    pub year: Option<i64>,
    /// COMPILATION tag (Xiph COMPILATION / ID3v2 TCMP / MP4 cpil).
    pub is_compilation: bool,
    /// Mix / live set / radio show — never gets lyrics; carries CUESHEET markers.
    pub is_mix_compilation: bool,
    pub date_added: Option<AppleDate>,
}

impl Track {
    pub fn new(path: PathBuf, title: String, file_format: AudioFileFormat) -> Self {
        Track {
            id: Uid::new_v4(),
            path,
            title,
            artist: None,
            album_artist: None,
            album: None,
            track_number: None,
            duration: None,
            artwork: None,
            file_format,
            sample_rate: None,
            bit_depth: None,
            genre: None,
            secondary_genres: Vec::new(),
            year: None,
            is_compilation: false,
            is_mix_compilation: false,
            date_added: None,
        }
    }

    /// Cheap-scan stub: title is the file stem; no tags yet.
    pub fn make_from_path(path: &Path) -> Option<Track> {
        let format = AudioFileFormat::classify(path)?;
        let title = path.file_stem()?.to_string_lossy().into_owned();
        Some(Track::new(path.to_path_buf(), title, format))
    }

    /// Copy with a fresh UUID (enqueuing the same track twice).
    pub fn with_new_id(&self) -> Track {
        Track { id: Uid::new_v4(), ..self.clone() }
    }

    /// Copy pointing at `new_path` with a fresh id and `dateAdded = now`
    /// (`Track.relocated(to:)`, used after an import copy).
    pub fn relocated(&self, new_path: PathBuf) -> Track {
        let fmt = AudioFileFormat::classify(&new_path).unwrap_or(self.file_format);
        Track {
            id: Uid::new_v4(),
            file_format: fmt,
            path: new_path,
            date_added: Some(AppleDate::now()),
            // The Mac resets secondaryGenres/isCompilation/isMixCompilation here;
            // they come from the (copied) file's tags, so they are kept.
            ..self.clone()
        }
    }

    /// `Album.id` grouping key used by the listening log.
    pub fn listening_album_id(&self) -> Option<String> {
        let album = self.album.as_ref()?;
        let artist = self
            .album_artist
            .as_deref()
            .or(self.artist.as_deref())
            .unwrap_or("Unknown Artist");
        Some(format!("{artist}|{album}"))
    }

    pub fn extension(&self) -> String {
        self.path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// `sortedForLibrary()` — (album, trackNumber, title).
pub fn sort_for_library(tracks: &mut [Track]) {
    tracks.sort_by(library_order);
}

pub fn library_order(a: &Track, b: &Track) -> Ordering {
    let aa = a.album.as_deref().unwrap_or("");
    let ab = b.album.as_deref().unwrap_or("");
    if aa != ab {
        return text::standard_compare(aa, ab);
    }
    let ta = a.track_number.unwrap_or(i64::MAX);
    let tb = b.track_number.unwrap_or(i64::MAX);
    if ta != tb {
        return ta.cmp(&tb);
    }
    text::standard_compare(&a.title, &b.title)
}

/// Relative path of `path` under `root`, `/`-separated, no leading slash.
/// `None` when `path` is not inside `root`.
pub fn relative_path(path: &Path, root: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    Some(parts.join("/"))
}

/// Joins a `/`-separated relative path onto `root` using native separators.
pub fn join_relative(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p
}

#[derive(Debug, Clone, PartialEq)]
pub struct Album {
    /// `"<albumArtist or display artist>|<album name>"`.
    pub id: String,
    pub name: String,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub secondary_genres: Vec<String>,
    pub artwork: Option<Artwork>,
    pub tracks: Vec<Track>,
}

impl Album {
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }
    pub fn total_duration(&self) -> f64 {
        self.tracks.iter().filter_map(|t| t.duration).sum()
    }
    pub fn is_compilation(&self) -> bool {
        self.tracks.iter().any(|t| t.is_compilation)
    }
    pub fn is_mix_compilation(&self) -> bool {
        self.tracks.iter().any(|t| t.is_mix_compilation)
    }
}

#[derive(Debug, Clone)]
pub struct ArtistSummary {
    /// Canonical key from `ArtistResolver`.
    pub id: String,
    pub display_name: String,
    pub albums: Vec<Album>,
    pub singles: Vec<Album>,
    pub appears_on: Vec<Album>,
    pub track_count: usize,
    pub artwork_sample: Option<Artwork>,
}

impl ArtistSummary {
    pub fn total_releases(&self) -> usize {
        self.albums.len() + self.singles.len() + self.appears_on.len()
    }
}

// MARK: - Playlist

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub id: Uid,
    /// Stable library identity; `None` only in pre-track-ID data.
    #[serde(rename = "trackID", default, skip_serializing_if = "Option::is_none")]
    pub track_id: Option<Uid>,
    #[serde(rename = "relativePath")]
    pub relative_path: String,
}

impl PlaylistEntry {
    pub fn new(track_id: Option<Uid>, relative_path: String) -> Self {
        PlaylistEntry { id: Uid::new_v4(), track_id, relative_path }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Playlist {
    pub id: Uid,
    pub name: String,
    pub entries: Vec<PlaylistEntry>,
    #[serde(rename = "dateCreated")]
    pub date_created: AppleDate,
    #[serde(rename = "customArtwork", with = "b64::opt", skip_serializing_if = "Option::is_none")]
    pub custom_artwork: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Playlist {
    pub const DESCRIPTION_MAX_LENGTH: usize = 200;

    pub fn new(name: String) -> Self {
        Playlist {
            id: Uid::new_v4(),
            name,
            entries: Vec::new(),
            date_created: AppleDate::now(),
            custom_artwork: None,
            description: None,
        }
    }
}

/// Decoding mirrors `Playlist.init(from:)`: `entries`, else legacy
/// `trackPaths`, else empty.
impl<'de> Deserialize<'de> for Playlist {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            id: Uid,
            name: String,
            #[serde(rename = "dateCreated")]
            date_created: AppleDate,
            #[serde(rename = "customArtwork", default, with = "b64::opt")]
            custom_artwork: Option<Vec<u8>>,
            #[serde(default)]
            description: Option<String>,
            #[serde(default)]
            entries: Option<serde_json::Value>,
            #[serde(rename = "trackPaths", default)]
            track_paths: Option<serde_json::Value>,
        }
        let raw = Raw::deserialize(d)?;
        let entries = if let Some(e) = raw.entries.and_then(|v| serde_json::from_value::<Vec<PlaylistEntry>>(v).ok()) {
            e
        } else if let Some(p) = raw.track_paths.and_then(|v| serde_json::from_value::<Vec<String>>(v).ok()) {
            p.into_iter().map(|rel| PlaylistEntry::new(None, rel)).collect()
        } else {
            Vec::new()
        };
        Ok(Playlist {
            id: raw.id,
            name: raw.name,
            entries,
            date_created: raw.date_created,
            custom_artwork: raw.custom_artwork,
            description: raw.description,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(title: &str, album: &str, n: i64) -> Track {
        let mut t = Track::new(PathBuf::from("/x"), title.into(), AudioFileFormat::Flac);
        t.album = Some(album.into());
        t.track_number = Some(n);
        t
    }

    #[test]
    fn make_from_path() {
        let t = Track::make_from_path(Path::new("/Users/test/Music/MyAlbum/03 Song Title.flac")).unwrap();
        assert_eq!(t.title, "03 Song Title");
        assert_eq!(t.album, None);
        assert_eq!(t.file_format, AudioFileFormat::Flac);
        assert!(Track::make_from_path(Path::new("/Users/test/docs/notes.pdf")).is_none());
    }

    #[test]
    fn sorted_for_library() {
        let mut v = vec![t("B Track", "B Album", 1), t("A Track", "A Album", 2), t("C Track", "A Album", 1)];
        sort_for_library(&mut v);
        let titles: Vec<_> = v.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["C Track", "A Track", "B Track"]);
    }

    #[test]
    fn playlist_legacy_track_paths() {
        let json = r#"{"id":"E621E1F8-C36C-495A-93FC-0C247A3E6E5F","name":"Old","dateCreated":700000000,"trackPaths":["a\/b.flac"]}"#;
        let p: Playlist = serde_json::from_str(json).unwrap();
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.entries[0].relative_path, "a/b.flac");
        assert!(p.entries[0].track_id.is_none());
    }

    #[test]
    fn playlist_round_trip_omits_nils() {
        let mut p = Playlist::new("Mix".into());
        p.entries.push(PlaylistEntry::new(Some(Uid::new_v4()), "A/B.flac".into()));
        let s = String::from_utf8(crate::apple_json::to_vec(&p).unwrap()).unwrap();
        assert!(!s.contains("customArtwork"));
        assert!(!s.contains("description"));
        assert!(s.contains(r#""relativePath":"A\/B.flac""#));
        let back: Playlist = serde_json::from_str(&s).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn relative_paths() {
        let root = Path::new("/lib");
        assert_eq!(relative_path(Path::new("/lib/A/b.flac"), root).as_deref(), Some("A/b.flac"));
        assert_eq!(relative_path(Path::new("/other/b.flac"), root), None);
    }
}
