//! `AmazonMatchService`: maps a track URL (typically Spotify) to its Amazon
//! Music equivalent via the public Odesli / song.link API. Best-effort: any
//! failure returns `None` so the caller falls back to the original URL.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Deserialize;

/// Odesli's unauthenticated quota is ~10 requests a minute.
const MIN_INTERVAL: Duration = Duration::from_millis(6500);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    links_by_platform: Option<Links>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Links {
    amazon_music: Option<Link>,
}

#[derive(Deserialize)]
struct Link {
    url: Option<String>,
}

fn decode(body: &str) -> Option<String> {
    serde_json::from_str::<Response>(body).ok()?.links_by_platform?.amazon_music?.url
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build()
            .into()
    })
}

#[derive(Default)]
pub struct AmazonMatcher {
    /// Serializes callers and spaces requests `MIN_INTERVAL` apart.
    last: Mutex<Option<Instant>>,
}

impl AmazonMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    fn throttle(&self) {
        let mut last = self.last.lock();
        if let Some(t) = *last {
            let elapsed = t.elapsed();
            if elapsed < MIN_INTERVAL {
                std::thread::sleep(MIN_INTERVAL - elapsed);
            }
        }
        *last = Some(Instant::now());
    }

    /// The Amazon Music URL for `track_url`, or `None`. One retry after a 429
    /// (honouring `Retry-After`, default 10 s).
    pub fn amazon_url(&self, track_url: &str) -> Option<String> {
        self.throttle();
        let (status, retry_after, body) = once(track_url)?;
        if status == 429 {
            std::thread::sleep(Duration::from_secs_f64(retry_after.unwrap_or(10.0)));
            let (status, _, body) = once(track_url)?;
            return (200..300).contains(&status).then(|| decode(&body)).flatten();
        }
        (200..300).contains(&status).then(|| decode(&body)).flatten()
    }
}

fn once(track_url: &str) -> Option<(u16, Option<f64>, String)> {
    let mut resp = agent()
        .get("https://api.song.link/v1-alpha.1/links")
        .query("url", track_url)
        .query("songIfSingle", "true")
        .header("User-Agent", "FLACtastic")
        .header("Accept", "application/json")
        .call()
        .ok()?;
    let status = resp.status().as_u16();
    let retry = resp.headers().get("Retry-After").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse().ok());
    let body = resp.body_mut().read_to_string().ok()?;
    Some((status, retry, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_amazon_link() {
        let body = r#"{"linksByPlatform":{"amazonMusic":{"url":"https://music.amazon.com/albums/B1?trackAsin=B2"},"spotify":{"url":"x"}}}"#;
        assert_eq!(decode(body).as_deref(), Some("https://music.amazon.com/albums/B1?trackAsin=B2"));
        assert_eq!(decode(r#"{"linksByPlatform":{}}"#), None);
        assert_eq!(decode("nope"), None);
    }
}
