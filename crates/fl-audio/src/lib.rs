//! FLACtastic audio: decode (symphonia), resample (libsoxr VHQ), the gapless
//! engine, output backends, and spectrum analysis.

pub mod decode;
pub mod engine;
pub mod output;
pub mod output_manager;
pub mod player;
pub mod soxr;
pub mod spectrum;

pub use engine::{Engine, Snapshot};
pub use output::{Backend, DeviceInfo, StreamSpec};
