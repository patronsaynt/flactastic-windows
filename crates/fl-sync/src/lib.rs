//! FLACtastic sync protocol v1 — interoperable with the macOS and iOS apps.
//!
//! Where the Mac code and `docs/sync-protocol-v1.md` disagree, the code wins
//! for anything on the wire; see `docs/SYNC-INTEROP.md`.

pub mod connection;
pub mod crypto;
pub mod exchange;
pub mod frame;
pub mod gatekeeper;
pub mod pairing;
pub mod protocol;
pub mod tls;
pub mod txt;
pub mod wire;

pub use connection::{ConnectionError, SyncConnection};
pub use crypto::Key;
pub use protocol::{DeviceKind, Direction, Version, VERSION};
pub use wire::WireMessage;
