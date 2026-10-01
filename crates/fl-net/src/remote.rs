//! `StreamerModels`: remote items returned by streaming services before they
//! become library tracks. Serialized camelCase for the UI, which hands tracks
//! back unchanged when enqueueing downloads.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteArtist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub picture_url: Option<String>,
}

impl RemoteArtist {
    pub fn named(name: &str) -> Self {
        RemoteArtist { id: name.to_owned(), name: name.to_owned(), url: None, picture_url: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCoverArt {
    pub url: String,
    #[serde(default)]
    pub width: Option<i64>,
    #[serde(default)]
    pub height: Option<i64>,
}

impl RemoteCoverArt {
    /// The largest cover by area (`RemoteCoverArt.best`), the first of equals.
    pub fn best(arts: &[RemoteCoverArt]) -> Option<&RemoteCoverArt> {
        let area = |a: &RemoteCoverArt| a.width.unwrap_or(0) * a.height.unwrap_or(0);
        let mut best: Option<&RemoteCoverArt> = None;
        for a in arts {
            if best.map_or(true, |b| area(a) > area(b)) {
                best = Some(a);
            }
        }
        best
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAlbumRef {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub cover_art: Vec<RemoteCoverArt>,
    #[serde(default)]
    pub release_year: Option<i64>,
    #[serde(default)]
    pub track_count: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTrack {
    pub id: String,
    pub title: String,
    pub artists: Vec<RemoteArtist>,
    #[serde(default)]
    pub album: Option<RemoteAlbumRef>,
    #[serde(default)]
    pub track_number: Option<i64>,
    #[serde(default)]
    pub disc_number: Option<i64>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub cover_art: Vec<RemoteCoverArt>,
    /// Service-native URL the user could open in a browser.
    #[serde(default)]
    pub url: Option<String>,
    pub service_id: String,
    pub is_lossless: bool,
}

impl RemoteTrack {
    /// A copy that downloads from `url` instead (`withSource(url:)`). A
    /// distinct id keeps per-track options from colliding when the same track
    /// is tried against two sources.
    pub fn with_source(&self, url: &str) -> RemoteTrack {
        RemoteTrack { id: format!("{}|src:{url}", self.id), url: Some(url.to_owned()), ..self.clone() }
    }

    pub fn artist_label(&self) -> String {
        self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAlbum {
    pub id: String,
    pub title: String,
    pub artists: Vec<RemoteArtist>,
    pub release_year: Option<i64>,
    pub cover_art: Vec<RemoteCoverArt>,
    pub url: Option<String>,
    pub track_count: Option<i64>,
    pub tracks: Vec<RemoteTrack>,
    pub service_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePlaylist {
    pub id: String,
    pub title: String,
    pub creator: Option<String>,
    pub cover_art: Vec<RemoteCoverArt>,
    pub url: Option<String>,
    pub tracks: Vec<RemoteTrack>,
    pub service_id: String,
}

/// `RemoteResolveResponse`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RemoteResolve {
    Track { track: RemoteTrack },
    Album { album: RemoteAlbum },
    Playlist { playlist: RemotePlaylist },
    #[serde(rename_all = "camelCase")]
    Artist { artist: RemoteArtist, top_tracks: Vec<RemoteTrack>, albums: Vec<RemoteAlbum> },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn art(w: Option<i64>, u: &str) -> RemoteCoverArt {
        RemoteCoverArt { url: u.into(), width: w, height: w }
    }

    #[test]
    fn best_cover_is_largest() {
        assert!(RemoteCoverArt::best(&[]).is_none());
        let arts = [art(Some(64), "s"), art(Some(640), "l"), art(Some(300), "m")];
        assert_eq!(RemoteCoverArt::best(&arts).unwrap().url, "l");
        let bare = [art(None, "a"), art(None, "b")];
        assert_eq!(RemoteCoverArt::best(&bare).unwrap().url, "a");
    }
}
