import { create } from "zustand";
import type { AppTab } from "./tabs";

/** Mirrors `NavigationRouter` plus the few UI-only flags ContentView owns. */
interface UIState {
  selectedTab: AppTab;
  showSettings: boolean;
  isQueueVisible: boolean;
  select(tab: AppTab): void;
  setShowSettings(v: boolean): void;
  toggleQueue(): void;
}

export const useUI = create<UIState>((set) => ({
  selectedTab: "Home",
  showSettings: false,
  isQueueVisible: false,
  select: (tab) => set({ selectedTab: tab }),
  setShowSettings: (v) => set({ showSettings: v }),
  toggleQueue: () => set((s) => ({ isQueueVisible: !s.isQueueVisible })),
}));
