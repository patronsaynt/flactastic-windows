//! `TrackMarker` and the embedded single-FILE `CUESHEET` codec.

use crate::apple_json::Uid;
use crate::text::{trim, trim_ws};

#[derive(Debug, Clone, PartialEq)]
pub struct TrackMarker {
    pub id: Uid,
    pub timestamp: f64,
    pub title: String,
}

impl TrackMarker {
    pub fn new(timestamp: f64, title: impl Into<String>) -> Self {
        TrackMarker { id: Uid::new_v4(), timestamp, title: title.into() }
    }

    /// "mm:ss", "m:ss" or "h:mm:ss" → seconds.
    pub fn parse_user_timestamp(text: &str) -> Option<f64> {
        let parts: Vec<&str> = trim_ws(text).split(':').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        let v: Option<Vec<f64>> = parts.iter().map(|p| swift_double(p)).collect();
        let v = v?;
        Some(match v.len() {
            1 => v[0],
            2 => v[0] * 60.0 + v[1],
            _ => v[0] * 3600.0 + v[1] * 60.0 + v[2],
        })
    }
}

/// `Double(String)` — rejects surrounding whitespace, unlike a lenient parse.
fn swift_double(s: &str) -> Option<f64> {
    if s.is_empty() || s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) {
        return None;
    }
    s.parse::<f64>().ok()
}

pub fn sorted_by_time(markers: &[TrackMarker]) -> Vec<TrackMarker> {
    let mut v = markers.to_vec();
    v.sort_by(|a, b| a.timestamp.partial_cmp(&b.timestamp).unwrap_or(std::cmp::Ordering::Equal));
    v
}

pub fn encode(markers: &[TrackMarker], file_name: &str) -> String {
    let mut lines = vec![format!("FILE \"{}\" WAVE", escape_quotes(file_name))];
    for (i, m) in sorted_by_time(markers).iter().enumerate() {
        lines.push(format!("  TRACK {:02} AUDIO", i + 1));
        let title = trim(&m.title);
        if !title.is_empty() {
            lines.push(format!("    TITLE \"{}\"", escape_quotes(title)));
        }
        lines.push(format!("    INDEX 01 {}", format_cue_timestamp(m.timestamp)));
    }
    lines.join("\n")
}

pub fn decode(text: &str) -> Vec<TrackMarker> {
    let mut markers = Vec::new();
    let mut pending_title: Option<String> = None;
    for raw in text.split('\n').filter(|l| !l.is_empty()) {
        let line = trim_ws(raw);
        if line.starts_with("TRACK ") {
            pending_title = None;
        } else if let Some(rest) = line.strip_prefix("TITLE ") {
            pending_title = Some(unquote(rest));
        } else if let Some(rest) = line.strip_prefix("INDEX 01 ") {
            if let Some(secs) = parse_cue_timestamp(trim_ws(rest)) {
                markers.push(TrackMarker::new(secs, pending_title.clone().unwrap_or_default()));
            }
        }
    }
    sorted_by_time(&markers)
}

/// mm:ss:ff (75 frames/s) → seconds.
pub fn parse_cue_timestamp(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').filter(|p| !p.is_empty()).collect();
    if parts.len() != 3 {
        return None;
    }
    let m: i64 = swift_int(parts[0])?;
    let sec: i64 = swift_int(parts[1])?;
    let f: i64 = swift_int(parts[2])?;
    Some((m * 60 + sec) as f64 + f as f64 / 75.0)
}

/// `Int(String)` — optional sign, ASCII digits only.
fn swift_int(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

pub fn format_cue_timestamp(seconds: f64) -> String {
    let total_frames = (seconds * 75.0).round() as i64;
    let f = total_frames % 75;
    let total_seconds = total_frames / 75;
    format!("{:02}:{:02}:{:02}", total_seconds / 60, total_seconds % 60, f)
}

fn escape_quotes(s: &str) -> String {
    s.replace('"', "'")
}

fn unquote(s: &str) -> String {
    let t = trim_ws(s);
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_owned()
    } else {
        t.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let markers = vec![TrackMarker::new(195.4933, "Second \"one\""), TrackMarker::new(0.0, "First")];
        let text = encode(&markers, "mix.flac");
        assert_eq!(
            text,
            "FILE \"mix.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"First\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Second 'one'\"\n    INDEX 01 03:15:37"
        );
        let back = decode(&text);
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].title, "Second 'one'");
        assert!((back[1].timestamp - (195.0 + 37.0 / 75.0)).abs() < 1e-9);
    }

    #[test]
    fn user_timestamps() {
        assert_eq!(TrackMarker::parse_user_timestamp("3:15"), Some(195.0));
        assert_eq!(TrackMarker::parse_user_timestamp("1:00:00"), Some(3600.0));
        assert_eq!(TrackMarker::parse_user_timestamp("abc"), None);
        assert_eq!(TrackMarker::parse_user_timestamp(""), None);
    }
}
