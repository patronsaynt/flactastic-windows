//! File decoding to interleaved stereo f32 at the source rate.
//!
//! Gapless: symphonia trims encoder delay/padding (LAME/Xing for MP3,
//! iTunSMPB/edit lists for MP4) when `enable_gapless` is set.
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
        let frames = params.n_frames.map(|n| {
            n.saturating_sub(u64::from(params.delay.unwrap_or(0))).saturating_sub(u64::from(params.padding.unwrap_or(0)))
        });
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
        let ts = (seconds * f64::from(self.sample_rate)).round() as u64;
        let target = SeekTo::TimeStamp { ts, track_id: self.track_id };
        let seeked = self.reader.seek(SeekMode::Accurate, target).map_err(|e| DecodeError::Format(e.to_string()))?;
        self.decoder.reset();
        self.skip_frames = seeked.required_ts.saturating_sub(seeked.actual_ts);
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
            let mut frames = samples.len() / ch;
            let mut start = 0;
            if self.skip_frames > 0 {
                let skip = self.skip_frames.min(frames as u64) as usize;
                self.skip_frames -= skip as u64;
                start = skip;
                frames -= skip;
                if frames == 0 {
                    continue;
                }
            }
            out.reserve(frames * 2);
            let s = &samples[start * ch..];
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
