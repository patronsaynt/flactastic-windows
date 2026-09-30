//! Offline engine tests through the null backend: bit-perfect passthrough,
//! gapless joins with and without resampling, seek, and queue edits.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fl_audio::output::null::NullBackend;
use fl_audio::soxr::Resampler;
use fl_audio::Engine;
use fl_core::Track;

/// Continuous stereo test signal (different tones per channel), as 16-bit ints.
fn signal(rate: usize, frames: usize) -> Vec<i32> {
    (0..frames)
        .flat_map(|i| {
            let t = i as f64 / rate as f64;
            let l = (0.4 * (2.0 * std::f64::consts::PI * 997.0 * t).sin() * 32767.0).round() as i32;
            let r = (0.3 * (2.0 * std::f64::consts::PI * 1499.0 * t).sin() * 32767.0).round() as i32;
            [l, r]
        })
        .collect()
}

fn write_flac(path: &Path, samples: &[i32], rate: usize) {
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
fn split_files(dir: &Path, rate: usize, cuts: &[usize], total: usize) -> (Vec<Track>, Vec<i32>) {
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

fn run_to_end(engine: &mut Engine, timeout: Duration) {
    let start = Instant::now();
    while !engine.finished {
        engine.update_time();
        assert!(start.elapsed() < timeout, "playback didn't finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn to_i16(v: &[f32]) -> Vec<i32> {
    v.iter().map(|s| (s * 32768.0).round() as i32).collect()
}

#[test]
fn bit_perfect_and_gapless_at_source_rate() {
    let dir = tempfile::tempdir().unwrap();
    let rate = 44_100;
    let (tracks, source) = split_files(dir.path(), rate, &[30_011, 61_777], 100_003);
    let backend = NullBackend::new(rate as u32);
    let mut e = Engine::new(backend.clone());
    e.set_volume(1.0);
    e.set_queue(tracks.clone(), 0);
    e.play();
    run_to_end(&mut e, Duration::from_secs(20));
    let out = to_i16(&backend.take());
    assert_eq!(out.len(), source.len(), "no inserted or dropped frames");
    assert!(out == source, "output must equal the decoded source sample-for-sample");
    assert_eq!(e.current_track.as_ref().map(|t| t.id), Some(tracks[2].id));
}

#[test]
fn resampled_joins_match_one_continuous_resample() {
    let dir = tempfile::tempdir().unwrap();
    let rate = 44_100;
    let (tracks, source) = split_files(dir.path(), rate, &[40_000, 70_123], 110_000);
    let backend = NullBackend::new(48_000);
    let mut e = Engine::new(backend.clone());
    e.set_volume(1.0);
    e.set_queue(tracks, 0);
    e.play();
    run_to_end(&mut e, Duration::from_secs(20));
    let out = backend.take();

    // Reference: the whole signal through one resampler.
    let src_f: Vec<f32> = source.iter().map(|v| *v as f32 / 32768.0).collect();
    let mut r = Resampler::new(44_100.0, 48_000.0, 2).unwrap();
    let mut reference = Vec::new();
    for c in src_f.chunks(16_384 * 2) {
        r.process(c, &mut reference).unwrap();
    }
    r.flush(&mut reference).unwrap();

    assert!((out.len() as i64 - reference.len() as i64).abs() <= 4, "{} vs {}", out.len(), reference.len());
    let n = out.len().min(reference.len());
    let max_diff = out[..n].iter().zip(&reference[..n]).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
    assert!(max_diff < 1e-5, "joins must not disturb the signal (max diff {max_diff})");
}

#[test]
fn seek_lands_on_the_exact_frame() {
    let dir = tempfile::tempdir().unwrap();
    let rate = 44_100;
    let (tracks, source) = split_files(dir.path(), rate, &[], 88_200);
    let backend = NullBackend::new(rate as u32);
    let mut e = Engine::new(backend.clone());
    e.set_volume(1.0);
    e.set_queue(tracks, 0);
    e.seek(1.0);
    e.play();
    run_to_end(&mut e, Duration::from_secs(20));
    let out = to_i16(&backend.take());
    assert_eq!(out, source[44_100 * 2..].to_vec());
}

#[test]
fn volume_scales_linearly() {
    let dir = tempfile::tempdir().unwrap();
    let (tracks, source) = split_files(dir.path(), 44_100, &[], 20_000);
    let backend = NullBackend::new(44_100);
    let mut e = Engine::new(backend.clone());
    e.set_volume(0.5);
    e.set_queue(tracks, 0);
    e.play();
    run_to_end(&mut e, Duration::from_secs(20));
    let out = backend.take();
    let i = 12_345;
    assert!((out[i] - 0.5 * source[i] as f32 / 32768.0).abs() < 1e-7);
}

#[test]
fn reorder_keeps_current_track_and_plays_new_next() {
    let dir = tempfile::tempdir().unwrap();
    let rate = 44_100;
    // Long enough that only the current track is buffered when we reorder.
    let (tracks, _) = split_files(dir.path(), rate, &[rate * 30, rate * 31], rate * 32);
    // Paced like a device, so only the current track is buffered at reorder time.
    let backend = NullBackend::paced(rate as u32, 10.0);
    let mut e = Engine::new(backend.clone());
    e.set_volume(1.0);
    e.set_queue(tracks.clone(), 0);
    e.play();
    std::thread::sleep(Duration::from_millis(200));
    e.update_time();
    // Swap the last two tracks.
    e.reorder_queue(vec![tracks[0].clone(), tracks[2].clone(), tracks[1].clone()], 0);
    run_to_end(&mut e, Duration::from_secs(30));
    let out = backend.take();
    assert_eq!(out.len() / 2, rate * 32, "reorder must not drop or repeat audio");
    assert_eq!(e.current_track.as_ref().map(|t| t.id), Some(tracks[1].id), "old 2nd track now plays last");
}

#[test]
fn repeat_one_loops_gaplessly() {
    let dir = tempfile::tempdir().unwrap();
    let (tracks, source) = split_files(dir.path(), 44_100, &[], 10_000);
    let backend = NullBackend::new(44_100);
    let mut e = Engine::new(backend.clone());
    e.set_volume(1.0);
    e.set_queue(tracks, 0);
    e.set_repeat_one(true);
    e.play();
    let start = Instant::now();
    while backend.captured.lock().unwrap().len() < source.len() * 3 {
        e.update_time();
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = to_i16(&backend.take());
    for k in 0..3 {
        assert_eq!(&out[k * source.len()..(k + 1) * source.len()], &source[..], "loop {k}");
    }
}

#[allow(dead_code)]
fn unused(_: PathBuf, _: Arc<()>) {}

#[test]
fn probe_decoder_directly() {
    let dir = tempfile::tempdir().unwrap();
    let (tracks, source) = split_files(dir.path(), 44_100, &[], 20_000);
    let mut s = fl_audio::decode::Source::open(&tracks[0].path).unwrap();
    eprintln!("rate {} ch {} frames {:?} bits {:?}", s.sample_rate, s.channels, s.frames, s.bits_per_sample);
    let mut out = Vec::new();
    let mut blocks = 0;
    loop {
        match s.next_block(&mut out) {
            Ok(true) => blocks += 1,
            Ok(false) => break,
            Err(e) => panic!("decode error after {blocks} blocks: {e}"),
        }
    }
    eprintln!("blocks {blocks} samples {}", out.len());
    assert_eq!(to_i16(&out), source);
}

