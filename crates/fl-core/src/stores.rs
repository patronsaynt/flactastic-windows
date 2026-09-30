//! App-data stores (not per-library): artist overrides, Deezer image cache,
//! lrclib lyrics cache, and the pinned Home highlight.
//!
//! Each lives in the app data directory (`%APPDATA%\FLACtastic` /
//! `$XDG_DATA_HOME/flactastic`) with the Mac's file names.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::apple_json::{self, b64, AppleDate};
use crate::lyrics::Lyrics;
use crate::resolvers::ArtistResolver;
use crate::text::trim;

const POSITIVE_TTL: f64 = 60.0 * 60.0 * 24.0 * 30.0;
const NEGATIVE_TTL: f64 = 60.0 * 60.0 * 24.0 * 7.0;

fn load_list<T: serde::de::DeserializeOwned>(path: &Path, tag: &str) -> Vec<T> {
    match apple_json::load::<Vec<T>>(path) {
        Ok(v) => v.unwrap_or_default(),
        Err(e) => {
            log::warn!("[{tag}] Failed to load: {e}");
            Vec::new()
        }
    }
}

// MARK: - ArtistStore

/// Display-only overrides, keyed by `ArtistResolver::key`. Never written to tags.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistOverride {
    pub canonical_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "b64::opt")]
    pub banner_image: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "b64::opt")]
    pub profile_image: Option<Vec<u8>>,
}

impl ArtistOverride {
    pub fn is_empty(&self) -> bool {
        self.display_name.as_deref().map(trim).unwrap_or("").is_empty()
            && self.banner_image.is_none()
            && self.profile_image.is_none()
    }
}

pub struct ArtistStore {
    pub overrides: HashMap<String, ArtistOverride>,
    file: PathBuf,
}

impl ArtistStore {
    pub fn new(app_data: &Path) -> Self {
        ArtistStore { overrides: HashMap::new(), file: app_data.join("artists.json") }
    }

    pub fn load(&mut self) {
        self.overrides = load_list::<ArtistOverride>(&self.file, "ArtistStore")
            .into_iter()
            .map(|o| (o.canonical_key.clone(), o))
            .collect();
    }

    pub fn save(&self) {
        let mut list: Vec<&ArtistOverride> = self.overrides.values().collect();
        list.sort_by(|a, b| a.canonical_key.cmp(&b.canonical_key));
        if let Err(e) = apple_json::save(&self.file, &list) {
            log::warn!("[ArtistStore] Failed to save artist overrides: {e}");
        }
    }

    pub fn get(&self, key: &str) -> Option<&ArtistOverride> {
        self.overrides.get(key)
    }

    pub fn upsert(&mut self, o: ArtistOverride) {
        if o.is_empty() {
            self.overrides.remove(&o.canonical_key);
        } else {
            self.overrides.insert(o.canonical_key.clone(), o);
        }
        self.save();
    }

    pub fn remove(&mut self, key: &str) {
        self.overrides.remove(key);
        self.save();
    }

    /// Display-name overrides for `library::all_artists`.
    pub fn display_overrides(&self) -> HashMap<String, String> {
        self.overrides
            .iter()
            .filter_map(|(k, o)| o.display_name.clone().map(|n| (k.clone(), n)))
            .collect()
    }

    /// User override wins, then the remote cache.
    pub fn resolved_profile_image<'a>(&'a self, key: &str, remote: &'a ArtistRemoteCache) -> Option<&'a [u8]> {
        if let Some(d) = self.overrides.get(key).and_then(|o| o.profile_image.as_deref()) {
            return Some(d);
        }
        remote.entries.get(key).and_then(|e| e.profile_image.as_deref())
    }
}

// MARK: - ArtistRemoteCache

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRemoteEntry {
    pub canonical_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deezer_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "b64::opt")]
    pub profile_image: Option<Vec<u8>>,
    pub fetched_at: AppleDate,
    /// Negative cache: Deezer had no usable match.
    pub not_found: bool,
}

pub struct ArtistRemoteCache {
    pub entries: HashMap<String, ArtistRemoteEntry>,
    file: PathBuf,
}

impl ArtistRemoteCache {
    pub fn new(app_data: &Path) -> Self {
        ArtistRemoteCache { entries: HashMap::new(), file: app_data.join("artist_remote_cache.json") }
    }

    pub fn load(&mut self) {
        self.entries = load_list::<ArtistRemoteEntry>(&self.file, "ArtistRemoteCache")
            .into_iter()
            .map(|e| (e.canonical_key.clone(), e))
            .collect();
    }

    pub fn save(&self) {
        let mut list: Vec<&ArtistRemoteEntry> = self.entries.values().collect();
        list.sort_by(|a, b| a.canonical_key.cmp(&b.canonical_key));
        if let Err(e) = apple_json::save(&self.file, &list) {
            log::warn!("[ArtistRemoteCache] Failed to save: {e}");
        }
    }

    pub fn set(&mut self, e: ArtistRemoteEntry) {
        self.entries.insert(e.canonical_key.clone(), e);
        self.save();
    }

    pub fn is_fresh(&self, key: &str, now: AppleDate) -> bool {
        self.entries.get(key).is_some_and(|e| {
            let age = now.since(e.fetched_at);
            age < if e.not_found { NEGATIVE_TTL } else { POSITIVE_TTL }
        })
    }
}

// MARK: - LyricsRemoteCache

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsCacheEntry {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plain_lyrics: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced_lyrics: Option<String>,
    pub fetched_at: AppleDate,
    pub not_found: bool,
}

/// `"<artistKey>|<titleKey>|<rounded seconds or -1>"`. Album is excluded.
pub fn lyrics_cache_key(artist: Option<&str>, title: Option<&str>, duration: Option<f64>) -> String {
    let a = ArtistResolver::key(artist.unwrap_or(""));
    let t = ArtistResolver::key(title.unwrap_or(""));
    let d = duration.map(|d| d.round() as i64).unwrap_or(-1);
    format!("{a}|{t}|{d}")
}

pub struct LyricsRemoteCache {
    pub entries: HashMap<String, LyricsCacheEntry>,
    file: PathBuf,
}

impl LyricsRemoteCache {
    pub fn new(app_data: &Path) -> Self {
        LyricsRemoteCache { entries: HashMap::new(), file: app_data.join("lyrics_cache.json") }
    }

    pub fn load(&mut self) {
        self.entries = load_list::<LyricsCacheEntry>(&self.file, "LyricsRemoteCache")
            .into_iter()
            .map(|e| (e.key.clone(), e))
            .collect();
    }

    pub fn save(&self) {
        let mut list: Vec<&LyricsCacheEntry> = self.entries.values().collect();
        list.sort_by(|a, b| a.key.cmp(&b.key));
        if let Err(e) = apple_json::save(&self.file, &list) {
            log::warn!("[LyricsRemoteCache] Failed to save: {e}");
        }
    }

    pub fn set(&mut self, e: LyricsCacheEntry) {
        self.entries.insert(e.key.clone(), e);
        self.save();
    }

    pub fn is_fresh(&self, key: &str, now: AppleDate) -> bool {
        self.entries.get(key).is_some_and(|e| {
            let age = now.since(e.fetched_at);
            age < if e.not_found { NEGATIVE_TTL } else { POSITIVE_TTL }
        })
    }
}

// MARK: - HomeHighlight

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightPick {
    pub lyric: String,
    pub song_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist_display: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "b64::opt")]
    pub image_data: Option<Vec<u8>>,
}

pub fn pinned_highlight_path(app_data: &Path) -> PathBuf {
    app_data.join("pinned_highlight.json")
}

/// Bound on embedded-lyrics file reads when no cached lyrics yield a line.
pub const HIGHLIGHT_FILE_READ_CAP: usize = 12;

/// Display-worthy lyric lines: 6–130 characters, not section markers.
pub fn highlight_candidate_lines(raw: &str, duration: Option<f64>) -> Vec<String> {
    let parsed = if looks_like_lrc(raw) {
        Lyrics::parse_lrc(raw)
    } else {
        Lyrics::from_plain_text(raw, duration.unwrap_or(0.0))
    };
    parsed
        .lines
        .iter()
        .map(|l| trim(&l.text).to_owned())
        .filter(|t| is_good_line(t))
        .map(|t| sanitize_quotes(&t))
        .collect()
}

/// `\[\d{1,2}:\d{2}` anywhere in the text.
fn looks_like_lrc(raw: &str) -> bool {
    let b = raw.as_bytes();
    (0..b.len()).any(|i| {
        if b[i] != b'[' {
            return false;
        }
        let rest = &b[i + 1..];
        let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
        (1..=2).contains(&digits)
            && rest.get(digits) == Some(&b':')
            && rest.get(digits + 1).is_some_and(u8::is_ascii_digit)
            && rest.get(digits + 2).is_some_and(u8::is_ascii_digit)
    })
}

fn is_good_line(t: &str) -> bool {
    let n = t.chars().count();
    if !(6..=130).contains(&n) {
        return false;
    }
    if t.starts_with('[') {
        return false;
    }
    !(t.starts_with('(') && t.ends_with(')'))
}

fn sanitize_quotes(t: &str) -> String {
    t.replace(['"', '\u{201C}', '\u{201D}'], "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artist_store_round_trip_and_empty_removal() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ArtistStore::new(dir.path());
        s.upsert(ArtistOverride {
            canonical_key: "a".into(),
            display_name: Some("A!".into()),
            banner_image: None,
            profile_image: Some(vec![1, 2, 3]),
        });
        let mut back = ArtistStore::new(dir.path());
        back.load();
        assert_eq!(back.get("a").unwrap().profile_image.as_deref(), Some(&[1u8, 2, 3][..]));
        back.upsert(ArtistOverride { canonical_key: "a".into(), display_name: Some("  ".into()), banner_image: None, profile_image: None });
        assert!(back.get("a").is_none());
    }

    #[test]
    fn freshness() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = LyricsRemoteCache::new(dir.path());
        let now = AppleDate::now();
        c.entries.insert(
            "k".into(),
            LyricsCacheEntry { key: "k".into(), plain_lyrics: None, synced_lyrics: None, fetched_at: now.adding(-8.0 * 86400.0), not_found: true },
        );
        assert!(!c.is_fresh("k", now));
        c.entries.get_mut("k").unwrap().not_found = false;
        assert!(c.is_fresh("k", now));
    }

    #[test]
    fn cache_key() {
        assert_eq!(lyrics_cache_key(Some("Beyoncé"), Some("Halo "), Some(261.6)), "beyonce|halo|262");
        assert_eq!(lyrics_cache_key(None, None, None), "||-1");
    }

    #[test]
    fn highlight_lines() {
        let lines = highlight_candidate_lines("[00:01.00][Chorus]\n[00:02.00]He said \"hello\" to me\n[00:03.00]short", None);
        assert_eq!(lines, vec!["He said 'hello' to me"]);
        assert!(looks_like_lrc("x [3:05] y"));
        assert!(!looks_like_lrc("[Chorus]"));
    }
}
