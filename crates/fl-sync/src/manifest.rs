//! `Manifest.swift`, `SyncFilter.swift`, `ContentHasher.swift` and
//! `SyncDiff.swift`: what one side describes to the other, how it's narrowed,
//! and the plan the receiver computes from the two manifests.

use std::collections::{BTreeSet, HashMap};
use std::io::Read;
use std::path::Path;

use fl_core::apple_json::{AppleDate, Uid};
use fl_core::AudioFileFormat;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::protocol::{Direction, FILE_CHUNK_BYTES};
use crate::wire::iso_date;

// MARK: - Entries

/// One track as described to a peer. Identity is `trackID`; `contentHash`
/// recovers files whose ID was lost (FAT volumes, zips). Display fields are
/// for the confirmation sheet only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackManifestEntry {
    #[serde(rename = "trackID")]
    pub track_id: Uid,
    /// Relative to the sender's root. Untrusted on receipt.
    pub relative_path: String,
    pub file_size: i64,
    /// Lowercase hex SHA-256 of the whole file.
    pub content_hash: String,
    pub format: AudioFileFormat,
    pub tag_fingerprint: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistManifestEntry {
    pub id: Uid,
    pub name: String,
    #[serde(with = "iso_date")]
    pub date_created: AppleDate,
    pub entry_count: i64,
    /// SHA-256 over the name and ordered `(trackID, relativePath)` pairs.
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryManifest {
    #[serde(rename = "deviceID")]
    pub device_id: Uid,
    #[serde(with = "iso_date")]
    pub generated_at: AppleDate,
    pub tracks: Vec<TrackManifestEntry>,
    pub playlists: Vec<PlaylistManifestEntry>,
}

impl LibraryManifest {
    /// First entry wins on duplicate keys (`uniquingKeysWith: { first, _ in first }`).
    pub fn tracks_by_id(&self) -> HashMap<Uid, &TrackManifestEntry> {
        let mut m = HashMap::new();
        for t in &self.tracks {
            m.entry(t.track_id).or_insert(t);
        }
        m
    }

    pub fn tracks_by_content_hash(&self) -> HashMap<&str, &TrackManifestEntry> {
        let mut m = HashMap::new();
        for t in &self.tracks {
            m.entry(t.content_hash.as_str()).or_insert(t);
        }
        m
    }

    pub fn playlists_by_id(&self) -> HashMap<Uid, &PlaylistManifestEntry> {
        let mut m = HashMap::new();
        for p in &self.playlists {
            m.entry(p.id).or_insert(p);
        }
        m
    }
}

// MARK: - Plan

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackConflict {
    pub incoming: TrackManifestEntry,
    pub existing: TrackManifestEntry,
    pub differing_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaylistConflict {
    pub incoming: PlaylistManifestEntry,
    pub existing: PlaylistManifestEntry,
}

/// What a run would do, computed by the receiving side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlan {
    pub direction: Direction,
    pub new_tracks: Vec<TrackManifestEntry>,
    pub track_conflicts: Vec<TrackConflict>,
    pub new_playlists: Vec<PlaylistManifestEntry>,
    pub playlist_conflicts: Vec<PlaylistConflict>,
}

impl SyncPlan {
    pub fn all_incoming_tracks(&self) -> Vec<&TrackManifestEntry> {
        self.new_tracks.iter().chain(self.track_conflicts.iter().map(|c| &c.incoming)).collect()
    }

    pub fn total_transfer_bytes(&self) -> i64 {
        self.all_incoming_tracks().iter().map(|t| t.file_size).sum()
    }

    pub fn overwrite_count(&self) -> usize {
        self.track_conflicts.len() + self.playlist_conflicts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.new_tracks.is_empty()
            && self.track_conflicts.is_empty()
            && self.new_playlists.is_empty()
            && self.playlist_conflicts.is_empty()
    }

    /// Stable digest of everything the plan would change.
    pub fn plan_hash(&self) -> String {
        let dir = match self.direction {
            Direction::Push => "push",
            Direction::Pull => "pull",
        };
        let mut parts = vec![dir.to_owned()];
        let mut group = |mut v: Vec<String>| {
            v.sort();
            parts.extend(v);
        };
        group(self.new_tracks.iter().map(|t| format!("n:{}:{}", t.track_id, t.content_hash)).collect());
        group(
            self.track_conflicts
                .iter()
                .map(|c| format!("c:{}:{}", c.incoming.track_id, c.incoming.content_hash))
                .collect(),
        );
        group(self.new_playlists.iter().map(|p| format!("p:{}:{}", p.id, p.content_hash)).collect());
        group(
            self.playlist_conflicts
                .iter()
                .map(|c| format!("q:{}:{}", c.incoming.id, c.incoming.content_hash))
                .collect(),
        );
        hex_digest(parts.join("\n").as_bytes())
    }

    /// The plan narrowed to a selection; IDs outside the plan are ignored, so
    /// a selection can only remove work.
    pub fn restricted(&self, sel: &SyncSelection) -> SyncPlan {
        if sel.is_everything() {
            return self.clone();
        }
        let keep_track = |id: &Uid| sel.track_ids.as_ref().map_or(true, |s| s.contains(id));
        let keep_playlist = |id: &Uid| sel.playlist_ids.as_ref().map_or(true, |s| s.contains(id));
        SyncPlan {
            direction: self.direction,
            new_tracks: self.new_tracks.iter().filter(|t| keep_track(&t.track_id)).cloned().collect(),
            track_conflicts: self.track_conflicts.iter().filter(|c| keep_track(&c.incoming.track_id)).cloned().collect(),
            new_playlists: self.new_playlists.iter().filter(|p| keep_playlist(&p.id)).cloned().collect(),
            playlist_conflicts: self
                .playlist_conflicts
                .iter()
                .filter(|c| keep_playlist(&c.incoming.id))
                .cloned()
                .collect(),
        }
    }
}

/// Which plan items to run. `None` means all of that kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSelection {
    #[serde(rename = "trackIDs", default, skip_serializing_if = "Option::is_none")]
    pub track_ids: Option<BTreeSet<Uid>>,
    #[serde(rename = "playlistIDs", default, skip_serializing_if = "Option::is_none")]
    pub playlist_ids: Option<BTreeSet<Uid>>,
}

impl SyncSelection {
    pub fn everything() -> Self {
        Self::default()
    }

    pub fn is_everything(&self) -> bool {
        self.track_ids.is_none() && self.playlist_ids.is_none()
    }
}

// MARK: - Filter

/// Narrows a run; applied by the sender when building its manifest and again
/// by the receiver as a bound on what it accepts. Every field defaults, so
/// filters from older peers keep decoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncFilter {
    pub excluded_formats: BTreeSet<AudioFileFormat>,
    /// `ArtistResolver.key` values.
    pub excluded_artist_keys: BTreeSet<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_file_size_bytes: Option<i64>,
    pub include_playlists: bool,
    #[serde(rename = "playlistIDAllowlist", skip_serializing_if = "Option::is_none")]
    pub playlist_id_allowlist: Option<BTreeSet<Uid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_total_transfer_bytes: Option<i64>,
}

impl Default for SyncFilter {
    fn default() -> Self {
        SyncFilter {
            excluded_formats: BTreeSet::new(),
            excluded_artist_keys: BTreeSet::new(),
            max_file_size_bytes: None,
            include_playlists: true,
            playlist_id_allowlist: None,
            max_total_transfer_bytes: None,
        }
    }
}

impl SyncFilter {
    pub fn unrestricted() -> Self {
        Self::default()
    }

    pub fn allows(&self, format: AudioFileFormat, artist_key: Option<&str>, file_size: i64) -> bool {
        if self.excluded_formats.contains(&format) {
            return false;
        }
        if artist_key.is_some_and(|k| self.excluded_artist_keys.contains(k)) {
            return false;
        }
        !self.max_file_size_bytes.is_some_and(|m| file_size > m)
    }

    pub fn allows_playlist(&self, id: Uid) -> bool {
        self.include_playlists && self.playlist_id_allowlist.as_ref().map_or(true, |a| a.contains(&id))
    }

    pub fn exceeds_total_cap(&self, bytes: i64) -> bool {
        self.max_total_transfer_bytes.is_some_and(|m| bytes > m)
    }
}

// MARK: - Hashing

pub fn hex_digest(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Streams a file's SHA-256 in transfer-sized chunks. `cancelled` is polled
/// between chunks.
pub fn hex_digest_of_file(path: &Path, cancelled: &dyn Fn() -> bool) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; FILE_CHUNK_BYTES];
    loop {
        if cancelled() {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Constant-time comparison of two hex digests.
pub fn digests_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// NFC, then trimmed of whitespace and newlines; `None` reads as "".
fn normalise(v: Option<&str>) -> String {
    let nfc: String = v.unwrap_or("").nfc().collect();
    fl_core::text::trim(&nfc).to_owned()
}

/// `TagFingerprint.compute`. Field order is part of the wire contract;
/// `isMixCompilation` contributes only when true (absent == false, so the
/// iOS app agrees).
#[allow(clippy::too_many_arguments)]
pub fn tag_fingerprint(
    title: &str,
    artist: Option<&str>,
    album_artist: Option<&str>,
    album: Option<&str>,
    track_number: Option<i64>,
    genre: Option<&str>,
    secondary_genres: &[String],
    year: Option<i64>,
    is_compilation: bool,
    is_mix_compilation: Option<bool>,
) -> String {
    let mut secondary: Vec<String> = secondary_genres.iter().map(|g| normalise(Some(g))).collect();
    secondary.sort();
    let mut fields = vec![
        normalise(Some(title)),
        normalise(artist),
        normalise(album_artist),
        normalise(album),
        track_number.map(|n| n.to_string()).unwrap_or_default(),
        normalise(genre),
        secondary.join(","),
        year.map(|n| n.to_string()).unwrap_or_default(),
        if is_compilation { "1" } else { "0" }.to_owned(),
    ];
    if is_mix_compilation == Some(true) {
        fields.push("mix".into());
    }
    hex_digest(fields.join("\u{1F}").as_bytes())
}

pub fn tag_fingerprint_for(t: &fl_core::Track) -> String {
    tag_fingerprint(
        &t.title,
        t.artist.as_deref(),
        t.album_artist.as_deref(),
        t.album.as_deref(),
        t.track_number,
        t.genre.as_deref(),
        &t.secondary_genres,
        t.year,
        t.is_compilation,
        Some(t.is_mix_compilation),
    )
}

/// `ManifestBuilder.contentHash(for:)`: name plus ordered entries; entry
/// UUIDs are local bookkeeping and left out.
pub fn playlist_content_hash(p: &fl_core::Playlist) -> String {
    let body: Vec<String> = p
        .entries
        .iter()
        .map(|e| format!("{}|{}", e.track_id.map_or_else(|| "-".to_owned(), |id| id.to_string()), e.relative_path))
        .collect();
    hex_digest(format!("{}\n{}", p.name, body.join("\n")).as_bytes())
}

pub fn playlist_manifest_entry(p: &fl_core::Playlist) -> PlaylistManifestEntry {
    PlaylistManifestEntry {
        id: p.id,
        name: p.name.clone(),
        date_created: p.date_created,
        entry_count: p.entries.len() as i64,
        content_hash: playlist_content_hash(p),
    }
}

// MARK: - Diff

/// `SyncDiff.plan`: what receiving `incoming` into `local` would do.
pub fn plan(incoming: &LibraryManifest, local: &LibraryManifest, direction: Direction, filter: &SyncFilter) -> SyncPlan {
    let local_by_id = local.tracks_by_id();
    let local_by_hash = local.tracks_by_content_hash();
    let mut new_tracks = Vec::new();
    let mut conflicts = Vec::new();
    for entry in &incoming.tracks {
        // Re-applied on receipt: a peer ignoring the agreed filter must not
        // be able to push excluded files.
        if !filter.allows(entry.format, None, entry.file_size) {
            continue;
        }
        if let Some(existing) = local_by_id.get(&entry.track_id) {
            if entry.content_hash != existing.content_hash || entry.tag_fingerprint != existing.tag_fingerprint {
                conflicts.push(TrackConflict {
                    incoming: entry.clone(),
                    existing: (*existing).clone(),
                    differing_fields: differing_fields(entry, existing),
                });
            }
            continue;
        }
        // Byte-identical under another ID: nothing to send (the receiver
        // adopts the sender's ID afterwards).
        if local_by_hash.contains_key(entry.content_hash.as_str()) {
            continue;
        }
        new_tracks.push(entry.clone());
    }

    let local_playlists = local.playlists_by_id();
    let mut new_playlists = Vec::new();
    let mut playlist_conflicts = Vec::new();
    for entry in &incoming.playlists {
        if !filter.allows_playlist(entry.id) {
            continue;
        }
        match local_playlists.get(&entry.id) {
            None => new_playlists.push(entry.clone()),
            Some(existing) => {
                if existing.content_hash != entry.content_hash || existing.name != entry.name {
                    playlist_conflicts.push(PlaylistConflict { incoming: entry.clone(), existing: (*existing).clone() });
                }
            }
        }
    }

    new_tracks.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    conflicts.sort_by(|a, b| a.incoming.relative_path.cmp(&b.incoming.relative_path));
    new_playlists.sort_by(|a, b| a.name.cmp(&b.name));
    playlist_conflicts.sort_by(|a, b| a.incoming.name.cmp(&b.incoming.name));
    SyncPlan { direction, new_tracks, track_conflicts: conflicts, new_playlists, playlist_conflicts }
}

/// Why a conflict row is in the list (presentational only).
pub fn differing_fields(incoming: &TrackManifestEntry, existing: &TrackManifestEntry) -> Vec<String> {
    let mut f = Vec::new();
    if normalise(Some(&incoming.title)) != normalise(Some(&existing.title)) {
        f.push("Title".to_owned());
    }
    if normalise(incoming.artist.as_deref()) != normalise(existing.artist.as_deref()) {
        f.push("Artist".to_owned());
    }
    if normalise(incoming.album.as_deref()) != normalise(existing.album.as_deref()) {
        f.push("Album".to_owned());
    }
    if incoming.tag_fingerprint != existing.tag_fingerprint && f.is_empty() {
        f.push("Other tags".to_owned());
    }
    if incoming.content_hash != existing.content_hash {
        f.push("Audio file".to_owned());
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(n: u8) -> Uid {
        Uid::parse(&format!("00000000-0000-0000-0000-0000000000{n:02X}")).unwrap()
    }

    pub(crate) fn entry(id: u8, path: &str, hash: &str, fp: &str) -> TrackManifestEntry {
        TrackManifestEntry {
            track_id: uid(id),
            relative_path: path.into(),
            file_size: 100,
            content_hash: hash.into(),
            format: AudioFileFormat::Flac,
            tag_fingerprint: fp.into(),
            title: path.into(),
            artist: Some("A".into()),
            album: None,
            album_artist: None,
        }
    }

    fn manifest(tracks: Vec<TrackManifestEntry>) -> LibraryManifest {
        LibraryManifest { device_id: uid(0xEE), generated_at: AppleDate(0.0), tracks, playlists: vec![] }
    }

    #[test]
    fn diff_matches_on_id_then_hash() {
        let incoming = manifest(vec![
            entry(1, "b.flac", "h1", "f1"),    // same everywhere: no-op
            entry(2, "a.flac", "h2", "fX"),    // tags differ: conflict
            entry(3, "c.flac", "h3", "f3"),    // same bytes, other id: skip
            entry(4, "d.flac", "h4", "f4"),    // new
        ]);
        let local = manifest(vec![entry(1, "b.flac", "h1", "f1"), entry(2, "a.flac", "h2", "f2"), entry(9, "z.flac", "h3", "f3")]);
        let p = plan(&incoming, &local, Direction::Pull, &SyncFilter::default());
        assert_eq!(p.new_tracks.iter().map(|t| t.track_id).collect::<Vec<_>>(), vec![uid(4)]);
        assert_eq!(p.track_conflicts.len(), 1);
        assert_eq!(p.track_conflicts[0].differing_fields, vec!["Other tags"]);
        // Filter re-applied on receipt.
        let mut f = SyncFilter::default();
        f.excluded_formats.insert(AudioFileFormat::Flac);
        assert!(plan(&incoming, &local, Direction::Pull, &f).is_empty());
    }

    #[test]
    fn plan_hash_and_restriction() {
        let p = SyncPlan {
            direction: Direction::Push,
            new_tracks: vec![entry(2, "b", "hb", "f"), entry(1, "a", "ha", "f")],
            track_conflicts: vec![],
            new_playlists: vec![],
            playlist_conflicts: vec![],
        };
        let expected = hex_digest(
            format!("push\nn:{}:ha\nn:{}:hb", uid(1), uid(2)).as_bytes(),
        );
        assert_eq!(p.plan_hash(), expected);
        let sel = SyncSelection { track_ids: Some([uid(1), uid(7)].into()), playlist_ids: None };
        let r = p.restricted(&sel);
        assert_eq!(r.new_tracks.len(), 1);
        assert_eq!(r.new_tracks[0].track_id, uid(1));
        assert_eq!(p.restricted(&SyncSelection::everything()), p);
    }

    #[test]
    fn fingerprint_rules() {
        let a = tag_fingerprint("T ", Some("A"), None, None, Some(1), None, &["b".into(), "a".into()], None, false, None);
        let b = tag_fingerprint("T", Some(" A"), Some(""), None, Some(1), None, &["a".into(), "b".into()], None, false, Some(false));
        assert_eq!(a, b, "whitespace, nil-vs-empty, genre order and mix=false don't matter");
        let c = tag_fingerprint("T", Some("A"), None, None, Some(1), None, &["a".into(), "b".into()], None, false, Some(true));
        assert_ne!(a, c);
        // NFD and NFC agree.
        let nfd = tag_fingerprint("Cafe\u{301}", None, None, None, None, None, &[], None, false, None);
        let nfc = tag_fingerprint("Caf\u{e9}", None, None, None, None, None, &[], None, false, None);
        assert_eq!(nfd, nfc);
        // Exact vector: fields joined by U+001F.
        let v = tag_fingerprint("T", None, None, None, None, None, &[], None, true, None);
        assert_eq!(v, hex_digest("T\u{1F}\u{1F}\u{1F}\u{1F}\u{1F}\u{1F}\u{1F}\u{1F}1".as_bytes()));
    }

    /// Golden value pinned in the Mac's `SyncFingerprintTests` (and the iOS
    /// repo): any drift here makes every cross-device track a conflict.
    #[test]
    fn fingerprint_matches_the_mac_golden_value() {
        let d = tag_fingerprint(
            "Roygbiv",
            Some("Boards of Canada"),
            Some("Boards of Canada"),
            Some("Music Has the Right to Children"),
            Some(8),
            Some("Electronic"),
            &["IDM".into(), "Ambient".into()],
            Some(1998),
            false,
            None,
        );
        assert_eq!(d, "a8a034c777902a3ef2c69aa40255d210678d5e47a4a7df65f2f1dd1fd0aa3211");
        assert_eq!(hex_digest(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_ne!(
            tag_fingerprint("AB", Some(""), None, None, None, None, &[], None, false, None),
            tag_fingerprint("A", Some("B"), None, None, None, None, &[], None, false, None)
        );
    }

    #[test]
    fn filter_decodes_with_defaults_and_encodes_like_swift() {
        let f: SyncFilter = serde_json::from_str("{}").unwrap();
        assert_eq!(f, SyncFilter::default());
        let json = serde_json::to_value(SyncFilter::default()).unwrap();
        assert_eq!(json, serde_json::json!({"excludedFormats": [], "excludedArtistKeys": [], "includePlaylists": true}));
        let p = LibraryManifest { device_id: uid(1), generated_at: AppleDate(0.0), tracks: vec![entry(1, "a", "h", "f")], playlists: vec![] };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["generatedAt"], "2001-01-01T00:00:00.000Z");
        assert!(v["tracks"][0].get("album").is_none());
        assert_eq!(v["tracks"][0]["trackID"], uid(1).to_string());
    }

    #[test]
    fn playlist_hash_uses_name_and_ordered_entries() {
        let mut p = fl_core::Playlist::new("Mix".into());
        p.entries.push(fl_core::PlaylistEntry::new(Some(uid(1)), "a.flac".into()));
        p.entries.push(fl_core::PlaylistEntry::new(None, "b.flac".into()));
        assert_eq!(playlist_content_hash(&p), hex_digest(format!("Mix\n{}|a.flac\n-|b.flac", uid(1)).as_bytes()));
        assert!(digests_match("ab", "ab") && !digests_match("ab", "ac") && !digests_match("a", "ab"));
    }
}
