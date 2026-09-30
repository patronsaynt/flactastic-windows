import { LayoutGrid, ListEnd, ListStart } from "lucide-react";
import { player } from "../../app/player";
import { useLibrary } from "../../app/library";
import { useUI } from "../../app/store";
import type { Track } from "../../lib/api";
import { menu, type MenuItem } from "../../components/menu/ContextMenu";

/** `playbackContextMenuItems`: Play Next / Add to Queue. */
export function playbackItems(tracks: Track[]): MenuItem[] {
  if (!tracks.length) return [];
  return [
    menu.button("Play Next", () => void player.playNext(tracks), ListStart),
    menu.button("Add to Queue", () => void player.addToQueue(tracks), ListEnd),
  ];
}

export function viewAlbumItem(track: Track): MenuItem {
  return menu.button(
    "View Album",
    () => {
      const albumId = useLibrary.getState().albumOfTrack.get(track.id);
      if (albumId) useUI.getState().navigateToAlbum(albumId);
    },
    LayoutGrid,
  );
}
