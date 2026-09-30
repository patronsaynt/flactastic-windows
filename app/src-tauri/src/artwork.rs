//! `ArtworkImageCache`: downsampled thumbnails served to the UI over the
//! `flart://` protocol, with a memory layer and a JPEG disk layer (quality
//! 0.85, FNV-1a file names, like the Mac).
//!
//! URLs: `flart://localhost/<content id>?px=<pixels>`; `px=0` returns the
//! original bytes (the artwork zoom overlay). Content ids are
//! `artwork_content_id` values, so every row showing an album's cover shares
//! one thumbnail.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use fl_core::model::artwork_content_id;
use fl_core::{Album, Artwork, Track};
use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};

const MEMORY_BUDGET: usize = 256 * 1024 * 1024;

pub struct ArtworkStore {
    sources: RwLock<HashMap<String, Artwork>>,
    memory: Mutex<Memory>,
    disk: PathBuf,
}

#[derive(Default)]
struct Memory {
    entries: IndexMap<String, Arc<Vec<u8>>>,
    bytes: usize,
}

fn fnv1a(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:x}")
}

impl ArtworkStore {
    pub fn new(cache_dir: PathBuf) -> ArtworkStore {
        let disk = cache_dir.join("ArtworkThumbnails");
        let _ = std::fs::create_dir_all(&disk);
        ArtworkStore { sources: RwLock::default(), memory: Mutex::default(), disk }
    }

    /// Rebuilds the id → bytes map from the library (tracks and albums).
    pub fn register_library(&self, tracks: &[Track], albums: &[Album]) {
        let mut m = HashMap::new();
        for a in tracks.iter().filter_map(|t| t.artwork.as_ref()).chain(albums.iter().filter_map(|a| a.artwork.as_ref())) {
            m.entry(artwork_content_id(a)).or_insert_with(|| a.clone());
        }
        let mut s = self.sources.write();
        // Keep extra (non-library) entries such as playlist covers.
        for (k, v) in s.drain() {
            if k.starts_with("x:") {
                m.insert(k, v);
            }
        }
        *s = m;
    }

    /// Registers artwork outside the library (playlist covers, queue copies).
    /// Returns its id.
    pub fn register(&self, art: &Artwork) -> String {
        let id = artwork_content_id(art);
        self.sources.write().entry(id.clone()).or_insert_with(|| art.clone());
        id
    }

    pub fn register_bytes(&self, key: &str, bytes: Vec<u8>) -> String {
        let id = format!("x:{key}:{}", artwork_content_id(&bytes));
        self.sources.write().insert(id.clone(), bytes.into());
        id
    }

    pub fn original(&self, id: &str) -> Option<Artwork> {
        self.sources.read().get(id).cloned()
    }

    /// Drops cached thumbnails for `id` (after an artwork edit).
    pub fn invalidate(&self, id: &str) {
        let prefix = format!("{id}#");
        let mut mem = self.memory.lock();
        let keys: Vec<String> = mem.entries.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
        for k in keys {
            if let Some(v) = mem.entries.shift_remove(&k) {
                mem.bytes -= v.len();
            }
            let _ = std::fs::remove_file(self.disk.join(fnv1a(&k) + ".jpg"));
        }
    }

    /// JPEG thumbnail no larger than `px` on its longest side.
    pub fn thumbnail(&self, id: &str, px: u32) -> Option<Arc<Vec<u8>>> {
        let key = format!("{id}#{px}");
        if let Some(v) = self.memory.lock().entries.get(&key) {
            return Some(v.clone());
        }
        let path = self.disk.join(fnv1a(&key) + ".jpg");
        if let Ok(bytes) = std::fs::read(&path) {
            let v = Arc::new(bytes);
            self.remember(key, v.clone());
            return Some(v);
        }
        let src = self.original(id)?;
        let img = image::load_from_memory(&src).ok()?;
        let img = if img.width().max(img.height()) > px {
            img.resize(px, px, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };
        let mut out = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(Cursor::new(&mut out), 85);
        img.to_rgb8().write_with_encoder(enc).ok()?;
        let v = Arc::new(out);
        let (p2, v2) = (path, v.clone());
        std::thread::spawn(move || {
            let tmp = p2.with_extension("tmp");
            if std::fs::write(&tmp, &*v2).is_ok() {
                let _ = std::fs::rename(&tmp, &p2);
            }
        });
        self.remember(key, v.clone());
        Some(v)
    }

    fn remember(&self, key: String, v: Arc<Vec<u8>>) {
        let mut mem = self.memory.lock();
        mem.bytes += v.len();
        if let Some(old) = mem.entries.insert(key, v) {
            mem.bytes -= old.len();
        }
        while mem.bytes > MEMORY_BUDGET {
            let Some((_, v)) = mem.entries.shift_remove_index(0) else { break };
            mem.bytes -= v.len();
        }
    }

    /// Decodes the thumbnail sizes the Collection shows first, off the UI's
    /// critical path (`prewarmArtworkCache`).
    pub fn prewarm(self: &Arc<Self>, albums: Vec<Album>, scale: f64) {
        let me = self.clone();
        std::thread::Builder::new()
            .name("fl-artwork-prewarm".into())
            .spawn(move || {
                for a in albums {
                    let Some(art) = &a.artwork else { continue };
                    let id = artwork_content_id(art);
                    for pt in [180.0, 48.0, 200.0, 36.0] {
                        let _ = me.thumbnail(&id, (pt * scale).ceil() as u32);
                    }
                }
            })
            .ok();
    }
}

/// Handles `flart://localhost/<id>?px=N`.
pub fn respond(store: &ArtworkStore, uri: &str) -> (u16, &'static str, Vec<u8>) {
    let rest = uri.split_once("://").map_or(uri, |(_, r)| r);
    let path = rest.split_once('/').map_or("", |(_, p)| p);
    let (id_enc, query) = path.split_once('?').unwrap_or((path, ""));
    let id = percent_decode(id_enc);
    let px: u32 = query.split('&').find_map(|kv| kv.strip_prefix("px=")).and_then(|v| v.parse().ok()).unwrap_or(0);
    if px == 0 {
        return match store.original(&id) {
            Some(a) => (200, fl_tags::mime_type(&a), a.to_vec()),
            None => (404, "text/plain", Vec::new()),
        };
    }
    match store.thumbnail(&id, px.min(4096)) {
        Some(v) => (200, "image/jpeg", (*v).clone()),
        None => (404, "text/plain", Vec::new()),
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_percent_escapes_and_hashes_like_mac() {
        assert_eq!(percent_decode("data%3A12-1-2-3"), "data:12-1-2-3");
        assert_eq!(percent_decode("a%2"), "a%2");
        // FNV-1a 64 of "" is the offset basis.
        assert_eq!(fnv1a(""), "cbf29ce484222325");
    }
}
