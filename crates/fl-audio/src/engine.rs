//! The gapless engine — a port of `PlayerEngine.swift`.
//!
//! One decoder thread at a time walks the live queue and writes frames
//! back-to-back into a lock-free ring (≈10 s of look-ahead, the
//! `BufferThrottle` limit). The output callback reads the ring. A per-entry
//! `{track, startFrame, endFrame}` table turns the callback's consumed-frame
//! count into the current track and position (`updateTime`, every 50 ms).
//! Every cancel bumps a generation counter that stale decoders check.
//!
//! `Engine` is driven from one thread (the player actor); decoder threads
//! share `Timeline` with it under a mutex, the way the Swift decode task
//! hops to the main actor.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fl_core::Track;
use parking_lot::Mutex;

use crate::decode::Source;
use crate::output::{Backend, OutputStream, StreamFormat, StreamSpec};
use crate::soxr::Resampler;
use crate::spectrum::Tap;

const OUTPUT_CHUNK_FRAMES: usize = 16_384;
const LOOKAHEAD_SECONDS: f64 = 10.0;

/// "Still decoding" end-frame sentinel.
const OPEN: i64 = i64::MAX;

#[derive(Debug, Clone)]
struct Entry {
    track: Track,
    start: i64,
    end: i64,
}

/// State shared between the engine and its decoder thread.
struct Timeline {
    queue: Vec<Track>,
    entries: Vec<Entry>,
    next_schedule_frame: i64,
    is_decoding: bool,
    repeat_one: bool,
    /// Duration read from the first decoded file (`duration = fileDuration`).
    decoded_duration: Option<f64>,
}

/// State the real-time render callback touches.
pub(crate) struct RenderShared {
    consumer: Mutex<rtrb::Consumer<f32>>,
    /// Output frames consumed since the last flush (player-node sample time).
    played: AtomicU64,
    playing: AtomicBool,
    volume_bits: AtomicU32,
    pub(crate) tap: Arc<Tap>,
}

struct DecodeShared {
    producer: Mutex<rtrb::Producer<f32>>,
    generation: AtomicU64,
    live_schedule_end: AtomicI64,
    out_rate: AtomicU32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub current_track: Option<Track>,
    pub is_playing: bool,
    pub current_time: f64,
    pub duration: Option<f64>,
    pub volume: f32,
    pub queue_ids: Vec<fl_core::Uid>,
    pub current_index: usize,
}

pub struct Engine {
    backend: Arc<dyn Backend>,
    spec: StreamSpec,
    stream: Option<Box<dyn OutputStream>>,
    format: Option<StreamFormat>,
    render: Arc<RenderShared>,
    decode: Arc<DecodeShared>,
    timeline: Arc<Mutex<Timeline>>,

    pub current_track: Option<Track>,
    pub is_playing: bool,
    pub current_time: f64,
    pub duration: Option<f64>,
    pub volume: f32,
    current_index: usize,
    seek_time_offset: f64,
    current_entry_start: i64,
    is_prepared: bool,
    decode_thread: Option<std::thread::JoinHandle<()>>,
    /// Set by `update_time` when playback ran off the end of the queue.
    pub finished: bool,
    pub last_error: Option<String>,
}

fn ring(rate: u32) -> (rtrb::Producer<f32>, rtrb::Consumer<f32>) {
    rtrb::RingBuffer::new((f64::from(rate) * LOOKAHEAD_SECONDS) as usize * 2)
}

impl Engine {
    pub fn new(backend: Arc<dyn Backend>) -> Engine {
        let (p, c) = ring(44_100);
        Engine {
            backend,
            spec: StreamSpec::default(),
            stream: None,
            format: None,
            render: Arc::new(RenderShared {
                consumer: Mutex::new(c),
                played: AtomicU64::new(0),
                playing: AtomicBool::new(false),
                volume_bits: AtomicU32::new(0.75f32.to_bits()),
                tap: Arc::new(Tap::default()),
            }),
            decode: Arc::new(DecodeShared {
                producer: Mutex::new(p),
                generation: AtomicU64::new(0),
                live_schedule_end: AtomicI64::new(0),
                out_rate: AtomicU32::new(44_100),
            }),
            timeline: Arc::new(Mutex::new(Timeline {
                queue: Vec::new(),
                entries: Vec::new(),
                next_schedule_frame: 0,
                is_decoding: false,
                repeat_one: false,
                decoded_duration: None,
            })),
            current_track: None,
            is_playing: false,
            current_time: 0.0,
            duration: None,
            volume: 0.75,
            current_index: 0,
            seek_time_offset: 0.0,
            current_entry_start: -1,
            is_prepared: false,
            decode_thread: None,
            finished: false,
            last_error: None,
        }
    }

    pub fn queue(&self) -> Vec<Track> {
        self.timeline.lock().queue.clone()
    }

    pub fn current_index(&self) -> usize {
        self.current_index
    }

    pub fn output_format(&self) -> Option<StreamFormat> {
        self.format
    }

    /// The spectrum tap, for `spectrum::Analyzer::attach`.
    pub fn tap(&self) -> Arc<Tap> {
        self.render.tap.clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        let tl = self.timeline.lock();
        Snapshot {
            current_track: self.current_track.clone(),
            is_playing: self.is_playing,
            current_time: self.current_time,
            duration: self.duration,
            volume: self.volume,
            queue_ids: tl.queue.iter().map(|t| t.id).collect(),
            current_index: self.current_index,
        }
    }

    fn out_rate(&self) -> f64 {
        f64::from(self.decode.out_rate.load(Ordering::Acquire))
    }

    // MARK: - Output

    fn open_stream(&mut self) -> Result<(), String> {
        self.stream = None;
        let render = self.render.clone();
        let stream = self.backend.open(&self.spec, Box::new(move |buf, ch| render_into(&render, buf, ch)))?;
        let fmt = stream.format();
        if self.format.map(|f| f.sample_rate) != Some(fmt.sample_rate) {
            // New device rate: a new ring sized for 10 s at that rate.
            let (p, c) = ring(fmt.sample_rate);
            *self.decode.producer.lock() = p;
            *self.render.consumer.lock() = c;
        }
        self.decode.out_rate.store(fmt.sample_rate, Ordering::Release);
        self.format = Some(fmt);
        self.stream = Some(stream);
        Ok(())
    }

    fn ensure_prepared(&mut self) {
        if self.is_prepared {
            return;
        }
        match self.open_stream() {
            Ok(()) => self.is_prepared = true,
            Err(e) => {
                log::error!("[PlayerEngine] Failed to prepare audio output: {e}");
                self.last_error = Some(e);
            }
        }
    }

    /// Applies a device / rate / bit-depth choice. Mid-playback this is one
    /// rebuild that resumes at the same position.
    pub fn apply_output(&mut self, spec: StreamSpec) {
        if let Some(id) = spec.device_id.clone().or_else(|| self.backend.default_device()) {
            if !spec.exclusive && (spec.sample_rate.is_some() || spec.bit_depth.is_some()) {
                if let Err(e) = self.backend.apply_device_format(&id, spec.sample_rate, spec.bit_depth) {
                    log::warn!("[PlayerEngine] Could not set device format: {e}");
                    self.last_error = Some(e);
                }
            }
        }
        self.spec = spec;
        if !self.is_prepared || self.timeline.lock().queue.is_empty() {
            if self.is_prepared {
                if let Err(e) = self.open_stream() {
                    self.last_error = Some(e);
                }
            }
            return;
        }
        self.rebuild_output();
    }

    /// The device went away or changed format underneath us.
    pub fn handle_configuration_change(&mut self) {
        if !self.is_prepared {
            return;
        }
        let in_sync = self
            .stream
            .as_ref()
            .zip(self.format)
            .is_some_and(|(s, f)| s.error().is_none() && s.format().sample_rate == f.sample_rate);
        if in_sync {
            return;
        }
        self.rebuild_output();
    }

    fn rebuild_output(&mut self) {
        let was_playing = self.is_playing;
        let saved = self.current_time;
        let idx = self.current_index;
        self.cancel_decode();
        self.flush();
        self.reset_timeline();
        if let Err(e) = self.open_stream() {
            log::error!("[PlayerEngine] Output rebuild failed: {e}");
            self.last_error = Some(e);
        }
        if !self.timeline.lock().queue.is_empty() {
            self.seek_time_offset = saved;
            self.start_decoding(idx, saved, false);
            if was_playing {
                self.play();
            }
        }
    }

    // MARK: - Transport

    pub fn set_queue(&mut self, tracks: Vec<Track>, start_at: usize) {
        self.ensure_prepared();
        self.cancel_decode();
        self.flush();
        self.reset_timeline();
        self.seek_time_offset = 0.0;
        self.finished = false;

        let empty = tracks.is_empty();
        self.current_index = start_at.min(tracks.len().saturating_sub(1));
        self.timeline.lock().queue = tracks;
        if empty {
            self.current_track = None;
            self.duration = None;
            self.current_time = 0.0;
            self.set_playing(false);
            return;
        }
        let t = self.timeline.lock().queue[self.current_index].clone();
        self.duration = t.duration;
        self.current_track = Some(t);
        self.current_time = 0.0;
        self.start_decoding(self.current_index, 0.0, false);
    }

    pub fn play(&mut self) {
        self.ensure_prepared();
        self.finished = false;
        self.set_playing(true);
    }

    pub fn pause(&mut self) {
        self.set_playing(false);
    }

    fn set_playing(&mut self, p: bool) {
        self.is_playing = p;
        self.render.playing.store(p, Ordering::Release);
    }

    pub fn toggle_play_pause(&mut self) {
        if self.is_playing {
            self.pause()
        } else {
            self.play()
        }
    }

    pub fn next(&mut self) {
        let q = self.queue();
        if self.current_index + 1 < q.len() {
            self.set_queue(q, self.current_index + 1);
            self.play();
        }
    }

    pub fn previous(&mut self) {
        let q = self.queue();
        if q.is_empty() {
            return;
        }
        let idx = if self.current_time > 3.0 { self.current_index } else { self.current_index.saturating_sub(1) };
        self.set_queue(q, idx);
        self.play();
    }

    pub fn seek(&mut self, seconds: f64) {
        if self.timeline.lock().queue.is_empty() {
            return;
        }
        let was_playing = self.is_playing;
        self.cancel_decode();
        self.flush();
        self.reset_timeline();
        let t = seconds.max(0.0);
        self.seek_time_offset = t;
        self.current_time = t;
        self.finished = false;
        self.start_decoding(self.current_index, t, false);
        if was_playing {
            self.play();
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        self.render.volume_bits.store(self.volume.to_bits(), Ordering::Release);
    }

    pub fn volume_up(&mut self) {
        self.set_volume(self.volume + 0.05);
    }

    pub fn volume_down(&mut self) {
        self.set_volume(self.volume - 0.05);
    }

    pub fn set_repeat_one(&mut self, on: bool) {
        let prev = std::mem::replace(&mut self.timeline.lock().repeat_one, on);
        if prev == on || !self.is_prepared || self.timeline.lock().queue.is_empty() {
            return;
        }
        // Only a pre-buffered different track needs flushing; otherwise the
        // decoder picks the flag up at the next loop boundary.
        if on && self.next_track_already_scheduled() {
            self.rebuild_decode_pipeline();
        }
    }

    fn next_track_already_scheduled(&self) -> bool {
        let cur = self.current_track.as_ref().map(|t| t.id);
        self.timeline.lock().entries.iter().any(|e| Some(e.track.id) != cur)
    }

    /// Appends without disturbing playback.
    pub fn append_tracks(&mut self, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        self.ensure_prepared();
        let (was_empty, resume, decoding) = {
            let mut tl = self.timeline.lock();
            let was_empty = tl.queue.is_empty();
            let resume = tl.queue.len();
            tl.queue.extend(tracks);
            (was_empty, resume, tl.is_decoding)
        };
        if was_empty {
            self.current_index = 0;
            let t = self.timeline.lock().queue[0].clone();
            self.duration = t.duration;
            self.current_track = Some(t);
            self.current_time = 0.0;
            self.seek_time_offset = 0.0;
            self.start_decoding(0, 0.0, false);
            return;
        }
        if !decoding {
            self.start_decoding(resume, 0.0, false);
        }
    }

    /// Inserts without a flush unless a future track is already buffered.
    pub fn insert_tracks(&mut self, tracks: Vec<Track>, at: usize) {
        if tracks.is_empty() {
            return;
        }
        self.ensure_prepared();
        if self.timeline.lock().queue.is_empty() {
            self.set_queue(tracks, 0);
            return;
        }
        let n = tracks.len();
        let (idx, len) = {
            let mut tl = self.timeline.lock();
            let idx = at.min(tl.queue.len());
            tl.queue.splice(idx..idx, tracks);
            (idx, tl.queue.len())
        };
        if idx <= self.current_index {
            self.current_index += n;
        }
        if !self.next_track_already_scheduled() {
            let decoding = self.timeline.lock().is_decoding;
            if !decoding && self.current_index + 1 < len {
                self.start_decoding(self.current_index + 1, 0.0, false);
            }
            return;
        }
        self.rebuild_decode_pipeline();
    }

    /// Removes an upcoming track, seamlessly when its audio isn't buffered.
    pub fn remove_from_queue(&mut self, index: usize) {
        let (removed, len) = {
            let tl = self.timeline.lock();
            (tl.queue.get(index).cloned(), tl.queue.len())
        };
        if index <= self.current_index || index >= len {
            return;
        }
        let removed = removed.unwrap();
        let (already, entries_empty) = {
            let tl = self.timeline.lock();
            (tl.entries.iter().any(|e| e.track.id == removed.id), tl.entries.is_empty())
        };
        let next_scheduled = self.next_track_already_scheduled();
        self.timeline.lock().queue.remove(index);

        if !already && !next_scheduled && !entries_empty {
            self.continue_current_track();
            return;
        }
        self.rebuild_decode_pipeline();
    }

    /// Rearranges the queue (shuffle, drag-reorder) without flushing unless a
    /// different track is already buffered.
    pub fn reorder_queue(&mut self, tracks: Vec<Track>, current_index: usize) {
        if tracks.is_empty() {
            return;
        }
        self.timeline.lock().queue = tracks;
        self.current_index = current_index;
        if self.next_track_already_scheduled() {
            self.rebuild_decode_pipeline();
            return;
        }
        self.continue_current_track();
    }

    /// Restart decode from the current track's continuation point, appending
    /// to its open entry (the `reorderQueue` seamless path).
    fn continue_current_track(&mut self) {
        let end = self.decode.live_schedule_end.load(Ordering::Acquire);
        let entry_start = self.timeline.lock().entries.last().map(|e| e.start).unwrap_or(0);
        let pre = (end - entry_start) as f64 / self.out_rate();
        let offset = self.seek_time_offset + pre;
        self.cancel_decode();
        let appending = {
            let mut tl = self.timeline.lock();
            tl.next_schedule_frame = end;
            !tl.entries.is_empty()
        };
        self.start_decoding(self.current_index, offset, appending);
    }

    fn rebuild_decode_pipeline(&mut self) {
        let was_playing = self.is_playing;
        let saved = self.current_time;
        self.cancel_decode();
        self.flush();
        self.reset_timeline();
        self.seek_time_offset = saved;
        self.current_time = saved;
        let t = self.timeline.lock().queue.get(self.current_index).cloned();
        if let Some(t) = t {
            self.duration = t.duration;
            self.current_track = Some(t);
        }
        self.start_decoding(self.current_index, saved, false);
        if was_playing {
            self.play();
        }
    }

    // MARK: - Pipeline control

    fn cancel_decode(&mut self) {
        self.decode.generation.fetch_add(1, Ordering::AcqRel);
        if let Some(t) = self.decode_thread.take() {
            // The thread checks the generation between chunks and while
            // waiting for ring space, so this returns promptly.
            let _ = t.join();
        }
        self.timeline.lock().is_decoding = false;
    }

    /// Drops everything buffered and resets the consumed-frame clock.
    fn flush(&mut self) {
        let _p = self.decode.producer.lock();
        let mut c = self.render.consumer.lock();
        let n = c.slots();
        if let Ok(chunk) = c.read_chunk(n) {
            chunk.commit_all();
        }
        self.render.played.store(0, Ordering::Release);
    }

    fn reset_timeline(&mut self) {
        let mut tl = self.timeline.lock();
        tl.entries.clear();
        tl.next_schedule_frame = 0;
        tl.decoded_duration = None;
        self.decode.live_schedule_end.store(0, Ordering::Release);
        self.current_entry_start = -1;
    }

    fn start_decoding(&mut self, from: usize, seek: f64, appending: bool) {
        let gen = self.decode.generation.load(Ordering::Acquire);
        self.timeline.lock().is_decoding = true;
        let timeline = self.timeline.clone();
        let decode = self.decode.clone();
        let handle = std::thread::Builder::new()
            .name("fl-decode".into())
            .spawn(move || decode_loop(timeline, decode, gen, from, seek, appending))
            .expect("spawn decoder");
        self.decode_thread = Some(handle);
    }

    // MARK: - Time tracking (50 ms tick)

    /// Maps consumed frames to the current entry. Returns true when anything
    /// observable changed.
    pub fn update_time(&mut self) -> bool {
        if let Some(d) = self.timeline.lock().decoded_duration.take() {
            self.duration = Some(d);
        }
        if !self.is_playing {
            return false;
        }
        let sample_time = self.render.played.load(Ordering::Acquire) as i64;
        let rate = self.out_rate();
        let mut tl = self.timeline.lock();
        let hit = tl.entries.iter().find(|e| sample_time >= e.start && (e.end == OPEN || sample_time < e.end)).cloned();
        if let Some(entry) = hit {
            if self.current_entry_start != entry.start {
                let initial = self.current_entry_start == -1;
                self.current_entry_start = entry.start;
                self.duration = if entry.end != OPEN {
                    entry.track.duration.or(Some((entry.end - entry.start) as f64 / rate))
                } else {
                    entry.track.duration.or(self.duration)
                };
                if let Some(i) = tl.queue.iter().position(|t| t.id == entry.track.id) {
                    self.current_index = i;
                }
                self.current_track = Some(entry.track.clone());
                if !initial {
                    self.seek_time_offset = 0.0;
                }
            }
            self.current_time = self.seek_time_offset + (sample_time - entry.start) as f64 / rate;
            while tl.entries.first().is_some_and(|f| f.end != OPEN && f.end <= sample_time) {
                tl.entries.remove(0);
            }
            return true;
        }
        if !tl.is_decoding && tl.entries.last().is_some_and(|l| l.end != OPEN && sample_time >= l.end) {
            // End state names the last track even if no tick landed inside it.
            let last = tl.entries.last().cloned().unwrap();
            if let Some(i) = tl.queue.iter().position(|t| t.id == last.track.id) {
                self.current_index = i;
            }
            if self.current_entry_start != last.start {
                self.duration = last.track.duration.or(Some((last.end - last.start) as f64 / rate));
                self.current_track = Some(last.track);
            }
            drop(tl);
            if let Some(d) = self.duration {
                self.current_time = d;
            }
            self.set_playing(false);
            self.finished = true;
            return true;
        }
        false
    }

    /// Frames the render callback reported as consumed (tests/diagnostics).
    pub fn played_frames(&self) -> u64 {
        self.render.played.load(Ordering::Acquire)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.cancel_decode();
        self.stream = None;
    }
}

// MARK: - Render (real-time thread)

fn render_into(r: &RenderShared, out: &mut [f32], ch: usize) -> usize {
    let frames = out.len() / ch;
    let mut produced = 0;
    if r.playing.load(Ordering::Acquire) {
        if let Some(mut cons) = r.consumer.try_lock() {
            let n = (cons.slots() / 2).min(frames);
            if n > 0 {
                let chunk = cons.read_chunk(n * 2).expect("slots checked");
                let (a, b) = chunk.as_slices();
                let vol = f32::from_bits(r.volume_bits.load(Ordering::Relaxed));
                let unity = vol == 1.0;
                let mut src = a.iter().chain(b.iter());
                for f in 0..n {
                    let (mut l, mut rr) = (*src.next().unwrap(), *src.next().unwrap());
                    if !unity {
                        l *= vol;
                        rr *= vol;
                    }
                    let o = &mut out[f * ch..(f + 1) * ch];
                    if ch == 1 {
                        o[0] = (l + rr) * 0.5;
                    } else {
                        o[0] = l;
                        o[1] = rr;
                        o[2..].fill(0.0);
                    }
                }
                chunk.commit_all();
                r.played.fetch_add(n as u64, Ordering::AcqRel);
                produced = n;
            }
        }
    }
    out[produced * ch..].fill(0.0);
    r.tap.feed(out, ch);
    produced
}

// MARK: - Decoder thread

fn cancelled(d: &DecodeShared, gen: u64) -> bool {
    d.generation.load(Ordering::Acquire) != gen
}

/// Pushes interleaved stereo into the ring, waiting for space. Returns false
/// when the generation moved on.
fn push(d: &DecodeShared, gen: u64, mut data: &[f32]) -> bool {
    while !data.is_empty() {
        {
            let mut p = d.producer.lock();
            if cancelled(d, gen) {
                return false;
            }
            let n = (p.slots() / 2 * 2).min(data.len());
            if n > 0 {
                let chunk = p.write_chunk_uninit(n).expect("slots checked");
                chunk.fill_from_iter(data[..n].iter().copied());
                data = &data[n..];
                continue;
            }
        }
        if cancelled(d, gen) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    true
}

struct Carry {
    resampler: Resampler,
    /// Entry to extend when the resampler's tail is flushed later.
    entry_start: i64,
    track_id: fl_core::Uid,
}

fn decode_loop(tl: Arc<Mutex<Timeline>>, d: Arc<DecodeShared>, gen: u64, from: usize, seek: f64, appending: bool) {
    let out_rate = f64::from(d.out_rate.load(Ordering::Acquire));
    let mut index = from;
    let mut first = true;
    let mut carry: Option<Carry> = None;
    let mut block = Vec::with_capacity(OUTPUT_CHUNK_FRAMES * 2);
    let mut resampled = Vec::with_capacity(OUTPUT_CHUNK_FRAMES * 4);

    // Flushes a carried resampler's tail onto the entry it belongs to.
    let flush_carry = |carry: &mut Option<Carry>, buf: &mut Vec<f32>| -> bool {
        let Some(mut c) = carry.take() else { return true };
        buf.clear();
        if c.resampler.flush(buf).is_err() || buf.is_empty() {
            return true;
        }
        if !push(&d, gen, buf) {
            return false;
        }
        let tail = (buf.len() / 2) as i64;
        let mut t = tl.lock();
        if let Some(e) = t.entries.iter_mut().find(|e| e.start == c.entry_start && e.track.id == c.track_id) {
            e.end += tail;
        }
        t.next_schedule_frame += tail;
        d.live_schedule_end.store(t.next_schedule_frame, Ordering::Release);
        true
    };

    loop {
        if cancelled(&d, gen) {
            return;
        }
        let track = {
            let t = tl.lock();
            if index >= t.queue.len() {
                drop(t);
                if !flush_carry(&mut carry, &mut resampled) {
                    return;
                }
                let mut t = tl.lock();
                if !cancelled(&d, gen) {
                    t.is_decoding = false;
                }
                return;
            }
            t.queue[index].clone()
        };

        let result: Result<(), String> = (|| {
            let mut src = Source::open(Path::new(&track.path)).map_err(|e| e.to_string())?;
            if first && seek > 0.0 {
                src.seek(seek).map_err(|e| e.to_string())?;
            }
            if first {
                tl.lock().decoded_duration = src.duration_secs();
            }
            let in_rate = f64::from(src.sample_rate);

            // Keep one resampler running across a join of same-rate tracks so
            // the filtered signal stays continuous; otherwise flush its tail.
            let reuse = carry.as_ref().is_some_and(|c| c.resampler.in_rate == in_rate && c.resampler.out_rate == out_rate);
            if !reuse && !flush_carry(&mut carry, &mut resampled) {
                return Ok(());
            }
            let mut resampler = match carry.take() {
                Some(c) if reuse => Some(c.resampler),
                _ if in_rate != out_rate => Some(Resampler::new(in_rate, out_rate, 2)?),
                _ => None,
            };

            let start = {
                let mut t = tl.lock();
                let sf = t.next_schedule_frame;
                if !(appending && first) {
                    t.entries.push(Entry { track: track.clone(), start: sf, end: OPEN });
                }
                d.live_schedule_end.store(sf, Ordering::Release);
                sf
            };
            // The continuation of an existing entry keeps that entry's start.
            let entry_start = if appending && first {
                tl.lock().entries.iter().rev().find(|e| e.track.id == track.id && e.end == OPEN).map(|e| e.start).unwrap_or(start)
            } else {
                start
            };

            let mut total: i64 = 0;
            loop {
                if cancelled(&d, gen) {
                    return Ok(());
                }
                block.clear();
                while block.len() < OUTPUT_CHUNK_FRAMES * 2 {
                    if !src.next_block(&mut block).map_err(|e| e.to_string())? {
                        break;
                    }
                }
                if block.is_empty() {
                    break;
                }
                let data: &[f32] = match &mut resampler {
                    Some(r) => {
                        resampled.clear();
                        r.process(&block, &mut resampled)?;
                        &resampled
                    }
                    None => &block,
                };
                if !push(&d, gen, data) {
                    return Ok(());
                }
                total += (data.len() / 2) as i64;
                d.live_schedule_end.store(start + total, Ordering::Release);
            }

            if cancelled(&d, gen) {
                return Ok(());
            }
            let actual_end = start + total;
            {
                let mut t = tl.lock();
                let idx = if appending && first {
                    t.entries.iter().position(|e| e.track.id == track.id && e.end == OPEN)
                } else {
                    t.entries.iter().position(|e| e.start == start && e.track.id == track.id)
                };
                if let Some(i) = idx {
                    t.entries[i].end = actual_end;
                }
                t.next_schedule_frame = actual_end;
            }
            if let Some(r) = resampler {
                carry = Some(Carry { resampler: r, entry_start, track_id: track.id });
            }
            Ok(())
        })();

        if cancelled(&d, gen) {
            return;
        }
        if let Err(e) = result {
            log::warn!("[PlayerEngine] Error decoding {}: {e}", track.title);
        }
        first = false;
        if !tl.lock().repeat_one {
            index += 1;
        }
    }
}
