//! `AudioFileFormat`, `AudioQuality` and `FormatUtils`.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFileFormat {
    Flac,
    Mp3,
    Wav,
    Aiff,
    Alac,
    Aac,
}

impl AudioFileFormat {
    pub const ALL: [AudioFileFormat; 6] = [Self::Flac, Self::Mp3, Self::Wav, Self::Aiff, Self::Alac, Self::Aac];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Aiff => "aiff",
            Self::Alac => "alac",
            Self::Aac => "aac",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Flac => "FLAC",
            Self::Mp3 => "MP3",
            Self::Wav => "WAV",
            Self::Aiff => "AIFF",
            Self::Alac => "ALAC",
            Self::Aac => "AAC",
        }
    }

    pub fn classify(path: &Path) -> Option<Self> {
        Self::classify_ext(path.extension()?.to_str()?)
    }

    pub fn classify_ext(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "flac" => Some(Self::Flac),
            "mp3" => Some(Self::Mp3),
            "wav" | "wave" => Some(Self::Wav),
            "aif" | "aiff" => Some(Self::Aiff),
            // ALAC or AAC; both decode fine.
            "m4a" => Some(Self::Alac),
            "aac" => Some(Self::Aac),
            _ => None,
        }
    }

    pub fn is_lossy(self) -> bool {
        matches!(self, Self::Mp3 | Self::Aac)
    }
}

/// Quality tiers from sample rate and bit depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioQuality {
    HiRes,
    Cd,
    Mid,
    Low,
}

impl AudioQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::HiRes => "Hi-Res",
            Self::Cd => "CD",
            Self::Mid => "Mid",
            Self::Low => "Low",
        }
    }

    pub fn classify(sample_rate: Option<f64>, bit_depth: Option<i64>, format: AudioFileFormat) -> Self {
        let rate = sample_rate.unwrap_or(0.0);
        let bits = bit_depth.unwrap_or(0);
        if format.is_lossy() {
            return if rate >= 44100.0 { Self::Mid } else { Self::Low };
        }
        if bits > 16 || rate > 44100.0 {
            Self::HiRes
        } else if (bits == 16 && rate >= 44100.0) || bits >= 16 || rate >= 44100.0 {
            Self::Cd
        } else {
            Self::Mid
        }
    }
}

/// `Int(x.rounded())` — Swift rounds half away from zero, like `f64::round`.
fn round_i(x: f64) -> i64 {
    x.round() as i64
}

pub fn format_duration(seconds: Option<f64>) -> String {
    let Some(seconds) = seconds.filter(|s| s.is_finite() && *s >= 0.0) else {
        return "--:--".into();
    };
    let total = round_i(seconds);
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "4h 12m" / "38m".
pub fn coarse_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0m".into();
    }
    let total = round_i(seconds);
    let (h, m) = (total / 3600, (total % 3600) / 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else {
        format!("{m}m")
    }
}

/// "64 tracks · 4h 12m".
pub fn playlist_summary(track_count: usize, duration: f64) -> String {
    let tracks = format!("{track_count} track{}", if track_count == 1 { "" } else { "s" });
    if duration > 0.0 {
        format!("{tracks} · {}", coarse_duration(duration))
    } else {
        tracks
    }
}

/// "96 kHz" / "44.1 kHz".
pub fn kilohertz_string(rate: f64) -> String {
    let khz = rate / 1000.0;
    if khz == khz.round() {
        format!("{khz:.0} kHz")
    } else {
        format!("{khz:.1} kHz")
    }
}

/// "FLAC · 24-BIT / 96 kHz".
pub fn tech_spec(format: AudioFileFormat, bit_depth: Option<i64>, sample_rate: Option<f64>) -> String {
    let mut parts = vec![format.display_name().to_owned()];
    let mut fidelity = Vec::new();
    if let Some(b) = bit_depth {
        fidelity.push(format!("{b}-BIT"));
    }
    if let Some(r) = sample_rate {
        fidelity.push(kilohertz_string(r));
    }
    if !fidelity.is_empty() {
        parts.push(fidelity.join(" / "));
    }
    parts.join(" · ")
}

pub fn format_sample_rate(rate: Option<f64>, bit_depth: Option<i64>) -> Option<String> {
    let rate = rate?;
    Some(match bit_depth {
        Some(b) => format!("{b}/{}", (rate / 1000.0) as i64),
        None => kilohertz_string(rate),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn classify() {
        let c = |p: &str| AudioFileFormat::classify(&PathBuf::from(p));
        assert_eq!(c("/music/album/01 track.flac"), Some(AudioFileFormat::Flac));
        assert_eq!(c("/music/album/01 track.mp3"), Some(AudioFileFormat::Mp3));
        assert_eq!(c("/music/album/01 track.wav"), Some(AudioFileFormat::Wav));
        assert_eq!(c("/music/album/01 track.aif"), Some(AudioFileFormat::Aiff));
        assert_eq!(c("/music/album/01 track.aiff"), Some(AudioFileFormat::Aiff));
        assert_eq!(c("/music/album/01 track.m4a"), Some(AudioFileFormat::Alac));
        assert_eq!(c("/music/readme.txt"), None);
        assert_eq!(AudioFileFormat::classify_ext("FLAC"), Some(AudioFileFormat::Flac));
        assert_eq!(AudioFileFormat::classify_ext("Mp3"), Some(AudioFileFormat::Mp3));
        assert_eq!(AudioFileFormat::classify_ext("WAV"), Some(AudioFileFormat::Wav));
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration(Some(0.0)), "0:00");
        assert_eq!(format_duration(Some(65.0)), "1:05");
        assert_eq!(format_duration(Some(3661.0)), "1:01:01");
        assert_eq!(format_duration(None), "--:--");
    }

    #[test]
    fn sample_rates() {
        assert_eq!(format_sample_rate(Some(44100.0), Some(16)).as_deref(), Some("16/44"));
        assert_eq!(format_sample_rate(Some(96000.0), Some(24)).as_deref(), Some("24/96"));
        assert_eq!(format_sample_rate(Some(44100.0), None).as_deref(), Some("44.1 kHz"));
        assert_eq!(format_sample_rate(None, None), None);
    }
}
