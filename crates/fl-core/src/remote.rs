//! Streaming-service track models and the "already in library" duplicate
//! guard shared by downloads and the Spotify playlist rebuild.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::import_copy::is_reserved_windows_name;
use crate::model::Track;
use crate::text::trim;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteArtist {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub picture_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCoverArt {
    pub url: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

impl RemoteCoverArt {
    /// Largest known area wins; unknown sizes rank last.
    pub fn best(arts: &[RemoteCoverArt]) -> Option<&RemoteCoverArt> {
        arts.iter().max_by_key(|a| a.width.unwrap_or(0) * a.height.unwrap_or(0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAlbumRef {
    pub id: String,
    pub title: String,
    pub url: Option<String>,
    pub cover_art: Vec<RemoteCoverArt>,
    pub release_year: Option<i64>,
    pub track_count: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTrack {
    pub id: String,
    pub title: String,
    pub artists: Vec<RemoteArtist>,
    pub album: Option<RemoteAlbumRef>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub duration_seconds: Option<f64>,
    pub cover_art: Vec<RemoteCoverArt>,
    pub url: Option<String>,
    /// "qobuz", "deezer", "soundcloud", …
    #[serde(rename = "serviceID")]
    pub service_id: String,
    pub is_lossless: bool,
}

fn normalize(s: &str) -> String {
    trim(s).to_lowercase()
}

/// "Song - Radio Edit" and "Song (Radio Edit)" compare equal.
fn loose_title(s: &str) -> String {
    let mut t = s.to_lowercase();
    for ch in ['(', ')', '[', ']'] {
        t = t.replace(ch, " ");
    }
    t = t.replace(" - ", " ");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn artist_tokens(raw: Option<&str>) -> HashSet<String> {
    let Some(raw) = raw.filter(|s| !s.is_empty()) else { return HashSet::new() };
    let mut w = raw.to_lowercase();
    for m in [" feat.", " feat ", " ft.", " ft ", " featuring "] {
        w = w.replace(m, ";");
    }
    w.split([';', ',', '&', '/']).map(|p| trim(p).to_owned()).filter(|p| !p.is_empty()).collect()
}

/// Strips a leading `^\d{1,3}\s*-\s*` track-number prefix from a file stem.
fn strip_leading_track_number(stem: &str) -> &str {
    let digits = stem.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=3).contains(&digits) {
        return stem;
    }
    let rest = stem[digits..].trim_start_matches(|c: char| c.is_whitespace());
    match rest.strip_prefix('-') {
        Some(r) => r.trim_start_matches(|c: char| c.is_whitespace()),
        None => stem,
    }
}

/// Shares an artist, or — only when one side has no artist info — the album.
fn overlaps(t: &Track, remote_artists: &HashSet<String>, remote_album: &str) -> bool {
    let mut lib = artist_tokens(t.artist.as_deref());
    lib.extend(artist_tokens(t.album_artist.as_deref()));
    if !remote_artists.is_empty() && !lib.is_empty() {
        return !remote_artists.is_disjoint(&lib);
    }
    !remote_album.is_empty() && normalize(t.album.as_deref().unwrap_or("")) == remote_album
}

/// Tiered title match (exact file name, exact tag title, then loose variants
/// that must share an artist/album). A title-only hit counts only when the
/// remote track has nothing to disambiguate on.
pub fn find_existing_match<'a>(remote: &RemoteTrack, tracks: &'a [Track]) -> Option<&'a Track> {
    let exact = normalize(&remote.title);
    if exact.is_empty() {
        return None;
    }
    let loose = loose_title(&remote.title);
    let names: Vec<&str> = remote.artists.iter().map(|a| a.name.as_str()).collect();
    let remote_artists = artist_tokens(Some(&names.join("; ")));
    let remote_album = remote.album.as_ref().map(|a| normalize(&a.title)).unwrap_or_default();

    struct Keys<'a> {
        t: &'a Track,
        file_exact: String,
        file_loose: String,
        tag_exact: String,
        tag_loose: String,
    }
    let keyed: Vec<Keys> = tracks
        .iter()
        .map(|t| {
            let stem = t.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let stem = strip_leading_track_number(&stem);
            Keys { t, file_exact: normalize(stem), file_loose: loose_title(stem), tag_exact: normalize(&t.title), tag_loose: loose_title(&t.title) }
        })
        .collect();

    let can_disambiguate = !remote_artists.is_empty() || !remote_album.is_empty();
    type Get = for<'k> fn(&'k Keys<'_>) -> &'k str;
    let tiers: [(&str, Get, bool); 4] = [
        (&exact, |k| &k.file_exact, false),
        (&exact, |k| &k.tag_exact, false),
        (&loose, |k| &k.file_loose, true),
        (&loose, |k| &k.tag_loose, true),
    ];
    for (key, get, require_overlap) in tiers {
        if key.is_empty() {
            continue;
        }
        let hits: Vec<&Track> = keyed.iter().filter(|k| get(k) == key).map(|k| k.t).collect();
        if hits.is_empty() {
            continue;
        }
        if let Some(m) = hits.iter().find(|t| overlaps(t, &remote_artists, &remote_album)) {
            return Some(m);
        }
        if require_overlap || can_disambiguate {
            continue;
        }
        return Some(hits[0]);
    }
    None
}

/// Download path component: the Mac maps `/` and `:` to `-`; the other
/// Windows-illegal characters are mapped the same way here.
pub fn download_sanitize(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if matches!(c, '/' | ':' | '\\' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '-' } else { c })
        .collect();
    let t = trim(&cleaned).trim_end_matches(['.', ' ']);
    if t.is_empty() {
        return "Untitled".into();
    }
    if t.starts_with('.') || is_reserved_windows_name(t) {
        format!("_{t}")
    } else {
        t.to_owned()
    }
}

/// `<root>/<first artist>/<album or Singles>/NN - <title>.<ext>`.
pub fn download_destination(root: &Path, track: &RemoteTrack, ext: &str) -> PathBuf {
    let artist = download_sanitize(track.artists.first().map(|a| a.name.as_str()).unwrap_or("Unknown Artist"));
    let album = download_sanitize(track.album.as_ref().map(|a| a.title.as_str()).unwrap_or("Singles"));
    let prefix = track.track_number.map(|n| format!("{n:02} - ")).unwrap_or_default();
    let file = format!("{}.{ext}", download_sanitize(&format!("{prefix}{}", track.title)));
    root.join(artist).join(album).join(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::AudioFileFormat;

    fn remote(title: &str, artists: &[&str], album: Option<&str>) -> RemoteTrack {
        RemoteTrack {
            id: "r".into(),
            title: title.into(),
            artists: artists
                .iter()
                .map(|n| RemoteArtist { id: "a".into(), name: (*n).into(), url: None, picture_url: None })
                .collect(),
            album: album.map(|t| RemoteAlbumRef { id: "b".into(), title: t.into(), url: None, cover_art: vec![], release_year: None, track_count: None }),
            track_number: None,
            disc_number: None,
            duration_seconds: None,
            cover_art: vec![],
            url: None,
            service_id: "test".into(),
            is_lossless: true,
        }
    }

    fn lib(title: &str, artist: Option<&str>, album: Option<&str>, filename: Option<&str>) -> Track {
        let p = PathBuf::from(format!(
            "/music/{}/{}/{}.flac",
            artist.unwrap_or("Unknown"),
            album.unwrap_or("Singles"),
            filename.unwrap_or(title)
        ));
        let mut t = Track::new(p, title.into(), AudioFileFormat::Flac);
        t.artist = artist.map(Into::into);
        t.album = album.map(Into::into);
        t
    }

    // DuplicateMatchTests

    #[test]
    fn same_title_different_artist_is_not_duplicate() {
        let l = [lib("Song", Some("Artist 2"), None, None)];
        assert!(find_existing_match(&remote("Song", &["Artist 1"], None), &l).is_none());
    }

    #[test]
    fn same_title_same_artist_is_duplicate() {
        let l = [lib("Song", Some("Artist 1"), None, None)];
        assert!(find_existing_match(&remote("Song", &["Artist 1"], None), &l).is_some());
    }

    #[test]
    fn artist_match_is_case_insensitive() {
        let l = [lib("Song", Some("ARTIST ONE"), None, None)];
        assert!(find_existing_match(&remote("Song", &["artist one"], None), &l).is_some());
    }

    #[test]
    fn multi_artist_tag_matches_primary() {
        let l = [lib("Song", Some("Artist 1; Guest"), None, None)];
        assert!(find_existing_match(&remote("Song", &["Artist 1"], None), &l).is_some());
    }

    #[test]
    fn loose_title_requires_artist_overlap() {
        let l = [lib("Song - Radio Edit", Some("Artist 2"), None, None)];
        assert!(find_existing_match(&remote("Song (Radio Edit)", &["Artist 1"], None), &l).is_none());
    }

    #[test]
    fn file_name_match_with_different_artist_is_not_duplicate() {
        let l = [lib("Song", Some("Artist 2"), None, Some("Song"))];
        assert!(find_existing_match(&remote("Song", &["Artist 1"], None), &l).is_none());
    }

    #[test]
    fn title_only_accepted_without_remote_artist_info() {
        let l = [lib("Song", Some("Artist 2"), None, None)];
        assert!(find_existing_match(&remote("Song", &[], None), &l).is_some());
    }

    #[test]
    fn self_titled_singles_by_different_artists_are_not_duplicates() {
        let l = [lib("Kiss", Some("Lil Peep"), Some("Kiss"), None)];
        assert!(find_existing_match(&remote("Kiss", &["Westwood"], Some("Kiss")), &l).is_none());
    }

    #[test]
    fn album_overlap_disambiguates_when_artist_missing() {
        let l = [lib("Song", None, Some("The Album"), None)];
        assert!(find_existing_match(&remote("Song", &["Artist 1"], Some("The Album")), &l).is_some());
    }

    #[test]
    fn leading_track_number_is_stripped() {
        assert_eq!(strip_leading_track_number("03 - Song"), "Song");
        assert_eq!(strip_leading_track_number("1234 - Song"), "1234 - Song");
        assert_eq!(strip_leading_track_number("99 Luftballons"), "99 Luftballons");
    }
}
