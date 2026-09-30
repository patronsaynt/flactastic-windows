//! Wire-contract constants (`Sync/Protocol/SyncProtocol.swift`).
//! Changing any value here is a protocol change.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Version {
    pub major: i64,
    pub minor: i64,
}

impl Version {
    /// Majors must match exactly; minors are additive.
    pub fn is_compatible(&self, other: &Version) -> bool {
        self.major == other.major && self.major >= MINIMUM_COMPATIBLE_MAJOR
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

pub const VERSION: Version = Version { major: 1, minor: 0 };
pub const MINIMUM_COMPATIBLE_MAJOR: i64 = 1;

pub const BONJOUR_SERVICE_TYPE: &str = "_flactastic._tcp";
pub const BONJOUR_DOMAIN: &str = "local.";

pub mod txt_key {
    pub const PROTOCOL_VERSION: &str = "v";
    pub const DEVICE_ID: &str = "id";
    pub const DISPLAY_NAME: &str = "n";
    pub const DEVICE_KIND: &str = "k";
    pub const PAIRING_OPEN: &str = "p";
}

pub const MAX_CONTROL_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const FILE_CHUNK_BYTES: usize = 1024 * 1024;
pub const MAX_FILE_BYTES: i64 = 2 * 1024 * 1024 * 1024;

pub const PAIRING_CODE_DIGITS: usize = 8;
pub const PAIRING_KDF_INFO: &str = "flactastic-pair-v1";
pub const SESSION_KDF_INFO: &str = "flactastic-session-v1";
pub const PAIRING_CODE_LIFETIME: Duration = Duration::from_secs(120);
pub const PAIRING_FAILURE_LIMIT: u32 = 3;
pub const PAIRING_LOCKOUT_DURATION: Duration = Duration::from_secs(60);

pub const MANIFEST_WAIT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub const PLAN_REVIEW_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Advertised device kind. The Mac and iOS apps decode `Hello.deviceKind` as
/// a closed enum, so Windows/Linux builds send `Other` (see SYNC-INTEROP.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceKind {
    #[serde(rename = "mac")]
    Mac,
    #[serde(rename = "iPhone")]
    IPhone,
    #[serde(rename = "iPad")]
    IPad,
    #[serde(rename = "other")]
    Other,
}

impl DeviceKind {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Mac => "mac",
            Self::IPhone => "iPhone",
            Self::IPad => "iPad",
            Self::Other => "other",
        }
    }

    pub fn from_raw(s: &str) -> Option<Self> {
        Some(match s {
            "mac" => Self::Mac,
            "iPhone" => Self::IPhone,
            "iPad" => Self::IPad,
            "other" => Self::Other,
            _ => return None,
        })
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Mac => "Mac",
            Self::IPhone => "iPhone",
            Self::IPad => "iPad",
            Self::Other => "Device",
        }
    }

    /// What this build advertises.
    pub const LOCAL: DeviceKind = DeviceKind::Other;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Local library is the source of truth.
    Push,
    /// Remote library is the source of truth.
    Pull,
}

impl Direction {
    pub fn inverted(self) -> Direction {
        match self {
            Self::Push => Self::Pull,
            Self::Pull => Self::Push,
        }
    }
}
