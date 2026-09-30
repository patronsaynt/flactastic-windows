//! Encoder priming/padding for MP4 audio (AAC in .m4a), which symphonia 0.5
//! doesn't apply. `AVAudioFile` trims both, so FLACtastic on the Mac plays
//! iTunes/ffmpeg AAC gaplessly; this reads the same information:
//!
//! 1. `iTunSMPB` (`moov/udta/meta/ilst/----`), Apple's gapless tag:
//!    `" 00000000 <delay> <padding> <valid length>..."` in hex.
//! 2. Otherwise the audio track's edit list (`edts/elst`): the first
//!    non-empty edit's `media_time` is the delay and its duration the length.
//!    The duration is in the *movie* timescale; files that use a coarse one
//!    (ffmpeg < 7 writes 1000) only know their length to ±½ tick, for every
//!    player alike.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Frames to drop at the start and how many to play after that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trim {
    pub delay: u64,
    pub length: u64,
}

struct BoxHdr {
    kind: [u8; 4],
    /// Offset of the payload.
    body: u64,
    /// Offset just past the box.
    end: u64,
}

fn read_hdr(f: &mut File, at: u64, limit: u64) -> Option<BoxHdr> {
    if at + 8 > limit {
        return None;
    }
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut h = [0u8; 8];
    f.read_exact(&mut h).ok()?;
    let mut size = u64::from(u32::from_be_bytes(h[..4].try_into().unwrap()));
    let kind: [u8; 4] = h[4..].try_into().unwrap();
    let mut body = at + 8;
    if size == 1 {
        let mut l = [0u8; 8];
        f.read_exact(&mut l).ok()?;
        size = u64::from_be_bytes(l);
        body += 8;
    } else if size == 0 {
        size = limit - at;
    }
    if size < body - at || at + size > limit {
        return None;
    }
    Some(BoxHdr { kind, body, end: at + size })
}

fn children(f: &mut File, start: u64, end: u64) -> Vec<BoxHdr> {
    let mut v = Vec::new();
    let mut at = start;
    while let Some(h) = read_hdr(f, at, end) {
        at = h.end;
        v.push(h);
    }
    v
}

fn find(f: &mut File, start: u64, end: u64, kind: &[u8; 4]) -> Option<BoxHdr> {
    children(f, start, end).into_iter().find(|h| &h.kind == kind)
}

fn read_body(f: &mut File, h: &BoxHdr, max: u64) -> Option<Vec<u8>> {
    let len = (h.end - h.body).min(max);
    f.seek(SeekFrom::Start(h.body)).ok()?;
    let mut b = vec![0u8; len as usize];
    f.read_exact(&mut b).ok()?;
    Some(b)
}

fn be32(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?)))
}

fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// Timescale from an `mvhd`/`mdhd` body (same layout up to the timescale).
fn timescale(b: &[u8]) -> Option<u64> {
    if b.first()? == &1 {
        be32(b, 20)
    } else {
        be32(b, 12)
    }
}

pub fn read(path: &Path, sample_rate: u32) -> Option<Trim> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let moov = find(&mut f, 0, len, b"moov")?;
    if let Some(t) = itunsmpb(&mut f, &moov) {
        return Some(t);
    }
    edit_list(&mut f, &moov, sample_rate)
}

fn itunsmpb(f: &mut File, moov: &BoxHdr) -> Option<Trim> {
    let udta = find(f, moov.body, moov.end, b"udta")?;
    let meta = find(f, udta.body, udta.end, b"meta")?;
    // `meta` is a full box: 4 bytes of version/flags before its children.
    let ilst = find(f, meta.body + 4, meta.end, b"ilst")?;
    for item in children(f, ilst.body, ilst.end).into_iter().filter(|h| &h.kind == b"----") {
        let parts = children(f, item.body, item.end);
        let name = parts.iter().find(|h| &h.kind == b"name").and_then(|h| read_body(f, h, 64))?;
        if name.get(4..) != Some(b"iTunSMPB".as_slice()) {
            continue;
        }
        let data = parts.iter().find(|h| &h.kind == b"data").and_then(|h| read_body(f, h, 256))?;
        let text = String::from_utf8_lossy(data.get(8..)?).into_owned();
        let fields: Vec<u64> = text.split_whitespace().filter_map(|s| u64::from_str_radix(s, 16).ok()).collect();
        // [0] reserved, [1] delay, [2] padding, [3] valid length.
        if fields.len() >= 4 && fields[3] > 0 {
            return Some(Trim { delay: fields[1], length: fields[3] });
        }
    }
    None
}

fn edit_list(f: &mut File, moov: &BoxHdr, sample_rate: u32) -> Option<Trim> {
    let mvhd = find(f, moov.body, moov.end, b"mvhd")?;
    let movie_ts = timescale(&read_body(f, &mvhd, 32)?)?;
    for trak in children(f, moov.body, moov.end).into_iter().filter(|h| &h.kind == b"trak") {
        let Some(mdia) = find(f, trak.body, trak.end, b"mdia") else { continue };
        let hdlr = find(f, mdia.body, mdia.end, b"hdlr");
        let is_audio = hdlr.and_then(|h| read_body(f, &h, 16)).is_some_and(|b| b.get(8..12) == Some(b"soun".as_slice()));
        if !is_audio {
            continue;
        }
        let mdhd = find(f, mdia.body, mdia.end, b"mdhd")?;
        let media_ts = timescale(&read_body(f, &mdhd, 32)?)?;
        let edts = find(f, trak.body, trak.end, b"edts")?;
        let elst = find(f, edts.body, edts.end, b"elst")?;
        let elst = read_body(f, &elst, 4096)?;
        let v1 = elst.first()? == &1;
        let count = be32(&elst, 4)?;
        let mut at = 8;
        for _ in 0..count {
            let (dur, media_time) = if v1 {
                let r = (be64(&elst, at)?, be64(&elst, at + 8)? as i64);
                at += 20;
                r
            } else {
                let r = (be32(&elst, at)?, be32(&elst, at + 4)? as u32 as i32 as i64);
                at += 12;
                r
            };
            if media_time < 0 {
                continue; // an empty edit (presentation offset)
            }
            if movie_ts == 0 || media_ts == 0 {
                return None;
            }
            let sr = u128::from(sample_rate);
            let delay = (media_time as u128 * sr / u128::from(media_ts)) as u64;
            let length = ((u128::from(dur) * sr + u128::from(movie_ts) / 2) / u128::from(movie_ts)) as u64;
            if media_time == 0 && dur == 0 {
                return None;
            }
            return Some(Trim { delay, length });
        }
        return None;
    }
    None
}
