//! File decoding to interleaved stereo f32 at the source rate.
//!
//! Gapless: symphonia trims encoder delay/padding for MP3 (LAME/Xing) when
//! `enable_gapless` is set; MP4 priming/padding (iTunSMPB or the edit list)
//! is trimmed here, see `mp4trim`.
//! Channel mapping follows `AVAudioConverter` with `downmix == false`: mono is
//! duplicated to both channels, more than two channels keep the first two.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("could not open {0}: {1}")]
    Open(String, String),
    #[error("unsupported or corrupt audio: {0}")]
    Format(String),
}

pub struct Source {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    pub sample_rate: u32,
    pub channels: usize,
    /// Total frames after gapless trimming, when the container says.
    pub frames: Option<u64>,
    /// Source bit depth, when known (lossless formats).
    pub bits_per_sample: Option<u32>,
    sample_buf: Option<(SampleBuffer<f32>, symphonia::core::audio::SignalSpec, u64)>,
    /// Frames still to discard after an accurate seek.
    skip_frames: u64,
    /// MP4 priming/padding to trim, in decoder-timeline frames.
    trim: Option<crate::mp4trim::Trim>,
    /// Decoder-timeline frame of the next decoded sample (for `trim`).
    raw_pos: u64,
    /// Timestamp units per frame (packet `ts` → frames).
    ts_per_frame: f64,
}

impl Source {
    pub fn open(path: &Path) -> Result<Source, DecodeError> {
        let name = path.display().to_string();
        let file = File::open(path).map_err(|e| DecodeError::Open(name.clone(), e.to_string()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let fmt_opts = FormatOptions { enable_gapless: true, ..Default::default() };
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &MetadataOptions::default())
            .map_err(|e| DecodeError::Format(e.to_string()))?;
        let reader = probed.format;
        let track = reader
            .default_track()
            .ok_or_else(|| DecodeError::Format("no audio track".into()))?;
        let params = track.codec_params.clone();
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|e| DecodeError::Format(e.to_string()))?;
        let sample_rate = params.sample_rate.ok_or_else(|| DecodeError::Format("unknown sample rate".into()))?;
        let channels = params.channels.map(|c| c.count()).unwrap_or(2).max(1);
        let is_mp4 = matches!(
            path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
            Some("m4a" | "mp4" | "m4b" | "aac")
        );
        let trim = if is_mp4 && params.delay.is_none() { crate::mp4trim::read(path, sample_rate) } else { None };
        let mut frames = params.n_frames.map(|n| {
            n.saturating_sub(u64::from(params.delay.unwrap_or(0))).saturating_sub(u64::from(params.padding.unwrap_or(0)))
        });
        if let Some(t) = trim {
            frames = Some(frames.map_or(t.length, |f| t.length.min(f.saturating_sub(t.delay))));
        }
        let ts_per_frame =
            params.time_base.map_or(1.0, |tb| f64::from(tb.denom) / f64::from(tb.numer) / f64::from(sample_rate));
        Ok(Source {
            track_id: track.id,
            reader,
            decoder,
            sample_rate,
            channels,
            frames,
            bits_per_sample: params.bits_per_sample,
            sample_buf: None,
            skip_frames: 0,
            trim,
            raw_pos: 0,
            ts_per_frame,
        })
    }

    pub fn duration_secs(&self) -> Option<f64> {
        self.frames.map(|f| f as f64 / f64::from(self.sample_rate))
    }

    /// Seeks to `seconds`; decoding resumes at exactly that frame.
    pub fn seek(&mut self, seconds: f64) -> Result<(), DecodeError> {
        if seconds <= 0.0 {
            return Ok(());
        }
        // Seek by rounded frame, not by time: truncating seconds × rate lands
        // one frame early when the product is a hair under an integer.
        let delay = self.trim.map_or(0, |t| t.delay);
        let frame = (seconds * f64::from(self.sample_rate)).round() as u64 + delay;
        // MDCT codecs (AAC) need the previous packet to reconstruct the
        // target one after a decoder reset: land two packets early and
        // decode through the pre-roll.
        let preroll = if self.trim.is_some() { 2048 } else { 0 };
        let ts = (frame.saturating_sub(preroll) as f64 * self.ts_per_frame).round() as u64;
        let target = SeekTo::TimeStamp { ts, track_id: self.track_id };
        let seeked = self.reader.seek(SeekMode::Accurate, target).map_err(|e| DecodeError::Format(e.to_string()))?;
        self.decoder.reset();
        let actual = (seeked.actual_ts as f64 / self.ts_per_frame).round() as u64;
        self.raw_pos = actual;
        self.skip_frames = frame.saturating_sub(actual);
        Ok(())
    }

    /// Appends the next decoded block as interleaved stereo. `Ok(false)` at end of stream.
    pub fn next_block(&mut self, out: &mut Vec<f32>) -> Result<bool, DecodeError> {
        loop {
            let packet = match self.reader.next_packet() {
                Ok(p) => p,
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(SymError::ResetRequired) => return Ok(false),
                Err(e) => return Err(DecodeError::Format(e.to_string())),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                // A corrupt packet is skipped, as players generally do.
                Err(SymError::DecodeError(_)) => continue,
                Err(e) => return Err(DecodeError::Format(e.to_string())),
            };
            let spec = *decoded.spec();
            let cap = decoded.capacity() as u64;
            let fits = matches!(&self.sample_buf, Some((_, s, c)) if *s == spec && *c >= cap);
            if !fits {
                self.sample_buf = Some((SampleBuffer::new(cap, spec), spec, cap));
            }
            let buf = &mut self.sample_buf.as_mut().unwrap().0;
            buf.copy_interleaved_ref(decoded);
            let samples = buf.samples();
            let ch = spec.channels.count().max(1);
            let decoded_frames = samples.len() / ch;
            let raw_start = self.raw_pos;
            self.raw_pos += decoded_frames as u64;
            let mut start = 0;
            let mut end = decoded_frames;
            if self.skip_frames > 0 {
                let skip = self.skip_frames.min(decoded_frames as u64) as usize;
                self.skip_frames -= skip as u64;
                start = skip;
            }
            if let Some(t) = self.trim {
                // Keep only [delay, delay + length) of the decoder timeline.
                let valid_end = t.delay + t.length;
                if raw_start >= valid_end {
                    return Ok(false);
                }
                start = start.max(t.delay.saturating_sub(raw_start).min(decoded_frames as u64) as usize);
                end = end.min((valid_end - raw_start) as usize);
            }
            if end <= start {
                continue;
            }
            let frames = end - start;
            out.reserve(frames * 2);
            let s = &samples[start * ch..end * ch];
            match ch {
                1 => {
                    for &v in s {
                        out.extend_from_slice(&[v, v]);
                    }
                }
                2 => out.extend_from_slice(&s[..frames * 2]),
                _ => {
                    for f in s.chunks_exact(ch) {
                        out.extend_from_slice(&f[..2]);
                    }
                }
            }
            return Ok(true);
        }
    }
}
