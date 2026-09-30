//! Album grouping (`LibraryStore.buildAlbums`), artist roll-up
//! (`LibraryStore+Artists`) and tag normalisation.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::model::{artwork_content_id, Album, ArtistSummary, Artwork, Track};
use crate::resolvers::ArtistResolver;
use crate::text;

/// Album display-name overrides (`ArtistOverride.displayName`), by canonical key.
pub type DisplayOverrides = HashMap<String, String>;

/// Groups tracks into albums.
///
/// Pass 1 groups by lowercased album name. Pass 2 subdivides by album artist
/// only when at least one track in the name group carries one, so a
/// compilation with per-track artists and no album artist stays one album.
pub fn build_albums(tracks: &[Track], artwork_cache: &HashMap<String, Option<Artwork>>) -> Vec<Album> {
    let mut by_name: IndexMap<String, Vec<&Track>> = IndexMap::new();
    for t in tracks {
        by_name
            .entry(t.album.as_deref().unwrap_or("Unknown Album").to_lowercase())
            .or_default()
            .push(t);
    }

    let mut result = Vec::new();
    for (_, name_group) in by_name {
        let has_album_artist = name_group.iter().any(|t| t.album_artist.is_some());
        let subgroups: Vec<Vec<&Track>> = if has_album_artist {
            let mut sub: IndexMap<&str, Vec<&Track>> = IndexMap::new();
            for t in name_group {
                sub.entry(t.album_artist.as_deref().unwrap_or("Unknown Artist")).or_default().push(t);
            }
            sub.into_values().collect()
        } else {
            vec![name_group]
        };

        for mut group in subgroups {
            group.sort_by_key(|t| t.track_number.unwrap_or(i64::MAX));
            let first = group[0];
            let aa = group.iter().find_map(|t| t.album_artist.clone());
            // Compare lead credits so one "A ; Guest" track doesn't make the
            // album read as "Various Artists".
            let mut distinct: IndexMap<String, ()> = IndexMap::new();
            for t in &group {
                if let Some(a) = &t.artist {
                    distinct.insert(ArtistResolver::primary_credit(a), ());
                }
            }
            let display_artist = if aa.is_some() {
                aa.clone()
            } else if distinct.len() > 1 {
                Some("Various Artists".to_owned())
            } else {
                distinct.keys().next().cloned()
            };
            let name = first.album.clone().unwrap_or_else(|| "Unknown Album".into());
            let key_artist = aa.clone().or_else(|| display_artist.clone()).unwrap_or_else(|| "Unknown Artist".into());
            let id = format!("{key_artist}|{name}");
            let tracks: Vec<Track> = group.iter().map(|t| (*t).clone()).collect();
            let artwork = match artwork_cache.get(&id) {
                Some(cached) => cached.clone(),
                None => dominant_artwork(&tracks),
            };
            result.push(Album {
                id,
                name,
                artist: display_artist,
                album_artist: aa,
                year: first.year,
                genre: first.genre.clone(),
                secondary_genres: first.secondary_genres.clone(),
                artwork,
                tracks,
            });
        }
    }

    result.sort_by(|a, b| text::standard_compare(&a.name, &b.name));
    result
}

/// The artwork shared by the most tracks; ties go to the first seen.
pub fn dominant_artwork(tracks: &[Track]) -> Option<Artwork> {
    let mut counts: IndexMap<String, (usize, Artwork)> = IndexMap::new();
    for t in tracks {
        let Some(art) = &t.artwork else { continue };
        counts
            .entry(artwork_content_id(art))
            .and_modify(|(c, _)| *c += 1)
            .or_insert((1, art.clone()));
    }
    // First maximum in insertion order wins the tie.
    let mut best: Option<&(usize, Artwork)> = None;
    for v in counts.values() {
        if best.is_none_or(|b| v.0 > b.0) {
            best = Some(v);
        }
    }
    best.map(|(_, a)| a.clone())
}

/// Collapses identical embedded covers onto one shared buffer. Returns true
/// when anything changed.
pub fn deduplicate_artwork(tracks: &mut [Track]) -> bool {
    let mut canon: HashMap<String, Artwork> = HashMap::new();
    let mut changed = false;
    for t in tracks.iter_mut() {
        let Some(art) = &t.artwork else { continue };
        let key = artwork_content_id(art);
        match canon.get(&key) {
            Some(c) => {
                if !std::sync::Arc::ptr_eq(c, art) {
                    t.artwork = Some(c.clone());
                    changed = true;
                }
            }
            None => {
                canon.insert(key, art.clone());
            }
        }
    }
    changed
}

/// Rewrites multi-artist credits to the explicit `" ; "` form, in memory only,
/// when every fragment is already a known artist. Two passes so names
/// confirmed in the first help resolve the second. Returns true on change.
pub fn normalise_artist_tags(tracks: &mut [Track]) -> bool {
    let mut did_change = false;
    for _ in 0..2 {
        let resolver = ArtistResolver::new(tracks);
        let mut pass_changed = false;
        for t in tracks.iter_mut() {
            let Some(raw) = t.artist.as_deref().filter(|s| !s.is_empty()) else { continue };
            if ArtistResolver::explicitly_separated(raw).is_some() {
                continue;
            }
            let pieces = resolver.split(Some(raw));
            if pieces.len() <= 1 {
                continue;
            }
            let joined = ArtistResolver::join_explicit(&pieces);
            if joined != raw {
                t.artist = Some(joined);
                pass_changed = true;
            }
        }
        if !pass_changed {
            break;
        }
        did_change = true;
    }
    did_change
}

/// Newest year first, then name.
pub fn album_order(a: &Album, b: &Album) -> std::cmp::Ordering {
    let (ya, yb) = (a.year.unwrap_or(0), b.year.unwrap_or(0));
    if ya != yb {
        return yb.cmp(&ya);
    }
    text::standard_compare(&a.name, &b.name)
}

/// Every artist with their albums (≥ 3 tracks), singles/EPs, and appears-on.
pub fn all_artists(albums: &[Album], resolver: &ArtistResolver, overrides: &DisplayOverrides) -> Vec<ArtistSummary> {
    struct Bucket {
        display_name: String,
        albums: Vec<Album>,
        singles: Vec<Album>,
        appears_on: Vec<Album>,
        album_ids: HashSet<String>,
        appears_on_ids: HashSet<String>,
        track_count: usize,
        artwork_sample: Option<Artwork>,
    }
    let new_bucket = |key: &str| Bucket {
        display_name: overrides
            .get(key)
            .map(|s| text::trim(s).to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| resolver.display_name(key)),
        albums: vec![],
        singles: vec![],
        appears_on: vec![],
        album_ids: HashSet::new(),
        appears_on_ids: HashSet::new(),
        track_count: 0,
        artwork_sample: None,
    };

    let mut buckets: IndexMap<String, Bucket> = IndexMap::new();

    for album in albums {
        // Compilations skip album-artist bucketing; performers still get
        // them under "Appears On".
        let primary_source = album.album_artist.as_deref().or(album.artist.as_deref());
        let primary_keys = if album.is_compilation() { vec![] } else { resolver.keys_for_credit(primary_source) };
        let primary_set: HashSet<&String> = primary_keys.iter().collect();

        for key in &primary_keys {
            let b = buckets.entry(key.clone()).or_insert_with(|| new_bucket(key));
            if b.album_ids.insert(album.id.clone()) {
                if album.tracks.len() >= 3 {
                    b.albums.push(album.clone());
                } else {
                    b.singles.push(album.clone());
                }
            }
            b.track_count += album.tracks.len();
            if b.artwork_sample.is_none() {
                b.artwork_sample = album.artwork.clone();
            }
        }

        for track in &album.tracks {
            for key in resolver.keys_for_credit(track.artist.as_deref()) {
                if primary_set.contains(&key) {
                    continue;
                }
                let b = buckets.entry(key.clone()).or_insert_with(|| new_bucket(&key));
                if !b.album_ids.contains(&album.id) && b.appears_on_ids.insert(album.id.clone()) {
                    b.appears_on.push(album.clone());
                }
                b.track_count += 1;
                if b.artwork_sample.is_none() {
                    b.artwork_sample = album.artwork.clone();
                }
            }
        }
    }

    let mut out: Vec<ArtistSummary> = buckets
        .into_iter()
        .map(|(key, mut b)| {
            b.albums.sort_by(album_order);
            b.singles.sort_by(album_order);
            b.appears_on.sort_by(album_order);
            ArtistSummary {
                id: key,
                display_name: b.display_name,
                albums: b.albums,
                singles: b.singles,
                appears_on: b.appears_on,
                track_count: b.track_count,
                artwork_sample: b.artwork_sample,
            }
        })
        .collect();
    out.sort_by(|a, b| text::standard_compare(&a.display_name, &b.display_name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::AudioFileFormat;

    fn trk(album: &str, artist: Option<&str>, aa: Option<&str>, n: i64) -> Track {
        let mut t = Track::new(format!("/l/{album}/{n}.flac").into(), format!("T{n}"), AudioFileFormat::Flac);
        t.album = Some(album.into());
        t.artist = artist.map(Into::into);
        t.album_artist = aa.map(Into::into);
        t.track_number = Some(n);
        t
    }

    #[test]
    fn guest_credit_does_not_make_various_artists() {
        let mut tracks: Vec<Track> = (1..=9).map(|n| trk("For Lack", Some("Deadmau5"), None, n)).collect();
        tracks.push(trk("For Lack", Some("Deadmau5 ; Rob Swire"), None, 10));
        let albums = build_albums(&tracks, &HashMap::new());
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].artist.as_deref(), Some("Deadmau5"));
        assert_eq!(albums[0].id, "Deadmau5|For Lack");
    }

    #[test]
    fn mixed_album_reads_various_artists() {
        let tracks = vec![
            trk("Mix", Some("Deadmau5"), None, 1),
            trk("Mix", Some("Eric Prydz"), None, 2),
            trk("Mix", Some("Adam Beyer"), None, 3),
        ];
        let albums = build_albums(&tracks, &HashMap::new());
        assert_eq!(albums[0].artist.as_deref(), Some("Various Artists"));
    }

    #[test]
    fn album_artist_tag_wins_and_splits_groups() {
        let tracks = vec![
            trk("Greatest Hits", Some("A"), Some("A"), 1),
            trk("Greatest Hits", Some("B"), Some("B"), 1),
        ];
        let albums = build_albums(&tracks, &HashMap::new());
        assert_eq!(albums.len(), 2);
        let tracks = vec![trk("X", Some("Deadmau5"), Some("deadmau5"), 1), trk("X", Some("Eric Prydz"), None, 2)];
        let albums = build_albums(&tracks, &HashMap::new());
        // The track without album artist forms its own "Unknown Artist" sub-group.
        assert_eq!(albums.len(), 2);
        assert!(albums.iter().any(|a| a.artist.as_deref() == Some("deadmau5")));
    }

    #[test]
    fn artist_roll_up_buckets() {
        let mut tracks: Vec<Track> = (1..=3).map(|n| trk("LP", Some("Solo"), None, n)).collect();
        tracks.push(trk("Single", Some("Solo"), None, 1));
        tracks.push(trk("Other LP", Some("Band"), None, 1));
        tracks.push(trk("Other LP", Some("Band ; Solo"), None, 2));
        tracks.push(trk("Other LP", Some("Band"), None, 3));
        let albums = build_albums(&tracks, &HashMap::new());
        let r = ArtistResolver::new(&tracks);
        let artists = all_artists(&albums, &r, &HashMap::new());
        let solo = artists.iter().find(|a| a.id == "solo").unwrap();
        assert_eq!(solo.albums.len(), 1);
        assert_eq!(solo.singles.len(), 1);
        assert_eq!(solo.appears_on.len(), 1);
        assert_eq!(solo.appears_on[0].name, "Other LP");
    }

    #[test]
    fn normalise_rewrites_known_collabs() {
        let mut tracks = vec![
            trk("a", Some("Skrillex"), None, 1),
            trk("b", Some("Diplo"), None, 1),
            trk("c", Some("Skrillex & Diplo"), None, 1),
            trk("d", Some("Earth, Wind & Fire"), None, 1),
        ];
        assert!(normalise_artist_tags(&mut tracks));
        assert_eq!(tracks[2].artist.as_deref(), Some("Skrillex ; Diplo"));
        // Mirrors the Mac (see docs/MAC-ISSUES.md).
        assert_eq!(tracks[3].artist.as_deref(), Some("Earth ; Wind ; Fire"));
    }
}
