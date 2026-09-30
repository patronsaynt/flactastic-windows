//! `PlaylistStore` — `<root>/.flactastic/playlists.json`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::apple_json::{self, Uid};
use crate::model::{join_relative, relative_path, Playlist, PlaylistEntry, Track};

#[derive(Default)]
pub struct PlaylistStore {
    pub playlists: Vec<Playlist>,
    root: Option<PathBuf>,
}

impl PlaylistStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn file(&self) -> Option<PathBuf> {
        let root = self.root.as_ref()?;
        let dir = root.join(".flactastic");
        let _ = std::fs::create_dir_all(&dir);
        Some(dir.join("playlists.json"))
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn load(&mut self, root: &Path) {
        self.root = Some(root.to_path_buf());
        self.playlists.clear();
        let Some(f) = self.file() else { return };
        match apple_json::load::<Vec<Playlist>>(&f) {
            Ok(Some(p)) => self.playlists = p,
            Ok(None) => {}
            Err(e) => log::warn!("[PlaylistStore] Failed to load playlists: {e}"),
        }
    }

    pub fn save(&self) {
        let Some(f) = self.file() else {
            log::warn!("[PlaylistStore] Cannot save: no library root URL set");
            return;
        };
        if let Err(e) = apple_json::save(&f, &self.playlists) {
            log::warn!("[PlaylistStore] Failed to save playlists: {e}");
        }
    }

    pub fn get(&self, id: Uid) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    fn index(&self, id: Uid) -> Option<usize> {
        self.playlists.iter().position(|p| p.id == id)
    }

    /// Insert or wholesale-replace a playlist received from a paired device.
    pub fn upsert_from_sync(&mut self, playlist: Playlist) {
        match self.index(playlist.id) {
            Some(i) => self.playlists[i] = playlist,
            None => self.playlists.push(playlist),
        }
        self.save();
    }

    pub fn create(&mut self, name: &str) -> Playlist {
        let p = Playlist::new(name.to_owned());
        self.playlists.push(p.clone());
        self.save();
        p
    }

    pub fn delete(&mut self, id: Uid) {
        self.playlists.retain(|p| p.id != id);
        self.save();
    }

    pub fn rename(&mut self, id: Uid, name: &str) {
        if let Some(i) = self.index(id) {
            self.playlists[i].name = name.to_owned();
            self.save();
        }
    }

    pub fn update_metadata(&mut self, id: Uid, name: &str, description: Option<String>, custom_artwork: Option<Vec<u8>>) {
        if let Some(i) = self.index(id) {
            let p = &mut self.playlists[i];
            p.name = name.to_owned();
            p.description = description;
            p.custom_artwork = custom_artwork;
            self.save();
        }
    }

    pub fn add_tracks(&mut self, tracks: &[Track], playlist_id: Uid, skip_duplicates: bool) {
        let Some(root) = self.root.clone() else { return };
        let Some(i) = self.index(playlist_id) else { return };
        let (by_id, by_path) = existing_keys(&self.playlists[i], skip_duplicates);
        for t in tracks {
            let Some(rel) = relative_path(&t.path, &root) else { continue };
            if skip_duplicates && (by_id.contains(&t.id) || by_path.contains(&rel)) {
                continue;
            }
            self.playlists[i].entries.push(PlaylistEntry::new(Some(t.id), rel));
        }
        self.save();
    }

    /// Appends by relative path in the given order (Spotify rebuild).
    pub fn append_entries(&mut self, relative_paths: &[String], track_ids: &[Option<Uid>], playlist_id: Uid) {
        let Some(i) = self.index(playlist_id) else { return };
        if relative_paths.len() != track_ids.len() {
            return;
        }
        for (p, id) in relative_paths.iter().zip(track_ids) {
            self.playlists[i].entries.push(PlaylistEntry::new(*id, p.clone()));
        }
        self.save();
    }

    pub fn duplicate_count(&self, tracks: &[Track], playlist_id: Uid) -> usize {
        let (Some(root), Some(p)) = (self.root.as_ref(), self.get(playlist_id)) else { return 0 };
        let (by_id, by_path) = existing_keys(p, true);
        tracks
            .iter()
            .filter(|t| {
                by_id.contains(&t.id) || relative_path(&t.path, root).is_some_and(|rel| by_path.contains(&rel))
            })
            .count()
    }

    pub fn remove_entries_at(&mut self, offsets: &[usize], playlist_id: Uid) {
        let Some(i) = self.index(playlist_id) else { return };
        let drop: HashSet<usize> = offsets.iter().copied().collect();
        let mut n = 0;
        self.playlists[i].entries.retain(|_| {
            let keep = !drop.contains(&n);
            n += 1;
            keep
        });
        self.save();
    }

    pub fn remove_entries(&mut self, ids: &HashSet<Uid>, playlist_id: Uid) {
        let Some(i) = self.index(playlist_id) else { return };
        self.playlists[i].entries.retain(|e| !ids.contains(&e.id));
        self.save();
    }

    /// Moves `source` to just before `destination` (drag-and-drop reorder).
    pub fn move_entry(&mut self, source: Uid, before: Uid, playlist_id: Uid) {
        let Some(pi) = self.index(playlist_id) else { return };
        let entries = &mut self.playlists[pi].entries;
        let (Some(src), Some(dst)) = (entries.iter().position(|e| e.id == source), entries.iter().position(|e| e.id == before))
        else {
            return;
        };
        if src == dst {
            return;
        }
        let item = entries.remove(src);
        let at = if src < dst { dst - 1 } else { dst };
        entries.insert(at, item);
        self.save();
    }

    /// Resolves entries to library tracks: by trackID, else by path (stamping
    /// the trackID for next time). Dangling entries are skipped.
    pub fn resolved_tracks(&mut self, playlist_id: Uid, library: &[Track], root: &Path) -> Vec<Track> {
        let Some(pi) = self.index(playlist_id) else { return vec![] };
        let mut by_id: HashMap<Uid, &Track> = HashMap::new();
        let mut by_path: HashMap<&Path, &Track> = HashMap::new();
        for t in library {
            by_id.entry(t.id).or_insert(t);
            by_path.entry(t.path.as_path()).or_insert(t);
        }
        let mut needs_save = false;
        let mut out = Vec::new();
        for e in self.playlists[pi].entries.iter_mut() {
            if let Some(t) = e.track_id.and_then(|id| by_id.get(&id)) {
                out.push((*t).clone());
                continue;
            }
            let abs = join_relative(root, &e.relative_path);
            if let Some(t) = by_path.get(abs.as_path()) {
                out.push((*t).clone());
                e.track_id = Some(t.id);
                needs_save = true;
            }
        }
        if needs_save {
            self.save();
        }
        out
    }

    /// Drops entries whose tracks are no longer in the library.
    pub fn reconcile(&mut self, library: &[Track], root: &Path) {
        let ids: HashSet<Uid> = library.iter().map(|t| t.id).collect();
        let paths: HashSet<&Path> = library.iter().map(|t| t.path.as_path()).collect();
        let mut changed = false;
        for p in self.playlists.iter_mut() {
            let before = p.entries.len();
            p.entries.retain(|e| match e.track_id {
                Some(id) => ids.contains(&id),
                None => paths.contains(join_relative(root, &e.relative_path).as_path()),
            });
            changed |= p.entries.len() != before;
        }
        if changed {
            self.save();
        }
    }
}

fn existing_keys(p: &Playlist, enabled: bool) -> (HashSet<Uid>, HashSet<String>) {
    if !enabled {
        return (HashSet::new(), HashSet::new());
    }
    (
        p.entries.iter().filter_map(|e| e.track_id).collect(),
        p.entries.iter().filter(|e| e.track_id.is_none()).map(|e| e.relative_path.clone()).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::AudioFileFormat;

    #[test]
    fn crud_and_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut s = PlaylistStore::new();
        s.load(root);
        let p = s.create("Road");
        let a = Track::new(root.join("A").join("a.flac"), "a".into(), AudioFileFormat::Flac);
        let b = Track::new(root.join("b.flac"), "b".into(), AudioFileFormat::Flac);
        s.add_tracks(&[a.clone(), b.clone()], p.id, false);
        assert_eq!(s.duplicate_count(&[a.clone()], p.id), 1);
        s.add_tracks(&[a.clone()], p.id, true);
        assert_eq!(s.get(p.id).unwrap().entries.len(), 2);
        assert_eq!(s.get(p.id).unwrap().entries[0].relative_path, "A/a.flac");

        let ids: Vec<Uid> = s.get(p.id).unwrap().entries.iter().map(|e| e.id).collect();
        s.move_entry(ids[1], ids[0], p.id);
        assert_eq!(s.get(p.id).unwrap().entries[0].id, ids[1]);

        let mut reloaded = PlaylistStore::new();
        reloaded.load(root);
        let got = reloaded.resolved_tracks(p.id, &[a.clone(), b.clone()], root);
        assert_eq!(got.len(), 2);

        reloaded.reconcile(&[a.clone()], root);
        assert_eq!(reloaded.get(p.id).unwrap().entries.len(), 1);
    }

    #[test]
    fn legacy_path_entries_get_stamped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut s = PlaylistStore::new();
        s.load(root);
        let p = s.create("Old");
        s.append_entries(&["x/y.flac".into()], &[None], p.id);
        let t = Track::new(root.join("x").join("y.flac"), "y".into(), AudioFileFormat::Flac);
        let got = s.resolved_tracks(p.id, &[t.clone()], root);
        assert_eq!(got.len(), 1);
        assert_eq!(s.get(p.id).unwrap().entries[0].track_id, Some(t.id));
    }
}
