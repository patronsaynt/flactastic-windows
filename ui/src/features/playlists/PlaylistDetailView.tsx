import { LayoutGrid, ListMusic, MinusCircle, Pencil, Play, Shuffle } from "lucide-react";
import { useCallback, useState } from "react";
import { useLibrary } from "../../app/library";
import { player, usePlayer } from "../../app/player";
import { playlistTracks, usePlaylists } from "../../app/playlists";
import { playlistsBackTitle, useUI } from "../../app/store";
import { api, type Playlist, type Track } from "../../lib/api";
import { playlistSummary } from "../../lib/format";
import { ArtworkView } from "../../components/ArtworkView";
import { ActionPill, BackLink, CircleIconButton, Eyebrow, TrackListHeader } from "../../components/chrome/Chrome";
import { contextMenu, menu, type MenuItem } from "../../components/menu/ContextMenu";
import { Modal } from "../../components/sheet/Sheet";
import { TrackRow } from "../../components/tracks/TrackRow";
import { artistMenuItems } from "../artist/ArtistLink";
import { playbackItems } from "../collection/menus";
import { useReorder } from "../../lib/reorder";
import { PlaylistEditorView } from "./PlaylistEditorView";
import "../collection/AlbumDetailView.css";
import "./Playlists.css";


/** `PlaylistDetailView` */
export function PlaylistDetailView({ playlistId }: { playlistId: string }) {
  const playlist = usePlaylists((s) => s.byId.get(playlistId));
  const tracksById = useLibrary((s) => s.tracksById);
  const backTitle = useUI(playlistsBackTitle);
  const goBack = useUI((s) => s.goBackInPlaylists);
  const currentId = usePlayer((s) => s.currentTrackId);
  const [editingName, setEditingName] = useState<string | null>(null);
  const [showEditor, setShowEditor] = useState(false);
  const closeEditor = useCallback(() => setShowEditor(false), []);
  const reorder = useReorder({ onMove: (src, dst) => void api.movePlaylistEntry(playlistId, src, dst) });

  if (!playlist) return <div className="not-found">Playlist not found</div>;
  const tracks = playlistTracks(playlist);

  const start = (index: number, shuffle: boolean | undefined) => {
    void player.play(tracks, index, playlist.name, shuffle);
    void api.recordPlaylistPlay(playlist.id);
  };

  const commitRename = () => {
    const n = editingName?.trim();
    if (n) void api.renamePlaylist(playlist.id, n);
    setEditingName(null);
  };

  // Resolved rows keep their entry index for numbering (`index + 1`).
  const rows = playlist.entries
    .map((e, i) => ({ entry: e, index: i, track: e.track ? tracksById.get(e.track) : undefined }))
    .filter((r): r is { entry: typeof r.entry; index: number; track: Track } => !!r.track);

  return (
    <div className="detail-scroll">
      <div className="detail-inner">
        <div style={{ paddingTop: 24 }}>
          <BackLink title={backTitle} onClick={goBack} />
        </div>
        <div className="album-header">
          <ArtworkView artwork={playlist.artwork} size={180} />
          <div className="album-header__text">
            <Eyebrow>Playlist</Eyebrow>
            {editingName != null ? (
              <input
                className="album-header__title playlist-title-field"
                value={editingName}
                placeholder="Playlist name"
                autoFocus
                onChange={(e) => setEditingName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitRename();
                  if (e.key === "Escape") setEditingName(null);
                }}
                onBlur={commitRename}
              />
            ) : (
              <h1 className="album-header__title" title="Double-click to rename" onDoubleClick={() => setEditingName(playlist.name)}>
                {playlist.name}
              </h1>
            )}
            <div className="album-header__meta">{playlistSummary(tracks.length, playlist.totalDuration)}</div>
            {playlist.description && <div className="playlist-description">{playlist.description}</div>}
            <div className="album-header__actions">
              {tracks.length > 0 && (
                <>
                  <ActionPill primary icon={Play} onClick={() => start(0, false)}>
                    Play
                  </ActionPill>
                  <ActionPill icon={Shuffle} onClick={() => start(Math.floor(Math.random() * tracks.length), true)}>
                    Shuffle
                  </ActionPill>
                </>
              )}
              <CircleIconButton icon={Pencil} title="Edit cover, name and description" onClick={() => setShowEditor(true)} />
            </div>
          </div>
        </div>

        {tracks.length === 0 ? (
          <div className="playlist-empty">
            <ListMusic size={40} strokeWidth={1.5} />
            <div className="playlist-empty__title">No tracks in this playlist</div>
            <div className="playlist-empty__hint">Right-click tracks in your collection to add them</div>
          </div>
        ) : (
          <>
            <TrackListHeader showDragHandle />
            <div>
              {rows.map(({ entry, index, track }, resolvedIndex) => {
                const playing = track.id === currentId;
                const isTarget = reorder.target === entry.id && reorder.dragging !== entry.id;
                return (
                  <div
                    key={entry.id}
                    className={"fl-row playlist-row" + (playing ? " is-filled" : "") + (isTarget ? " is-drop-target" : "")}
                    style={{ opacity: reorder.dragging === entry.id ? 0.4 : 1 }}
                    {...reorder.rowProps(entry.id)}
                    {...reorder.handleProps(entry.id, track.title)}
                    onDoubleClick={() => start(resolvedIndex, undefined)}
                    onContextMenu={contextMenu(() => entryMenu(playlist, entry.id, track))}
                  >
                    <TrackRow track={track} isPlaying={playing} displayNumber={index + 1} showDragHandle showAlbumArt />
                  </div>
                );
              })}
            </div>
          </>
        )}
      </div>
      {reorder.ghost}
      <Modal open={showEditor} onClose={closeEditor}>
        <PlaylistEditorView playlistId={playlist.id} onClose={closeEditor} />
      </Modal>
    </div>
  );
}

function entryMenu(playlist: Playlist, entryId: string, track: Track): MenuItem[] {
  return [
    ...playbackItems([track]),
    menu.divider,
    menu.button(
      "View Album",
      () => {
        const albumId = useLibrary.getState().albumOfTrack.get(track.id);
        if (albumId) useUI.getState().navigateToAlbum(albumId);
      },
      LayoutGrid,
    ),
    ...artistMenuItems(track.artistLinks),
    menu.divider,
    menu.button("Remove from Playlist", () => void api.removePlaylistEntries(playlist.id, [entryId]), MinusCircle),
  ];
}
