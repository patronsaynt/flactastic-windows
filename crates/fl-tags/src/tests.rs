use super::*;
use std::path::PathBuf;

use fl_core::AudioFileFormat;

/// 16-bit stereo PCM WAV, `secs` of silence at `rate`.
fn write_wav(path: &Path, rate: u32, secs: u32) {
    let frames = rate * secs;
    let data_len = frames * 4;
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
    b.resize(44 + data_len as usize, 0);
    std::fs::write(path, b).unwrap();
}

/// 24-bit stereo FLAC with a quiet ramp.
fn write_flac(path: &Path, rate: usize) {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    let frames = rate;
    let samples: Vec<i32> = (0..frames * 2).map(|i| ((i as i32 % 2000) - 1000) * 100).collect();
    let cfg = flacenc::config::Encoder::default().into_verified().unwrap();
    let source = flacenc::source::MemSource::from_samples(&samples, 2, 24, rate);
    let stream = flacenc::encode_with_fixed_block_size(&cfg, source, cfg.block_size).unwrap();
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).unwrap();
    std::fs::write(path, sink.as_slice()).unwrap();
}

fn fixture(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join(name);
    match p.extension().and_then(|e| e.to_str()) {
        Some("wav") => write_wav(&p, 44100, 1),
        Some("flac") => write_flac(&p, 96000),
        _ => unreachable!(),
    }
    (dir, p)
}

fn tag_write(title: &str) -> TagWrite {
    TagWrite {
        title: title.into(),
        artist: Some("Deadmau5 ; Rob Swire".into()),
        album: Some("For Lack of a Better Name".into()),
        year: Some(2009),
        genre: Some("Electronic".into()),
        secondary_genres: vec!["House".into(), "electronic".into(), "Progressive".into()],
        track_number: Some(7),
        artwork: ArtworkChange::Unchanged,
        album_artist: Some(Some("deadmau5".into())),
        compilation: Some(true),
        mix_compilation: None,
    }
}

fn scan(p: &Path) -> Track {
    let mut t = Track::make_from_path(p).unwrap();
    apply_tags(&mut t, &TagFile::open(p).unwrap());
    t
}

#[test]
fn flac_round_trip_matches_scanner() {
    let (_d, p) = fixture("Ghosts ’n’ Stuff — ünïcödé.flac");
    let t = Track::make_from_path(&p).unwrap();
    assert_eq!(t.file_format, AudioFileFormat::Flac);

    let written = write(&t, &tag_write("Ghosts 'n' Stuff")).unwrap();
    let read = scan(&p);
    assert_eq!(read.title, "Ghosts 'n' Stuff");
    assert_eq!(read.artist.as_deref(), Some("Deadmau5 ; Rob Swire"));
    assert_eq!(read.album_artist.as_deref(), Some("deadmau5"));
    assert_eq!(read.genre.as_deref(), Some("Electronic"));
    assert_eq!(read.secondary_genres, vec!["House", "Progressive"]);
    assert_eq!(read.year, Some(2009));
    assert_eq!(read.track_number, Some(7));
    assert!(read.is_compilation && !read.is_mix_compilation);
    // `write` returns what a fresh scan reads.
    assert_eq!(written.genre, read.genre);
    assert_eq!(written.secondary_genres, read.secondary_genres);

    let f = TagFile::open(&p).unwrap();
    assert_eq!(f.property("GENRE").as_deref(), Some("Electronic ; House ; Progressive"));
    let a = f.audio().unwrap();
    assert_eq!((a.sample_rate, a.channels, a.bits_per_sample), (96000, 2, 24));
    assert_eq!(a.length_ms, 1000);
    assert_eq!(bits_per_sample(&p), 24);
}

#[test]
fn mix_compilation_clears_lyrics_and_compilation() {
    let (_d, p) = fixture("mix.flac");
    let t = Track::make_from_path(&p).unwrap();
    write(&t, &tag_write("Mix")).unwrap();
    write_lyrics(&p, Some("[00:01.00]la")).unwrap();
    assert_eq!(read_lyrics(&p).as_deref(), Some("[00:01.00]la"));

    let mut w = tag_write("Mix");
    w.compilation = None;
    w.mix_compilation = Some(true);
    let u = write(&t, &w).unwrap();
    assert!(u.is_mix_compilation && !u.is_compilation);
    let r = scan(&p);
    assert!(r.is_mix_compilation && !r.is_compilation);
    assert_eq!(read_lyrics(&p), None);
}

#[test]
fn artwork_and_markers() {
    let (_d, p) = fixture("art.flac");
    let t = Track::make_from_path(&p).unwrap();
    let png = [0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
    let mut w = tag_write("Art");
    w.artwork = ArtworkChange::Updated(png.to_vec());
    write(&t, &w).unwrap();
    assert_eq!(read_picture(&p).as_deref(), Some(&png[..]));

    w.artwork = ArtworkChange::Removed;
    write(&t, &w).unwrap();
    assert_eq!(read_picture(&p), None);

    let markers = vec![TrackMarker::new(0.0, "Intro"), TrackMarker::new(95.5, "Drop")];
    write_markers(&p, &markers).unwrap();
    let back = read_markers(&p);
    assert_eq!(back.len(), 2);
    assert_eq!(back[1].title, "Drop");
    write_markers(&p, &[]).unwrap();
    assert!(read_markers(&p).is_empty());
}

#[test]
fn wav_round_trip() {
    let (_d, p) = fixture("tone.wav");
    let t = Track::make_from_path(&p).unwrap();
    let mut w = tag_write("Tone");
    w.compilation = Some(false);
    write(&t, &w).unwrap();
    let r = scan(&p);
    assert_eq!(r.title, "Tone");
    assert_eq!(r.album_artist.as_deref(), Some("deadmau5"));
    assert!(!r.is_compilation);
    let a = TagFile::open(&p).unwrap().audio().unwrap();
    assert_eq!((a.sample_rate, a.bits_per_sample), (44100, 16));
    override_title(&p, "Retitled").unwrap();
    assert_eq!(scan(&p).title, "Retitled");
}

#[test]
fn missing_file_errors() {
    let t = Track::new(PathBuf::from("Z:/nope/none.flac"), "x".into(), AudioFileFormat::Flac);
    assert!(matches!(write(&t, &tag_write("x")), Err(TagError::FileNotFound(_))));
    assert!(TagFile::open(Path::new("Z:/nope/none.flac")).is_none());
}

#[test]
fn mime_sniffing() {
    assert_eq!(mime_type(&[0xFF, 0xD8, 0xFF, 0xE0]), "image/jpeg");
    assert_eq!(mime_type(&[0x89, 0x50, 0x4E, 0x47]), "image/png");
    assert_eq!(mime_type(b"GIF8"), "image/jpeg");
}
