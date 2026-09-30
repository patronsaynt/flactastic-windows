//! The player actor: one thread owns the `Player` (engine + queue logic) and
//! the `OutputManager`, runs the 50 ms tick, and publishes state.
//!
//! Commands arrive over a channel, so the Tauri command handlers never touch
//! the engine directly and never block on audio work.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use fl_audio::output::{Backend, DeviceEvent};
use fl_audio::output_manager::{OutputManager, OutputSelection, OutputStatus};
use fl_audio::player::{PlayRecord, Player, RepeatMode};
use fl_audio::spectrum::{Analyzer, BIN_COUNT};
use fl_audio::Engine;
use fl_core::{Track, Uid};
use serde::Serialize;

pub enum Cmd {
    /// `shuffle`: set the shuffle flag first (album Play / Shuffle buttons).
    StartQueue { tracks: Vec<Track>, start: usize, source: Option<String>, play: bool, shuffle: Option<bool> },
    PlayNext(Vec<Track>),
    AddToQueue(Vec<Track>),
    TogglePlayPause,
    Play,
    Pause,
    Next,
    Previous,
    Seek(f64),
    SetVolume(f32),
    VolumeUp,
    VolumeDown,
    SetRepeat(RepeatMode),
    ToggleShuffle,
    JumpTo(usize),
    RemoveFromQueue(usize),
    MoveTrack { source: Uid, destination: Uid },
    SetQueueVisible(bool),
    SelectDevice(Option<String>),
    SelectSampleRate(Option<f64>),
    SelectBitDepth(Option<i64>),
    SetExclusive(bool),
    Device(DeviceEvent),
    SetCountedPlayFraction(f64),
    /// Commit the in-flight listen (library switch, quit).
    FlushPending,
    /// Start/stop the spectrum analyzer (visualizer visible).
    SetSpectrum(bool),
    /// Replace library tracks in the queue after a metadata edit.
    UpdateTracks(Vec<Track>),
    OutputStatus(Sender<OutputStatus>),
    Snapshot(Sender<PlayerSnapshot>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub track: crate::dto::TrackDto,
    pub user_queued: bool,
}

/// Everything the transport UI shows. `queue` is only included when it
/// changed (`queueRevision`), to keep the 10 Hz stream small.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub current_track_id: Option<String>,
    pub current_index: usize,
    pub is_playing: bool,
    pub current_time: f64,
    pub duration: Option<f64>,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub playback_source: Option<String>,
    pub is_queue_visible: bool,
    pub queue_revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<Vec<QueueItem>>,
    /// The device stream rate, for "Output 48 kHz" style readouts.
    pub output_sample_rate: Option<u32>,
    pub last_error: Option<String>,
}

pub struct Callbacks {
    pub state: Box<dyn Fn(PlayerSnapshot) + Send>,
    pub plays: Box<dyn Fn(Vec<PlayRecord>) + Send>,
    pub output_selection: Box<dyn Fn(OutputSelection, OutputStatus) + Send>,
    pub spectrum: Arc<dyn Fn([f32; BIN_COUNT]) + Send + Sync>,
    /// Volume changed (persisted as `flactastic.volume`).
    pub volume: Box<dyn Fn(f32) + Send>,
}

#[derive(Clone)]
pub struct PlayerHandle {
    tx: Sender<Cmd>,
}

impl PlayerHandle {
    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    pub fn snapshot(&self) -> Option<PlayerSnapshot> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.send(Cmd::Snapshot(tx));
        rx.recv_timeout(Duration::from_secs(2)).ok()
    }

    pub fn output_status(&self) -> Option<OutputStatus> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.send(Cmd::OutputStatus(tx));
        rx.recv_timeout(Duration::from_secs(5)).ok()
    }
}

pub struct Init {
    pub backend: Arc<dyn Backend>,
    pub selection: OutputSelection,
    pub volume: f32,
    pub counted_play_fraction: f64,
    pub root: Arc<parking_lot::RwLock<Option<std::path::PathBuf>>>,
}

pub fn spawn(init: Init, cb: Callbacks) -> PlayerHandle {
    let (tx, rx) = crossbeam_channel::unbounded::<Cmd>();
    // Device notifications arrive on OS threads; route them into the actor.
    let dev_tx = tx.clone();
    init.backend.watch(Arc::new(move |e| {
        let _ = dev_tx.send(Cmd::Device(e));
    }));
    std::thread::Builder::new()
        .name("fl-player".into())
        .spawn(move || run(init, cb, rx))
        .expect("spawn player thread");
    PlayerHandle { tx }
}

struct State {
    player: Player,
    output: OutputManager,
    queue_visible: bool,
    queue_revision: u64,
    last_queue: Vec<(Uid, bool)>,
    last_sent: Option<PlayerSnapshot>,
    last_emit: Instant,
    analyzer: Option<Analyzer>,
    root: Arc<parking_lot::RwLock<Option<std::path::PathBuf>>>,
}

fn run(init: Init, cb: Callbacks, rx: Receiver<Cmd>) {
    let mut engine = Engine::new(init.backend.clone());
    engine.set_volume(init.volume);
    let mut output = OutputManager::new(init.backend, init.selection);
    // Route to the saved device/format before anything plays.
    output.start(&mut engine);
    let mut player = Player::new(engine);
    player.counted_play_fraction = init.counted_play_fraction;
    let mut s = State {
        player,
        output,
        queue_visible: false,
        queue_revision: 0,
        last_queue: Vec::new(),
        last_sent: None,
        last_emit: Instant::now(),
        analyzer: None,
        root: init.root,
    };
    let tick = Duration::from_millis(50);
    let mut next_tick = Instant::now() + tick;
    loop {
        let timeout = next_tick.saturating_duration_since(Instant::now());
        match rx.recv_timeout(timeout) {
            Ok(cmd) => {
                let forced = handle(&mut s, cmd, &cb);
                publish(&mut s, &cb, forced);
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                s.player.tick();
                let plays = s.player.take_plays();
                if !plays.is_empty() {
                    (cb.plays)(plays);
                }
                publish(&mut s, &cb, false);
                next_tick = Instant::now() + tick;
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                s.player.flush_pending();
                let plays = s.player.take_plays();
                if !plays.is_empty() {
                    (cb.plays)(plays);
                }
                return;
            }
        }
    }
}

/// Returns true when the UI must hear about it right away.
fn handle(s: &mut State, cmd: Cmd, cb: &Callbacks) -> bool {
    let p = &mut s.player;
    match cmd {
        Cmd::StartQueue { tracks, start, source, play, shuffle } => {
            if let Some(s) = shuffle {
                p.is_shuffle_enabled = s;
            }
            p.start_fresh_queue(tracks, start, source);
            if play {
                p.engine.play();
            }
        }
        Cmd::PlayNext(t) => p.play_next(&t),
        Cmd::AddToQueue(t) => p.add_to_queue(&t),
        Cmd::TogglePlayPause => p.engine.toggle_play_pause(),
        Cmd::Play => p.engine.play(),
        Cmd::Pause => p.engine.pause(),
        Cmd::Next => p.next(),
        Cmd::Previous => p.previous(),
        Cmd::Seek(t) => p.engine.seek(t),
        Cmd::SetVolume(v) => {
            p.engine.set_volume(v);
            (cb.volume)(p.engine.volume);
        }
        Cmd::VolumeUp => {
            p.engine.volume_up();
            (cb.volume)(p.engine.volume);
        }
        Cmd::VolumeDown => {
            p.engine.volume_down();
            (cb.volume)(p.engine.volume);
        }
        Cmd::SetRepeat(m) => p.set_repeat_mode(m),
        Cmd::ToggleShuffle => p.toggle_shuffle(),
        Cmd::JumpTo(i) => p.jump_to(i),
        Cmd::RemoveFromQueue(i) => p.remove_from_queue(i),
        Cmd::MoveTrack { source, destination } => p.move_track(source, destination),
        Cmd::SetQueueVisible(v) => s.queue_visible = v,
        Cmd::SelectDevice(id) => {
s.output.select_device(&mut s.player.engine, id);
            report_output(s, cb);
        }
        Cmd::SelectSampleRate(r) => {
s.output.select_sample_rate(&mut s.player.engine, r);
            report_output(s, cb);
        }
        Cmd::SelectBitDepth(b) => {
s.output.select_bit_depth(&mut s.player.engine, b);
            report_output(s, cb);
        }
        Cmd::SetExclusive(on) => {
s.output.set_exclusive(&mut s.player.engine, on);
            report_output(s, cb);
        }
        Cmd::Device(ev) => {
match ev {
                DeviceEvent::DevicesChanged => s.output.handle_system_change(&mut s.player.engine),
                DeviceEvent::FormatChanged(_) => s.output.handle_format_change(&mut s.player.engine),
            }
            report_output(s, cb);
        }
        Cmd::SetCountedPlayFraction(f) => p.counted_play_fraction = f.clamp(0.0, 1.0),
        Cmd::FlushPending => {
            p.flush_pending();
            let plays = p.take_plays();
            if !plays.is_empty() {
                (cb.plays)(plays);
            }
        }
        Cmd::SetSpectrum(on) => {
            if on && s.analyzer.is_none() {
                let f = cb.spectrum.clone();
                s.analyzer = Some(Analyzer::attach(p.engine.tap(), move |frame| f(frame)));
            } else if !on {
                s.analyzer = None;
            }
            return false;
        }
        Cmd::UpdateTracks(updated) => {
            let q = p.engine.queue();
            if q.iter().any(|t| updated.iter().any(|u| u.id == t.id)) {
                let nq: Vec<Track> = q.into_iter().map(|t| updated.iter().find(|u| u.id == t.id).cloned().unwrap_or(t)).collect();
                let idx = p.engine.current_index();
                p.engine.reorder_queue(nq, idx);
            }
        }
        Cmd::OutputStatus(reply) => {
            let _ = reply.send(s.output.status(&s.player.engine));
            return false;
        }
        Cmd::Snapshot(reply) => {
            let snap = build_snapshot(s, true);
            let _ = reply.send(snap);
            return false;
        }
    }
    true
}

fn report_output(s: &State, cb: &Callbacks) {
    (cb.output_selection)(s.output.selection.clone(), s.output.status(&s.player.engine));
}

fn build_snapshot(s: &mut State, full_queue: bool) -> PlayerSnapshot {
    let p = &s.player;
    let e = &p.engine;
    let queue = e.queue();
    let q_ids: Vec<(Uid, bool)> = queue.iter().map(|t| (t.id, p.user_queued.contains(&t.id))).collect();
    let queue_changed = q_ids != s.last_queue;
    if queue_changed {
        s.queue_revision += 1;
        s.last_queue = q_ids;
    }
    let root = s.root.read().clone();
    PlayerSnapshot {
        current_track_id: e.current_track.as_ref().map(|t| t.id.to_string()),
        current_index: e.current_index(),
        is_playing: e.is_playing,
        current_time: e.current_time,
        duration: e.duration,
        volume: e.volume,
        shuffle: p.is_shuffle_enabled,
        repeat: p.repeat_mode(),
        playback_source: p.playback_source.clone(),
        is_queue_visible: s.queue_visible,
        queue_revision: s.queue_revision,
        queue: (queue_changed || full_queue).then(|| {
            queue
                .iter()
                .map(|t| QueueItem {
                    track: crate::dto::TrackDto::from_track(t, root.as_deref()),
                    user_queued: p.user_queued.contains(&t.id),
                })
                .collect()
        }),
        output_sample_rate: e.output_format().map(|f| f.sample_rate),
        last_error: e.last_error.clone(),
    }
}

/// Sends a snapshot when something besides the clock changed, or every
/// 100 ms while playing (the UI interpolates between).
fn publish(s: &mut State, cb: &Callbacks, forced: bool) {
    let snap = build_snapshot(s, false);
    let structural = match &s.last_sent {
        None => true,
        Some(prev) => {
            snap.queue.is_some()
                || prev.current_track_id != snap.current_track_id
                || prev.is_playing != snap.is_playing
                || prev.duration != snap.duration
                || prev.volume != snap.volume
                || prev.shuffle != snap.shuffle
                || prev.repeat != snap.repeat
                || prev.is_queue_visible != snap.is_queue_visible
                || prev.current_index != snap.current_index
                || prev.output_sample_rate != snap.output_sample_rate
                || prev.last_error != snap.last_error
                || (prev.current_time - snap.current_time).abs() > 1.0
        }
    };
    let clock_due = snap.is_playing && s.last_emit.elapsed() >= Duration::from_millis(100);
    if forced || structural || clock_due {
        s.last_emit = Instant::now();
        s.last_sent = Some(snap.clone());
        (cb.state)(snap);
    }
}
