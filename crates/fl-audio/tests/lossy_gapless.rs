//! Gapless joins for lossy formats: a continuous tone split across three
//! files, encoded with ffmpeg (MP3 via LAME with its delay/padding tag; AAC in
//! M4A with an edit list), played back to back through the engine.
//!
//! Needs `ffmpeg` on PATH (or `FFMPEG`); skipped with a note otherwise.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use fl_audio::output::null::NullBackend;
use fl_audio::Engine;
use fl_core::Track;

fn ffmpeg() -> Option<PathBuf> {
    let exe = std::env::var_os("FFMPEG").map(PathBuf::from).unwrap_or_else(|| "ffmpeg".into());
    let ok = Command::new(&exe).arg("-version").output().map(|o| o.status.success()).unwrap_or(false);
    if ok {
        return Some(exe);
    }
    // winget's per-user package, for shells started before the install.
    let pkgs = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Microsoft/WinGet/Packages");
    std::fs::read_dir(pkgs).ok()?.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("Gyan.FFmpeg")).find_map(|pkg| {
        std::fs::read_dir(pkg.path()).ok()?.flatten().map(|d| d.path().join("bin/ffmpeg.exe")).find(|p| p.exists())
    })
}

fn write_wav(path: &Path, samples: &[i16], rate: u32) {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

/// A slow sine sweep in both channels: smooth everywhere, so any gap, repeat
/// or dropped block at a join shows up as a jump against the source.
fn source(rate: u32, frames: usize) -> Vec<f64> {
    (0..frames)
        .map(|i| {
            let t = i as f64 / f64::from(rate);
            0.5 * (2.0 * std::f64::consts::PI * (220.0 * t + 40.0 * t * t)).sin()
        })
        .collect()
}

fn encode(ff: &Path, wav: &Path, out: &Path, codec: &[&str]) {
    let st = Command::new(ff)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(wav)
        .args(codec)
        .arg(out)
        .status()
        .unwrap();
    assert!(st.success(), "ffmpeg failed for {}", out.display());
}

/// ffmpeg's own decode (which applies LAME/edit-list trimming), left channel.
fn reference_decode(ff: &Path, file: &Path, out: &Path) -> Vec<f64> {
    let st = Command::new(ff)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(file)
        .args(["-c:a", "pcm_f32le"])
        .arg(out)
        .status()
        .unwrap();
    assert!(st.success());
    let bytes = std::fs::read(out).unwrap();
    let pos = bytes.windows(4).position(|w| w == b"data").unwrap() + 8;
    bytes[pos..].chunks_exact(8).map(|c| f64::from(f32::from_le_bytes(c[..4].try_into().unwrap()))).collect()
}

/// Plays three files back to back and compares against the per-file reference
/// decodes laid end to end: any gap, overlap or leftover priming at a join
/// shows up as a large error there. (Comparing against the original signal
/// would mostly measure the encoder, which rings at each file's hard start.)
fn run(ext: &str, codec: &[&str]) {
    let Some(ff) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping {ext} gapless test");
        return;
    };
    let rate = 44_100u32;
    let cuts = [70_001usize, 151_003];
    let total = 230_017usize;
    let src = source(rate, total);
    let dir = tempfile::tempdir().unwrap();
    let mut tracks = Vec::new();
    let mut reference = Vec::new();
    let mut prev = 0;
    for (i, &cut) in cuts.iter().chain(std::iter::once(&total)).enumerate() {
        let pcm: Vec<i16> = src[prev..cut]
            .iter()
            .flat_map(|s| {
                let v = (s * 32767.0).round() as i16;
                [v, v]
            })
            .collect();
        let wav = dir.path().join(format!("{i}.wav"));
        write_wav(&wav, &pcm, rate);
        let p = dir.path().join(format!("{i}.{ext}"));
        encode(&ff, &wav, &p, codec);
        let r = reference_decode(&ff, &p, &dir.path().join(format!("{i}.ref.wav")));
        assert_eq!(r.len(), cut - prev, "{ext}: ffmpeg's own decode of file {i} has the wrong length");
        reference.extend(r);
        tracks.push(Track::make_from_path(&p).unwrap());
        prev = cut;
    }

    let backend = NullBackend::new(rate);
    let mut e = Engine::new(backend.clone());
    e.set_volume(1.0);
    e.set_queue(tracks, 0);
    e.play();
    let start = Instant::now();
    while !e.finished {
        e.update_time();
        assert!(start.elapsed() < Duration::from_secs(30), "playback didn't finish");
        std::thread::sleep(Duration::from_millis(5));
    }
    let out: Vec<f64> = backend.take().chunks(2).map(|f| f64::from(f[0])).collect();
    let out = &out[..out.len().min(e.played_frames() as usize)];
    assert_eq!(out.len(), total, "{ext}: played length differs from the source (delay/padding not trimmed)");

    let err_db = |lo: usize, hi: usize| {
        let e: f64 = (lo..hi).map(|i| (out[i] - reference[i]).powi(2)).sum::<f64>() / (hi - lo) as f64;
        10.0 * (e / 0.125).log10()
    };
    let overall = err_db(0, total);
    eprintln!("{ext}: vs reference decode {overall:.1} dB");
    assert!(overall < -90.0, "{ext}: differs from the reference decode by {overall:.1} dB");
    for &c in &cuts {
        let j = err_db(c - 1024, c + 1024);
        eprintln!("{ext}: join at {c}: {j:.1} dB");
        assert!(j < -90.0, "{ext}: join at {c} differs by {j:.1} dB");
    }
}

#[test]
fn mp3_lame_is_gapless() {
    run("mp3", &["-c:a", "libmp3lame", "-b:a", "320k"]);
}

#[test]
fn aac_m4a_is_gapless() {
    run("m4a", &["-c:a", "aac", "-b:a", "256k"]);
}

#[test]
fn aac_seek_lands_on_the_exact_frame() {
    let Some(ff) = ffmpeg() else {
        eprintln!("ffmpeg not found; skipping");
        return;
    };
    let rate = 44_100u32;
    let src = source(rate, 132_300);
    let dir = tempfile::tempdir().unwrap();
    let pcm: Vec<i16> = src.iter().flat_map(|s| { let v = (s * 32767.0).round() as i16; [v, v] }).collect();
    let wav = dir.path().join("a.wav");
    write_wav(&wav, &pcm, rate);
    let p = dir.path().join("a.m4a");
    encode(&ff, &wav, &p, &["-c:a", "aac", "-b:a", "256k"]);
    let reference = reference_decode(&ff, &p, &dir.path().join("ref.wav"));
    for secs in [0.5, 1.0, 1.7371] {
        let mut s = fl_audio::decode::Source::open(&p).unwrap();
        s.seek(secs).unwrap();
        let mut v = Vec::new();
        while v.len() < 8192 * 2 && s.next_block(&mut v).unwrap() {}
        let at = (secs * f64::from(rate)).round() as usize;
        let err: f64 = (0..8192).map(|i| (f64::from(v[i * 2]) - reference[at + i]).powi(2)).sum::<f64>() / 8192.0;
        let db = 10.0 * (err / 0.125).log10();
        eprintln!("seek {secs}: {db:.1} dB");
        assert!(db < -90.0, "seek to {secs}s differs from the reference by {db:.1} dB");
    }
}
