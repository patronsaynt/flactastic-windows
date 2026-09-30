//! Resampler quality at every rate pair the engine uses: THD+N of a 1 kHz
//! tone, passband flatness near the top of the audio band, and rejection of
//! content above the output Nyquist (aliasing) when downsampling.
//!
//! Measured on the libsoxr VHQ path exactly as the engine drives it
//! (interleaved stereo f32, processed in engine-sized blocks).

use std::f64::consts::PI;

use fl_audio::soxr::Resampler;

const BLOCK: usize = 16_384;

fn tone(rate: f64, freq: f64, amp: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|i| {
            let s = (amp * (2.0 * PI * freq * i as f64 / rate).sin()) as f32;
            [s, s]
        })
        .collect()
}

fn resample(input: &[f32], in_rate: f64, out_rate: f64) -> Vec<f32> {
    let mut r = Resampler::new(in_rate, out_rate, 2).unwrap();
    let mut out = Vec::new();
    let mut buf = Vec::new();
    for chunk in input.chunks(BLOCK * 2) {
        buf.clear();
        r.process(chunk, &mut buf).unwrap();
        out.extend_from_slice(&buf);
    }
    buf.clear();
    r.flush(&mut buf).unwrap();
    out.extend_from_slice(&buf);
    out
}

/// Least-squares fit of `a·cos + b·sin + c` at `freq` over the left channel,
/// ignoring the filter's settling at each end. Returns (amplitude, residual RMS).
fn fit(out: &[f32], rate: f64, freq: f64) -> (f64, f64) {
    let n = out.len() / 2;
    let (lo, hi) = (n / 5, n - n / 5);
    let w = 2.0 * PI * freq / rate;
    // Normal equations for [cos, sin, 1].
    let mut m = [[0.0f64; 3]; 3];
    let mut v = [0.0f64; 3];
    for i in lo..hi {
        let x = f64::from(out[i * 2]);
        let basis = [(w * i as f64).cos(), (w * i as f64).sin(), 1.0];
        for r in 0..3 {
            v[r] += basis[r] * x;
            for c in 0..3 {
                m[r][c] += basis[r] * basis[c];
            }
        }
    }
    let p = solve3(m, v);
    let mut err = 0.0;
    for i in lo..hi {
        let x = f64::from(out[i * 2]);
        let y = p[0] * (w * i as f64).cos() + p[1] * (w * i as f64).sin() + p[2];
        err += (x - y) * (x - y);
    }
    ((p[0] * p[0] + p[1] * p[1]).sqrt(), (err / (hi - lo) as f64).sqrt())
}

fn solve3(mut m: [[f64; 3]; 3], mut v: [f64; 3]) -> [f64; 3] {
    for c in 0..3 {
        let p = (c..3).max_by(|a, b| m[*a][c].abs().total_cmp(&m[*b][c].abs())).unwrap();
        m.swap(c, p);
        v.swap(c, p);
        for r in 0..3 {
            if r != c {
                let f = m[r][c] / m[c][c];
                for k in 0..3 {
                    m[r][k] -= f * m[c][k];
                }
                v[r] -= f * v[c];
            }
        }
    }
    [v[0] / m[0][0], v[1] / m[1][1], v[2] / m[2][2]]
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

fn rms(v: &[f32]) -> f64 {
    let n = v.len() / 2;
    let (lo, hi) = (n / 5, n - n / 5);
    ((lo..hi).map(|i| f64::from(v[i * 2]).powi(2)).sum::<f64>() / (hi - lo) as f64).sqrt()
}

const PAIRS: [(f64, f64); 9] = [
    (44_100.0, 48_000.0),
    (44_100.0, 88_200.0),
    (44_100.0, 96_000.0),
    (44_100.0, 192_000.0),
    (48_000.0, 44_100.0),
    (88_200.0, 44_100.0),
    (96_000.0, 44_100.0),
    (96_000.0, 48_000.0),
    (192_000.0, 48_000.0),
];

#[test]
fn thd_n_of_1khz_tone_below_minus_130_db() {
    for (fi, fo) in PAIRS {
        let amp = 10f64.powf(-3.0 / 20.0);
        let out = resample(&tone(fi, 997.0, amp, fi as usize), fi, fo);
        let expected = (fi as usize as f64 * fo / fi).round();
        assert!((out.len() as f64 / 2.0 - expected).abs() <= 1.0, "{fi}->{fo}: {} frames, want {expected}", out.len() / 2);
        let (a, resid) = fit(&out, fo, 997.0);
        let thdn = db(resid / a);
        let gain = db(a / amp);
        eprintln!("{fi:>6} -> {fo:>6}: THD+N {thdn:7.1} dB, gain {gain:+.5} dB");
        assert!(thdn < -130.0, "{fi}->{fo}: THD+N {thdn:.1} dB");
        assert!(gain.abs() < 0.001, "{fi}->{fo}: gain {gain} dB");
    }
}

#[test]
fn passband_is_flat_to_20khz() {
    for (fi, fo) in PAIRS {
        let top = 20_000.0f64.min(0.45 * fi.min(fo));
        for f in [100.0, 10_000.0, top] {
            let out = resample(&tone(fi, f, 0.5, fi as usize / 2), fi, fo);
            let (a, _) = fit(&out, fo, f);
            let dev = db(a / 0.5);
            eprintln!("{fi:>6} -> {fo:>6}: {f:>7.0} Hz {dev:+.4} dB");
            assert!(dev.abs() < 0.02, "{fi}->{fo} at {f} Hz: {dev:+.4} dB");
        }
    }
}

#[test]
fn content_above_output_nyquist_is_rejected() {
    for (fi, fo) in PAIRS.into_iter().filter(|(fi, fo)| fo < fi) {
        // Halfway between the output Nyquist and the input Nyquist, where an
        // alias would land squarely in the audible band.
        let f = (fo / 2.0 + fi / 2.0) / 2.0;
        let out = resample(&tone(fi, f, 0.5, fi as usize / 2), fi, fo);
        let level = db(rms(&out) / (0.5 / 2f64.sqrt()));
        eprintln!("{fi:>6} -> {fo:>6}: {f:>7.0} Hz leaks at {level:7.1} dB");
        assert!(level < -130.0, "{fi}->{fo}: {f} Hz aliases at {level:.1} dB");
    }
}
