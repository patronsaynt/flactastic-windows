/**
 * Download-side state mirrored from Rust: the job list
 * (`DownloadCoordinator.jobs`), Lucida's bridge state, the playlist rebuild
 * phase and the Spotify connection.
 */
import { create } from "zustand";
import {
  api,
  on,
  type DownloadJob,
  type LucidaOptions,
  type LucidaState,
  type RebuildPhase,
  type SpotifyState,
} from "../lib/api";

interface DownloadsState {
  jobs: DownloadJob[];
  lucida: LucidaState;
  rebuild: RebuildPhase;
  spotify: SpotifyState;
}

export const useDownloads = create<DownloadsState>(() => ({
  jobs: [],
  lucida: { phase: { kind: "idle" }, needsUserChallenge: false },
  rebuild: { kind: "idle" },
  spotify: { connection: { kind: "disconnected" }, playlists: [], playlistsError: null },
}));

const reloadJobs = () => void api.downloadJobs().then((jobs) => useDownloads.setState({ jobs }));

export function startDownloadsSync(): () => void {
  reloadJobs();
  void api.lucidaState().then((lucida) => useDownloads.setState({ lucida }));
  void api.rebuildState().then((rebuild) => useDownloads.setState({ rebuild }));
  void api.spotifyState().then((spotify) => useDownloads.setState({ spotify }));
  const stops = [
    on("downloads://changed", reloadJobs),
    on<DownloadJob>("downloads://job", (job) =>
      useDownloads.setState((s) => ({ jobs: s.jobs.map((j) => (j.id === job.id ? job : j)) })),
    ),
    on<LucidaState>("lucida://state", (lucida) => useDownloads.setState({ lucida })),
    on<RebuildPhase>("rebuild://changed", (rebuild) => useDownloads.setState({ rebuild })),
    on<SpotifyState>("spotify://changed", (spotify) => useDownloads.setState({ spotify })),
  ];
  return () => stops.forEach((s) => s());
}

export const isTerminal = (j: DownloadJob) =>
  j.status.kind === "completed" || j.status.kind === "failed" || j.status.kind === "cancelled" || j.status.kind === "skipped";

// MARK: - LucidaOptions

export const FORMATS: { value: LucidaOptions["format"]; label: string }[] = [
  { value: "original", label: "Original (highest quality)" },
  { value: "flac", label: "FLAC" },
  { value: "mp3", label: "MP3" },
  { value: "ogg-vorbis", label: "Ogg Vorbis" },
  { value: "opus", label: "Opus" },
  { value: "m4a-aac", label: "M4A (AAC)" },
  { value: "wav", label: "WAV" },
  { value: "bitcrush", label: "Bitcrush" },
];

const KBPS = ["320", "256", "192", "128"].map((v) => ({ value: v, label: `${v} kb/s` }));

/** Quality presets per format; the first is the default. */
export function qualitiesFor(format: LucidaOptions["format"]): { value: string; label: string }[] {
  switch (format) {
    case "flac":
      return [{ value: "16", label: "16-bit 44.1 kHz" }];
    case "mp3":
    case "ogg-vorbis":
    case "m4a-aac":
      return KBPS;
    case "opus":
      return [...KBPS, { value: "96", label: "96 kb/s" }, { value: "64", label: "64 kb/s" }];
    default:
      return [];
  }
}

/** `DebugState`: session-only developer toggles. */
export const useDebug = create<{ lucidaDebugEnabled: boolean }>(() => ({ lucidaDebugEnabled: false }));

export const defaultOptions = (): LucidaOptions => ({
  region: "auto",
  addMetadata: true,
  compatibility: false,
  format: "original",
  quality: null,
});
