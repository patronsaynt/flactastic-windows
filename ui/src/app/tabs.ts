import {
  AudioWaveform,
  CircleArrowDown,
  House,
  ListMusic,
  ListVideo,
  SlidersHorizontal,
  type LucideIcon,
} from "lucide-react";

/** `AppTab` — raw values double as labels. */
export type AppTab = "Home" | "Collection" | "Playlists" | "Download" | "Organizer" | "Visualizer";

export const libraryTabs: AppTab[] = ["Home", "Collection", "Playlists"];
export const toolTabs: AppTab[] = ["Download", "Organizer", "Visualizer"];

/** SF Symbol → Lucide (see docs/ICON-MAP.md). */
export const tabIcons: Record<AppTab, LucideIcon> = {
  Home: House, // music.note.house
  Collection: ListMusic, // music.note.list
  Playlists: ListVideo, // list.bullet.rectangle
  Download: CircleArrowDown, // arrow.down.circle
  Organizer: SlidersHorizontal, // slider.horizontal.3
  Visualizer: AudioWaveform, // waveform
};
