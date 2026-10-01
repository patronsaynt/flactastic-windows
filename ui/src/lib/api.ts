/**
 * Typed bridge to the Rust side (app/src-tauri/src/commands.rs). Shapes
 * mirror the DTOs in dto.rs / player_actor.rs.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke, isTauri } from "./native";

export type AudioFileFormat = "flac" | "mp3" | "wav" | "aiff" | "alac" | "aac";
export type AudioQuality = "hiRes" | "cd" | "mid" | "low";
export type RepeatMode = "off" | "all" | "one";

export interface Track {
  id: string;
  path: string;
  relPath: string | null;
  title: string;
  artist: string | null;
  artistDisplay: string | null;
  albumArtist: string | null;
  album: string | null;
  trackNumber: number | null;
  duration: number | null;
  fileFormat: AudioFileFormat;
  sampleRate: number | null;
  bitDepth: number | null;
  quality: AudioQuality;
  genre: string | null;
  secondaryGenres: string[];
  year: number | null;
  isCompilation: boolean;
  isMixCompilation: boolean;
  /** Unix seconds. */
  dateAdded: number | null;
  /** Artwork content id for `artworkUrl`. */
  artwork: string | null;
  /** `artist ?? albumArtist` split into linkable artists (library tracks only). */
  artistLinks?: ArtistLinkPiece[];
}

export interface ArtistLinkPiece {
  name: string;
  key: string;
}

export interface Album {
  id: string;
  name: string;
  artist: string | null;
  albumArtist: string | null;
  year: number | null;
  genre: string | null;
  secondaryGenres: string[];
  artwork: string | null;
  trackIds: string[];
  totalDuration: number;
  isCompilation: boolean;
  isMixCompilation: boolean;
  /** `album.artist` as links (detail header). */
  artistLinks: ArtistLinkPiece[];
  /** `albumArtist ?? artist` as links (context menu). */
  albumArtistLinks: ArtistLinkPiece[];
}

export interface Artist {
  id: string;
  displayName: string;
  albumIds: string[];
  singleIds: string[];
  appearsOnIds: string[];
  trackCount: number;
  artworkSample: string | null;
  image: string | null;
}

export interface ArtistDetail {
  artist: Artist;
  banner: string | null;
  bannerIsTrue: boolean;
  baseColor: [number, number, number] | null;
}

export interface ArtistOverride {
  displayName: string | null;
  banner: string | null;
  profile: string | null;
}

export interface PlaylistEntry {
  id: string;
  /** Library track id, or null when the entry no longer resolves. */
  track: string | null;
}

export interface Playlist {
  id: string;
  name: string;
  description: string | null;
  /** Unix seconds. */
  dateCreated: number;
  customArtwork: string | null;
  /** `customArtwork ?? first resolved track's artwork`. */
  artwork: string | null;
  entries: PlaylistEntry[];
  trackIds: string[];
  totalDuration: number;
}

export interface RecentContext {
  kind: "album" | "playlist";
  targetID: string;
  title: string;
  subtitle: string;
  date: number;
}

export interface RankedItem {
  name: string;
  plays: number;
  minutes: number;
}

export interface AlbumRank {
  albumId: string;
  album: string;
  artist: string;
  plays: number;
  minutes: number;
}

export interface HomeMetrics {
  hasHistory: boolean;
  recentlyPlayed: RecentContext[];
  weeklyMinutes: number[];
  weeklyDayLabels: string[];
  hoursListened: number;
  tracksPlayed: number;
  albumsPlayed: number;
  sessions: number;
  topGenre: { name: string; share: number } | null;
  streakDays: number;
  topArtists: RankedItem[];
  topAlbums: AlbumRank[];
  footerAlbumCount: number;
  footerHours: number;
}

export interface HomeHighlight {
  lyric: string;
  songTitle: string;
  artistDisplay: string | null;
  image: string | null;
  isPinned: boolean;
}

export type ArtworkEdit = { kind: "unchanged" } | { kind: "removed" } | { kind: "updated"; id: string };

/** `MetadataWriter.write` arguments. Omit `albumArtist` to leave it alone; `null` clears it. */
export interface TrackEdit {
  title: string;
  artist: string | null;
  album: string | null;
  year: number | null;
  genre: string | null;
  secondaryGenres: string[];
  trackNumber: number | null;
  artwork: ArtworkEdit;
  albumArtist?: string | null;
  compilation?: boolean | null;
  mixCompilation?: boolean | null;
}

export interface LyricLine {
  timestamp: number | null;
  text: string;
}

export interface Marker {
  id: string;
  timestamp: number;
  title: string;
}

export interface LoadedImage {
  id: string;
  width: number;
  height: number;
}

export type ScanState =
  | { state: "idle" }
  | { state: "scanning" }
  | { state: "refreshing" }
  | { state: "done"; count: number }
  | { state: "failed"; message: string };

export interface LibrarySnapshot {
  revision: number;
  root: string | null;
  scanState: ScanState;
  hasCompletedInitialLoad: boolean;
  tracks: Track[];
  albums: Album[];
}

export interface LibraryChanged {
  revision: number;
  scanState: ScanState;
  hasCompletedInitialLoad: boolean;
}

export interface QueueItem {
  track: Track;
  userQueued: boolean;
}

export interface PlayerSnapshot {
  currentTrackId: string | null;
  currentIndex: number;
  isPlaying: boolean;
  currentTime: number;
  duration: number | null;
  volume: number;
  shuffle: boolean;
  repeat: RepeatMode;
  playbackSource: string | null;
  isQueueVisible: boolean;
  queueRevision: number;
  queue?: QueueItem[];
  outputSampleRate: number | null;
  lastError: string | null;
}

export interface DeviceInfo {
  id: string;
  name: string;
}

export interface OutputStatus {
  devices: DeviceInfo[];
  defaultDeviceId: string | null;
  effectiveDeviceId: string | null;
  selectedDeviceId: string | null;
  selectedSampleRate: number | null;
  selectedBitDepth: number | null;
  exclusive: boolean;
  isSelectedDeviceMissing: boolean;
  availableSampleRates: number[];
  availableBitDepths: number[];
  currentSampleRate: number | null;
  currentBitDepth: number | null;
  streamSampleRate: number | null;
  backend: string;
}

// MARK: - Downloads (StreamerModels, DownloadCoordinator, Lucida, Spotify)

export interface RemoteArtist {
  id: string;
  name: string;
  url?: string | null;
  pictureUrl?: string | null;
}

export interface RemoteCoverArt {
  url: string;
  width?: number | null;
  height?: number | null;
}

export interface RemoteAlbumRef {
  id: string;
  title: string;
  url?: string | null;
  coverArt: RemoteCoverArt[];
  releaseYear?: number | null;
  trackCount?: number | null;
}

export interface RemoteTrack {
  id: string;
  title: string;
  artists: RemoteArtist[];
  album?: RemoteAlbumRef | null;
  trackNumber?: number | null;
  discNumber?: number | null;
  durationSeconds?: number | null;
  coverArt: RemoteCoverArt[];
  url?: string | null;
  serviceId: string;
  isLossless: boolean;
}

export interface RemoteAlbum {
  id: string;
  title: string;
  artists: RemoteArtist[];
  releaseYear: number | null;
  coverArt: RemoteCoverArt[];
  url: string | null;
  trackCount: number | null;
  tracks: RemoteTrack[];
  serviceId: string;
}

export interface RemotePlaylist {
  id: string;
  title: string;
  creator: string | null;
  coverArt: RemoteCoverArt[];
  url: string | null;
  tracks: RemoteTrack[];
  serviceId: string;
}

export type RemoteResolve =
  | { kind: "track"; track: RemoteTrack }
  | { kind: "album"; album: RemoteAlbum }
  | { kind: "playlist"; playlist: RemotePlaylist }
  | { kind: "artist"; artist: RemoteArtist; topTracks: RemoteTrack[]; albums: RemoteAlbum[] };

export type LucidaFormat = "original" | "flac" | "mp3" | "ogg-vorbis" | "opus" | "m4a-aac" | "wav" | "bitcrush";

export interface LucidaOptions {
  region: string;
  addMetadata: boolean;
  compatibility: boolean;
  format: LucidaFormat;
  quality: string | null;
}

export type JobStatus =
  | { kind: "queued" }
  | { kind: "downloading"; receivedBytes: number; totalBytes: number | null }
  | { kind: "tagging" }
  | { kind: "finishing" }
  | { kind: "completed"; path: string }
  | { kind: "failed"; message: string }
  | { kind: "cancelled" }
  | { kind: "skipped"; path: string };

export interface DownloadJob {
  id: string;
  track: RemoteTrack;
  status: JobStatus;
  trustEmbeddedMetadata: boolean;
}

export type LucidaPhase = { kind: "idle" } | { kind: "loading" } | { kind: "ready" } | { kind: "failed"; message: string };

export interface LucidaState {
  phase: LucidaPhase;
  needsUserChallenge: boolean;
}

export interface LucidaLogEntry {
  id: number;
  timestamp: number;
  kind: "nav" | "bridge" | "ok" | "error" | "info";
  message: string;
}

export interface RebuildSummary {
  playlistName: string;
  total: number;
  downloaded: number;
  reused: { index: number; title: string; artist: string }[];
  failures: { index: number; title: string; artist: string; reason: string }[];
  alreadyInPlaylist: number;
  isResume: boolean;
}

export type RebuildPhase =
  | { kind: "idle" }
  | { kind: "fetchingArtwork" }
  | { kind: "running"; current: number; total: number }
  | { kind: "finished"; summary: RebuildSummary };

export interface SpotifyPlaylistSummary {
  id: string;
  name: string;
  owner: string | null;
  /** -1 for the synthetic Liked Songs card. */
  trackCount: number;
  coverArtUrl: string | null;
  externalUrl: string;
}

export type SpotifyConnection = { kind: "disconnected" } | { kind: "connecting" } | { kind: "connected"; displayName: string };

export interface SpotifyState {
  connection: SpotifyConnection;
  playlists: SpotifyPlaylistSummary[];
  playlistsError: string | null;
}

export const LIKED_SONGS_ID = "__liked_songs__";

/** Settings are the Mac's `flactastic.*` UserDefaults keys, verbatim. */
export type SettingsMap = Record<string, unknown>;

export const api = {
  appReady: () => invoke<void>("app_ready"),
  appVersion: () => invoke<string>("app_version"),

  getSettings: () => invoke<SettingsMap>("get_settings"),
  setSetting: (key: string, value: unknown) => invoke<SettingsMap>("set_setting", { key, value }),

  openLibrary: (path: string) => invoke<void>("open_library", { path }),
  bootstrapLibrary: () => invoke<boolean>("bootstrap_library"),
  createDefaultMusicFolder: () => invoke<string>("create_default_music_folder"),
  refreshLibrary: () => invoke<void>("refresh_library"),
  librarySnapshot: () => invoke<LibrarySnapshot>("library_snapshot"),
  removeTracks: (ids: string[]) => invoke<number>("remove_tracks", { ids }),

  playTracks: (ids: string[], start: number, source?: string | null, shuffle?: boolean) =>
    invoke<void>("play_tracks", { ids, start, source: source ?? null, shuffle: shuffle ?? null }),
  playNext: (ids: string[]) => invoke<void>("play_next", { ids }),
  addToQueue: (ids: string[]) => invoke<void>("add_to_queue", { ids }),
  transport: (action: "toggle" | "play" | "pause" | "next" | "previous" | "volumeUp" | "volumeDown" | "shuffle") =>
    invoke<void>("transport", { action }),
  seek: (seconds: number) => invoke<void>("seek", { seconds }),
  setVolume: (volume: number) => invoke<void>("set_volume", { volume }),
  setRepeat: (mode: RepeatMode) => invoke<void>("set_repeat", { mode }),
  jumpTo: (index: number) => invoke<void>("jump_to", { index }),
  removeFromQueue: (index: number) => invoke<void>("remove_from_queue", { index }),
  moveQueueTrack: (source: string, destination: string) => invoke<void>("move_queue_track", { source, destination }),
  setQueueVisible: (visible: boolean) => invoke<void>("set_queue_visible", { visible }),
  playerSnapshot: () => invoke<PlayerSnapshot | null>("player_snapshot"),
  setSpectrum: (enabled: boolean) => invoke<void>("set_spectrum", { enabled }),

  outputStatus: () => invoke<OutputStatus | null>("output_status"),
  selectOutputDevice: (id: string | null) => invoke<void>("select_output_device", { id }),
  selectOutputSampleRate: (rate: number | null) => invoke<void>("select_output_sample_rate", { rate }),
  selectOutputBitDepth: (bits: number | null) => invoke<void>("select_output_bit_depth", { bits }),
  setExclusiveOutput: (enabled: boolean) => invoke<void>("set_exclusive_output", { enabled }),

  artists: () => invoke<Artist[]>("artists"),
  artistDetail: (key: string) => invoke<ArtistDetail | null>("artist_detail", { key }),
  ensureArtistImage: (key: string, displayName: string) => invoke<void>("ensure_artist_image", { key, displayName }),
  artistOverride: (key: string) => invoke<ArtistOverride>("artist_override", { key }),
  saveArtistOverride: (key: string, displayName: string | null, banner: string | null, profile: string | null) =>
    invoke<void>("save_artist_override", { key, displayName, banner, profile }),
  resetArtistOverride: (key: string) => invoke<void>("reset_artist_override", { key }),
  loadImageFile: (path: string) => invoke<LoadedImage>("load_image_file", { path }),
  writeTrackMetadata: (id: string, edit: TrackEdit) => invoke<void>("write_track_metadata", { id, edit }),
  writeTracksMetadata: (albumId: string | null, edits: { id: string; edit: TrackEdit }[]) =>
    invoke<void>("write_tracks_metadata", { albumId, edits }),
  readTrackLyrics: (id: string) => invoke<string>("read_track_lyrics", { id }),
  writeTrackLyrics: (id: string, lyrics: string) => invoke<void>("write_track_lyrics", { id, lyrics }),
  parseLyricsForSync: (raw: string) => invoke<LyricLine[]>("parse_lyrics_for_sync", { raw }),
  serializeLrc: (lines: LyricLine[]) => invoke<string>("serialize_lrc", { lines }),
  readTrackMarkers: (id: string) => invoke<Marker[]>("read_track_markers", { id }),
  writeTrackMarkers: (id: string, markers: Marker[]) => invoke<void>("write_track_markers", { id, markers }),
  parseMarkerTimestamp: (text: string) => invoke<number | null>("parse_marker_timestamp", { text }),

  importLoad: (paths: string[]) => invoke<Track[]>("import_load", { paths }),
  importCommit: (items: { token: string; edit: TrackEdit | null }[], albumFolder: { artist: string | null; album: string } | null) =>
    invoke<{ trackIds: string[]; error: string | null }>("import_commit", { items, albumFolder }),

  visualizerLyrics: (trackId: string, neighbourIds: string[]) =>
    invoke<{ state: string; lines?: LyricLine[]; isSynced?: boolean }>("visualizer_lyrics", { trackId, neighbourIds }),
  visualizerBackdrop: (trackId: string) => invoke<{ key: string; image: string | null }>("visualizer_backdrop", { trackId }),

  homeMetrics: (range: string) => invoke<HomeMetrics>("home_metrics", { range }),
  homeHighlight: () => invoke<HomeHighlight | null>("home_highlight"),
  toggleHighlightPin: () => invoke<HomeHighlight | null>("toggle_highlight_pin"),

  playlists: () => invoke<Playlist[]>("playlists"),
  createPlaylist: (name: string) => invoke<string>("create_playlist", { name }),
  deletePlaylist: (id: string) => invoke<void>("delete_playlist", { id }),
  renamePlaylist: (id: string, name: string) => invoke<void>("rename_playlist", { id, name }),
  updatePlaylistMetadata: (id: string, name: string, description: string | null, artwork: string | null) =>
    invoke<void>("update_playlist_metadata", { id, name, description, artwork }),
  playlistDuplicateCount: (id: string, trackIds: string[]) => invoke<number>("playlist_duplicate_count", { id, trackIds }),
  addToPlaylist: (id: string, trackIds: string[], skipDuplicates: boolean) =>
    invoke<void>("add_to_playlist", { id, trackIds, skipDuplicates }),
  createPlaylistAndAdd: (name: string, trackIds: string[]) => invoke<void>("create_playlist_and_add", { name, trackIds }),
  removePlaylistEntries: (id: string, entryIds: string[]) => invoke<void>("remove_playlist_entries", { id, entryIds }),
  movePlaylistEntry: (id: string, source: string, before: string) => invoke<void>("move_playlist_entry", { id, source, before }),
  recordPlaylistPlay: (id: string) => invoke<void>("record_playlist_play", { id }),
  recordAlbumPlay: (id: string) => invoke<void>("record_album_play", { id }),

  cropImage: (id: string, rect: [number, number, number, number], outWidth: number, outHeight: number) =>
    invoke<string>("crop_image", { id, rect, outWidth, outHeight }),
  downloadResolve: (url: string) => invoke<RemoteResolve>("download_resolve", { url }),
  downloadJobs: () => invoke<DownloadJob[]>("download_jobs"),
  downloadEnqueue: (tracks: RemoteTrack[], options: LucidaOptions) => invoke<void>("download_enqueue", { tracks, options }),
  downloadCancel: (id: string) => invoke<void>("download_cancel", { id }),
  downloadCancelAll: () => invoke<void>("download_cancel_all"),
  downloadClearCompleted: () => invoke<void>("download_clear_completed"),
  remoteArtwork: (url: string) => invoke<{ id: string; accent: [number, number, number] | null }>("remote_artwork", { url }),

  lucidaState: () => invoke<LucidaState>("lucida_state"),
  lucidaLog: () => invoke<LucidaLogEntry[]>("lucida_log"),
  lucidaClearLog: () => invoke<void>("lucida_clear_log"),
  lucidaWarmUp: () => invoke<void>("lucida_warm_up"),
  lucidaReload: () => invoke<void>("lucida_reload"),
  lucidaClearSiteData: () => invoke<void>("lucida_clear_site_data"),
  lucidaShowWebview: () => invoke<void>("lucida_show_webview"),
  lucidaRevealChallenge: () => invoke<void>("lucida_reveal_challenge"),
  lucidaDismissChallenge: () => invoke<void>("lucida_dismiss_challenge"),

  rebuildState: () => invoke<RebuildPhase>("rebuild_state"),
  rebuildStart: (playlist: RemotePlaylist, options: LucidaOptions) => invoke<void>("rebuild_start", { playlist, options }),
  rebuildCancel: () => invoke<void>("rebuild_cancel"),
  rebuildDismiss: () => invoke<void>("rebuild_dismiss"),

  spotifyState: () => invoke<SpotifyState>("spotify_state"),
  spotifyConnect: () => invoke<void>("spotify_connect"),
  spotifyCancelConnect: () => invoke<void>("spotify_cancel_connect"),
  spotifyDisconnect: () => invoke<void>("spotify_disconnect"),
  spotifyLoadPlaylists: () => invoke<void>("spotify_load_playlists"),
  spotifyResolvePlaylist: (url: string, authed: boolean, liked = false) =>
    invoke<{ playlist: RemotePlaylist; wasTruncated: boolean }>("spotify_resolve_playlist", { url, authed, liked }),
};

export function on<T>(event: string, cb: (payload: T) => void): () => void {
  if (!isTauri) return () => {};
  let un: UnlistenFn | undefined;
  let dead = false;
  void listen<T>(event, (e) => cb(e.payload)).then((u) => {
    if (dead) u();
    else un = u;
  });
  return () => {
    dead = true;
    un?.();
  };
}

/**
 * `flart://` URL for an artwork id at `points` CSS pixels. The pixel size
 * accounts for the display's scale and the UI zoom, like
 * `ArtworkImageCache.pixelSize(pointSize:scale:)`. `points = 0` gives the
 * original image.
 */
export function artworkUrl(id: string, points: number, zoom = 1): string {
  const px = points === 0 ? 0 : Math.ceil(points * (window.devicePixelRatio || 1) * zoom);
  const enc = encodeURIComponent(id);
  // WebView2 (Windows) maps custom schemes to http://<scheme>.localhost.
  const base = navigator.userAgent.includes("Windows") ? "http://flart.localhost/" : "flart://localhost/";
  return `${base}${enc}?px=${px}`;
}
