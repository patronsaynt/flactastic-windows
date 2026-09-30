/**
 * The artist index (`allArtists`), rebuilt on the Rust side whenever the
 * library, the overrides, or a fetched picture changes.
 */
import { create } from "zustand";
import { api, on, type Artist } from "../lib/api";
import { useLibrary } from "./library";

interface ArtistsState {
  artists: Artist[];
  byId: Map<string, Artist>;
  /** Bumps on every rebuild, so detail pages refetch. */
  revision: number;
}

export const useArtists = create<ArtistsState>(() => ({ artists: [], byId: new Map(), revision: 0 }));

let timer: ReturnType<typeof setTimeout> | undefined;
let inFlight = false;
let again = false;

async function rebuild() {
  if (inFlight) {
    again = true;
    return;
  }
  inFlight = true;
  try {
    const artists = (await api.artists()) ?? [];
    useArtists.setState((s) => ({ artists, byId: new Map(artists.map((a) => [a.id, a])), revision: s.revision + 1 }));
  } finally {
    inFlight = false;
    if (again) {
      again = false;
      schedule();
    }
  }
}

/** Coalesces bursts (a scan's batches, a wave of Deezer pictures). */
function schedule(delay = 250) {
  clearTimeout(timer);
  timer = setTimeout(() => void rebuild(), delay);
}

export function startArtistsSync(): () => void {
  let lastRevision = useLibrary.getState().revision;
  const unsubLibrary = useLibrary.subscribe((s) => {
    if (s.revision !== lastRevision) {
      lastRevision = s.revision;
      schedule();
    }
  });
  const off = on("artists://changed", () => schedule(400));
  schedule(0);
  return () => {
    unsubLibrary();
    off();
    clearTimeout(timer);
  };
}
