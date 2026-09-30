/**
 * Which editor or import sheet is open. The Mac attaches each sheet to the
 * view that opened it (imports to `ContentView` via `ImportCoordinator`);
 * one host at the root keeps them above everything here.
 */
import { create } from "zustand";
import type { Album, Track } from "../lib/api";

export type ImportMode = "track" | "album" | "playlist";

export type EditorSheet =
  | { kind: "track"; track: Track }
  | { kind: "album"; album: Album }
  | { kind: "merge"; tracks: Track[]; onDone?: () => void }
  | { kind: "import"; mode: ImportMode };

export const useEditors = create<{ sheet: EditorSheet | null }>(() => ({ sheet: null }));

export const editors = {
  track: (track: Track) => useEditors.setState({ sheet: { kind: "track", track } }),
  album: (album: Album) => useEditors.setState({ sheet: { kind: "album", album } }),
  merge: (tracks: Track[], onDone?: () => void) => useEditors.setState({ sheet: { kind: "merge", tracks, onDone } }),
  /** `ImportCoordinator.begin` */
  import: (mode: ImportMode) => useEditors.setState({ sheet: { kind: "import", mode } }),
  close: () => useEditors.setState({ sheet: null }),
};
