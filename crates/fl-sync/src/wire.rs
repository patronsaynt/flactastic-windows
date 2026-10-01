//! Control messages (`WireMessage.swift`): `{"t": tag, "d": payload}`,
//! encoded with Foundation's sorted keys, ISO-8601 dates with milliseconds,
//! base64 `Data` and uppercase UUIDs.

use fl_core::apple_json::{self, b64, AppleDate, Uid};
use fl_core::{Playlist, PlaylistEntry};
use serde::{Deserialize, Serialize};

use crate::protocol::{DeviceKind, Version};

/// ISO-8601 with fractional seconds (`[.withInternetDateTime, .withFractionalSeconds]`).
pub mod iso_date {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn format(d: AppleDate) -> String {
        // ISO8601DateFormatter emits UTC with exactly three fraction digits.
        let ms = (d.unix_seconds() * 1000.0).round() as i64;
        let dt = chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default();
        dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    /// A missing fractional part is rejected, as on the Mac.
    pub fn parse(s: &str) -> Option<AppleDate> {
        let t_pos = s.find('T')?;
        let time = &s[t_pos + 1..];
        let frac_ok = time
            .find('.')
            .map(|i| time[i + 1..].chars().next().is_some_and(|c| c.is_ascii_digit()))
            .unwrap_or(false);
        if !frac_ok {
            return None;
        }
        let dt = chrono::DateTime::parse_from_rfc3339(s).ok()?;
        Some(AppleDate::from_chrono(&dt))
    }

    pub fn serialize<S: Serializer>(d: &AppleDate, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format(*d))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<AppleDate, D::Error> {
        let s = String::deserialize(d)?;
        parse(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("Expected ISO-8601 date with fractional seconds, got {s}."))
        })
    }

    pub mod opt {
        use super::*;

        pub fn serialize<S: Serializer>(d: &Option<AppleDate>, s: S) -> Result<S::Ok, S::Error> {
            match d {
                Some(d) => s.serialize_str(&format(*d)),
                None => s.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<AppleDate>, D::Error> {
            match Option::<String>::deserialize(d)? {
                Some(s) => parse(&s).map(Some).ok_or_else(|| serde::de::Error::custom("bad ISO-8601 date")),
                None => Ok(None),
            }
        }
    }
}

// MARK: - Payloads

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub version: Version,
    #[serde(rename = "deviceID")]
    pub device_id: Uid,
    pub display_name: String,
    pub device_kind: DeviceKind,
    pub is_paired: bool,
    /// `ChannelBinding` proof; required when `is_paired`.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "b64::opt")]
    pub proof: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairCommit {
    #[serde(with = "b64")]
    pub commitment: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairGuestKey {
    #[serde(with = "b64")]
    pub public_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairReveal {
    #[serde(with = "b64")]
    pub public_key: Vec<u8>,
    #[serde(with = "b64")]
    pub nonce: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairConfirm {
    #[serde(with = "b64")]
    pub mac: Vec<u8>,
    #[serde(rename = "deviceID")]
    pub device_id: Uid,
    pub display_name: String,
    pub device_kind: DeviceKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairResult {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncRequest {
    /// As the *initiating* device means it; the responder inverts it.
    pub direction: crate::protocol::Direction,
    pub filter: crate::manifest::SyncFilter,
    pub manifest: crate::manifest::LibraryManifest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanProposal {
    pub plan: crate::manifest::SyncPlan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_free_bytes: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanDecision {
    /// The full plan's hash; the receiver refuses if its own differs.
    pub plan_hash: String,
    pub approved: bool,
    /// What the user ticked; `None` means all of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<crate::manifest::SyncSelection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStart {
    #[serde(rename = "trackID")]
    pub track_id: Uid,
    /// Relative to the sender's root; untrusted.
    pub relative_path: String,
    pub file_size: i64,
    pub content_hash: String,
    pub tag_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAccept {
    #[serde(rename = "trackID")]
    pub track_id: Uid,
    pub resume_offset: i64,
    pub skip: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileEnd {
    #[serde(rename = "trackID")]
    pub track_id: Uid,
}

/// `Playlist` as it crosses the wire (ISO-8601 `dateCreated`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WirePlaylist {
    pub id: Uid,
    pub name: String,
    #[serde(default)]
    pub entries: Option<Vec<PlaylistEntry>>,
    #[serde(default, skip_serializing)]
    pub track_paths: Option<Vec<String>>,
    #[serde(with = "iso_date")]
    pub date_created: AppleDate,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "b64::opt")]
    pub custom_artwork: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl From<&Playlist> for WirePlaylist {
    fn from(p: &Playlist) -> Self {
        WirePlaylist {
            id: p.id,
            name: p.name.clone(),
            entries: Some(p.entries.clone()),
            track_paths: None,
            date_created: p.date_created,
            custom_artwork: p.custom_artwork.clone(),
            description: p.description.clone(),
        }
    }
}

impl From<WirePlaylist> for Playlist {
    fn from(w: WirePlaylist) -> Self {
        let entries = match (w.entries, w.track_paths) {
            (Some(e), _) => e,
            (None, Some(paths)) => paths.into_iter().map(|p| PlaylistEntry::new(None, p)).collect(),
            _ => Vec::new(),
        };
        Playlist {
            id: w.id,
            name: w.name,
            entries,
            date_created: w.date_created,
            custom_artwork: w.custom_artwork,
            description: w.description,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaylistPayload {
    pub playlists: Vec<WirePlaylist>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    pub tracks_transferred: i64,
    pub playlists_transferred: i64,
    pub bytes_transferred: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cancellation {
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureCode {
    IncompatibleVersion,
    NotPaired,
    PairingClosed,
    PairingFailed,
    RateLimited,
    UnsupportedMessage,
    UnexpectedMessage,
    InvalidPath,
    HashMismatch,
    SizeExceeded,
    InsufficientStorage,
    PlanStale,
    InternalFailure,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtocolFailure {
    pub code: FailureCode,
    pub message: String,
}

impl ProtocolFailure {
    pub fn new(code: FailureCode, message: impl Into<String>) -> Self {
        ProtocolFailure { code, message: message.into() }
    }
}

// MARK: - WireMessage

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "d", rename_all = "camelCase")]
pub enum WireMessage {
    Hello(Hello),
    HelloAck(Hello),
    PairCommit(PairCommit),
    PairGuestKey(PairGuestKey),
    PairReveal(PairReveal),
    PairConfirm(PairConfirm),
    PairResult(PairResult),
    SyncRequest(SyncRequest),
    PlanProposal(PlanProposal),
    PlanDecision(PlanDecision),
    FileStart(FileStart),
    FileAccept(FileAccept),
    FileEnd(FileEnd),
    Playlists(PlaylistPayload),
    SyncComplete(Completion),
    Cancel(Cancellation),
    ProtocolError(ProtocolFailure),
}

impl WireMessage {
    /// Deterministic bytes, matching the Mac's `WireMessage.encoded()`.
    pub fn encoded(&self) -> Vec<u8> {
        apple_json::to_vec_sorted(self).expect("wire messages always serialize")
    }

    pub fn decoded(bytes: &[u8]) -> Result<WireMessage, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    pub fn tag(&self) -> &'static str {
        match self {
            Self::Hello(_) => "hello",
            Self::HelloAck(_) => "helloAck",
            Self::PairCommit(_) => "pairCommit",
            Self::PairGuestKey(_) => "pairGuestKey",
            Self::PairReveal(_) => "pairReveal",
            Self::PairConfirm(_) => "pairConfirm",
            Self::PairResult(_) => "pairResult",
            Self::SyncRequest(_) => "syncRequest",
            Self::PlanProposal(_) => "planProposal",
            Self::PlanDecision(_) => "planDecision",
            Self::FileStart(_) => "fileStart",
            Self::FileAccept(_) => "fileAccept",
            Self::FileEnd(_) => "fileEnd",
            Self::Playlists(_) => "playlists",
            Self::SyncComplete(_) => "syncComplete",
            Self::Cancel(_) => "cancel",
            Self::ProtocolError(_) => "protocolError",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_bytes_match_foundation() {
        let id = Uid::parse("E621E1F8-C36C-495A-93FC-0C247A3E6E5F").unwrap();
        let m = WireMessage::Hello(Hello {
            version: crate::protocol::VERSION,
            device_id: id,
            display_name: "Falco's PC".into(),
            device_kind: DeviceKind::Other,
            is_paired: true,
            proof: Some(vec![0xFB, 0xFF, 0x01]),
        });
        let s = String::from_utf8(m.encoded()).unwrap();
        assert_eq!(
            s,
            r#"{"d":{"deviceID":"E621E1F8-C36C-495A-93FC-0C247A3E6E5F","deviceKind":"other","displayName":"Falco's PC","isPaired":true,"proof":"+\/8B","version":{"major":1,"minor":0}},"t":"hello"}"#
        );
        assert_eq!(WireMessage::decoded(s.as_bytes()).unwrap(), m);
    }

    #[test]
    fn nil_optionals_are_omitted() {
        let m = WireMessage::PairResult(PairResult { success: true, failure_reason: None });
        assert_eq!(String::from_utf8(m.encoded()).unwrap(), r#"{"d":{"success":true},"t":"pairResult"}"#);
    }

    #[test]
    fn dates_need_fractional_seconds() {
        assert!(iso_date::parse("2026-09-29T12:00:00Z").is_none());
        let d = iso_date::parse("2026-09-29T12:00:00.250Z").unwrap();
        assert_eq!(iso_date::format(d), "2026-09-29T12:00:00.250Z");
        assert!(iso_date::parse("2026-09-29T14:00:00.250+02:00").is_some());
    }

    #[test]
    fn playlists_round_trip() {
        let mut p = Playlist::new("Road".into());
        p.date_created = AppleDate::from_unix(1_790_000_000.123);
        p.entries.push(PlaylistEntry::new(None, "A/B.flac".into()));
        let m = WireMessage::Playlists(PlaylistPayload { playlists: vec![(&p).into()] });
        let bytes = m.encoded();
        let s = String::from_utf8(bytes.clone()).unwrap();
        assert!(s.contains(r#""dateCreated":"2026-09-21T"#), "{s}");
        assert!(s.contains(r#""relativePath":"A\/B.flac""#));
        let WireMessage::Playlists(back) = WireMessage::decoded(&bytes).unwrap() else { panic!() };
        let back: Playlist = back.playlists[0].clone().into();
        assert_eq!(back.entries, p.entries);
        assert!((back.date_created.0 - p.date_created.0).abs() < 0.001);
    }

    #[test]
    fn unknown_tag_is_an_error() {
        assert!(WireMessage::decoded(br#"{"t":"teleport","d":{}}"#).is_err());
    }
}
