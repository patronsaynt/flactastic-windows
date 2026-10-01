//! `SyncPickList`'s tree: a plan's incoming tracks grouped artist → album →
//! track, plus its playlists. The tick state (exclusions, tri-state marks)
//! lives in the UI; this is the grouping, tested once here.

use std::collections::HashMap;

use fl_core::text::{fold_diacritics_and_case, standard_compare, trim};
use fl_core::Uid;
use serde::Serialize;

use crate::manifest::{PlaylistManifestEntry, SyncPlan, TrackManifestEntry};

pub const UNKNOWN_ARTIST: &str = "Unknown Artist";
pub const UNKNOWN_ALBUM: &str = "Unknown Album";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickTrack {
    pub entry: TrackManifestEntry,
    /// `differingFields` joined, when this replaces a track the receiver has.
    pub replaces: Option<String>,
    /// The track's own artist when it differs from the album's (a guest).
    pub credited_artist: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickAlbum {
    pub id: String,
    pub title: String,
    pub tracks: Vec<PickTrack>,
    #[serde(rename = "trackIDs")]
    pub track_ids: Vec<Uid>,
    pub bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickArtist {
    pub id: String,
    pub name: String,
    pub albums: Vec<PickAlbum>,
    #[serde(rename = "trackIDs")]
    pub track_ids: Vec<Uid>,
    pub bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickPlaylist {
    pub entry: PlaylistManifestEntry,
    pub replaces_existing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PickList {
    pub artists: Vec<PickArtist>,
    pub playlists: Vec<PickPlaylist>,
}

fn display_name(raw: Option<&str>, fallback: &str) -> String {
    let t = trim(raw.unwrap_or(""));
    if t.is_empty() {
        fallback.to_owned()
    } else {
        t.to_owned()
    }
}

/// `[.caseInsensitive, .diacriticInsensitive, .widthInsensitive]`.
fn fold_key(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    fold_diacritics_and_case(&s.nfkc().collect::<String>())
}

fn folder(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

/// The most common album-artist tag, else the most common track artist; ties
/// go to the alphabetically first.
fn album_artist(tracks: &[PickTrack]) -> String {
    fn most_common(names: Vec<String>) -> Option<String> {
        let mut counts: HashMap<String, (String, usize)> = HashMap::new();
        for n in names {
            counts.entry(fold_key(&n)).or_insert_with(|| (n.clone(), 0)).1 += 1;
        }
        counts.into_values().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0))).map(|(n, _)| n)
    }
    let tagged: Vec<String> =
        tracks.iter().filter_map(|t| t.entry.album_artist.as_deref()).map(|a| display_name(Some(a), "")).filter(|s| !s.is_empty()).collect();
    if let Some(n) = most_common(tagged) {
        return n;
    }
    let credited: Vec<String> =
        tracks.iter().filter_map(|t| t.entry.artist.as_deref()).map(|a| display_name(Some(a), "")).filter(|s| !s.is_empty()).collect();
    most_common(credited).unwrap_or_else(|| UNKNOWN_ARTIST.to_owned())
}

pub fn build(plan: &SyncPlan) -> PickList {
    let replacements: HashMap<Uid, String> =
        plan.track_conflicts.iter().map(|c| (c.incoming.track_id, c.differing_fields.join(", "))).collect();
    let tracks: Vec<PickTrack> = plan
        .new_tracks
        .iter()
        .map(|e| PickTrack { entry: e.clone(), replaces: None, credited_artist: None })
        .chain(plan.track_conflicts.iter().map(|c| PickTrack {
            entry: c.incoming.clone(),
            replaces: replacements.get(&c.incoming.track_id).cloned(),
            credited_artist: None,
        }))
        .collect();

    // 1. Albums keyed by title *and* folder, never by track artist (so an EP
    //    with featured artists stays whole, and two "Greatest Hits" stay apart).
    let mut album_buckets: Vec<(String, String, Vec<PickTrack>)> = Vec::new();
    let mut bucket_index: HashMap<String, usize> = HashMap::new();
    for t in tracks {
        let title = display_name(t.entry.album.as_deref(), UNKNOWN_ALBUM);
        let key = format!("{}\u{1F}{}", fold_key(&title), folder(&t.entry.relative_path));
        match bucket_index.get(&key) {
            Some(&i) => album_buckets[i].2.push(t),
            None => {
                bucket_index.insert(key.clone(), album_buckets.len());
                album_buckets.push((key, title, vec![t]));
            }
        }
    }

    // 2. Each album filed under one artist.
    let mut artist_names: HashMap<String, String> = HashMap::new();
    let mut albums_by_artist: HashMap<String, Vec<PickAlbum>> = HashMap::new();
    for (album_key, title, bucket) in album_buckets {
        let artist_name = album_artist(&bucket);
        let artist_key = fold_key(&artist_name);
        artist_names.entry(artist_key.clone()).or_insert_with(|| artist_name.clone());
        let mut sorted = bucket;
        sorted.sort_by(|a, b| a.entry.relative_path.cmp(&b.entry.relative_path));
        for t in &mut sorted {
            let own = display_name(t.entry.artist.as_deref(), &artist_name);
            if fold_key(&own) != artist_key {
                t.credited_artist = Some(own);
            }
        }
        let track_ids: Vec<Uid> = sorted.iter().map(|t| t.entry.track_id).collect();
        let bytes = sorted.iter().map(|t| t.entry.file_size).sum();
        albums_by_artist.entry(artist_key.clone()).or_default().push(PickAlbum {
            id: format!("{artist_key}\u{1F}{album_key}"),
            title,
            tracks: sorted,
            track_ids,
            bytes,
        });
    }

    let mut artists: Vec<PickArtist> = albums_by_artist
        .into_iter()
        .map(|(key, mut albums)| {
            albums.sort_by(|a, b| standard_compare(&a.title, &b.title));
            PickArtist {
                name: artist_names.get(&key).cloned().unwrap_or_else(|| UNKNOWN_ARTIST.to_owned()),
                track_ids: albums.iter().flat_map(|a| a.track_ids.iter().copied()).collect(),
                bytes: albums.iter().map(|a| a.bytes).sum(),
                id: key,
                albums,
            }
        })
        .collect();
    artists.sort_by(|a, b| standard_compare(&a.name, &b.name));

    let mut playlists: Vec<PickPlaylist> = plan
        .new_playlists
        .iter()
        .map(|p| PickPlaylist { entry: p.clone(), replaces_existing: false })
        .chain(plan.playlist_conflicts.iter().map(|c| PickPlaylist { entry: c.incoming.clone(), replaces_existing: true }))
        .collect();
    playlists.sort_by(|a, b| standard_compare(&a.entry.name, &b.entry.name));
    PickList { artists, playlists }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::TrackConflict;
    use crate::protocol::Direction;
    use fl_core::AudioFileFormat;

    fn e(path: &str, artist: Option<&str>, album: Option<&str>, album_artist: Option<&str>) -> TrackManifestEntry {
        TrackManifestEntry {
            track_id: Uid::new_v4(),
            relative_path: path.into(),
            file_size: 10,
            content_hash: path.into(),
            format: AudioFileFormat::Flac,
            tag_fingerprint: String::new(),
            title: path.into(),
            artist: artist.map(Into::into),
            album: album.map(Into::into),
            album_artist: album_artist.map(Into::into),
        }
    }

    #[test]
    fn groups_eps_with_guests_under_one_artist() {
        let conflict = e("Band/EP/02.flac", Some("Band feat. Guest"), Some("EP"), None);
        let plan = SyncPlan {
            direction: Direction::Pull,
            new_tracks: vec![
                e("Band/EP/01.flac", Some("Band"), Some("EP"), None),
                e("Band/EP/03.flac", Some("Band"), Some("EP"), None),
                e("Other/Greatest Hits/01.flac", Some("Other"), Some("Greatest Hits"), None),
                e("Band/Greatest Hits/01.flac", Some("band "), Some("Greatest Hits"), None),
                e("loose.flac", None, None, None),
            ],
            track_conflicts: vec![TrackConflict { incoming: conflict.clone(), existing: conflict, differing_fields: vec!["Title".into(), "Artist".into()] }],
            new_playlists: vec![],
            playlist_conflicts: vec![],
        };
        let list = build(&plan);
        let names: Vec<&str> = list.artists.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["Band", "Other", UNKNOWN_ARTIST]);
        let band = &list.artists[0];
        assert_eq!(band.albums.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), vec!["EP", "Greatest Hits"]);
        let ep = &band.albums[0];
        assert_eq!(ep.tracks.len(), 3, "the guest track stays on its EP");
        assert_eq!(ep.tracks[1].credited_artist.as_deref(), Some("Band feat. Guest"));
        assert_eq!(ep.tracks[1].replaces.as_deref(), Some("Title, Artist"));
        assert_eq!(band.track_ids.len(), 4);
        assert_eq!(list.artists[2].albums[0].title, UNKNOWN_ALBUM);
    }

    #[test]
    fn album_artist_tag_wins_and_ties_break_alphabetically() {
        let t = |a: Option<&str>, aa: Option<&str>| PickTrack { entry: e("x/y.flac", a, Some("A"), aa), replaces: None, credited_artist: None };
        assert_eq!(album_artist(&[t(Some("X"), Some("Various")), t(Some("Y"), None)]), "Various");
        assert_eq!(album_artist(&[t(Some("Zed"), None), t(Some("Abe"), None)]), "Abe");
        assert_eq!(album_artist(&[t(None, None)]), UNKNOWN_ARTIST);
    }
}
