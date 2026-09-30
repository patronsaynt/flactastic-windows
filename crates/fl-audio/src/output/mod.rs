//! Output backends. The engine renders interleaved f32 at the stream's rate
//! and channel count; each backend converts to what its device takes.

#[cfg(target_os = "linux")]
pub mod alsa;
pub mod null;
#[cfg(windows)]
pub mod wasapi;

use std::sync::Arc;

/// Rates offered when a device doesn't enumerate discrete values
/// (`AudioHAL.standardRates`).
pub const STANDARD_RATES: [f64; 10] =
    [44_100.0, 48_000.0, 88_200.0, 96_000.0, 176_400.0, 192_000.0, 352_800.0, 384_000.0, 705_600.0, 768_000.0];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Stable across reboots and re-plugs (WASAPI endpoint ID / PipeWire node name).
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    F32,
    I16,
    /// 24-bit packed in 3 bytes.
    I24,
    /// 24 valid bits in a 32-bit container, MSB-aligned (WASAPI).
    I24In32,
    /// 24 valid bits in the low bytes of a 32-bit container (ALSA `S24_LE`).
    I24In32Lsb,
    I32,
}

impl SampleFormat {
    pub fn bits(self) -> u32 {
        match self {
            Self::I16 => 16,
            Self::I24 | Self::I24In32 | Self::I24In32Lsb => 24,
            Self::F32 | Self::I32 => 32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample: SampleFormat,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamSpec {
    /// `None` follows the system default device.
    pub device_id: Option<String>,
    /// Exclusive mode only: the rate/bit depth to open at. In shared mode the
    /// device's own format (set via `apply_device_format`) is used.
    pub sample_rate: Option<f64>,
    pub bit_depth: Option<i64>,
    /// Windows: WASAPI exclusive mode.
    pub exclusive: bool,
}

/// Fills `buf` (interleaved, `channels` wide). Returns how many frames came
/// from real audio (the rest is silence). Runs on the device's real-time thread.
pub type Render = Box<dyn FnMut(&mut [f32], usize) -> usize + Send>;

pub trait OutputStream: Send {
    fn format(&self) -> StreamFormat;
    fn device_id(&self) -> &str;
    /// Set when the stream died (device removed, format change).
    fn error(&self) -> Option<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    /// Devices added/removed or the default changed.
    DevicesChanged,
    /// A device's format changed underneath us.
    FormatChanged(String),
}

pub trait Backend: Send + Sync {
    fn name(&self) -> &'static str;
    fn devices(&self) -> Vec<DeviceInfo>;
    fn default_device(&self) -> Option<String>;
    /// Rates the device can run at (`availableSampleRates`).
    fn available_sample_rates(&self, device_id: &str) -> Vec<f64>;
    /// Bit depths available at `rate` (`availableBitDepths`).
    fn available_bit_depths(&self, device_id: &str, rate: f64) -> Vec<i64>;
    /// The device's current (shared/nominal) rate and physical bit depth.
    fn current_format(&self, device_id: &str) -> Option<(f64, Option<i64>)>;
    /// Changes the device's own format — the macOS "set nominal rate /
    /// physical format" equivalent. `None` leaves that part alone.
    fn apply_device_format(&self, device_id: &str, rate: Option<f64>, bits: Option<i64>) -> Result<(), String>;
    fn open(&self, spec: &StreamSpec, render: Render) -> Result<Box<dyn OutputStream>, String>;
    /// Hot-plug / default-device / format notifications.
    fn watch(&self, on_event: Arc<dyn Fn(DeviceEvent) + Send + Sync>);
}

/// `expandRates`: discrete entries pass through, ranges expand to the
/// standard rates they contain; sorted, de-duplicated.
pub fn expand_rates(ranges: &[(f64, f64)]) -> Vec<f64> {
    let mut v: Vec<f64> = Vec::new();
    for &(lo, hi) in ranges {
        if lo == hi {
            if lo > 0.0 {
                v.push(lo);
            }
        } else {
            v.extend(STANDARD_RATES.iter().copied().filter(|r| *r >= lo && *r <= hi));
        }
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v.dedup();
    v
}

// MARK: - Sample conversion

/// Converts interleaved f32 to the device's integer format.
///
/// Samples that are exactly representable at the target depth (every sample of
/// a bit-perfect stream) pass through untouched; anything else gets TPDF
/// dither of ±1 LSB before rounding. Full scale is ±1.0, clipped.
pub struct Quantizer {
    rng: u32,
}

impl Default for Quantizer {
    fn default() -> Self {
        Quantizer { rng: 0x9E37_79B9 }
    }
}

impl Quantizer {
    #[inline]
    fn rand(&mut self) -> f32 {
        // xorshift32 → [0, 1)
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / 16_777_216.0
    }

    #[inline]
    pub fn quantize(&mut self, s: f32, bits: u32) -> i32 {
        let scale = (1u64 << (bits - 1)) as f64;
        let x = f64::from(s) * scale;
        let max = scale - 1.0;
        let q = if x.fract() == 0.0 { x } else { (x + f64::from(self.rand() - self.rand())).round() };
        q.clamp(-scale, max) as i32
    }

    /// Writes `src` into `dst` bytes in `fmt` (little-endian).
    pub fn write(&mut self, src: &[f32], fmt: SampleFormat, dst: &mut [u8]) {
        match fmt {
            SampleFormat::F32 => {
                for (s, d) in src.iter().zip(dst.chunks_exact_mut(4)) {
                    d.copy_from_slice(&s.clamp(-1.0, 1.0).to_le_bytes());
                }
            }
            SampleFormat::I16 => {
                for (s, d) in src.iter().zip(dst.chunks_exact_mut(2)) {
                    d.copy_from_slice(&(self.quantize(*s, 16) as i16).to_le_bytes());
                }
            }
            SampleFormat::I24 => {
                for (s, d) in src.iter().zip(dst.chunks_exact_mut(3)) {
                    d.copy_from_slice(&self.quantize(*s, 24).to_le_bytes()[..3]);
                }
            }
            SampleFormat::I24In32 => {
                for (s, d) in src.iter().zip(dst.chunks_exact_mut(4)) {
                    d.copy_from_slice(&(self.quantize(*s, 24) << 8).to_le_bytes());
                }
            }
            SampleFormat::I24In32Lsb => {
                for (s, d) in src.iter().zip(dst.chunks_exact_mut(4)) {
                    d.copy_from_slice(&self.quantize(*s, 24).to_le_bytes());
                }
            }
            SampleFormat::I32 => {
                for (s, d) in src.iter().zip(dst.chunks_exact_mut(4)) {
                    // f32 carries 24 bits of mantissa; scale without dither.
                    let v = (f64::from(*s) * 2_147_483_648.0).round().clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
                    d.copy_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
}

pub fn bytes_per_sample(fmt: SampleFormat) -> usize {
    match fmt {
        SampleFormat::I16 => 2,
        SampleFormat::I24 => 3,
        SampleFormat::F32 | SampleFormat::I24In32 | SampleFormat::I24In32Lsb | SampleFormat::I32 => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_rates_like_mac() {
        assert_eq!(expand_rates(&[(44100.0, 44100.0), (48000.0, 192000.0)]), vec![44100.0, 48000.0, 88200.0, 96000.0, 176400.0, 192000.0]);
        assert_eq!(expand_rates(&[(0.0, 0.0)]), Vec::<f64>::new());
    }

    #[test]
    fn exact_samples_pass_bit_perfect() {
        let mut q = Quantizer::default();
        for v in [-32768i32, -1, 0, 1, 12345, 32767] {
            assert_eq!(q.quantize(v as f32 / 32768.0, 16), v);
        }
        for v in [-8_388_608i32, -1, 0, 7, 8_388_607] {
            assert_eq!(q.quantize(v as f32 / 8_388_608.0, 24), v);
        }
        assert_eq!(q.quantize(1.5, 16), 32767);
    }

    #[test]
    fn inexact_samples_are_dithered_within_one_lsb() {
        let mut q = Quantizer::default();
        let x = 0.123_456_7f32;
        let exact = f64::from(x) * 32768.0;
        for _ in 0..1000 {
            let v = q.quantize(x, 16);
            assert!((f64::from(v) - exact).abs() <= 1.5);
        }
    }
}
