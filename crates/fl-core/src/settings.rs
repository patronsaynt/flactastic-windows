//! App settings — the Mac's `UserDefaults` keys and defaults, stored as
//! `settings.json` in the app config directory.
//!
//! Keys keep their `flactastic.*` names. Unknown keys are preserved on save
//! so a newer build's settings survive a downgrade.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::apple_json::{self, Uid};
use crate::organizer::OrganizerProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FadeAnimationDirection {
    #[default]
    Up,
    LeftToRight,
    RightToLeft,
}

impl FadeAnimationDirection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Up => "Upward",
            Self::LeftToRight => "Left to Right",
            Self::RightToLeft => "Right to Left",
        }
    }
}

/// Visualizer modes in mode-wheel order (the order is load-bearing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum VisualizerMode {
    AlbumArtLarge,
    #[default]
    AlbumArtLargeDetails,
    AlbumArtSmallDetails,
    AlbumArtWheel,
    Lyrics,
    SpectrumRadial,
    SpectrumHorizontal,
    Spectrogram,
}

impl VisualizerMode {
    pub const ALL: [VisualizerMode; 8] = [
        Self::AlbumArtLarge,
        Self::AlbumArtLargeDetails,
        Self::AlbumArtSmallDetails,
        Self::AlbumArtWheel,
        Self::Lyrics,
        Self::SpectrumRadial,
        Self::SpectrumHorizontal,
        Self::Spectrogram,
    ];

    pub fn wheel_label(self) -> &'static str {
        match self {
            Self::AlbumArtLarge => "Large Art",
            Self::AlbumArtLargeDetails => "Large Art + Details",
            Self::AlbumArtSmallDetails => "Small Art + Details",
            Self::AlbumArtWheel => "Cover Wheel",
            Self::Lyrics => "Lyrics",
            Self::SpectrumRadial => "Radial Spectrum",
            Self::SpectrumHorizontal => "Horizontal Spectrum",
            Self::Spectrogram => "Spectrogram",
        }
    }

    pub fn requires_audio_tap(self) -> bool {
        matches!(self, Self::SpectrumRadial | Self::SpectrumHorizontal | Self::Spectrogram)
    }
}

/// A stored mode the current build no longer knows ("bigPicture") falls back
/// to the default, as on the Mac.
fn de_visualizer_mode<'de, D: serde::Deserializer<'de>>(d: D) -> Result<VisualizerMode, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

macro_rules! defaults {
    ($($name:ident: $ty:ty = $val:expr;)*) => {
        $(fn $name() -> $ty { $val })*
    };
}

defaults! {
    d_volume: f32 = 0.75;
    d_ui_scale: f64 = 1.0;
    d_true: bool = true;
    d_counted: f64 = 0.90;
    d_collection_sort: String = "album".into();
    d_all_tracks_sort: String = "dateAdded".into();
    d_stats_range: String = "allTime".into();
    d_profiles: Vec<OrganizerProfile> = vec![OrganizerProfile::default_profile()];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "flactastic.lastRootPath", default, skip_serializing_if = "Option::is_none")]
    pub last_root_path: Option<String>,
    #[serde(rename = "flactastic.volume", default = "d_volume")]
    pub volume: f32,
    #[serde(rename = "flactastic.useLightMode", default)]
    pub use_light_mode: bool,
    #[serde(rename = "flactastic.useListLayout", default)]
    pub use_list_layout: bool,
    /// Clamped to 0.9…1.35 by the UI.
    #[serde(rename = "flactastic.uiScale", default = "d_ui_scale")]
    pub ui_scale: f64,
    #[serde(rename = "flactastic.showMenuBarPlayer", default = "d_true")]
    pub show_menu_bar_player: bool,
    #[serde(rename = "flactastic.roundedArtwork", default = "d_true")]
    pub rounded_artwork: bool,
    #[serde(rename = "flactastic.showArtworkShadow", default = "d_true")]
    pub show_artwork_shadow: bool,
    #[serde(rename = "flactastic.fadeAnimationsEnabled", default = "d_true")]
    pub fade_animations_enabled: bool,
    #[serde(rename = "flactastic.fadeAnimationDirection", default)]
    pub fade_animation_direction: FadeAnimationDirection,
    #[serde(rename = "flactastic.hasCompletedOnboarding", default)]
    pub has_completed_onboarding: bool,
    #[serde(rename = "flactastic.groupByArtist", default)]
    pub group_by_artist: bool,
    #[serde(rename = "flactastic.customGenres", default)]
    pub custom_genres: Vec<String>,
    #[serde(rename = "flactastic.autoFetchArtistImages", default = "d_true")]
    pub auto_fetch_artist_images: bool,
    #[serde(rename = "flactastic.discordRichPresenceEnabled", default = "d_true")]
    pub discord_rich_presence_enabled: bool,
    #[serde(rename = "flactastic.visualizerMode", default, deserialize_with = "de_visualizer_mode")]
    pub visualizer_mode: VisualizerMode,
    #[serde(rename = "flactastic.lyricsLookupEnabled", default = "d_true")]
    pub lyrics_lookup_enabled: bool,
    #[serde(rename = "flactastic.saveLyricsToFiles", default = "d_true")]
    pub save_lyrics_to_files: bool,
    #[serde(rename = "flactastic.showVpnNotice", default = "d_true")]
    pub show_vpn_notice: bool,
    #[serde(rename = "flactastic.showDownloadTab", default)]
    pub show_download_tab: bool,
    #[serde(rename = "flactastic.showSpotifyLikedSongs", default = "d_true")]
    pub show_spotify_liked_songs: bool,
    /// Fraction heard for a listen to count as a play; clamped to 0…1.
    #[serde(rename = "flactastic.countedPlayFraction", default = "d_counted")]
    pub counted_play_fraction: f64,
    /// Pinned output device ID; `None` follows the system default.
    #[serde(rename = "flactastic.outputDeviceUID", default, skip_serializing_if = "Option::is_none")]
    pub output_device_uid: Option<String>,
    /// Output rate applied to the device; `None` leaves the device alone.
    #[serde(rename = "flactastic.outputSampleRate", default, skip_serializing_if = "Option::is_none")]
    pub output_sample_rate: Option<f64>,
    #[serde(rename = "flactastic.outputBitDepth", default, skip_serializing_if = "Option::is_none")]
    pub output_bit_depth: Option<i64>,
    /// Windows only: WASAPI exclusive mode instead of changing the shared format.
    #[serde(rename = "flactastic.outputExclusiveMode", default)]
    pub output_exclusive_mode: bool,

    // @AppStorage values
    #[serde(rename = "flactastic.collectionSort", default = "d_collection_sort")]
    pub collection_sort: String,
    #[serde(rename = "flactastic.allTracksSort", default = "d_all_tracks_sort")]
    pub all_tracks_sort: String,
    #[serde(rename = "flactastic.allTracksAscending", default)]
    pub all_tracks_ascending: bool,
    #[serde(rename = "flactastic.home.statsRange", default = "d_stats_range")]
    pub home_stats_range: String,

    // Organizer
    #[serde(rename = "flactastic.organizer.profiles", default = "d_profiles")]
    pub organizer_profiles: Vec<OrganizerProfile>,
    #[serde(rename = "flactastic.organizer.selectedProfileID", default, skip_serializing_if = "Option::is_none")]
    pub organizer_selected_profile_id: Option<Uid>,

    // Sync identity
    #[serde(rename = "flactastic.sync.deviceID", default, skip_serializing_if = "Option::is_none")]
    pub sync_device_id: Option<Uid>,
    #[serde(rename = "flactastic.sync.deviceName", default, skip_serializing_if = "Option::is_none")]
    pub sync_device_name: Option<String>,

    /// Keys this build doesn't know, kept verbatim.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        serde_json::from_str("{}").expect("all fields have defaults")
    }
}

impl Settings {
    pub fn path(config_dir: &Path) -> PathBuf {
        config_dir.join("settings.json")
    }

    pub fn load(config_dir: &Path) -> Settings {
        let mut s = match apple_json::load::<Settings>(&Self::path(config_dir)) {
            Ok(Some(s)) => s,
            Ok(None) => Settings::default(),
            Err(e) => {
                log::warn!("[Settings] unreadable settings.json, using defaults: {e}");
                Settings::default()
            }
        };
        s.normalise();
        s
    }

    pub fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        apple_json::write_atomic(&Self::path(config_dir), &bytes)
    }

    /// Clamp and repair invariants the Mac enforces in `didSet`/`init`.
    pub fn normalise(&mut self) {
        self.counted_play_fraction = self.counted_play_fraction.clamp(0.0, 1.0);
        self.extra.remove("flactastic.showBigPictureFullScreenToggle");
        if self.organizer_profiles.is_empty() {
            self.organizer_profiles = d_profiles();
        }
        let sel = self.organizer_selected_profile_id;
        if !self.organizer_profiles.iter().any(|p| Some(p.id) == sel) {
            self.organizer_selected_profile_id = Some(self.organizer_profiles[0].id);
        }
    }

    pub fn selected_organizer_profile(&self) -> &OrganizerProfile {
        let sel = self.organizer_selected_profile_id;
        self.organizer_profiles.iter().find(|p| Some(p.id) == sel).unwrap_or(&self.organizer_profiles[0])
    }

    /// Stable random ID for this installation (created on first use).
    pub fn device_id(&mut self) -> Uid {
        *self.sync_device_id.get_or_insert_with(Uid::new_v4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_mac() {
        let s = Settings::default();
        assert_eq!(s.volume, 0.75);
        assert!(s.show_menu_bar_player && s.rounded_artwork && s.lyrics_lookup_enabled);
        assert!(!s.show_download_tab && !s.use_light_mode);
        assert_eq!(s.visualizer_mode, VisualizerMode::AlbumArtLargeDetails);
        assert_eq!(s.counted_play_fraction, 0.9);
        assert_eq!(s.organizer_profiles.len(), 1);
    }

    #[test]
    fn round_trip_keeps_unknown_keys_and_migrates_mode() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            Settings::path(dir.path()),
            r#"{"flactastic.visualizerMode":"bigPicture","flactastic.future":42,"flactastic.countedPlayFraction":3}"#,
        )
        .unwrap();
        let mut s = Settings::load(dir.path());
        assert_eq!(s.visualizer_mode, VisualizerMode::AlbumArtLargeDetails);
        assert_eq!(s.counted_play_fraction, 1.0);
        let id = s.device_id();
        s.save(dir.path()).unwrap();
        let back = Settings::load(dir.path());
        assert_eq!(back.extra["flactastic.future"], 42);
        assert_eq!(back.sync_device_id, Some(id));
    }
}
