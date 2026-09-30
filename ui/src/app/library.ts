/**
 * `LibraryStore` on the UI side: the latest snapshot, refetched when the
 * backend reports a new revision (coalesced so a scan's batches don't
 * refetch more than a few times a second).
 */
import { create } from "zustand";
import { api, on, type Album, type LibraryChanged, type ScanState, type Track } from "../lib/api";

interface LibraryState {
  revision: number;
  root: string | null;
  scanState: ScanState;
  hasCompletedInitialLoad: boolean;
  tracks: Track[];
  albums: Album[];
  tracksById: Map<string, Track>;
  albumsById: Map<string, Album>;
  /** Album id per track id. */
  albumOfTrack: Map<string, string>;
}

export const useLibrary = create<LibraryState>(() => ({
  revision: -1,
  root: null,
  scanState: { state: "idle" },
  hasCompletedInitialLoad: false,
  tracks: [],
  albums: [],
  tracksById: new Map(),
  albumsById: new Map(),
  albumOfTrack: new Map(),
}));

let fetching = false;
let pending = false;

async function refetch() {
  if (fetching) {
    pending = true;
    return;
  }
  fetching = true;
  try {
    const snap = await api.librarySnapshot();
    if (snap) {
      const albumOfTrack = new Map<string, string>();
      for (const a of snap.albums) for (const t of a.trackIds) albumOfTrack.set(t, a.id);
      useLibrary.setState({
        revision: snap.revision,
        root: snap.root,
        scanState: snap.scanState,
        hasCompletedInitialLoad: snap.hasCompletedInitialLoad,
        tracks: snap.tracks,
        albums: snap.albums,
        tracksById: new Map(snap.tracks.map((t) => [t.id, t])),
        albumsById: new Map(snap.albums.map((a) => [a.id, a])),
        albumOfTrack,
      });
    }
  } finally {
    fetching = false;
    if (pending) {
      pending = false;
      setTimeout(refetch, 200);
    }
  }
}

export function startLibrarySync(): () => void {
  void refetch();
  return on<LibraryChanged>("library://changed", (c) => {
    // Cheap fields apply immediately; the track list follows.
    useLibrary.setState({ scanState: c.scanState, hasCompletedInitialLoad: c.hasCompletedInitialLoad });
    if (c.revision !== useLibrary.getState().revision) void refetch();
  });
}

export function albumTracks(album: Album): Track[] {
  const byId = useLibrary.getState().tracksById;
  return album.trackIds.map((id) => byId.get(id)).filter((t): t is Track => !!t);
}
