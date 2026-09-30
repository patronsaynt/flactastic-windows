//! `ListeningStore` — `<root>/.flactastic/listening.json`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};

use crate::apple_json::{self, AppleDate, Uid};
use crate::model::{Album, Playlist, Track};
use crate::resolvers::ArtistResolver;
use crate::text;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayEvent {
    pub id: Uid,
    /// When the track *started* playing.
    pub date: AppleDate,
    #[serde(rename = "trackID")]
    pub track_id: Uid,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(rename = "albumID", default, skip_serializing_if = "Option::is_none")]
    pub album_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_genres: Option<Vec<String>>,
    pub seconds_listened: f64,
    /// Qualified as a play under the ~90%-heard rule.
    pub counted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecentKind {
    Album,
    Playlist,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentContext {
    pub kind: RecentKind,
    /// `Album.id`, or the playlist UUID string.
    #[serde(rename = "targetID")]
    pub target_id: String,
    pub title: String,
    pub subtitle: String,
    pub date: AppleDate,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedItem {
    pub name: String,
    pub plays: usize,
    pub minutes: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumRank {
    pub album_id: String,
    pub album: String,
    pub artist: String,
    pub plays: usize,
    pub minutes: f64,
}

#[derive(Serialize, Deserialize)]
struct PersistedData {
    events: Vec<PlayEvent>,
    contexts: Vec<RecentContext>,
}

#[derive(Default)]
pub struct ListeningStore {
    pub events: Vec<PlayEvent>,
    /// Intentionally-played albums/playlists, newest last.
    pub recent_contexts: Vec<RecentContext>,
    root: Option<PathBuf>,
}

const MAX_RECENT_CONTEXTS: usize = 50;
const SESSION_GAP: f64 = 30.0 * 60.0;

impl ListeningStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn file_path(root: &Path) -> PathBuf {
        root.join(".flactastic").join("listening.json")
    }

    /// Replaces in-memory history with `root`'s. Accepts the legacy bare-array format.
    pub fn load(&mut self, root: &Path) {
        self.root = Some(root.to_path_buf());
        self.events.clear();
        self.recent_contexts.clear();
        let Ok(bytes) = std::fs::read(Self::file_path(root)) else { return };
        if let Ok(d) = serde_json::from_slice::<PersistedData>(&bytes) {
            self.events = d.events;
            self.recent_contexts = d.contexts;
        } else {
            match serde_json::from_slice::<Vec<PlayEvent>>(&bytes) {
                Ok(e) => self.events = e,
                Err(e) => log::warn!("[ListeningStore] Failed to load listening history: {e}"),
            }
        }
    }

    /// Encoded snapshot, for writing off the UI thread.
    pub fn snapshot(&self) -> Option<(PathBuf, Vec<u8>)> {
        let root = self.root.as_ref()?;
        let data = PersistedDataRef { events: &self.events, contexts: &self.recent_contexts };
        Some((Self::file_path(root), apple_json::to_vec(&data).ok()?))
    }

    pub fn save(&self) {
        if let Some((path, bytes)) = self.snapshot() {
            if let Err(e) = apple_json::write_atomic(&path, &bytes) {
                log::warn!("[ListeningStore] Failed to save listening history: {e}");
            }
        }
    }

    /// Appends an event. Negligible listens (< 1 s) that didn't count are dropped.
    /// Returns true when something was recorded (caller persists).
    pub fn record(&mut self, track: &Track, started_at: AppleDate, seconds_listened: f64, counted: bool) -> bool {
        if !counted && seconds_listened < 1.0 {
            return false;
        }
        self.events.push(PlayEvent {
            id: Uid::new_v4(),
            date: started_at,
            track_id: track.id,
            title: track.title.clone(),
            artist: track.artist.clone().or_else(|| track.album_artist.clone()),
            album_id: track.listening_album_id(),
            album: track.album.clone(),
            genre: track.genre.clone(),
            secondary_genres: Some(track.secondary_genres.clone()),
            seconds_listened,
            counted,
        });
        true
    }

    pub fn record_context_play(&mut self, kind: RecentKind, target_id: String, title: String, subtitle: String) {
        self.recent_contexts.retain(|c| !(c.kind == kind && c.target_id == target_id));
        self.recent_contexts.push(RecentContext { kind, target_id, title, subtitle, date: AppleDate::now() });
        if self.recent_contexts.len() > MAX_RECENT_CONTEXTS {
            let n = self.recent_contexts.len() - MAX_RECENT_CONTEXTS;
            self.recent_contexts.drain(..n);
        }
    }

    pub fn record_album_play(&mut self, album: &Album) {
        let subtitle = if album.is_mix_compilation() {
            "Mix Compilation".to_owned()
        } else if album.is_compilation() {
            "Compilation".to_owned()
        } else {
            ArtistResolver::display_string(album.artist.as_deref()).unwrap_or_else(|| "Unknown Artist".into())
        };
        self.record_context_play(RecentKind::Album, album.id.clone(), album.name.clone(), subtitle);
    }

    pub fn record_playlist_play(&mut self, p: &Playlist) {
        self.record_context_play(RecentKind::Playlist, p.id.uuid_string(), p.name.clone(), "Playlist".into());
    }

    // MARK: - Metrics

    pub fn has_history(&self) -> bool {
        !self.events.is_empty()
    }

    fn filtered(&self, since: Option<AppleDate>) -> impl Iterator<Item = &PlayEvent> {
        self.events.iter().filter(move |e| since.is_none_or(|s| e.date.0 >= s.0))
    }

    pub fn total_seconds_listened(&self, since: Option<AppleDate>) -> f64 {
        self.filtered(since).map(|e| e.seconds_listened).sum()
    }

    pub fn albums_played_count(&self, since: Option<AppleDate>) -> usize {
        self.filtered(since).filter(|e| e.counted).filter_map(|e| e.album_id.as_ref()).collect::<HashSet<_>>().len()
    }

    pub fn tracks_played_count(&self, since: Option<AppleDate>) -> usize {
        self.filtered(since).filter(|e| e.counted).count()
    }

    pub fn recently_played(&self, limit: usize) -> Vec<RecentContext> {
        let mut v = self.recent_contexts.clone();
        v.sort_by(|a, b| b.date.0.partial_cmp(&a.date.0).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(limit);
        v
    }

    pub fn session_count(&self, since: Option<AppleDate>) -> usize {
        let mut dates: Vec<f64> = self.filtered(since).map(|e| e.date.0).collect();
        if dates.is_empty() {
            return 0;
        }
        dates.sort_by(|a, b| a.partial_cmp(b).unwrap());
        1 + dates.windows(2).filter(|w| w[1] - w[0] > SESSION_GAP).count()
    }

    fn local_day(d: AppleDate) -> NaiveDate {
        d.to_chrono_utc().with_timezone(&Local).date_naive()
    }

    /// Consecutive days, ending today or yesterday, with at least one play.
    pub fn current_streak_days(&self) -> usize {
        self.streak_as_of(Local::now().date_naive())
    }

    pub fn streak_as_of(&self, today: NaiveDate) -> usize {
        let days: HashSet<NaiveDate> = self.events.iter().map(|e| Self::local_day(e.date)).collect();
        if days.is_empty() {
            return 0;
        }
        let mut cursor = today;
        if !days.contains(&today) {
            let y = today - Duration::days(1);
            if !days.contains(&y) {
                return 0;
            }
            cursor = y;
        }
        let mut streak = 0;
        while days.contains(&cursor) {
            streak += 1;
            cursor -= Duration::days(1);
        }
        streak
    }

    /// Minutes for each of the last 7 days, oldest → newest (index 6 = today).
    pub fn weekly_minutes(&self) -> [f64; 7] {
        let today = Local::now().date_naive();
        let mut b = [0.0; 7];
        for e in &self.events {
            let diff = (today - Self::local_day(e.date)).num_days();
            if (0..7).contains(&diff) {
                b[6 - diff as usize] += e.seconds_listened / 60.0;
            }
        }
        b
    }

    /// Short weekday labels aligned with `weekly_minutes`.
    pub fn weekly_day_labels(&self) -> [String; 7] {
        let today = Local::now().date_naive();
        std::array::from_fn(|i| {
            let d = today - Duration::days(6 - i as i64);
            ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][d.weekday().num_days_from_monday() as usize].to_owned()
        })
    }

    pub fn top_artists(&self, limit: usize, since: Option<AppleDate>) -> Vec<RankedItem> {
        let mut v = self.aggregate(|e| e.artist.as_deref(), since);
        v.truncate(limit);
        v
    }

    pub fn top_albums_this_week(&self, limit: usize) -> Vec<AlbumRank> {
        let week_ago = Local::now() - Duration::days(7);
        let week_ago = AppleDate::from_chrono(&week_ago);
        let mut by_album: HashMap<String, AlbumRank> = HashMap::new();
        for e in self.events.iter().filter(|e| e.date.0 >= week_ago.0) {
            let Some(id) = &e.album_id else { continue };
            let r = by_album.entry(id.clone()).or_insert_with(|| AlbumRank {
                album_id: id.clone(),
                album: e.album.clone().unwrap_or_else(|| "Unknown Album".into()),
                artist: e.artist.clone().unwrap_or_default(),
                plays: 0,
                minutes: 0.0,
            });
            if e.counted {
                r.plays += 1;
            }
            r.minutes += e.seconds_listened / 60.0;
        }
        let mut v: Vec<AlbumRank> = by_album.into_values().filter(|r| r.minutes > 0.0).collect();
        v.sort_by(|a, b| {
            b.minutes
                .partial_cmp(&a.minutes)
                .unwrap()
                .then(b.plays.cmp(&a.plays))
                .then_with(|| text::case_insensitive_compare(&a.album, &b.album))
        });
        v.truncate(limit);
        v
    }

    /// Primary and secondary genres weighted equally, deduped per event.
    pub fn top_genre(&self, since: Option<AppleDate>) -> Option<(String, f64)> {
        let counted: Vec<&PlayEvent> = self.filtered(since).filter(|e| e.counted).collect();
        if counted.is_empty() {
            return None;
        }
        let mut counts: HashMap<String, usize> = HashMap::new();
        for e in &counted {
            let mut seen = HashSet::new();
            let all = e.genre.iter().chain(e.secondary_genres.iter().flatten()).filter(|g| !g.is_empty());
            for g in all {
                if seen.insert(g.to_lowercase()) {
                    *counts.entry(g.clone()).or_default() += 1;
                }
            }
        }
        let (name, n) = counts.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))?;
        Some((name, n as f64 / counted.len() as f64))
    }

    fn aggregate<'a>(&'a self, key: impl Fn(&'a PlayEvent) -> Option<&'a str>, since: Option<AppleDate>) -> Vec<RankedItem> {
        let mut minutes: HashMap<&str, f64> = HashMap::new();
        let mut plays: HashMap<&str, usize> = HashMap::new();
        for e in self.filtered(since) {
            let Some(name) = key(e).filter(|n| !n.is_empty()) else { continue };
            *minutes.entry(name).or_default() += e.seconds_listened / 60.0;
            if e.counted {
                *plays.entry(name).or_default() += 1;
            }
        }
        let mut v: Vec<RankedItem> = minutes
            .into_iter()
            .map(|(n, m)| RankedItem { name: n.to_owned(), plays: plays.get(n).copied().unwrap_or(0), minutes: m })
            .collect();
        v.sort_by(|a, b| {
            if a.minutes != b.minutes {
                b.minutes.partial_cmp(&a.minutes).unwrap()
            } else {
                text::case_insensitive_compare(&a.name, &b.name)
            }
        });
        v
    }
}

#[derive(Serialize)]
struct PersistedDataRef<'a> {
    events: &'a [PlayEvent],
    contexts: &'a [RecentContext],
}

/// Local midnight of `day` as an `AppleDate` (for "since" filters).
pub fn local_midnight(day: NaiveDate) -> AppleDate {
    let dt = Local.from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap()).earliest().unwrap();
    AppleDate::from_chrono(&dt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::AudioFileFormat;

    fn ev(date: AppleDate, secs: f64, counted: bool) -> PlayEvent {
        PlayEvent {
            id: Uid::new_v4(),
            date,
            track_id: Uid::new_v4(),
            title: "t".into(),
            artist: Some("A".into()),
            album_id: Some("A|X".into()),
            album: Some("X".into()),
            genre: Some("Rock".into()),
            secondary_genres: Some(vec!["rock".into(), "Pop".into()]),
            seconds_listened: secs,
            counted,
        }
    }

    #[test]
    fn record_rules_and_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ListeningStore::new();
        s.load(dir.path());
        let t = Track::new(dir.path().join("a.flac"), "a".into(), AudioFileFormat::Flac);
        assert!(!s.record(&t, AppleDate::now(), 0.5, false));
        assert!(s.record(&t, AppleDate::now(), 0.5, true));
        s.record_context_play(RecentKind::Album, "x".into(), "X".into(), "A".into());
        s.save();
        let mut back = ListeningStore::new();
        back.load(dir.path());
        assert_eq!(back.events.len(), 1);
        assert_eq!(back.recent_contexts.len(), 1);
    }

    #[test]
    fn legacy_bare_array_loads() {
        let dir = tempfile::tempdir().unwrap();
        let p = ListeningStore::file_path(dir.path());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let legacy = serde_json::to_vec(&vec![ev(AppleDate::now(), 10.0, true)]).unwrap();
        std::fs::write(&p, legacy).unwrap();
        let mut s = ListeningStore::new();
        s.load(dir.path());
        assert_eq!(s.events.len(), 1);
    }

    #[test]
    fn metrics() {
        let now = AppleDate::now();
        let mut s = ListeningStore::new();
        s.events = vec![ev(now.adding(-4000.0), 120.0, true), ev(now, 60.0, false)];
        assert_eq!(s.session_count(None), 2);
        assert_eq!(s.tracks_played_count(None), 1);
        assert_eq!(s.albums_played_count(None), 1);
        let (g, share) = s.top_genre(None).unwrap();
        assert!(g == "Rock" || g == "Pop");
        assert_eq!(share, 1.0);
        assert_eq!(s.top_artists(5, None)[0].minutes, 3.0);
        assert!(s.current_streak_days() >= 1);
    }
}
