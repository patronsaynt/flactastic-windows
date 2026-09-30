//! What the UI receives. Tracks travel without artwork bytes; artwork is
//! fetched by content key through the `flart://` protocol.

use std::path::Path;

use fl_core::model::{artwork_content_id, relative_path};
use fl_core::{Album, ArtistResolver, AudioQuality, Track};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDto {
    pub id: String,
    pub path: String,
    pub rel_path: Option<String>,
    pub title: String,
    pub artist: Option<String>,
    /// `ArtistResolver.displayString(artist)`: explicit lists as "A, B".
    pub artist_display: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<i64>,
    pub duration: Option<f64>,
    pub file_format: fl_core::AudioFileFormat,
    pub sample_rate: Option<f64>,
    pub bit_depth: Option<i64>,
    pub quality: AudioQuality,
    pub genre: Option<String>,
    pub secondary_genres: Vec<String>,
    pub year: Option<i64>,
    pub is_compilation: bool,
    pub is_mix_compilation: bool,
    /// Unix seconds.
    pub date_added: Option<f64>,
    /// `artwork_content_id` of the embedded picture.
    pub artwork: Option<String>,
    /// `artist ?? albumArtist` resolved into linkable artists (library
    /// snapshots only; the queue looks tracks up by id).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub artist_links: Vec<ArtistLink>,
}

/// One linkable piece of a credit (`ArtistLink` / `artistContextMenuItems`).
#[derive(Debug, Clone, Serialize)]
pub struct ArtistLink {
    pub name: String,
    pub key: String,
}

pub fn artist_links(resolver: &ArtistResolver, credit: Option<&str>) -> Vec<ArtistLink> {
    resolver
        .split(credit)
        .into_iter()
        .map(|name| ArtistLink { key: ArtistResolver::key(&name), name })
        .collect()
}

impl TrackDto {
    pub fn from_track(t: &Track, root: Option<&Path>) -> TrackDto {
        TrackDto {
            id: t.id.to_string(),
            path: t.path.to_string_lossy().into_owned(),
            rel_path: root.and_then(|r| relative_path(&t.path, r)),
            title: t.title.clone(),
            artist: t.artist.clone(),
            artist_display: ArtistResolver::display_string(t.artist.as_deref()),
            album_artist: t.album_artist.clone(),
            album: t.album.clone(),
            track_number: t.track_number,
            duration: t.duration,
            file_format: t.file_format,
            sample_rate: t.sample_rate,
            bit_depth: t.bit_depth,
            quality: AudioQuality::classify(t.sample_rate, t.bit_depth, t.file_format),
            genre: t.genre.clone(),
            secondary_genres: t.secondary_genres.clone(),
            year: t.year,
            is_compilation: t.is_compilation,
            is_mix_compilation: t.is_mix_compilation,
            date_added: t.date_added.map(|d| d.unix_seconds()),
            artwork: t.artwork.as_ref().map(|a| artwork_content_id(a)),
            artist_links: Vec::new(),
        }
    }

    pub fn with_links(mut self, t: &Track, resolver: &ArtistResolver) -> TrackDto {
        self.artist_links = artist_links(resolver, t.artist.as_deref().or(t.album_artist.as_deref()));
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDto {
    pub id: String,
    pub name: String,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub secondary_genres: Vec<String>,
    pub artwork: Option<String>,
    pub track_ids: Vec<String>,
    pub total_duration: f64,
    pub is_compilation: bool,
    pub is_mix_compilation: bool,
    /// `album.artist` as links (the detail header's `ArtistLink`).
    pub artist_links: Vec<ArtistLink>,
    /// `albumArtist ?? artist` as links (the context menu).
    pub album_artist_links: Vec<ArtistLink>,
}

impl AlbumDto {
    pub fn from_album(a: &Album, resolver: &ArtistResolver) -> AlbumDto {
        AlbumDto {
            id: a.id.clone(),
            name: a.name.clone(),
            artist: a.artist.clone(),
            album_artist: a.album_artist.clone(),
            year: a.year,
            genre: a.genre.clone(),
            secondary_genres: a.secondary_genres.clone(),
            artwork: a.artwork.as_ref().map(|x| artwork_content_id(x)),
            track_ids: a.tracks.iter().map(|t| t.id.to_string()).collect(),
            total_duration: a.total_duration(),
            is_compilation: a.is_compilation(),
            is_mix_compilation: a.is_mix_compilation(),
            artist_links: artist_links(resolver, a.artist.as_deref()),
            album_artist_links: artist_links(resolver, a.album_artist.as_deref().or(a.artist.as_deref())),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    pub revision: u64,
    pub root: Option<String>,
    pub scan_state: fl_library::ScanState,
    pub has_completed_initial_load: bool,
    pub tracks: Vec<TrackDto>,
    pub albums: Vec<AlbumDto>,
}
