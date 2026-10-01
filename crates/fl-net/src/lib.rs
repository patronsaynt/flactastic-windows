//! Web clients (`Networking/`). Blocking calls on a shared agent; callers run
//! them on worker threads.

pub mod deezer;
pub mod lrclib;
pub mod lucida;
pub mod odesli;
pub mod remote;
pub mod spotify;

use std::sync::OnceLock;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("invalid query")]
    InvalidQuery,
    #[error("bad response: {0}")]
    BadResponse(String),
    #[error(transparent)]
    Http(#[from] ureq::Error),
}

/// `URLSessionConfiguration` with a 10 s request / 20 s resource timeout.
pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent(concat!("FLACtastic/", env!("CARGO_PKG_VERSION")))
            .build()
            .into()
    })
}
