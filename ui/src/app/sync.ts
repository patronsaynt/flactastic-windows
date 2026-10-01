/**
 * `SyncModel` mirror plus the confirmation checklist's tick state
 * (`SyncPickList`'s exclusions and tri-state marks; the tree comes from Rust).
 */
import { useEffect } from "react";
import { create } from "zustand";
import { api, on, type PickList, type SyncSelection, type SyncState } from "../lib/api";

export const useSync = create<{ state: SyncState | null }>(() => ({ state: null }));

let subscribers = 0;
let stop: (() => void) | null = null;

/**
 * While mounted, networking runs (`begin()` / `end()` are reference counted
 * on the backend, as on the Mac).
 */
export function useSyncSession() {
  useEffect(() => {
    if (subscribers++ === 0) {
      stop = on<SyncState>("sync://changed", (state) => useSync.setState({ state }));
    }
    void api.syncBegin().then(() => api.syncState().then((s) => s && useSync.setState({ state: s })));
    return () => {
      void api.syncEnd();
      if (--subscribers === 0) {
        stop?.();
        stop = null;
      }
    };
  }, []);
}

export type Mark = "all" | "some" | "none";

/** `SyncPickList`: records exclusions; the default is everything. */
export class PickState {
  readonly allTracks: string[];
  readonly allPlaylists: string[];
  private bytes = new Map<string, number>();

  constructor(
    readonly list: PickList,
    readonly excludedTracks: ReadonlySet<string> = new Set(),
    readonly excludedPlaylists: ReadonlySet<string> = new Set(),
  ) {
    this.allTracks = list.artists.flatMap((a) => a.trackIDs);
    this.allPlaylists = list.playlists.map((p) => p.entry.id);
    for (const a of list.artists) for (const al of a.albums) for (const t of al.tracks) this.bytes.set(t.entry.trackID, t.entry.fileSize);
  }

  private with(tracks: Set<string>, playlists: Set<string>) {
    return new PickState(this.list, tracks, playlists);
  }

  get isEverything() {
    return this.excludedTracks.size === 0 && this.excludedPlaylists.size === 0;
  }

  setEverything(included: boolean) {
    return this.with(included ? new Set() : new Set(this.allTracks), included ? new Set() : new Set(this.allPlaylists));
  }

  mark(ids: string[]): Mark {
    const excluded = ids.filter((id) => this.excludedTracks.has(id)).length;
    if (excluded === 0) return "all";
    return excluded === ids.length ? "none" : "some";
  }

  /** Ticking a partly-ticked group ticks all of it; only a full one unticks. */
  toggle(ids: string[]) {
    const next = new Set(this.excludedTracks);
    if (this.mark(ids) === "all") ids.forEach((id) => next.add(id));
    else ids.forEach((id) => next.delete(id));
    return this.with(next, new Set(this.excludedPlaylists));
  }

  includedCount(ids: string[]) {
    return ids.filter((id) => !this.excludedTracks.has(id)).length;
  }

  isPlaylistIncluded(id: string) {
    return !this.excludedPlaylists.has(id);
  }

  togglePlaylist(id: string) {
    const next = new Set(this.excludedPlaylists);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return this.with(new Set(this.excludedTracks), next);
  }

  get selectedTrackCount() {
    return this.allTracks.length - this.excludedTracks.size;
  }

  get selectedPlaylistCount() {
    return this.allPlaylists.length - this.excludedPlaylists.size;
  }

  get selectedBytes() {
    let n = 0;
    for (const [id, b] of this.bytes) if (!this.excludedTracks.has(id)) n += b;
    return n;
  }

  get isEmptySelection() {
    return this.selectedTrackCount === 0 && this.selectedPlaylistCount === 0;
  }

  /** What goes on the wire: nothing when nothing was unticked. */
  get selection(): SyncSelection {
    const s: SyncSelection = {};
    if (this.excludedTracks.size) s.trackIDs = this.allTracks.filter((id) => !this.excludedTracks.has(id));
    if (this.excludedPlaylists.size) s.playlistIDs = this.allPlaylists.filter((id) => !this.excludedPlaylists.has(id));
    return s;
  }
}

/** `ByteCountFormatter` (`.file`: decimal units). */
export function byteCount(n: number): string {
  if (n < 1000) return `${n} bytes`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1000;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

/** `SyncPeerStore.lastSyncedDescription`. */
export function lastSyncedDescription(unix: number | null): string {
  if (unix == null) return "Never synced";
  const secs = Date.now() / 1000 - unix;
  if (secs < 60) return "Synced just now";
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "always" });
  const steps: [number, Intl.RelativeTimeFormatUnit][] = [
    [60, "minute"],
    [3600, "hour"],
    [86400, "day"],
    [604800, "week"],
    [2629800, "month"],
    [31557600, "year"],
  ];
  let unit: Intl.RelativeTimeFormatUnit = "minute";
  let size = 60;
  for (const [s, u] of steps) if (secs >= s) [size, unit] = [s, u];
  return "Last synced " + rtf.format(-Math.floor(secs / size), unit);
}
