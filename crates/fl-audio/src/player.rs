//! `PlayerState.swift` plus the repeat handling from `ContentView.handleRepeat`:
//! shuffle/unshuffle, the user-queued section, repeat modes and the
//! listening-history tracker, layered over the gapless [`Engine`].
//!
//! The owner calls [`Player::tick`] every 50 ms (the engine's `updateTime`
//! cadence) and drains [`Player::take_plays`] into the `ListeningStore`.

use std::collections::HashSet;

use fl_core::{AppleDate, Track, Uid};
use serde::{Deserialize, Serialize};

use crate::engine::Engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

/// One finished listen, ready for `ListeningStore::record`.
#[derive(Debug, Clone)]
pub struct PlayRecord {
    pub track: Track,
    pub started_at: AppleDate,
    pub seconds_listened: f64,
    pub counted: bool,
}

/// Largest forward jump still credited as real listening (`maxCreditedStep`).
const MAX_CREDITED_STEP: f64 = 1.5;

struct Tracker {
    track: Option<Track>,
    start: AppleDate,
    listened: f64,
    last_observed: f64,
}

pub struct Player {
    pub engine: Engine,
    pub is_shuffle_enabled: bool,
    repeat_mode: RepeatMode,
    original_queue: Vec<Track>,
    pub user_queued: HashSet<Uid>,
    pub playback_source: Option<String>,
    /// Settings → Config "counted play" threshold, read live.
    pub counted_play_fraction: f64,
    tracker: Tracker,
    plays: Vec<PlayRecord>,
    was_playing: bool,
}

impl Player {
    pub fn new(engine: Engine) -> Player {
        Player {
            engine,
            is_shuffle_enabled: false,
            repeat_mode: RepeatMode::Off,
            original_queue: Vec::new(),
            user_queued: HashSet::new(),
            playback_source: None,
            counted_play_fraction: 0.90,
            tracker: Tracker { track: None, start: AppleDate::now(), listened: 0.0, last_observed: 0.0 },
            plays: Vec::new(),
            was_playing: false,
        }
    }

    pub fn repeat_mode(&self) -> RepeatMode {
        self.repeat_mode
    }

    pub fn set_repeat_mode(&mut self, mode: RepeatMode) {
        if self.repeat_mode == mode {
            return;
        }
        self.repeat_mode = mode;
        self.engine.set_repeat_one(mode == RepeatMode::One);
    }

    pub fn is_user_queued(&self, t: &Track) -> bool {
        self.user_queued.contains(&t.id)
    }

    /// Shuffling applies only to source tracks; user-queued tracks keep their
    /// "next up" slot in their original order.
    pub fn toggle_shuffle(&mut self) {
        let was = self.is_shuffle_enabled;
        self.is_shuffle_enabled = !was;

        let q = self.engine.queue();
        let Some(playing) = self.engine.current_track.clone() else { return };
        if q.is_empty() {
            return;
        }
        let cur = self.engine.current_index();
        let upcoming: &[Track] = if cur + 1 < q.len() { &q[cur + 1..] } else { &[] };
        let user_upcoming: Vec<Track> = upcoming.iter().filter(|t| self.user_queued.contains(&t.id)).cloned().collect();

        if !was {
            self.original_queue = q.clone();
            let mut rest: Vec<Track> = upcoming.iter().filter(|t| !self.user_queued.contains(&t.id)).cloned().collect();
            rest.extend(q[..cur].iter().filter(|t| !self.user_queued.contains(&t.id)).cloned());
            fastrand::shuffle(&mut rest);
            let mut nq = vec![playing];
            nq.extend(user_upcoming);
            nq.extend(rest);
            self.engine.reorder_queue(nq, 0);
        } else {
            let orig = std::mem::take(&mut self.original_queue);
            let Some(oi) = orig.iter().position(|t| t.id == playing.id) else { return };
            let before: Vec<Track> = orig[..oi].iter().filter(|t| !self.user_queued.contains(&t.id)).cloned().collect();
            let after = orig[oi + 1..].iter().filter(|t| !self.user_queued.contains(&t.id)).cloned();
            let idx = before.len();
            let mut nq = before;
            nq.push(playing);
            nq.extend(user_upcoming);
            nq.extend(after);
            self.engine.reorder_queue(nq, idx);
        }
    }

    /// Next, respecting repeat-all at the end of the queue.
    pub fn next(&mut self) {
        let q = self.engine.queue();
        if q.is_empty() {
            return;
        }
        let n = self.engine.current_index() + 1;
        if n < q.len() {
            self.engine.set_queue(q, n);
            self.engine.play();
        } else if self.repeat_mode == RepeatMode::All {
            self.engine.set_queue(q, 0);
            self.engine.play();
        } else {
            self.engine.pause();
        }
    }

    pub fn previous(&mut self) {
        self.engine.previous();
    }

    /// Play All / double-click: a fresh queue, shuffled if shuffle is on.
    pub fn start_fresh_queue(&mut self, tracks: Vec<Track>, start_at: usize, source: Option<String>) {
        self.user_queued.clear();
        self.playback_source = source;
        if self.is_shuffle_enabled && tracks.len() > 1 {
            let idx = start_at.min(tracks.len() - 1);
            self.original_queue = tracks.clone();
            let mut rest = tracks;
            let selected = rest.remove(idx);
            fastrand::shuffle(&mut rest);
            let mut q = vec![selected];
            q.extend(rest);
            self.engine.set_queue(q, 0);
        } else {
            self.original_queue.clear();
            self.engine.set_queue(tracks, start_at);
        }
    }

    pub fn play_next(&mut self, tracks: &[Track]) {
        if tracks.is_empty() {
            return;
        }
        let fresh: Vec<Track> = tracks.iter().map(Track::with_new_id).collect();
        let ids = fresh.iter().map(|t| t.id);
        if self.engine.queue().is_empty() {
            self.user_queued = ids.collect();
            self.playback_source = None;
            self.engine.set_queue(fresh, 0);
            self.engine.play();
        } else {
            self.user_queued.extend(ids);
            let at = self.engine.current_index() + 1;
            self.engine.insert_tracks(fresh, at);
        }
    }

    /// Appends to the user-queued section, ahead of the remaining source tracks.
    pub fn add_to_queue(&mut self, tracks: &[Track]) {
        if tracks.is_empty() {
            return;
        }
        let fresh: Vec<Track> = tracks.iter().map(Track::with_new_id).collect();
        let ids = fresh.iter().map(|t| t.id);
        let q = self.engine.queue();
        if q.is_empty() {
            self.user_queued = ids.collect();
            self.playback_source = None;
            self.engine.set_queue(fresh, 0);
            self.engine.play();
            return;
        }
        let mut at = self.engine.current_index() + 1;
        while at < q.len() && self.user_queued.contains(&q[at].id) {
            at += 1;
        }
        self.user_queued.extend(ids);
        if at >= q.len() {
            self.engine.append_tracks(fresh);
        } else {
            self.engine.insert_tracks(fresh, at);
        }
    }

    /// Queue-panel drag: move `source` to just before `destination`.
    pub fn move_track(&mut self, source: Uid, destination: Uid) {
        let mut q = self.engine.queue();
        let (Some(src), Some(dst)) = (q.iter().position(|t| t.id == source), q.iter().position(|t| t.id == destination)) else {
            return;
        };
        if src == dst {
            return;
        }
        let item = q.remove(src);
        let ins = if src < dst { dst - 1 } else { dst };
        q.insert(ins, item);
        let mut cur = self.engine.current_index();
        if src == cur {
            cur = ins;
        } else {
            if src < cur {
                cur -= 1;
            }
            if ins <= cur {
                cur += 1;
            }
        }
        self.engine.reorder_queue(q, cur);
    }

    /// Only upcoming tracks can be removed.
    pub fn remove_from_queue(&mut self, index: usize) {
        let q = self.engine.queue();
        if index <= self.engine.current_index() || index >= q.len() {
            return;
        }
        self.user_queued.remove(&q[index].id);
        self.engine.remove_from_queue(index);
    }

    pub fn jump_to(&mut self, index: usize) {
        let q = self.engine.queue();
        if index >= q.len() {
            return;
        }
        self.engine.set_queue(q, index);
        self.engine.play();
    }

    // MARK: - Tick

    /// The 50 ms tick: advances the engine clock, handles end-of-queue repeat,
    /// and feeds the listening tracker. Returns true when state changed.
    pub fn tick(&mut self) -> bool {
        let changed = self.engine.update_time();
        let now_playing = self.engine.is_playing;
        if self.was_playing && !now_playing && self.engine.current_track.is_some() {
            // Stopped naturally at the end (not a user pause)?
            let reached_end = self.engine.duration.is_some_and(|d| d > 0.0 && self.engine.current_time >= d - 0.5);
            if reached_end {
                self.handle_repeat();
            }
        }
        self.was_playing = self.engine.is_playing;
        self.update_listening_tracker();
        changed
    }

    fn handle_repeat(&mut self) {
        match self.repeat_mode {
            RepeatMode::Off => {}
            RepeatMode::One => {
                self.engine.seek(0.0);
                self.engine.play();
            }
            RepeatMode::All => {
                // Same queue (not a fresh one): user-queued markers survive.
                let q = self.engine.queue();
                if !q.is_empty() {
                    self.engine.set_queue(q, 0);
                    self.engine.play();
                }
            }
        }
    }

    // MARK: - Listening history

    /// Emits the in-flight listen (if any). Call before switching libraries.
    pub fn flush_pending(&mut self) {
        let Some(track) = self.tracker.track.take() else { return };
        let duration = track.duration.or(self.engine.duration);
        let fraction = self.counted_play_fraction;
        let listened = self.tracker.listened;
        let counted = if fraction <= 0.0 {
            listened > 0.0
        } else if let Some(d) = duration.filter(|d| *d > 0.0) {
            listened >= d * fraction
        } else {
            listened >= 240.0 * fraction
        };
        self.plays.push(PlayRecord { track, started_at: self.tracker.start, seconds_listened: listened, counted });
        self.tracker.listened = 0.0;
        self.tracker.last_observed = 0.0;
    }

    fn arm(&mut self, track: Option<Track>) {
        let Some(track) = track else { return };
        self.tracker = Tracker { track: Some(track), start: AppleDate::now(), listened: 0.0, last_observed: self.engine.current_time };
    }

    fn update_listening_tracker(&mut self) {
        let engine_track = self.engine.current_track.clone();
        let now = self.engine.current_time;
        if engine_track.as_ref().map(|t| t.id) != self.tracker.track.as_ref().map(|t| t.id) {
            self.flush_pending();
            self.arm(engine_track);
            return;
        }
        let Some(tracked) = &self.tracker.track else { return };
        // A repeat: jumped from near the end back to the start.
        let duration = tracked.duration.or(self.engine.duration);
        // The Mac check lacks `now < last_observed`, so tracks under 2 s re-fire
        // every tick (MAC-ISSUES.md).
        if duration.is_some_and(|d| d > 0.0 && self.tracker.last_observed >= d - 2.0 && now < 2.0 && now < self.tracker.last_observed) {
            self.flush_pending();
            self.arm(engine_track);
            return;
        }
        let step = now - self.tracker.last_observed;
        if self.engine.is_playing && step > 0.0 && step <= MAX_CREDITED_STEP {
            self.tracker.listened += step;
        }
        self.tracker.last_observed = now;
    }

    /// Plays finished since the last call, for `ListeningStore::record`.
    pub fn take_plays(&mut self) -> Vec<PlayRecord> {
        std::mem::take(&mut self.plays)
    }
}
