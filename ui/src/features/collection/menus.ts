import { CopyPlus, LayoutGrid, ListEnd, ListStart, Pencil, Plus, Trash2 } from "lucide-react";
import { editors } from "../../app/editors";
import { player } from "../../app/player";
import { useLibrary } from "../../app/library";
import { useUI } from "../../app/store";
import { api, type Album, type Track } from "../../lib/api";
import { platform } from "../../lib/native";
import { requestAddToPlaylist, usePlaylists } from "../../app/playlists";
import { confirmDialog } from "../../components/sheet/ConfirmDialog";
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

/** `addToPlaylistMenuItem`: existing playlists, then "New playlist name…". */
export function addToPlaylistItem(tracks: Track[]): MenuItem {
  return menu.submenu("Add to Playlist", addToPlaylistChildren(tracks, false), CopyPlus);
}

/**
 * The playlist list itself. `emptyLabel` is the player bar's variant, which
 * says "No playlists yet" instead of going straight to the text field.
 */
export function addToPlaylistChildren(tracks: Track[], emptyLabel: boolean): MenuItem[] {
  const playlists = usePlaylists.getState().playlists;
  const items: MenuItem[] = [];
  if (!playlists.length) {
    if (emptyLabel) items.push(menu.label("No playlists yet"));
  } else {
    for (const p of playlists) items.push(menu.button(p.name, () => void requestAddToPlaylist(tracks, p)));
    items.push(menu.divider);
  }
  items.push(
    menu.textField(
      "New playlist name…",
      (name) => void api.createPlaylistAndAdd(name, tracks.map((t) => t.id)),
      Plus,
    ),
  );
  return items;
}

/** `removeFromLibraryConfirmation`. */
export function confirmRemoveFromLibrary(title: string, tracks: Track[]) {
  const n = tracks.length;
  const bin = platform === "windows" ? "the Recycle Bin" : "the Trash";
  confirmDialog({
    title: `Remove "${title}" from Library?`,
    message:
      n === 1
        ? `The file will be deleted from your library and moved to ${bin}.`
        : `${n} files will be deleted from your library and moved to ${bin}.`,
    buttons: [{ title: "Delete from Library", destructive: true, action: () => void api.removeTracks(tracks.map((t) => t.id)) }],
  });
}

export function editTrackItem(track: Track): MenuItem {
  return menu.button("Edit...", () => editors.track(track), Pencil);
}

export function editAlbumItem(album: Album): MenuItem {
  return menu.button("Edit...", () => editors.album(album), Pencil);
}

export function removeFromLibraryItem(title: string, tracks: Track[]): MenuItem {
  return menu.button("Remove from Library", () => confirmRemoveFromLibrary(title, tracks), Trash2);
}
