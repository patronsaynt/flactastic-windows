//! FLACtastic sync protocol v1 — interoperable with the macOS and iOS apps.
//!
//! Where the Mac code and `docs/sync-protocol-v1.md` disagree, the code wins
//! for anything on the wire; see `docs/SYNC-INTEROP.md`.

pub mod builder;
pub mod connection;
pub mod crypto;
pub mod discovery;
pub mod exchange;
pub mod frame;
pub mod fs_space;
pub mod gatekeeper;
pub mod manifest;
pub mod pairing;
pub mod picklist;
pub mod path_sanitizer;
pub mod protocol;
pub mod session;
pub mod tls;
pub mod transfer;
pub mod txt;
pub mod wire;

pub use connection::{ConnectionError, SyncConnection};
pub use crypto::Key;
pub use protocol::{DeviceKind, Direction, Version, VERSION};
pub use wire::WireMessage;
