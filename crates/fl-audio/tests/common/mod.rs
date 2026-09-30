//! Shared test fixtures.
#![allow(dead_code)]

use std::path::Path;

use fl_core::Track;

/// Continuous stereo test signal (different tones per channel), as 16-bit ints.
pub fn signal(rate: usize, frames: usize) -> Vec<i32> {
    (0..frames)
        .flat_map(|i| {
            let t = i as f64 / rate as f64;
            let l = (0.4 * (2.0 * std::f64::consts::PI * 997.0 * t).sin() * 32767.0).round() as i32;
            let r = (0.3 * (2.0 * std::f64::consts::PI * 1499.0 * t).sin() * 32767.0).round() as i32;
            [l, r]
        })
        .collect()
}

pub fn write_flac(path: &Path, samples: &[i32], rate: usize) {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    let cfg = flacenc::config::Encoder::default().into_verified().unwrap();
    let src = flacenc::source::MemSource::from_samples(samples, 2, 16, rate);
    let stream = flacenc::encode_with_fixed_block_size(&cfg, src, cfg.block_size).unwrap();
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).unwrap();
    let mut bytes = sink.as_slice().to_vec();
    // flacenc stores the final (short) block in STREAMINFO's minimum block
    // size; libFLAC excludes the last block, so min == max for fixed-size
    // streams. Match libFLAC so decoders treat the stream as fixed-block.
    bytes.copy_within(10..12, 8);
    std::fs::write(path, bytes).unwrap();
}

/// Splits the signal at odd frame counts into three files.
pub fn split_files(dir: &Path, rate: usize, cuts: &[usize], total: usize) -> (Vec<Track>, Vec<i32>) {
    let s = signal(rate, total);
    let mut tracks = Vec::new();
    let mut prev = 0;
    for (i, &cut) in cuts.iter().chain(std::iter::once(&total)).enumerate() {
        let p = dir.join(format!("{i:02}.flac"));
        write_flac(&p, &s[prev * 2..cut * 2], rate);
        let mut t = Track::make_from_path(&p).unwrap();
        t.duration = Some((cut - prev) as f64 / rate as f64);
        tracks.push(t);
        prev = cut;
    }
    (tracks, s)
}

