//! `LrcLibClient`: `GET https://lrclib.net/api/get`.

use std::time::Duration;

use serde::Deserialize;

use crate::{agent, NetError};

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LrcLibResponse {
    pub id: Option<i64>,
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
    pub instrumental: Option<bool>,
}

/// lrclib asks clients to identify themselves.
const USER_AGENT: &str = "FLACtastic (https://github.com/patronsaynt/flactastic)";

/// `None` on 404 (no match). Transport failures are retried twice with a
/// short jittered backoff (250 ms, 500 ms).
pub fn fetch_lyrics(artist: &str, title: &str, album: Option<&str>, duration: Option<f64>) -> Result<Option<LrcLibResponse>, NetError> {
    let (artist, title) = (artist.trim(), title.trim());
    if artist.is_empty() || title.is_empty() {
        return Err(NetError::InvalidQuery);
    }
    let mut attempt = 1;
    loop {
        let mut req = agent()
            .get("https://lrclib.net/api/get")
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .query("artist_name", artist)
            .query("track_name", title);
        if let Some(a) = album.filter(|a| !a.is_empty()) {
            req = req.query("album_name", a);
        }
        if let Some(d) = duration {
            req = req.query("duration", d.round().to_string());
        }
        match req.config().http_status_as_error(false).build().call() {
            Ok(mut resp) => {
                let status = resp.status().as_u16();
                if status == 404 {
                    return Ok(None);
                }
                if !(200..300).contains(&status) {
                    return Err(NetError::BadResponse(format!("http {status}")));
                }
                return Ok(Some(resp.body_mut().read_json()?));
            }
            Err(e) if is_transport(&e) && attempt < 3 => {
                let backoff = 0.25 * f64::from(1u32 << (attempt - 1)) + f64::from(fastrand_u8()) / 2550.0;
                std::thread::sleep(Duration::from_secs_f64(backoff));
                attempt += 1;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Connection-level failures worth another try (never TLS trust failures).
pub fn is_transport(e: &ureq::Error) -> bool {
    matches!(e, ureq::Error::Io(_) | ureq::Error::Timeout(_) | ureq::Error::ConnectionFailed | ureq::Error::HostNotFound)
}

fn fastrand_u8() -> u8 {
    // Tiny jitter source without another dependency.
    (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0) % 256) as u8
}

/// `LyricsFetcher.primaryArtist`: the lead credit for the query.
pub fn primary_artist(raw: Option<&str>) -> Option<String> {
    let t = raw?.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(first) = fl_core::ArtistResolver::explicitly_separated(t).and_then(|p| p.into_iter().next()) {
        return Some(first);
    }
    let mut head = t.to_owned();
    for sep in [", ", " & ", " feat. ", " feat ", " ft. ", " ft ", " x ", " X ", " vs. ", " vs "] {
        if let Some((i, _)) = fl_core::text::find_ascii_ci(&head, sep) {
            head.truncate(i);
        }
    }
    Some(head.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_artist_takes_the_lead_credit() {
        assert_eq!(primary_artist(Some("Future, Metro Boomin")).as_deref(), Some("Future"));
        assert_eq!(primary_artist(Some("Deadmau5 ; Rob Swire")).as_deref(), Some("Deadmau5"));
        assert_eq!(primary_artist(Some("A FEAT. B & C")).as_deref(), Some("A"));
        assert_eq!(primary_artist(Some("  ")), None);
        assert_eq!(primary_artist(None), None);
    }
}
