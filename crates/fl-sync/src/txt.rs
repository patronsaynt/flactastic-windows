//! Bonjour TXT record codec (`TXTRecordCodec.swift`).

use std::collections::HashMap;

use fl_core::Uid;
use unicode_normalization::UnicodeNormalization;

use crate::protocol::{txt_key, DeviceKind, Version, VERSION};

pub const MAX_VALUE_BYTES: usize = 63;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub device_id: Uid,
    pub display_name: String,
    pub kind: DeviceKind,
    pub protocol_version: Version,
    pub is_pairing_open: bool,
}

impl DiscoveredPeer {
    pub fn is_compatible(&self) -> bool {
        VERSION.is_compatible(&self.protocol_version)
    }
}

pub fn encode(device_id: Uid, display_name: &str, kind: DeviceKind, is_pairing_open: bool) -> Vec<(String, String)> {
    vec![
        (txt_key::PROTOCOL_VERSION.into(), format!("{}.{}", VERSION.major, VERSION.minor)),
        (txt_key::DEVICE_ID.into(), device_id.uuid_string()),
        (txt_key::DISPLAY_NAME.into(), clamp(display_name)),
        (txt_key::DEVICE_KIND.into(), kind.raw_value().into()),
        (txt_key::PAIRING_OPEN.into(), if is_pairing_open { "1" } else { "0" }.into()),
    ]
}

/// `None` for unusable records (ordinary network noise).
pub fn decode(entries: &HashMap<String, String>) -> Option<DiscoveredPeer> {
    let device_id = Uid::parse(entries.get(txt_key::DEVICE_ID)?)?;
    let protocol_version = parse_version(entries.get(txt_key::PROTOCOL_VERSION).map(String::as_str))?;
    let kind = entries.get(txt_key::DEVICE_KIND).and_then(|k| DeviceKind::from_raw(k)).unwrap_or(DeviceKind::Other);
    Some(DiscoveredPeer {
        device_id,
        display_name: sanitize_display_name(entries.get(txt_key::DISPLAY_NAME).map(String::as_str), kind.display_name()),
        kind,
        protocol_version,
        is_pairing_open: entries.get(txt_key::PAIRING_OPEN).map(String::as_str) == Some("1"),
    })
}

pub fn parse_version(raw: Option<&str>) -> Option<Version> {
    let parts: Vec<&str> = raw?.split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() != 2 {
        return None;
    }
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit()) && !s.is_empty();
    if !digits(parts[0]) || !digits(parts[1]) {
        return None;
    }
    Some(Version { major: parts[0].parse().ok()?, minor: parts[1].parse().ok()? })
}

/// Unicode general category Cf (format characters) — part of Foundation's
/// `CharacterSet.controlCharacters` alongside Cc.
fn is_format_char(c: char) -> bool {
    matches!(c as u32,
        0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x0890..=0x0891 | 0x08E2 | 0x180E
        | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF
        | 0xFFF9..=0xFFFB | 0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3
        | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F)
}

/// Strips control/format characters (incl. bidi overrides), NFC-normalises,
/// trims, and clamps to 63 characters.
pub fn sanitize_display_name(raw: Option<&str>, fallback: &str) -> String {
    let Some(raw) = raw else { return fallback.to_owned() };
    let cleaned: String = raw.nfc().filter(|c| !c.is_control() && !is_format_char(*c)).collect();
    let t = cleaned.trim();
    if t.is_empty() {
        fallback.to_owned()
    } else {
        t.chars().take(63).collect()
    }
}

fn clamp(v: &str) -> String {
    let mut s = v.to_owned();
    while s.len() > MAX_VALUE_BYTES {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_unknown_kind() {
        let id = Uid::new_v4();
        let mut m: HashMap<String, String> = encode(id, "Studio PC", DeviceKind::Other, true).into_iter().collect();
        let p = decode(&m).unwrap();
        assert_eq!(p.device_id, id);
        assert!(p.is_pairing_open && p.is_compatible());
        m.insert("k".into(), "windows".into());
        assert_eq!(decode(&m).unwrap().kind, DeviceKind::Other);
        m.insert("v".into(), "x.1".into());
        assert!(decode(&m).is_none());
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_display_name(Some("\u{202E}evil\u{0007} "), "Mac"), "evil");
        assert_eq!(sanitize_display_name(Some("  "), "Mac"), "Mac");
        assert_eq!(sanitize_display_name(Some(&"x".repeat(80)), "Mac").len(), 63);
        assert_eq!(clamp(&"é".repeat(40)).len(), 62);
    }
}
