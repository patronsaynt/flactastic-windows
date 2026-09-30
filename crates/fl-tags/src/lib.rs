//! Tag access through vendored TagLib 2.x — a port of the macOS
//! `MetadataWriter` and the tag half of `LibraryScanner.loadMetadata`.
//!
//! Writes are serialised through one process-wide lock (the Mac's
//! `MetadataWriter` is an actor). Reads may run concurrently.

use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};
use std::path::Path;
use std::sync::Mutex;

use fl_core::cue::{self, TrackMarker};
use fl_core::{GenreResolver, Track};

mod ffi {
    use super::*;

    #[repr(C)]
    pub struct FlTlFile {
        _p: [u8; 0],
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct FlTlAudio {
        pub length_ms: c_int,
        pub sample_rate: c_int,
        pub channels: c_int,
        pub bitrate_kbps: c_int,
        pub bits_per_sample: c_int,
    }

    extern "C" {
        pub fn fl_tl_open(path: *const c_char) -> *mut FlTlFile;
        pub fn fl_tl_close(f: *mut FlTlFile);
        pub fn fl_tl_save(f: *mut FlTlFile) -> c_int;
        pub fn fl_tl_free(p: *mut c_void);
        pub fn fl_tl_tag_get(f: *mut FlTlFile, which: c_int) -> *mut c_char;
        pub fn fl_tl_tag_set(f: *mut FlTlFile, which: c_int, v: *const c_char);
        pub fn fl_tl_tag_year(f: *mut FlTlFile) -> c_uint;
        pub fn fl_tl_tag_track(f: *mut FlTlFile) -> c_uint;
        pub fn fl_tl_tag_set_year(f: *mut FlTlFile, y: c_uint);
        pub fn fl_tl_tag_set_track(f: *mut FlTlFile, t: c_uint);
        pub fn fl_tl_property_get(f: *mut FlTlFile, key: *const c_char) -> *mut c_char;
        pub fn fl_tl_property_set(f: *mut FlTlFile, key: *const c_char, v: *const c_char);
        pub fn fl_tl_set_picture(f: *mut FlTlFile, data: *const c_char, size: c_uint, mime: *const c_char) -> c_int;
        pub fn fl_tl_remove_pictures(f: *mut FlTlFile);
        pub fn fl_tl_read_picture(f: *mut FlTlFile, out_size: *mut c_uint) -> *mut u8;
        pub fn fl_tl_audio(f: *mut FlTlFile, out: *mut FlTlAudio) -> c_int;
        pub fn fl_tl_bits_per_sample(path: *const c_char) -> c_int;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TagError {
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("Could not open \"{0}\" for writing. Check file permissions.")]
    FileOpenFailed(String),
    #[error("Failed to save changes to \"{0}\".")]
    SaveFailed(String),
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn c_path(p: &Path) -> Option<CString> {
    CString::new(p.to_string_lossy().as_bytes()).ok()
}

fn c_str(s: &str) -> CString {
    // Interior NULs can't cross the C API; TagLib would truncate there anyway.
    CString::new(s.replace('\0', "")).unwrap()
}

/// Takes ownership of a heap string from the shim.
unsafe fn take_string(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = CStr::from_ptr(p).to_string_lossy().into_owned();
    ffi::fl_tl_free(p.cast());
    Some(s)
}

const TITLE: c_int = 0;
const ARTIST: c_int = 1;
const ALBUM: c_int = 2;
const GENRE: c_int = 3;

/// An open TagLib file. Closed on drop.
pub struct TagFile {
    raw: *mut ffi::FlTlFile,
}

// A FileRef is used from one thread at a time; it can move between threads.
unsafe impl Send for TagFile {}

impl TagFile {
    /// `None` when TagLib can't open the file or doesn't consider it valid.
    pub fn open(path: &Path) -> Option<TagFile> {
        let c = c_path(path)?;
        let raw = unsafe { ffi::fl_tl_open(c.as_ptr()) };
        (!raw.is_null()).then_some(TagFile { raw })
    }

    fn tag(&self, which: c_int) -> Option<String> {
        unsafe { take_string(ffi::fl_tl_tag_get(self.raw, which)) }
    }

    pub fn title(&self) -> Option<String> {
        self.tag(TITLE)
    }
    pub fn artist(&self) -> Option<String> {
        self.tag(ARTIST)
    }
    pub fn album(&self) -> Option<String> {
        self.tag(ALBUM)
    }
    pub fn genre(&self) -> Option<String> {
        self.tag(GENRE)
    }
    pub fn year(&self) -> u32 {
        unsafe { ffi::fl_tl_tag_year(self.raw) }
    }
    pub fn track(&self) -> u32 {
        unsafe { ffi::fl_tl_tag_track(self.raw) }
    }

    pub fn set_tag(&mut self, which: c_int, value: &str) {
        let v = c_str(value);
        unsafe { ffi::fl_tl_tag_set(self.raw, which, v.as_ptr()) }
    }
    pub fn set_year(&mut self, y: u32) {
        unsafe { ffi::fl_tl_tag_set_year(self.raw, y) }
    }
    pub fn set_track(&mut self, t: u32) {
        unsafe { ffi::fl_tl_tag_set_track(self.raw, t) }
    }

    /// First value of a generic property; `None` when unset or empty.
    pub fn property(&self, key: &str) -> Option<String> {
        let k = c_str(key);
        unsafe { take_string(ffi::fl_tl_property_get(self.raw, k.as_ptr())) }
    }

    /// Empty string clears the property.
    pub fn set_property(&mut self, key: &str, value: &str) {
        let (k, v) = (c_str(key), c_str(value));
        unsafe { ffi::fl_tl_property_set(self.raw, k.as_ptr(), v.as_ptr()) }
    }

    /// `"1"`, `"true"`, `"yes"`… — first character 1/t/T/y/Y.
    pub fn flag(&self, key: &str) -> bool {
        self.property(key).and_then(|v| v.chars().next()).is_some_and(|c| matches!(c, '1' | 't' | 'T' | 'y' | 'Y'))
    }

    pub fn set_flag(&mut self, key: &str, on: bool) {
        self.set_property(key, if on { "1" } else { "" });
    }

    pub fn picture(&self) -> Option<Vec<u8>> {
        let mut size: c_uint = 0;
        unsafe {
            let p = ffi::fl_tl_read_picture(self.raw, &mut size);
            if p.is_null() {
                return None;
            }
            let v = std::slice::from_raw_parts(p, size as usize).to_vec();
            ffi::fl_tl_free(p.cast());
            (!v.is_empty()).then_some(v)
        }
    }

    pub fn remove_pictures(&mut self) {
        unsafe { ffi::fl_tl_remove_pictures(self.raw) }
    }

    pub fn set_picture(&mut self, data: &[u8]) -> bool {
        let mime = c_str(mime_type(data));
        unsafe { ffi::fl_tl_set_picture(self.raw, data.as_ptr().cast(), data.len() as c_uint, mime.as_ptr()) != 0 }
    }

    pub fn audio(&self) -> Option<AudioProps> {
        let mut a = ffi::FlTlAudio::default();
        (unsafe { ffi::fl_tl_audio(self.raw, &mut a) } != 0).then(|| AudioProps {
            length_ms: a.length_ms.max(0) as u32,
            sample_rate: a.sample_rate.max(0) as u32,
            channels: a.channels.max(0) as u32,
            bitrate_kbps: a.bitrate_kbps.max(0) as u32,
            bits_per_sample: a.bits_per_sample.max(0) as u32,
        })
    }

    pub fn save(&mut self) -> bool {
        unsafe { ffi::fl_tl_save(self.raw) != 0 }
    }
}

impl Drop for TagFile {
    fn drop(&mut self) {
        unsafe { ffi::fl_tl_close(self.raw) }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioProps {
    pub length_ms: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub bitrate_kbps: u32,
    pub bits_per_sample: u32,
}

/// `taglib_helper_bits_per_sample` (Average-accuracy properties).
pub fn bits_per_sample(path: &Path) -> u32 {
    let Some(c) = c_path(path) else { return 0 };
    unsafe { ffi::fl_tl_bits_per_sample(c.as_ptr()) }.max(0) as u32
}

/// JPEG / PNG sniffed from the magic bytes; JPEG otherwise.
pub fn mime_type(data: &[u8]) -> &'static str {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else {
        "image/jpeg"
    }
}

// MARK: - Scanner read

/// Applies the tag fields `LibraryScanner.loadMetadata` reads through TagLib,
/// plus the embedded picture. Returns the open file for property reuse.
pub fn apply_tags(track: &mut Track, f: &TagFile) {
    if let Some(v) = f.title().filter(|s| !s.is_empty()) {
        track.title = v;
    }
    if let Some(v) = f.artist().filter(|s| !s.is_empty()) {
        track.artist = Some(v);
    }
    if let Some(v) = f.album().filter(|s| !s.is_empty()) {
        track.album = Some(v);
    }
    if let Some(v) = f.genre().filter(|s| !s.is_empty()) {
        let (p, s) = GenreResolver::split(Some(&v));
        track.genre = p;
        track.secondary_genres = s;
    }
    let y = f.year();
    if y > 0 {
        track.year = Some(y as i64);
    }
    let n = f.track();
    if n > 0 {
        track.track_number = Some(n as i64);
    }
    if let Some(aa) = f.property("ALBUMARTIST") {
        track.album_artist = Some(aa);
    }
    track.is_compilation = f.flag("COMPILATION");
    track.is_mix_compilation = f.flag("MIXCOMPILATION");
    if let Some(p) = f.picture() {
        track.artwork = Some(p.into());
    }
}

/// Only the embedded picture (cache-hit path of the scanner).
pub fn read_picture(path: &Path) -> Option<Vec<u8>> {
    TagFile::open(path)?.picture()
}

// MARK: - MetadataWriter

static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq)]
pub enum ArtworkChange {
    Unchanged,
    Removed,
    Updated(Vec<u8>),
}

/// `None` leaves a flag/field alone; `Some(v)` writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct TagWrite {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub secondary_genres: Vec<String>,
    pub track_number: Option<i64>,
    pub artwork: ArtworkChange,
    /// `Some(None)` / `Some(Some(""))` clears ALBUMARTIST.
    pub album_artist: Option<Option<String>>,
    pub compilation: Option<bool>,
    pub mix_compilation: Option<bool>,
}

fn open_for_write(path: &Path) -> Result<TagFile, TagError> {
    if !path.exists() {
        return Err(TagError::FileNotFound(file_name(path)));
    }
    TagFile::open(path).ok_or_else(|| TagError::FileOpenFailed(file_name(path)))
}

fn save(mut f: TagFile, path: &Path) -> Result<(), TagError> {
    if f.save() {
        Ok(())
    } else {
        Err(TagError::SaveFailed(file_name(path)))
    }
}

/// Writes text tags (and optionally flags/artwork), returning the track as a
/// fresh scan would read it. `id`, `path`, format and audio properties are kept.
pub fn write(track: &Track, w: &TagWrite) -> Result<Track, TagError> {
    let _g = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = &track.path;
    let mut f = open_for_write(path)?;

    f.set_tag(TITLE, &w.title);
    f.set_tag(ARTIST, w.artist.as_deref().unwrap_or(""));
    f.set_tag(ALBUM, w.album.as_deref().unwrap_or(""));
    let genre_tag = GenreResolver::join(w.genre.as_deref(), &w.secondary_genres);
    f.set_tag(GENRE, genre_tag.as_deref().unwrap_or(""));
    f.set_year(w.year.unwrap_or(0).max(0) as u32);
    f.set_track(w.track_number.unwrap_or(0).max(0) as u32);

    if let Some(aa) = &w.album_artist {
        f.set_property("ALBUMARTIST", aa.as_deref().unwrap_or(""));
    }
    // Compilation and Mix Compilation exclude each other.
    if let Some(on) = w.compilation {
        f.set_flag("COMPILATION", on);
        if on {
            f.set_flag("MIXCOMPILATION", false);
        }
    }
    if let Some(on) = w.mix_compilation {
        f.set_flag("MIXCOMPILATION", on);
        if on {
            // Mix compilations never carry lyrics.
            f.set_property("LYRICS", "");
            f.set_flag("COMPILATION", false);
        }
    }
    match &w.artwork {
        ArtworkChange::Unchanged => {}
        ArtworkChange::Removed => f.remove_pictures(),
        ArtworkChange::Updated(d) => {
            f.remove_pictures();
            f.set_picture(d);
        }
    }
    save(f, path)?;

    let mut u = track.clone();
    if !w.title.is_empty() {
        u.title = w.title.clone();
    }
    u.artist = w.artist.clone().filter(|s| !s.is_empty());
    u.album = w.album.clone().filter(|s| !s.is_empty());
    let (g, s) = GenreResolver::split(genre_tag.as_deref());
    u.genre = g;
    u.secondary_genres = s;
    u.year = w.year;
    u.track_number = w.track_number;
    if let Some(aa) = &w.album_artist {
        u.album_artist = aa.clone().filter(|s| !s.is_empty());
    }
    if let Some(on) = w.compilation {
        u.is_compilation = on;
        if on {
            u.is_mix_compilation = false;
        }
    }
    if let Some(on) = w.mix_compilation {
        u.is_mix_compilation = on;
        if on {
            u.is_compilation = false;
        }
    }
    match &w.artwork {
        ArtworkChange::Unchanged => {}
        ArtworkChange::Removed => u.artwork = None,
        ArtworkChange::Updated(d) => u.artwork = Some(d.clone().into()),
    }
    Ok(u)
}

/// LYRICS (Xiph LYRICS / ID3v2 USLT / MP4 ©lyr). `None`/empty clears.
pub fn write_lyrics(path: &Path, lyrics: Option<&str>) -> Result<(), TagError> {
    let _g = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = open_for_write(path)?;
    f.set_property("LYRICS", lyrics.unwrap_or(""));
    save(f, path)
}

/// Replaces only the title (Spotify rebuild keeps Lucida's other tags).
pub fn override_title(path: &Path, title: &str) -> Result<(), TagError> {
    let _g = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = open_for_write(path)?;
    f.set_tag(TITLE, title);
    save(f, path)
}

pub fn read_lyrics(path: &Path) -> Option<String> {
    TagFile::open(path)?.property("LYRICS")
}

/// Markers as an embedded CUESHEET; an empty slice clears it.
pub fn write_markers(path: &Path, markers: &[TrackMarker]) -> Result<(), TagError> {
    let _g = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = open_for_write(path)?;
    let sheet = if markers.is_empty() { String::new() } else { cue::encode(markers, &file_name(path)) };
    f.set_property("CUESHEET", &sheet);
    save(f, path)
}

pub fn read_markers(path: &Path) -> Vec<TrackMarker> {
    TagFile::open(path).and_then(|f| f.property("CUESHEET")).map(|s| cue::decode(&s)).unwrap_or_default()
}

#[cfg(test)]
mod tests;
