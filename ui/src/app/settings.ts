/**
 * `Settings`: the raw `flactastic.*` map plus typed accessors for the keys
 * the UI reads. Writes go through the backend, which persists and echoes.
 */
import { create } from "zustand";
import { api, on, type SettingsMap } from "../lib/api";

interface SettingsState {
  raw: SettingsMap;
  loaded: boolean;
}

export const useSettingsStore = create<SettingsState>(() => ({ raw: {}, loaded: false }));

export function startSettingsSync(): () => void {
  void api.getSettings().then((raw) => raw && useSettingsStore.setState({ raw, loaded: true }));
  return on<SettingsMap>("settings://changed", (raw) => useSettingsStore.setState({ raw, loaded: true }));
}

export function setSetting(key: string, value: unknown) {
  useSettingsStore.setState((s) => ({ raw: { ...s.raw, [key]: value } }));
  void api.setSetting(key, value);
}

const defaults = {
  "flactastic.useLightMode": false,
  "flactastic.useListLayout": false,
  "flactastic.uiScale": 1,
  "flactastic.roundedArtwork": true,
  "flactastic.showArtworkShadow": true,
  "flactastic.fadeAnimationsEnabled": true,
  "flactastic.fadeAnimationDirection": "up",
  "flactastic.hasCompletedOnboarding": false,
  "flactastic.groupByArtist": false,
  "flactastic.showDownloadTab": false,
  "flactastic.showMenuBarPlayer": true,
  "flactastic.volume": 0.75,
  "flactastic.collectionSort": "Album",
  "flactastic.allTracksSort": "Date Added",
  "flactastic.allTracksAscending": false,
  "flactastic.visualizerMode": "albumArtLargeDetails",
  "flactastic.countedPlayFraction": 0.9,
} as const;

type Known = typeof defaults;

/** Typed read of a known key, with the Mac default. */
export function useSetting<K extends keyof Known>(key: K): Known[K] extends boolean ? boolean : Known[K] extends number ? number : string {
  return useSettingsStore((s) => (s.raw[key] ?? defaults[key]) as never);
}

export function getSetting<K extends keyof Known>(key: K): unknown {
  return useSettingsStore.getState().raw[key] ?? defaults[key];
}
