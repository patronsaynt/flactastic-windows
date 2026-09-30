//! `LibraryScanner`: the cheap folder walk and per-file metadata load.

use std::path::Path;

use fl_core::metadata_cache::MetadataCacheEntry;
use fl_core::{AppleDate, AudioFileFormat, Track};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError {
    RootNotFound,
    RootNotReadable,
    Cancelled,
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ScanError::RootNotFound => "rootNotFound",
            ScanError::RootNotReadable => "rootNotReadable",
            ScanError::Cancelled => "cancelled",
        })
    }
}

/// Hidden entries are skipped like `.skipsHiddenFiles`: dot-names on every OS
/// (which covers `.flactastic` and AppleDouble `._*` files), plus the hidden
/// attribute on Windows.
fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    if entry.file_name().to_string_lossy().starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if entry.metadata().is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0) {
            return true;
        }
    }
    false
}

/// Walks `root` for supported audio files. `dateAdded` prefers the creation
/// time (the closest to APFS "added to directory"), then modification time.
pub fn scan(root: &Path, cancelled: &dyn Fn() -> bool) -> Result<Vec<Track>, ScanError> {
    if !root.is_dir() {
        return Err(ScanError::RootNotFound);
    }
    if std::fs::read_dir(root).is_err() {
        return Err(ScanError::RootNotReadable);
    }
    let mut tracks = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_entry(|e| !is_hidden(e)) {
        if cancelled() {
            return Err(ScanError::Cancelled);
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(mut t) = Track::make_from_path(entry.path()) else { continue };
        if let Ok(md) = entry.metadata() {
            t.date_added = md.created().or_else(|_| md.modified()).ok().map(AppleDate::from_system_time);
        }
        tracks.push(t);
    }
    fl_core::model::sort_for_library(&mut tracks);
    Ok(tracks)
}

/// `loadMetadata(for:cached:)`. A valid cache entry skips the parse and only
/// re-reads the embedded picture (none at all when the cache says there is
/// none). Otherwise tags come from TagLib and the audio format from the
/// decoder the engine plays with, so durations match playback exactly
/// (gapless-trimmed, like `AVURLAsset.duration`).
pub fn load_metadata(track: &Track, cached: Option<&MetadataCacheEntry>) -> Track {
    let mut t = track.clone();
    if let Some(c) = cached.filter(|c| c.is_valid_for(&track.path)) {
        c.apply(&mut t);
        if c.has_artwork {
            t.artwork = fl_tags::read_picture(&track.path).map(Into::into);
        }
        return t;
    }

    let tags = fl_tags::TagFile::open(&track.path);
    if let Some(f) = &tags {
        fl_tags::apply_tags(&mut t, f);
    }

    match fl_audio::decode::Source::open(&track.path) {
        Ok(src) => {
            if let Some(d) = src.duration_secs().filter(|d| d.is_finite() && *d > 0.0) {
                t.duration = Some(d);
            }
            t.sample_rate = Some(f64::from(src.sample_rate));
            // Bit depth means something only for PCM/lossless sources.
            if !t.file_format.is_lossy() {
                t.bit_depth = src.bits_per_sample.map(i64::from);
            }
        }
        Err(e) => log::debug!("[LibraryScanner] no audio properties for {}: {e}", track.path.display()),
    }
    // Estimated-length streams (VBR MP3 without a Xing header) and anything
    // the decoder couldn't open fall back to TagLib's properties.
    if t.duration.is_none() || t.sample_rate.is_none() {
        if let Some(a) = tags.as_ref().and_then(|f| f.audio()) {
            if t.duration.is_none() && a.length_ms > 0 {
                t.duration = Some(f64::from(a.length_ms) / 1000.0);
            }
            if t.sample_rate.is_none() && a.sample_rate > 0 {
                t.sample_rate = Some(f64::from(a.sample_rate));
            }
        }
    }
    // FLAC/ALAC-class containers: TagLib's bit depth when the decoder has none
    // (AAC in .m4a reports TagLib's 16, as on the Mac).
    if t.bit_depth.is_none() && matches!(t.file_format, AudioFileFormat::Flac | AudioFileFormat::Alac) {
        drop(tags);
        let bits = fl_tags::bits_per_sample(&track.path);
        if bits > 0 {
            t.bit_depth = Some(i64::from(bits));
        }
    }
    t
}
