//! `ContentHashCache` + `ManifestBuilder`: this device's library as a
//! `LibraryManifest`, with SHA-256s remembered in
//! `<root>/.flactastic/sync-hashes.json` (same size + mtime validity rule as
//! the metadata cache) so only changed files are re-read.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use fl_core::apple_json::{self, AppleDate, Uid};
use fl_core::model::relative_path;
use fl_core::{ArtistResolver, Playlist, Track};
use serde::{Deserialize, Serialize};

use crate::manifest::{
    hex_digest_of_file, playlist_manifest_entry, tag_fingerprint_for, LibraryManifest, SyncFilter, TrackManifestEntry,
};
use crate::protocol::MAX_FILE_BYTES;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub content_hash: String,
    pub file_size: i64,
    /// Seconds since 2001 (Foundation's default `Date` coding).
    pub mtime: AppleDate,
}

impl CacheEntry {
    /// The millisecond tolerance absorbs date round-trips (and filesystem
    /// timestamp precision when a library is shared between machines).
    fn is_valid(&self, file_size: i64, mtime: AppleDate) -> bool {
        self.file_size == file_size && (self.mtime.0 - mtime.0).abs() < 0.001
    }
}

#[derive(Debug, Default)]
pub struct ContentHashCache {
    entries: HashMap<String, CacheEntry>,
}

impl ContentHashCache {
    pub fn sidecar(root: &Path) -> PathBuf {
        root.join(".flactastic").join("sync-hashes.json")
    }

    /// Any failure means an empty cache (it costs time, nothing else).
    pub fn load(root: &Path) -> Self {
        let entries = apple_json::load::<HashMap<String, CacheEntry>>(&Self::sidecar(root)).ok().flatten().unwrap_or_default();
        ContentHashCache { entries }
    }

    pub fn save(&self, root: &Path) {
        let url = Self::sidecar(root);
        if let Some(dir) = url.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = apple_json::save(&url, &self.entries) {
            log::warn!("[ContentHashCache] Failed to save sidecar: {e}");
        }
    }

    pub fn hash(&self, path: &str, file_size: i64, mtime: AppleDate) -> Option<&str> {
        self.entries.get(path).filter(|e| e.is_valid(file_size, mtime)).map(|e| e.content_hash.as_str())
    }

    pub fn store(&mut self, hash: String, path: &str, file_size: i64, mtime: AppleDate) {
        self.entries.insert(path.to_owned(), CacheEntry { content_hash: hash, file_size, mtime });
    }

    /// Drops entries for files no longer in the library.
    pub fn prune(&mut self, live: &HashSet<String>) {
        self.entries.retain(|k, _| live.contains(k));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Album artist first, then track artist (Collection grouping precedence),
/// keyed like `ArtistResolver`.
pub fn artist_key(t: &Track) -> Option<String> {
    let raw = t.album_artist.as_deref().or(t.artist.as_deref())?;
    if fl_core::text::trim_ws(raw).is_empty() {
        return None;
    }
    Some(ArtistResolver::key(raw))
}

fn file_size(p: &Path) -> Option<i64> {
    std::fs::metadata(p).ok().map(|m| m.len() as i64)
}

/// Builds the manifest, hashing only what the filter admits (excluded files
/// are never opened), and refreshes the hash sidecar. `progress` gets 0…1;
/// `cancelled` is polled between (and inside) files.
pub fn build(
    tracks: &[Track],
    playlists: &[Playlist],
    root: &Path,
    device_id: Uid,
    filter: &SyncFilter,
    mut progress: impl FnMut(f64),
    cancelled: &dyn Fn() -> bool,
) -> Result<LibraryManifest, BuildError> {
    let mut cache = ContentHashCache::load(root);
    let candidates: Vec<&Track> = tracks
        .iter()
        .filter(|t| filter.allows(t.file_format, artist_key(t).as_deref(), file_size(&t.path).unwrap_or(0)))
        .collect();
    let mut entries = Vec::with_capacity(candidates.len());
    let mut live = HashSet::new();
    let total = candidates.len().max(1);

    for (i, t) in candidates.iter().enumerate() {
        if cancelled() {
            return Err(BuildError::Cancelled);
        }
        // A track outside the root can't be described by a relative path.
        let Some(rel) = relative_path(&t.path, root) else { continue };
        let Ok(meta) = std::fs::metadata(&t.path) else { continue };
        let size = meta.len() as i64;
        if size > MAX_FILE_BYTES {
            continue;
        }
        let mtime = meta.modified().map(AppleDate::from_system_time).unwrap_or(AppleDate(0.0));
        live.insert(rel.clone());
        let hash = match cache.hash(&rel, size, mtime) {
            Some(h) => h.to_owned(),
            None => match hex_digest_of_file(&t.path, cancelled) {
                Ok(h) => {
                    cache.store(h.clone(), &rel, size, mtime);
                    h
                }
                Err(_) if cancelled() => return Err(BuildError::Cancelled),
                Err(_) => continue,
            },
        };
        entries.push(TrackManifestEntry {
            track_id: t.id,
            relative_path: rel,
            file_size: size,
            content_hash: hash,
            format: t.file_format,
            tag_fingerprint: tag_fingerprint_for(t),
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            album_artist: t.album_artist.clone(),
        });
        progress((i + 1) as f64 / total as f64);
    }

    cache.prune(&live);
    cache.save(root);

    let playlist_entries = playlists.iter().filter(|p| filter.allows_playlist(p.id)).map(playlist_manifest_entry).collect();
    Ok(LibraryManifest { device_id, generated_at: AppleDate::now(), tracks: entries, playlists: playlist_entries })
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("No music folder is open.")]
    NoLibraryRoot,
    #[error("cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fl_core::AudioFileFormat;

    fn track(root: &Path, rel: &str, body: &[u8], artist: &str) -> Track {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        let mut t = Track::new(p, rel.into(), AudioFileFormat::classify(Path::new(rel)).unwrap());
        t.artist = Some(artist.into());
        t
    }

    #[test]
    fn builds_filters_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let a = track(root, "A/one.flac", b"one", "Band");
        let b = track(root, "B/two.mp3", b"two", "Other");
        let c = track(root, "B/three.flac", b"three", "Excluded Artist");
        let mut f = SyncFilter::default();
        f.excluded_formats.insert(AudioFileFormat::Mp3);
        f.excluded_artist_keys.insert(ArtistResolver::key("excluded artist"));
        let m = build(&[a.clone(), b, c], &[], root, Uid::new_v4(), &f, |_| {}, &|| false).unwrap();
        assert_eq!(m.tracks.len(), 1);
        assert_eq!(m.tracks[0].relative_path, "A/one.flac");
        assert_eq!(m.tracks[0].content_hash, crate::manifest::hex_digest(b"one"));
        // Only admitted files are cached; a second build reuses the entry.
        let cache = ContentHashCache::load(root);
        assert_eq!(cache.len(), 1);
        let again = build(&[a], &[], root, Uid::new_v4(), &SyncFilter::default(), |_| {}, &|| false).unwrap();
        assert_eq!(again.tracks[0].content_hash, m.tracks[0].content_hash);
        assert!(matches!(build(&[], &[], root, Uid::new_v4(), &f, |_| {}, &|| true), Ok(_)));
    }

    #[test]
    fn cache_validity_and_sidecar_format() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = ContentHashCache::default();
        c.store("h".into(), "a.flac", 10, AppleDate(100.0));
        assert_eq!(c.hash("a.flac", 10, AppleDate(100.0004)), Some("h"));
        assert_eq!(c.hash("a.flac", 10, AppleDate(100.01)), None);
        assert_eq!(c.hash("a.flac", 11, AppleDate(100.0)), None);
        c.save(dir.path());
        let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(ContentHashCache::sidecar(dir.path())).unwrap()).unwrap();
        assert_eq!(raw["a.flac"]["contentHash"], "h");
        assert_eq!(raw["a.flac"]["mtime"], 100.0);
        c.prune(&HashSet::new());
        assert!(c.is_empty());
    }
}
