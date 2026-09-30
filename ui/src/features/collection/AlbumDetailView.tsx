import { Pencil, Play, Shuffle } from "lucide-react";
import { useEffect, useRef } from "react";
import { albumTracks, useLibrary } from "../../app/library";
import { player, usePlayer } from "../../app/player";
import { collectionBackTitle, useUI } from "../../app/store";
import { api, type Album } from "../../lib/api";
import { formatDuration } from "../../lib/format";
import { plural } from "../../lib/text";
import { ArtworkView } from "../../components/ArtworkView";
import { ActionPill, BackLink, CircleIconButton, Eyebrow, TrackListHeader } from "../../components/chrome/Chrome";
import { contextMenu, menu } from "../../components/menu/ContextMenu";
import { RiseFadeIn } from "../../components/RiseFadeIn";
import { TrackRow } from "../../components/tracks/TrackRow";
import { ArtistLink, withArtistItems } from "../artist/ArtistLink";
import { addToPlaylistItem, playbackItems, removeFromLibraryItem } from "./menus";
import "./AlbumDetailView.css";

/** `AlbumDetailView` */
export function AlbumDetailView({ albumId }: { albumId: string }) {
  const albumsById = useLibrary((s) => s.albumsById);
  const albums = useLibrary((s) => s.albums);
  useLibrary((s) => s.tracksById);
  const backTitle = useUI(collectionBackTitle);
  const goBack = useUI((s) => s.goBackInCollection);
  const zoom = useUI((s) => s.setArtworkZoom);
  const currentId = usePlayer((s) => s.currentTrackId);

  // After an edit renames the album its id changes; follow its tracks.
  const known = useRef<Set<string>>(new Set());
  const album: Album | undefined =
    albumsById.get(albumId) ?? (known.current.size ? albums.find((a) => a.trackIds.some((t) => known.current.has(t))) : undefined);
  useEffect(() => {
    if (album) known.current = new Set(album.trackIds);
  }, [album]);

  if (!album) {
    return <div className="not-found">Album not found</div>;
  }
  const tracks = albumTracks(album);

  const playAlbum = (shuffle: boolean) => {
    const start = shuffle ? Math.floor(Math.random() * tracks.length) : 0;
    void player.play(tracks, start, album.name, shuffle);
    void api.recordAlbumPlay(album.id);
  };

  const pieces: string[] = [];
  if (album.year != null) pieces.push(String(album.year));
  if (album.genre) pieces.push(album.genre);
  pieces.push(...album.secondaryGenres);
  pieces.push(plural(tracks.length, "track"));
  pieces.push(formatDuration(album.totalDuration));

  return (
    <div className="detail-scroll">
      <div className="detail-inner">
        <div style={{ paddingTop: 24 }}>
          <BackLink title={backTitle} onClick={goBack} />
        </div>
        <div className="album-header">
          <ArtworkView artwork={album.artwork} size={180} className="zoomable" onClick={() => album.artwork && zoom(album.artwork)} />
          <div className="album-header__text">
            <Eyebrow>{album.isMixCompilation ? "Mix Compilation" : "Album"}</Eyebrow>
            <h1 className="album-header__title">{album.name}</h1>
            <div className="album-header__meta">
              {album.isCompilation ? <span>Compilation</span> : <ArtistLink links={album.artistLinks} />}
              {pieces.map((p, i) => (
                <span key={i}> · {p}</span>
              ))}
            </div>
            <div className="album-header__actions">
              <ActionPill primary icon={Play} onClick={() => playAlbum(false)}>
                Play
              </ActionPill>
              <ActionPill icon={Shuffle} onClick={() => playAlbum(true)}>
                Shuffle
              </ActionPill>
              <CircleIconButton icon={Pencil} title="Edit album" onClick={() => {}} />
            </div>
          </div>
        </div>
        <TrackListHeader />
        <div>
          {tracks.map((t, i) => (
            <RiseFadeIn
              key={t.id}
              index={i}
              className={"fl-row" + (t.id === currentId ? " is-filled" : "")}
              onDoubleClick={() => {
                void player.play(tracks, i, album.name);
                void api.recordAlbumPlay(album.id);
              }}
              onContextMenu={contextMenu(() =>
                withArtistItems(
                  [...playbackItems([t]), menu.divider, removeFromLibraryItem(t.title, [t]), menu.divider, addToPlaylistItem([t])],
                  t.artistLinks,
                ),
              )}
            >
              <TrackRow track={t} isPlaying={t.id === currentId} />
            </RiseFadeIn>
          ))}
        </div>
      </div>
    </div>
  );
}
