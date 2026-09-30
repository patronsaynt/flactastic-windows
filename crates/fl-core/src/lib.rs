//! FLACtastic core: models, library grouping, resolvers, cue/lyrics parsing,
//! and persistence that stays byte-compatible with the macOS app.

pub mod apple_json;
pub mod cue;
pub mod format;
pub mod import_copy;
pub mod library;
pub mod listening;
pub mod lyrics;
pub mod metadata_cache;
pub mod model;
pub mod organizer;
pub mod playlist_store;
pub mod remote;
pub mod resolvers;
pub mod settings;
pub mod stores;
pub mod text;
pub mod track_ids;

pub use apple_json::{AppleDate, Uid};
pub use format::{AudioFileFormat, AudioQuality};
pub use model::{Album, ArtistSummary, Artwork, Playlist, PlaylistEntry, Track};
pub use resolvers::{ArtistResolver, GenreResolver};
