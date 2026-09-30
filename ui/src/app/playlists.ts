/**
 * `PlaylistStore` on the UI side (resolved against the library in Rust),
 * plus `PlaylistAddCoordinator`'s duplicate check.
 */
import { create } from "zustand";
import { api, on, type Playlist, type Track } from "../lib/api";
import { confirmDialog } from "../components/sheet/ConfirmDialog";
import { useLibrary } from "./library";

interface PlaylistsState {
  playlists: Playlist[];
  byId: Map<string, Playlist>;
}

export const usePlaylists = create<PlaylistsState>(() => ({ playlists: [], byId: new Map() }));

let timer: ReturnType<typeof setTimeout> | undefined;

async function refetch() {
  const playlists = (await api.playlists()) ?? [];
  usePlaylists.setState({ playlists, byId: new Map(playlists.map((p) => [p.id, p])) });
}

function schedule(delay = 0) {
  clearTimeout(timer);
  timer = setTimeout(() => void refetch(), delay);
}

export function startPlaylistsSync(): () => void {
  let lastRevision = useLibrary.getState().revision;
  const unsub = useLibrary.subscribe((s) => {
    if (s.revision !== lastRevision) {
      lastRevision = s.revision;
      schedule(250);
    }
  });
  const off = on("playlists://changed", () => schedule());
  schedule();
  return () => {
    unsub();
    off();
    clearTimeout(timer);
  };
}

export function playlistTracks(p: Playlist): Track[] {
  const byId = useLibrary.getState().tracksById;
  return p.trackIds.map((id) => byId.get(id)).filter((t): t is Track => !!t);
}

/**
 * `PlaylistAddCoordinator.request`: adds at once, or asks what to do with
 * tracks the playlist already holds.
 */
export async function requestAddToPlaylist(tracks: Track[], playlist: Playlist) {
  if (!tracks.length) return;
  const ids = tracks.map((t) => t.id);
  const dupes = (await api.playlistDuplicateCount(playlist.id, ids)) ?? 0;
  if (dupes === 0) {
    await api.addToPlaylist(playlist.id, ids, false);
    return;
  }
  const total = tracks.length;
  const fresh = total - dupes;
  const title = dupes === total ? "Already in Playlist" : "Duplicate Tracks";
  const message =
    dupes === total
      ? `All ${total} ${total === 1 ? "track is" : "tracks are"} already in "${playlist.name}".`
      : `${dupes} of ${total} ${dupes === 1 ? "track" : "tracks"} are already in "${playlist.name}".`;
  confirmDialog({
    title,
    message,
    buttons: [
      ...(fresh > 0
        ? [{ title: `Skip Duplicates (Add ${fresh})`, action: () => void api.addToPlaylist(playlist.id, ids, true) }]
        : []),
      { title: "Add Anyway", action: () => void api.addToPlaylist(playlist.id, ids, false) },
    ],
  });
}
