//! Spectrum analysis (`SpectrumAnalyzer.swift`).
//!
//! The render callback copies post-volume channel 0 into a small ring while a
//! visualizer is attached. An analyzer thread takes the latest 1024 samples
//! about every 100 ms (the rate AVAudioEngine's tap fires at) and produces 64
//! log-spaced bins; the UI smooths them at 60 Hz (attack 0.6, decay 0.97).
//!
//! Scaling reproduces vDSP exactly: periodic Hann window
//! `0.5·(1 − cos 2πn/N)`, `vDSP_fft_zrip` (2× the DFT), then `1/N`, so each bin
//! is `2·|X[k]|/N`; bins take the peak over their range and map through
//! `min(1, sqrt(peak · 8))`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use realfft::RealFftPlanner;

pub const BIN_COUNT: usize = 64;
pub const FFT_SIZE: usize = 1024;
pub const HISTORY_DEPTH: usize = 256;
pub const DISPLAY_HZ: f64 = 60.0;

/// Written by the render callback.
#[derive(Default)]
pub struct Tap {
    enabled: AtomicBool,
    producer: Mutex<Option<rtrb::Producer<f32>>>,
}

impl Tap {
    #[inline]
    pub(crate) fn feed(&self, out: &[f32], ch: usize) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        if let Some(mut guard) = self.producer.try_lock() {
            if let Some(p) = guard.as_mut() {
                let n = (out.len() / ch).min(p.slots());
                if n > 0 {
                    if let Ok(chunk) = p.write_chunk_uninit(n) {
                        chunk.fill_from_iter(out.iter().step_by(ch).copied());
                    }
                }
            }
        }
    }
}

/// Precomputed FFT state, reused across frames.
pub struct Fft {
    fft: Arc<dyn realfft::RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<realfft::num_complex::Complex<f32>>,
    scratch: Vec<realfft::num_complex::Complex<f32>>,
    mags: Vec<f32>,
}

impl Default for Fft {
    fn default() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        let window = (0..FFT_SIZE)
            .map(|n| (0.5 * (1.0 - (2.0 * std::f64::consts::PI * n as f64 / FFT_SIZE as f64).cos())) as f32)
            .collect();
        Fft {
            input: fft.make_input_vec(),
            spectrum: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            window,
            fft,
            mags: vec![0.0; FFT_SIZE / 2],
        }
    }
}

impl Fft {
    /// `computeMagnitudes`: `samples` must hold at least `FFT_SIZE` values.
    pub fn magnitudes(&mut self, samples: &[f32]) -> [f32; BIN_COUNT] {
        for (i, x) in self.input.iter_mut().enumerate() {
            *x = samples[i] * self.window[i];
        }
        self.fft.process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch).expect("sizes match");
        let half = FFT_SIZE / 2;
        let n = FFT_SIZE as f32;
        for k in 1..half {
            self.mags[k] = 2.0 * self.spectrum[k].norm() / n;
        }
        // vDSP packs DC and Nyquist into bin 0; no output bin reads it.
        self.mags[0] = 2.0 * (self.spectrum[0].re.hypot(self.spectrum[half].re)) / n;

        let mut out = [0f32; BIN_COUNT];
        let (min_idx, max_idx) = (1.0f32, (half - 1) as f32);
        for (b, o) in out.iter_mut().enumerate() {
            let t0 = b as f32 / BIN_COUNT as f32;
            let t1 = (b + 1) as f32 / BIN_COUNT as f32;
            let lo = (min_idx * (max_idx / min_idx).powf(t0)) as usize;
            let hi = (lo + 1).max((min_idx * (max_idx / min_idx).powf(t1)) as usize);
            let upper = hi.min(half);
            let peak = self.mags[lo..upper].iter().fold(0f32, |m, v| m.max(*v));
            *o = (peak * 8.0).sqrt().min(1.0);
        }
        out
    }
}

/// Runs the analysis loop while attached; `on_frame` receives each target.
pub struct Analyzer {
    tap: Arc<Tap>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Analyzer {
    pub fn attach(tap: Arc<Tap>, on_frame: impl Fn([f32; BIN_COUNT]) + Send + 'static) -> Analyzer {
        let (p, mut c) = rtrb::RingBuffer::<f32>::new(FFT_SIZE * 16);
        *tap.producer.lock() = Some(p);
        tap.enabled.store(true, Ordering::Release);
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let thread = std::thread::Builder::new()
            .name("fl-spectrum".into())
            .spawn(move || {
                let mut fft = Fft::default();
                // Circular history of the most recent FFT_SIZE samples.
                let mut hist = vec![0f32; FFT_SIZE];
                let mut pos = 0usize;
                let mut fill = 0usize;
                let mut window = vec![0f32; FFT_SIZE];
                while !s2.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(100));
                    let n = c.slots();
                    if let Ok(chunk) = c.read_chunk(n) {
                        let (a, b) = chunk.as_slices();
                        for &v in a.iter().chain(b) {
                            hist[pos] = v;
                            pos = (pos + 1) % FFT_SIZE;
                        }
                        fill = (fill + n).min(FFT_SIZE);
                        chunk.commit_all();
                    }
                    let frame = if fill >= FFT_SIZE && n > 0 {
                        window[..FFT_SIZE - pos].copy_from_slice(&hist[pos..]);
                        window[FFT_SIZE - pos..].copy_from_slice(&hist[..pos]);
                        fft.magnitudes(&window)
                    } else {
                        [0.0; BIN_COUNT]
                    };
                    on_frame(frame);
                }
            })
            .expect("spawn analyzer");
        Analyzer { tap, stop, thread: Some(thread) }
    }
}

impl Drop for Analyzer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.tap.enabled.store(false, Ordering::Release);
        *self.tap.producer.lock() = None;
    }
}

/// 60 Hz UI smoothing (`SpectrumAnalyzer.tick`), for tests and non-JS consumers.
pub fn smooth(current: &mut [f32; BIN_COUNT], target: &[f32; BIN_COUNT]) {
    for (c, t) in current.iter_mut().zip(target) {
        if *t > *c {
            *c += (*t - *c) * 0.6;
        } else {
            *c *= 0.97;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scale_sine_fills_its_bin() {
        // A 0.5-amplitude sine centred on FFT bin 64 → |X| = A·N/4 after Hann,
        // so 2|X|/N = A/2 = 0.25, and sqrt(0.25·8) clamps to 1.
        let samples: Vec<f32> =
            (0..FFT_SIZE).map(|i| (0.5 * (2.0 * std::f64::consts::PI * 64.0 * i as f64 / FFT_SIZE as f64).sin()) as f32).collect();
        let mut f = Fft::default();
        let m = f.magnitudes(&samples);
        assert_eq!(m.iter().cloned().fold(0f32, f32::max), 1.0);
        // Quiet signal: 2|X|/N = 0.005/2 → sqrt(0.02) ≈ 0.1414.
        let quiet: Vec<f32> = samples.iter().map(|v| v / 100.0).collect();
        let peak = f.magnitudes(&quiet).iter().cloned().fold(0f32, f32::max);
        assert!((peak - 0.02f32.sqrt()).abs() < 1e-3, "{peak}");
    }

    #[test]
    fn silence_is_zero_and_smoothing_decays() {
        let mut f = Fft::default();
        assert!(f.magnitudes(&[0.0; FFT_SIZE]).iter().all(|v| *v == 0.0));
        let mut cur = [1.0; BIN_COUNT];
        smooth(&mut cur, &[0.0; BIN_COUNT]);
        assert!((cur[0] - 0.97).abs() < 1e-6);
        let mut cur = [0.0; BIN_COUNT];
        smooth(&mut cur, &[1.0; BIN_COUNT]);
        assert!((cur[0] - 0.6).abs() < 1e-6);
    }
}
