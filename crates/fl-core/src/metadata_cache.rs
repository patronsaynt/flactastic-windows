//! `<root>/.flactastic/metadata-cache.json` — parsed tags keyed by relative
//! path, validated against file size and mtime (within 1 ms).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::apple_json::{self, AppleDate};
use crate::model::{relative_path, Track};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataCacheEntry {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    /// Required by the Mac decoder — always written.
    pub secondary_genres: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    pub is_compilation: bool,
    pub is_mix_compilation: bool,
    pub has_artwork: bool,
    pub file_size: i64,
    pub mtime: AppleDate,
}

/// Size and modification time of a file.
pub fn file_stamp(path: &Path) -> Option<(i64, AppleDate)> {
    let md = std::fs::metadata(path).ok()?;
    Some((md.len() as i64, AppleDate::from_system_time(md.modified().ok()?)))
}

impl MetadataCacheEntry {
    pub fn new(t: &Track, file_size: i64, mtime: AppleDate) -> Self {
        MetadataCacheEntry {
            title: t.title.clone(),
            artist: t.artist.clone(),
            album_artist: t.album_artist.clone(),
            album: t.album.clone(),
            track_number: t.track_number,
            duration: t.duration,
            sample_rate: t.sample_rate,
            bit_depth: t.bit_depth,
            genre: t.genre.clone(),
            secondary_genres: t.secondary_genres.clone(),
            year: t.year,
            is_compilation: t.is_compilation,
            is_mix_compilation: t.is_mix_compilation,
            has_artwork: t.artwork.is_some(),
            file_size,
            mtime,
        }
    }

    pub fn is_valid_for(&self, path: &Path) -> bool {
        match file_stamp(path) {
            Some((size, mtime)) => size == self.file_size && (mtime.0 - self.mtime.0).abs() < 0.001,
            None => false,
        }
    }

    /// Hydrates tag fields. `id`, `path`, `file_format`, `date_added` and
    /// `artwork` are left alone.
    pub fn apply(&self, t: &mut Track) {
        t.title = self.title.clone();
        t.artist = self.artist.clone();
        t.album_artist = self.album_artist.clone();
        t.album = self.album.clone();
        t.track_number = self.track_number;
        t.duration = self.duration;
        t.sample_rate = self.sample_rate;
        t.bit_depth = self.bit_depth;
        t.genre = self.genre.clone();
        t.secondary_genres = self.secondary_genres.clone();
        t.year = self.year;
        t.is_compilation = self.is_compilation;
        t.is_mix_compilation = self.is_mix_compilation;
    }
}

pub fn sidecar_path(root: &Path) -> PathBuf {
    root.join(".flactastic").join("metadata-cache.json")
}

/// Any failure means an empty cache (full parse).
pub fn load(root: &Path) -> HashMap<String, MetadataCacheEntry> {
    apple_json::load(&sidecar_path(root)).ok().flatten().unwrap_or_default()
}

/// Stats every track and rewrites the sidecar atomically. Blocking.
pub fn rebuild(tracks: &[Track], root: &Path) {
    let mut entries = HashMap::with_capacity(tracks.len());
    for t in tracks {
        let Some(rel) = relative_path(&t.path, root) else { continue };
        let Some((size, mtime)) = file_stamp(&t.path) else { continue };
        entries.insert(rel, MetadataCacheEntry::new(t, size, mtime));
    }
    if let Err(e) = apple_json::save(&sidecar_path(root), &entries) {
        log::warn!("[TrackMetadataCache] Failed to save sidecar: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::AudioFileFormat;

    #[test]
    fn round_trip_and_validation() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("A").join("x.flac");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, b"abc").unwrap();
        let mut t = Track::new(f.clone(), "X".into(), AudioFileFormat::Flac);
        t.artist = Some("Y".into());
        rebuild(&[t.clone()], dir.path());

        let raw = std::fs::read_to_string(sidecar_path(dir.path())).unwrap();
        assert!(raw.contains(r#""A\/x.flac""#));
        assert!(raw.contains(r#""secondaryGenres":[]"#));
        assert!(!raw.contains("albumArtist"));

        let m = load(dir.path());
        let e = &m["A/x.flac"];
        assert!(e.is_valid_for(&f));
        let mut stub = Track::new(f.clone(), "x".into(), AudioFileFormat::Flac);
        e.apply(&mut stub);
        assert_eq!(stub.artist.as_deref(), Some("Y"));

        std::fs::write(&f, b"abcd").unwrap();
        assert!(!e.is_valid_for(&f));
    }

    #[test]
    fn decodes_mac_written_entry() {
        let json = r#"{"Artist\/Album\/01 Song.flac":{"title":"Song","artist":"A","secondaryGenres":["Pop"],"isCompilation":false,"isMixCompilation":false,"hasArtwork":true,"fileSize":1234,"mtime":780000000.123456,"sampleRate":44100,"bitDepth":16,"duration":201.5}}"#;
        let m: HashMap<String, MetadataCacheEntry> = serde_json::from_str(json).unwrap();
        let e = &m["Artist/Album/01 Song.flac"];
        assert_eq!(e.sample_rate, Some(44100.0));
        assert_eq!(e.bit_depth, Some(16));
        assert!(e.has_artwork);
    }
}
