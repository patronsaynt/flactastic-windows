/**
 * Which editor sheet is open. The Mac attaches each sheet to the view that
 * opened it; one host at the root keeps them above everything here.
 */
import { create } from "zustand";
import type { Album, Track } from "../lib/api";

export type EditorSheet =
  | { kind: "track"; track: Track }
  | { kind: "album"; album: Album }
  | { kind: "merge"; tracks: Track[]; onDone?: () => void };

export const useEditors = create<{ sheet: EditorSheet | null }>(() => ({ sheet: null }));

export const editors = {
  track: (track: Track) => useEditors.setState({ sheet: { kind: "track", track } }),
  album: (album: Album) => useEditors.setState({ sheet: { kind: "album", album } }),
  merge: (tracks: Track[], onDone?: () => void) => useEditors.setState({ sheet: { kind: "merge", tracks, onDone } }),
  close: () => useEditors.setState({ sheet: null }),
};
