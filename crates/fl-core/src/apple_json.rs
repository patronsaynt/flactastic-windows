//! JSON conventions matching Foundation's `JSONEncoder`/`JSONDecoder` defaults,
//! so sidecar files under `<root>/.flactastic/` stay readable by the macOS app.
//!
//! - `Date` → Double seconds since 2001-01-01T00:00:00Z (`.deferredToDate`).
//! - `UUID` → uppercase hyphenated string; decoding accepts any case.
//! - `Data` → standard base64 with padding.
//! - `nil` optionals are omitted (`encodeIfPresent`).
//! - `/` is written as `\/`, and integral doubles print without a fraction,
//!   both as Foundation does.

use std::fmt;
use std::io::{self, Write};
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// Seconds between 1970-01-01 and 2001-01-01 (`NSTimeIntervalSince1970`).
pub const APPLE_EPOCH_OFFSET: f64 = 978_307_200.0;

// MARK: - Uid

/// A UUID that serializes the way Foundation does: uppercase, hyphenated.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Uid(pub uuid::Uuid);

impl Uid {
    pub fn new_v4() -> Self {
        Uid(uuid::Uuid::new_v4())
    }
    pub const fn nil() -> Self {
        Uid(uuid::Uuid::nil())
    }
    /// The 16 raw RFC-4122 bytes (never mixed-endian).
    pub fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }
    pub fn from_bytes(b: [u8; 16]) -> Self {
        Uid(uuid::Uuid::from_bytes(b))
    }
    /// `uuidString` — 36 uppercase ASCII characters.
    pub fn uuid_string(&self) -> String {
        let mut buf = [0u8; uuid::fmt::Hyphenated::LENGTH];
        self.0.hyphenated().encode_upper(&mut buf).to_owned()
    }
    /// `UUID(uuidString:)` — accepts upper or lower case, hyphenated form only.
    pub fn parse(s: &str) -> Option<Self> {
        if s.len() != 36 {
            return None;
        }
        uuid::Uuid::try_parse(s).ok().map(Uid)
    }
}

impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.uuid_string())
    }
}

impl fmt::Debug for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uid({})", self.uuid_string())
    }
}

impl FromStr for Uid {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uid::parse(s).ok_or_else(|| format!("invalid UUID string: {s}"))
    }
}

impl Serialize for Uid {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.uuid_string())
    }
}

impl<'de> Deserialize<'de> for Uid {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Uid;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a UUID string")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Uid, E> {
                Uid::parse(v).ok_or_else(|| E::custom(format!("Attempted to decode UUID from invalid UUID string: {v}")))
            }
        }
        d.deserialize_str(V)
    }
}

// MARK: - AppleDate

/// `Foundation.Date` — a Double of seconds since the 2001 reference date.
#[derive(Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AppleDate(pub f64);

impl AppleDate {
    pub fn now() -> Self {
        Self::from_system_time(SystemTime::now())
    }
    pub fn from_system_time(t: SystemTime) -> Self {
        let unix = match t.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs_f64(),
            Err(e) => -e.duration().as_secs_f64(),
        };
        AppleDate(unix - APPLE_EPOCH_OFFSET)
    }
    pub fn from_unix(secs: f64) -> Self {
        AppleDate(secs - APPLE_EPOCH_OFFSET)
    }
    pub fn unix_seconds(&self) -> f64 {
        self.0 + APPLE_EPOCH_OFFSET
    }
    pub fn to_system_time(&self) -> SystemTime {
        let u = self.unix_seconds();
        if u >= 0.0 {
            UNIX_EPOCH + Duration::from_secs_f64(u)
        } else {
            UNIX_EPOCH - Duration::from_secs_f64(-u)
        }
    }
    /// `timeIntervalSince`.
    pub fn since(&self, other: AppleDate) -> f64 {
        self.0 - other.0
    }
    pub fn adding(&self, secs: f64) -> Self {
        AppleDate(self.0 + secs)
    }
    pub fn to_chrono_utc(&self) -> chrono::DateTime<chrono::Utc> {
        let u = self.unix_seconds();
        let secs = u.floor();
        let nanos = ((u - secs) * 1e9).round().clamp(0.0, 999_999_999.0) as u32;
        chrono::DateTime::from_timestamp(secs as i64, nanos).unwrap_or_default()
    }
    pub fn from_chrono<Tz: chrono::TimeZone>(dt: &chrono::DateTime<Tz>) -> Self {
        let unix = dt.timestamp() as f64 + f64::from(dt.timestamp_subsec_nanos()) / 1e9;
        AppleDate::from_unix(unix)
    }
}

impl fmt::Debug for AppleDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AppleDate({} = {})", self.0, self.to_chrono_utc().to_rfc3339())
    }
}

// MARK: - Data (base64)

/// serde helpers for `Data` fields: standard base64 with padding.
pub mod b64 {
    use super::*;

    pub fn encode(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    pub fn decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(s)
    }

    pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&encode(v.as_ref()))
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: From<Vec<u8>>>(d: D) -> Result<T, D::Error> {
        let s = String::deserialize(d)?;
        decode(&s).map(T::from).map_err(de::Error::custom)
    }

    /// For `Option<Data>` fields; pair with `skip_serializing_if = "Option::is_none"`
    /// and `default`.
    pub mod opt {
        use super::*;

        pub fn serialize<S: Serializer, T: AsRef<[u8]>>(v: &Option<T>, s: S) -> Result<S::Ok, S::Error> {
            match v {
                Some(v) => s.serialize_str(&encode(v.as_ref())),
                None => s.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>, T: From<Vec<u8>>>(d: D) -> Result<Option<T>, D::Error> {
            let s: Option<String> = Option::deserialize(d)?;
            match s {
                Some(s) => decode(&s).map(|v| Some(T::from(v))).map_err(de::Error::custom),
                None => Ok(None),
            }
        }
    }
}

// MARK: - Encoder output

/// serde_json formatter reproducing Foundation's output details:
/// `/` escaped as `\/` and integral doubles printed without `.0`.
#[derive(Default)]
pub struct AppleFormatter;

impl serde_json::ser::Formatter for AppleFormatter {
    fn write_f64<W: ?Sized + Write>(&mut self, w: &mut W, value: f64) -> io::Result<()> {
        write_apple_f64(w, value)
    }

    fn write_f32<W: ?Sized + Write>(&mut self, w: &mut W, value: f32) -> io::Result<()> {
        write_apple_f64(w, f64::from(value))
    }

    fn write_string_fragment<W: ?Sized + Write>(&mut self, w: &mut W, fragment: &str) -> io::Result<()> {
        let mut start = 0;
        for (i, b) in fragment.bytes().enumerate() {
            if b == b'/' {
                w.write_all(&fragment.as_bytes()[start..i])?;
                w.write_all(b"\\/")?;
                start = i + 1;
            }
        }
        w.write_all(&fragment.as_bytes()[start..])
    }
}

fn write_apple_f64<W: ?Sized + Write>(w: &mut W, value: f64) -> io::Result<()> {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        write!(w, "{}", value as i64)
    } else {
        let mut buf = ryu_like(value);
        // serde_json/ryu prints e.g. "1e16"; Foundation prints "1e+16".
        if let Some(pos) = buf.find('e') {
            if !buf[pos + 1..].starts_with(['-', '+']) {
                buf.insert(pos + 1, '+');
            }
        }
        w.write_all(buf.as_bytes())
    }
}

fn ryu_like(v: f64) -> String {
    // serde_json's own f64 output (shortest round-trip representation).
    serde_json::to_string(&v).unwrap_or_else(|_| "0".into())
}

/// Encode with Foundation's conventions (compact, unsorted keys).
pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(256);
    let mut ser = serde_json::Serializer::with_formatter(&mut out, AppleFormatter);
    value.serialize(&mut ser)?;
    Ok(out)
}

/// `data.write(to:options:.atomic)` — write a temp file beside the target, then rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    replace_file(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Rename that replaces an existing destination on every platform.
pub fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    // std::fs::rename uses MoveFileExW(MOVEFILE_REPLACE_EXISTING) on Windows
    // and rename(2) elsewhere; both replace atomically on the same volume.
    std::fs::rename(from, to)
}

/// Encode `value` with Foundation conventions and write it atomically.
pub fn save<T: Serialize + ?Sized>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = to_vec(value).map_err(io::Error::other)?;
    write_atomic(path, &bytes)
}

/// Read and decode a JSON file. `Ok(None)` when the file does not exist.
pub fn load<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_round_trips_uppercase() {
        let u = Uid::parse("e621e1f8-c36c-495a-93fc-0c247a3e6e5f").unwrap();
        assert_eq!(u.uuid_string(), "E621E1F8-C36C-495A-93FC-0C247A3E6E5F");
        let json = serde_json::to_string(&u).unwrap();
        assert_eq!(json, "\"E621E1F8-C36C-495A-93FC-0C247A3E6E5F\"");
        let back: Uid = serde_json::from_str(&json).unwrap();
        assert_eq!(back, u);
        assert!(Uid::parse("E621E1F8C36C495A93FC0C247A3E6E5F").is_none());
    }

    #[test]
    fn formatter_matches_foundation() {
        #[derive(Serialize)]
        struct S {
            p: &'static str,
            d: f64,
            i: f64,
            big: f64,
        }
        let out = to_vec(&S { p: "a/b", d: 0.75, i: 44100.0, big: 1e20 }).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), r#"{"p":"a\/b","d":0.75,"i":44100,"big":1e+20}"#);
    }

    #[test]
    fn apple_date_epoch() {
        let d = AppleDate(0.0);
        assert_eq!(d.to_chrono_utc().to_rfc3339(), "2001-01-01T00:00:00+00:00");
        let now = AppleDate::now();
        let back = AppleDate::from_system_time(now.to_system_time());
        assert!((back.0 - now.0).abs() < 1e-6);
    }
}
