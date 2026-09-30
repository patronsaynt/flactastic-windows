import { useVirtualizer } from "@tanstack/react-virtual";
import { ListMusic, Play, Shuffle } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { useLibrary } from "../../app/library";
import { player, usePlayer } from "../../app/player";
import type { Track } from "../../lib/api";
import { containsCI, standardCompare } from "../../lib/text";
import { ActionPill, TrackListHeader } from "../../components/chrome/Chrome";
import { contextMenu, menu } from "../../components/menu/ContextMenu";
import { TrackRow } from "../../components/tracks/TrackRow";
import { playbackItems, viewAlbumItem } from "./menus";

function sortTracks(tracks: Track[], by: string, ascending: boolean): Track[] {
  const r = [...tracks];
  switch (by) {
    case "Song Name":
      r.sort((a, b) => standardCompare(a.title, b.title));
      break;
    case "Artist":
      r.sort((a, b) => {
        const l = a.artist ?? "";
        const rr = b.artist ?? "";
        if (!l !== !rr) return l ? -1 : 1;
        return standardCompare(l, rr);
      });
      break;
    default:
      // Missing timestamps sort to the end (before reversal, like the Mac).
      r.sort((a, b) => {
        if (a.dateAdded != null && b.dateAdded != null) return a.dateAdded - b.dateAdded;
        if (a.dateAdded == null && b.dateAdded != null) return 1;
        if (a.dateAdded != null && b.dateAdded == null) return -1;
        return standardCompare(a.title, b.title);
      });
  }
  return ascending ? r : r.reverse();
}

/** `AllTracksView`: Play All / Shuffle All and a virtualized track list. */
export function AllTracksView({ search, sort, ascending }: { search: string; sort: string; ascending: boolean }) {
  const tracks = useLibrary((s) => s.tracks);
  const currentId = usePlayer((s) => s.currentTrackId);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [anchor, setAnchor] = useState<string | null>(null);
  const visible = useMemo(() => {
    const q = search;
    const f = q ? tracks.filter((t) => containsCI(t.title, q) || containsCI(t.artist, q) || containsCI(t.album, q)) : tracks;
    return sortTracks(f, sort, ascending);
  }, [tracks, search, sort, ascending]);

  const scroller = useRef<HTMLDivElement>(null);
  const v = useVirtualizer({
    count: visible.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => 54,
    overscan: 12,
  });

  const playAll = (shuffle: boolean) => {
    if (!visible.length) return;
    const start = shuffle ? Math.floor(Math.random() * visible.length) : 0;
    void player.play(visible, start, "Library", shuffle);
    setSelection(new Set());
    setAnchor(null);
  };

  const select = (t: Track, shift: boolean) => {
    if (shift) {
      const a = anchor ?? t.id;
      const ai = visible.findIndex((x) => x.id === a);
      const ci = visible.findIndex((x) => x.id === t.id);
      if (ai < 0 || ci < 0) return;
      const [lo, hi] = ai < ci ? [ai, ci] : [ci, ai];
      setSelection(new Set(visible.slice(lo, hi + 1).map((x) => x.id)));
      if (!anchor) setAnchor(t.id);
    } else {
      setSelection(new Set([t.id]));
      setAnchor(t.id);
    }
  };

  const contextTracks = (primary: Track) =>
    selection.size >= 2 && selection.has(primary.id) ? visible.filter((t) => selection.has(t.id)) : [primary];

  return (
    <div className="all-tracks">
      <div className="all-tracks__controls">
        <ActionPill primary icon={Play} onClick={() => playAll(false)} disabled={!visible.length}>
          Play All
        </ActionPill>
        <ActionPill icon={Shuffle} onClick={() => playAll(true)} disabled={!visible.length}>
          Shuffle All
        </ActionPill>
      </div>
      {tracks.length === 0 ? (
        <Empty message="No tracks in your library" />
      ) : (
        <>
          <div className="all-tracks__header">
            <TrackListHeader />
          </div>
          <div ref={scroller} className="all-tracks__scroll" onClick={(e) => e.target === e.currentTarget && setSelection(new Set())}>
            {visible.length === 0 ? (
              <Empty message="No tracks match your search" />
            ) : (
              <div style={{ height: v.getTotalSize() + 100, position: "relative" }}>
                {v.getVirtualItems().map((row) => {
                  const t = visible[row.index];
                  const playing = t.id === currentId;
                  const selected = selection.has(t.id);
                  return (
                    <div
                      key={t.id}
                      data-index={row.index}
                      ref={v.measureElement}
                      className={"fl-row" + (playing ? " is-filled" : "")}
                      style={{
                        position: "absolute",
                        top: 0,
                        left: 0,
                        right: 0,
                        transform: `translateY(${row.start}px)`,
                        background: !playing && selected ? "color-mix(in srgb, var(--surface-elevated) 55%, transparent)" : undefined,
                      }}
                      onClick={(e) => select(t, e.shiftKey)}
                      onDoubleClick={() => {
                        void player.play(visible, row.index, "Library");
                        setSelection(new Set());
                        setAnchor(null);
                      }}
                      onContextMenu={contextMenu(() => {
                        const ts = contextTracks(t);
                        return [...playbackItems(ts), menu.divider, viewAlbumItem(t)];
                      })}
                    >
                      <TrackRow track={t} isPlaying={playing} displayNumber={row.index + 1} showAlbumArt showAlbumInSubtitle />
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}

function Empty({ message }: { message: string }) {
  return (
    <div className="empty-state">
      <ListMusic size={36} strokeWidth={1.5} />
      <div>{message}</div>
    </div>
  );
}
