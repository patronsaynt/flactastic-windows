import { create } from "zustand";
import type { AppTab } from "./tabs";

/** `NavigationRoute`: album ids plain, artist pages as `artist:<key>`. */
export const route = {
  artistPrefix: "artist:",
  artist: (key: string) => `artist:${key}`,
  artistKey: (v: string) => (v.startsWith("artist:") ? v.slice(7) : null),
};

interface ReturnPoint {
  tab: AppTab;
  collectionPath: string[];
  playlistsPath: string[];
  destinationTab: AppTab;
  entryDepth: number;
}

/** `NavigationRouter` plus the few UI-only flags ContentView owns. */
interface UIState {
  selectedTab: AppTab;
  collectionPath: string[];
  playlistsPath: string[];
  artworkZoom: string | null;
  showSettings: boolean;
  /** `PlayerState.isLyricsSyncActive`: the sync sheet owns the spacebar. */
  lyricsSyncActive: boolean;
  returnStack: ReturnPoint[];
  /** Ids whose entrance animation already played this run (`revealed*IDs`). */
  revealed: Set<string>;
  select(tab: AppTab): void;
  setShowSettings(v: boolean): void;
  setLyricsSyncActive(v: boolean): void;
  setArtworkZoom(id: string | null): void;
  pushCollection(v: string): void;
  navigateToAlbum(id: string): void;
  navigateToPlaylist(id: string): void;
  navigateToArtist(key: string): void;
  goBackInCollection(): void;
  goBackInPlaylists(): void;
}

function collectionPageTitle(r: string) {
  return route.artistKey(r) != null ? "Back to Artist" : "Back to Album";
}

function backTitle(p: ReturnPoint): string {
  switch (p.tab) {
    case "Collection":
      return p.collectionPath.length ? collectionPageTitle(p.collectionPath[p.collectionPath.length - 1]) : "Back to Collection";
    case "Playlists":
      return p.playlistsPath.length ? "Back to Playlist" : "Back to Playlists";
    default:
      return `Back to ${p.tab}`;
  }
}

export const useUI = create<UIState>((set, get) => {
  const pending = (tab: AppTab, depth: number) => {
    const p = get().returnStack.at(-1);
    return p && p.destinationTab === tab && p.entryDepth === depth ? p : null;
  };
  const restore = (tab: AppTab, depth: number) => {
    const p = pending(tab, depth);
    if (!p) return false;
    set((s) => ({
      returnStack: s.returnStack.slice(0, -1),
      collectionPath: p.collectionPath,
      playlistsPath: p.playlistsPath,
      selectedTab: p.tab,
    }));
    return true;
  };
  const jump = (tab: AppTab, destinationDepth: number) => {
    const s = get();
    const point: ReturnPoint = {
      tab: s.selectedTab,
      collectionPath: s.collectionPath,
      playlistsPath: s.playlistsPath,
      destinationTab: tab,
      entryDepth: destinationDepth,
    };
    set({ returnStack: [...s.returnStack, point], selectedTab: tab });
  };
  return {
    selectedTab: "Home",
    collectionPath: [],
    playlistsPath: [],
    artworkZoom: null,
    showSettings: false,
    lyricsSyncActive: false,
    returnStack: [],
    revealed: new Set(),
    // A manual tab switch abandons any "return to where I came from" context.
    select: (tab) => set((s) => (tab === s.selectedTab ? {} : { selectedTab: tab, returnStack: [] })),
    setShowSettings: (v) => set({ showSettings: v }),
    setLyricsSyncActive: (v) => set({ lyricsSyncActive: v }),
    setArtworkZoom: (id) => set({ artworkZoom: id }),
    pushCollection: (v) => set((s) => ({ collectionPath: [...s.collectionPath, v] })),
    navigateToAlbum(id) {
      const s = get();
      if (s.selectedTab === "Collection") {
        if (s.collectionPath.at(-1) !== id) set({ collectionPath: [...s.collectionPath, id] });
        return;
      }
      jump("Collection", 1);
      set({ collectionPath: [id] });
    },
    navigateToPlaylist(id) {
      const s = get();
      if (s.selectedTab === "Playlists") {
        if (s.playlistsPath.at(-1) !== id) set({ playlistsPath: [...s.playlistsPath, id] });
        return;
      }
      jump("Playlists", 1);
      set({ playlistsPath: [id] });
    },
    navigateToArtist(key) {
      const value = route.artist(key);
      const s = get();
      if (s.selectedTab !== "Collection") jump("Collection", s.collectionPath.length + 1);
      if (get().collectionPath.at(-1) === value) return;
      set((st) => ({ collectionPath: [...st.collectionPath, value] }));
    },
    goBackInCollection() {
      const n = get().collectionPath.length;
      if (!n || restore("Collection", n)) return;
      set((s) => ({ collectionPath: s.collectionPath.slice(0, -1) }));
    },
    goBackInPlaylists() {
      const n = get().playlistsPath.length;
      if (!n || restore("Playlists", n)) return;
      set((s) => ({ playlistsPath: s.playlistsPath.slice(0, -1) }));
    },
  };
});

export function collectionBackTitle(s: UIState): string {
  const n = s.collectionPath.length;
  const p = s.returnStack.at(-1);
  if (p && p.destinationTab === "Collection" && p.entryDepth === n) return backTitle(p);
  if (n >= 2) return collectionPageTitle(s.collectionPath[n - 2]);
  return "Back to Albums";
}

export function playlistsBackTitle(s: UIState): string {
  const n = s.playlistsPath.length;
  const p = s.returnStack.at(-1);
  if (p && p.destinationTab === "Playlists" && p.entryDepth === n) return backTitle(p);
  return "Back to Playlists";
}
