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

/** Settings are the Mac's `flactastic.*` UserDefaults keys, verbatim. */
export type SettingsMap = Record<string, unknown>;

export const api = {
  appReady: () => invoke<void>("app_ready"),
  appVersion: () => invoke<string>("app_version"),

  getSettings: () => invoke<SettingsMap>("get_settings"),
  setSetting: (key: string, value: unknown) => invoke<SettingsMap>("set_setting", { key, value }),

  openLibrary: (path: string) => invoke<void>("open_library", { path }),
  bootstrapLibrary: () => invoke<boolean>("bootstrap_library"),
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
