//! libsoxr at VHQ (28-bit, linear phase, 95% passband — soxr's defaults for
//! `SOXR_VHQ`), interleaved float32. Stands in for `AVAudioConverter` with
//! `sampleRateConverterQuality = .max`.

use std::ffi::{c_char, c_uint, c_ulong, c_void, CStr};

#[allow(non_camel_case_types)]
type soxr_t = *mut c_void;
#[allow(non_camel_case_types)]
type soxr_error_t = *const c_char;

#[repr(C)]
struct IoSpec {
    itype: c_uint,
    otype: c_uint,
    scale: f64,
    e: *mut c_void,
    flags: c_ulong,
}

#[repr(C)]
struct QualitySpec {
    precision: f64,
    phase_response: f64,
    passband_end: f64,
    stopband_begin: f64,
    e: *mut c_void,
    flags: c_ulong,
}

#[repr(C)]
struct RuntimeSpec {
    log2_min_dft_size: c_uint,
    log2_large_dft_size: c_uint,
    coef_size_kbytes: c_uint,
    num_threads: c_uint,
    e: *mut c_void,
    flags: c_ulong,
}

const SOXR_FLOAT32_I: c_uint = 0;
const SOXR_VHQ: c_ulong = 6; // SOXR_28_BITQ

extern "C" {
    fn soxr_create(
        input_rate: f64,
        output_rate: f64,
        num_channels: c_uint,
        error: *mut soxr_error_t,
        io_spec: *const IoSpec,
        quality_spec: *const QualitySpec,
        runtime_spec: *const RuntimeSpec,
    ) -> soxr_t;
    fn soxr_process(
        resampler: soxr_t,
        input: *const c_void,
        ilen: usize,
        idone: *mut usize,
        output: *mut c_void,
        olen: usize,
        odone: *mut usize,
    ) -> soxr_error_t;
    fn soxr_delete(resampler: soxr_t);
    fn soxr_clear(resampler: soxr_t) -> soxr_error_t;
    fn soxr_quality_spec(recipe: c_ulong, flags: c_ulong) -> QualitySpec;
    fn soxr_io_spec(itype: c_uint, otype: c_uint) -> IoSpec;
    fn soxr_runtime_spec(num_threads: c_uint) -> RuntimeSpec;
    fn soxr_delay(resampler: soxr_t) -> f64;
}

fn check(e: soxr_error_t) -> Result<(), String> {
    if e.is_null() {
        Ok(())
    } else {
        Err(unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned())
    }
}

/// One streaming resampler for a fixed rate pair and channel count.
pub struct Resampler {
    raw: soxr_t,
    channels: usize,
    pub in_rate: f64,
    pub out_rate: f64,
}

// soxr_t is used from one thread at a time.
unsafe impl Send for Resampler {}

impl Resampler {
    pub fn new(in_rate: f64, out_rate: f64, channels: usize) -> Result<Resampler, String> {
        let mut err: soxr_error_t = std::ptr::null();
        let raw = unsafe {
            let io = soxr_io_spec(SOXR_FLOAT32_I, SOXR_FLOAT32_I);
            let q = soxr_quality_spec(SOXR_VHQ, 0);
            let rt = soxr_runtime_spec(1);
            soxr_create(in_rate, out_rate, channels as c_uint, &mut err, &io, &q, &rt)
        };
        check(err)?;
        if raw.is_null() {
            return Err("soxr_create returned null".into());
        }
        Ok(Resampler { raw, channels, in_rate, out_rate })
    }

    /// Feeds interleaved `input` and appends every produced frame to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        let ch = self.channels;
        let in_frames = input.len() / ch;
        let mut offset = 0;
        loop {
            let want = ((in_frames - offset) as f64 * self.out_rate / self.in_rate) as usize + 1024;
            let start = out.len();
            out.resize(start + want * ch, 0.0);
            let (mut idone, mut odone) = (0usize, 0usize);
            let e = unsafe {
                soxr_process(
                    self.raw,
                    input[offset * ch..].as_ptr().cast(),
                    in_frames - offset,
                    &mut idone,
                    out[start..].as_mut_ptr().cast(),
                    want,
                    &mut odone,
                )
            };
            out.truncate(start + odone * ch);
            check(e)?;
            offset += idone;
            if offset >= in_frames && odone < want {
                return Ok(());
            }
        }
    }

    /// Signals end of input and appends the filter tail.
    pub fn flush(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        let ch = self.channels;
        loop {
            let want = 8192;
            let start = out.len();
            out.resize(start + want * ch, 0.0);
            let mut odone = 0usize;
            let e = unsafe {
                soxr_process(self.raw, std::ptr::null(), 0, std::ptr::null_mut(), out[start..].as_mut_ptr().cast(), want, &mut odone)
            };
            out.truncate(start + odone * ch);
            check(e)?;
            if odone == 0 {
                return Ok(());
            }
        }
    }

    /// Ready for a fresh signal, same configuration.
    pub fn clear(&mut self) {
        unsafe {
            soxr_clear(self.raw);
        }
    }

    /// Current delay in output frames.
    pub fn delay(&self) -> f64 {
        unsafe { soxr_delay(self.raw) }
    }
}

impl Drop for Resampler {
    fn drop(&mut self) {
        unsafe { soxr_delete(self.raw) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: f64, freq: f64, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (2.0 * std::f64::consts::PI * freq * i as f64 / rate).sin() as f32 * 0.5;
                [v, v]
            })
            .collect()
    }

    #[test]
    fn length_and_amplitude_preserved() {
        let input = sine(44100.0, 1000.0, 44100);
        let mut r = Resampler::new(44100.0, 96000.0, 2).unwrap();
        let mut out = Vec::new();
        for chunk in input.chunks(8192 * 2) {
            r.process(chunk, &mut out).unwrap();
        }
        r.flush(&mut out).unwrap();
        let frames = out.len() / 2;
        assert!((frames as i64 - 96000).abs() <= 1, "got {frames} frames");
        // Mid-signal peak ≈ 0.5.
        let peak = out[20000..150000].iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.001, "peak {peak}");
    }
}
