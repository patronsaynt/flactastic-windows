//! LRC / plain-text lyrics.

use serde::{Deserialize, Serialize};

use crate::text::trim_ws;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricsLine {
    /// `None` for an unsynced (plain-text) source.
    pub timestamp: Option<f64>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub lines: Vec<LyricsLine>,
    pub is_synced: bool,
}

impl Lyrics {
    /// Most recent line whose timestamp ≤ `time`; 0 before anything plays.
    pub fn current_line_index(&self, time: f64) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let mut idx = 0;
        for (i, line) in self.lines.iter().enumerate() {
            let Some(ts) = line.timestamp else { continue };
            if ts <= time {
                idx = i;
            } else {
                break;
            }
        }
        Some(idx)
    }

    /// Multi-timestamp lines, `[mm:ss]`/`[mm:ss.xx]`; metadata tags dropped.
    pub fn parse_lrc(raw: &str) -> Lyrics {
        let mut out: Vec<LyricsLine> = Vec::new();
        for line in raw.split(['\n', '\r']).filter(|l| !l.is_empty()) {
            let mut rest = line;
            let mut stamps = Vec::new();
            while rest.starts_with('[') {
                let Some(close) = rest.find(']') else { break };
                if let Some(ts) = parse_timestamp(&rest[1..close]) {
                    stamps.push(ts);
                }
                rest = &rest[close + 1..];
            }
            let text = trim_ws(rest).to_owned();
            for ts in stamps {
                out.push(LyricsLine { timestamp: Some(ts), text: text.clone() });
            }
        }
        out.sort_by(|a, b| {
            a.timestamp.unwrap_or(0.0).partial_cmp(&b.timestamp.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
        });
        let synced = !out.is_empty();
        Lyrics { lines: out, is_synced: synced }
    }

    /// Pseudo-synced lines spread uniformly across `duration`.
    pub fn from_plain_text(raw: &str, duration: f64) -> Lyrics {
        let raw = raw.replace("\r\n", "\n");
        let parts: Vec<&str> = raw.split('\n').collect();
        let step = duration.max(1.0) / parts.len() as f64;
        let lines = parts
            .iter()
            .enumerate()
            .map(|(i, t)| LyricsLine { timestamp: Some(i as f64 * step), text: trim_ws(t).to_owned() })
            .collect();
        Lyrics { lines, is_synced: false }
    }

    /// `[mm:ss.xx]Text` for stamped lines; plain text otherwise.
    pub fn serialize_lrc(lines: &[(Option<f64>, String)]) -> String {
        lines
            .iter()
            .map(|(ts, text)| match ts {
                Some(ts) if *ts >= 0.0 => {
                    let minutes = (*ts as i64) / 60;
                    let seconds = ts % 60.0;
                    format!("[{minutes:02}:{seconds:05.2}]{text}")
                }
                _ => text.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// `mm:ss[.xx]`; `None` for metadata tags like `[ar:Artist]`.
fn parse_timestamp(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let m: i64 = {
        let p = parts[0];
        let digits = p.strip_prefix(['+', '-']).unwrap_or(p);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        p.parse().ok()?
    };
    let sec_field = parts[1];
    if sec_field.is_empty() || sec_field.starts_with(char::is_whitespace) || sec_field.ends_with(char::is_whitespace) {
        return None;
    }
    let sec: f64 = sec_field.parse().ok()?;
    Some(m as f64 * 60.0 + sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc() {
        let l = Lyrics::parse_lrc("[ar:Someone]\n[00:05.00][00:01.50]Hook\n[00:03]Verse\n[00:04.00]");
        assert!(l.is_synced);
        let ts: Vec<f64> = l.lines.iter().map(|l| l.timestamp.unwrap()).collect();
        assert_eq!(ts, vec![1.5, 3.0, 4.0, 5.0]);
        assert_eq!(l.lines[2].text, "");
        assert_eq!(l.current_line_index(3.5), Some(1));
        assert_eq!(l.current_line_index(0.0), Some(0));
    }

    #[test]
    fn serializes_lrc() {
        let s = Lyrics::serialize_lrc(&[(Some(65.5), "A".into()), (None, "B".into())]);
        assert_eq!(s, "[01:05.50]A\nB");
    }

    #[test]
    fn plain_text_is_pseudo_synced() {
        let l = Lyrics::from_plain_text("a\r\nb", 10.0);
        assert!(!l.is_synced);
        assert_eq!(l.lines[1].timestamp, Some(5.0));
    }
}
