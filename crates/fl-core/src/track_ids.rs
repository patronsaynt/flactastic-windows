//! `TrackIDStore` — stable per-file UUIDs.
//!
//! The UUID lives on the file itself (xattr `com.flactastic.trackID` on
//! macOS/Linux, the NTFS alternate data stream of the same name on Windows).
//! `<root>/.flactastic/track-ids.json` (`{relativePath: UUID}`) is a read-ahead
//! cache and a fallback for filesystems that strip the identity.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::apple_json::{self, Uid};

pub const XATTR_NAME: &str = "com.flactastic.trackID";

/// Reads/writes the per-file identity. Implemented in `fl-platform`.
pub trait FileIdentity: Send + Sync {
    /// Must return `None` unless the stored value is exactly a 36-byte UUID string.
    fn read(&self, path: &Path) -> Option<Uid>;
    /// Best effort; failures are ignored (the sidecar still records the ID).
    fn write(&self, path: &Path, id: Uid);
}

/// No-op identity backend (tests, or filesystems without xattr/ADS support).
pub struct NoFileIdentity;

impl FileIdentity for NoFileIdentity {
    fn read(&self, _: &Path) -> Option<Uid> {
        None
    }
    fn write(&self, _: &Path, _: Uid) {}
}

pub fn sidecar_dir(root: &Path) -> PathBuf {
    root.join(".flactastic")
}

#[derive(Default)]
pub struct TrackIdStore {
    cache: HashMap<String, Uid>,
    sidecar: Option<PathBuf>,
}

impl TrackIdStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads `<root>/.flactastic/track-ids.json`. Call before scanning.
    pub fn load(&mut self, root: &Path) {
        let dir = sidecar_dir(root);
        let _ = std::fs::create_dir_all(&dir);
        let url = dir.join("track-ids.json");
        self.cache.clear();
        match apple_json::load::<HashMap<String, Uid>>(&url) {
            Ok(Some(m)) => self.cache = m,
            Ok(None) => {}
            Err(e) => log::warn!("[TrackIDStore] Failed to load sidecar: {e}. Starting fresh."),
        }
        self.sidecar = Some(url);
    }

    /// Writes the sidecar. Call once per batch, not per file.
    pub fn save(&self) {
        let Some(url) = &self.sidecar else { return };
        if let Err(e) = apple_json::save(url, &self.cache) {
            log::warn!("[TrackIDStore] Failed to save sidecar: {e}");
        }
    }

    /// 1. identity on the file (and re-key the cache if it moved);
    /// 2. sidecar entry, restored onto the file;
    /// 3. a fresh UUID, written to both.
    pub fn assign(&mut self, ident: &dyn FileIdentity, file: &Path, relative_path: &str) -> Uid {
        if let Some(id) = ident.read(file) {
            self.cache.insert(relative_path.to_owned(), id);
            return id;
        }
        if let Some(id) = self.cache.get(relative_path).copied() {
            ident.write(file, id);
            return id;
        }
        let fresh = Uid::new_v4();
        ident.write(file, fresh);
        self.cache.insert(relative_path.to_owned(), fresh);
        fresh
    }

    /// Forces a received file to carry the sender's ID. Only for files this
    /// device just wrote during sync.
    pub fn adopt(&mut self, ident: &dyn FileIdentity, id: Uid, file: &Path, relative_path: &str) {
        ident.write(file, id);
        self.cache.insert(relative_path.to_owned(), id);
    }

    /// Re-keys the cache after the Organizer moves files; saves if anything changed.
    pub fn rename_paths(&mut self, map: &HashMap<String, String>) {
        let mut changed = false;
        for (old, new) in map {
            if old == new {
                continue;
            }
            if let Some(id) = self.cache.remove(old) {
                self.cache.insert(new.clone(), id);
                changed = true;
            }
        }
        if changed {
            self.save();
        }
    }

    pub fn get(&self, relative_path: &str) -> Option<Uid> {
        self.cache.get(relative_path).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemIdent(Mutex<HashMap<PathBuf, Uid>>);
    impl FileIdentity for MemIdent {
        fn read(&self, p: &Path) -> Option<Uid> {
            self.0.lock().unwrap().get(p).copied()
        }
        fn write(&self, p: &Path, id: Uid) {
            self.0.lock().unwrap().insert(p.to_path_buf(), id);
        }
    }

    #[test]
    fn assign_priorities_and_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let ident = MemIdent::default();
        let mut s = TrackIdStore::new();
        s.load(dir.path());
        let f = dir.path().join("a.flac");
        let id = s.assign(&ident, &f, "a.flac");
        assert_eq!(s.assign(&ident, &f, "a.flac"), id);
        s.save();

        // Identity stripped: the sidecar restores it onto the file.
        ident.0.lock().unwrap().clear();
        let mut s2 = TrackIdStore::new();
        s2.load(dir.path());
        assert_eq!(s2.assign(&ident, &f, "a.flac"), id);
        assert_eq!(ident.read(&f), Some(id));

        // Moved file keeps its identity under the new key.
        let g = dir.path().join("b.flac");
        ident.write(&g, id);
        assert_eq!(s2.assign(&ident, &g, "B/b.flac"), id);
        assert_eq!(s2.get("B/b.flac"), Some(id));

        let raw = std::fs::read_to_string(dir.path().join(".flactastic/track-ids.json")).unwrap();
        assert!(raw.contains(&id.uuid_string()));
    }
}
