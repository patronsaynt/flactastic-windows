import { GripHorizontal, ListMusic, MinusCircle, Volume2, X } from "lucide-react";
import { useState } from "react";
import { player, usePlayer } from "../../app/player";
import type { Track } from "../../lib/api";
import { formatDuration } from "../../lib/format";
import { ArtworkView } from "../ArtworkView";
import { contextMenu, menu } from "../menu/ContextMenu";
import { addToPlaylistItem, editTrackItem, viewAlbumItem } from "../../features/collection/menus";
import { useLibrary } from "../../app/library";
import { linksFor, withArtistItems } from "../../features/artist/ArtistLink";
import { api } from "../../lib/api";
import "./QueuePanel.css";

/** `QueuePanelView`: Now Playing, then Next Up with drag-to-reorder. */
export function QueuePanel() {
  const queue = usePlayer((s) => s.queue);
  const currentIndex = usePlayer((s) => s.currentIndex);
  const current = usePlayer((s) => s.currentTrack);
  const [dragging, setDragging] = useState<string | null>(null);
  const [target, setTarget] = useState<string | null>(null);
  const upcoming = queue.slice(currentIndex + 1).map((q, i) => ({ ...q, index: currentIndex + 1 + i }));

  return (
    <div className="queue-panel">
      <div className="queue-panel__header">
        <span className="queue-panel__title">Queue</span>
        <button className="queue-panel__close" onClick={() => player.setQueueVisible(false)}>
          <X size={14} strokeWidth={2.2} />
        </button>
      </div>
      <div className="queue-panel__divider" />
      {!current && queue.length === 0 ? (
        <div className="queue-panel__empty">
          <ListMusic size={28} strokeWidth={1.5} />
          <div>Queue is empty</div>
          <div className="queue-panel__hint">Right-click an album or track to queue it</div>
        </div>
      ) : (
        <div className="queue-panel__list">
          {current && (
            <>
              <div className="queue-panel__section">Now Playing</div>
              <div className="queue-now" onContextMenu={contextMenu(() => withArtistItems([viewAlbumItem(current)], linksFor(current)))}>
                <ArtworkView artwork={current.artwork} size={48} />
                <div className="queue-row__text">
                  <div className="queue-now__title">{current.title}</div>
                  {current.artistDisplay && <div className="queue-row__artist">{current.artistDisplay}</div>}
                </div>
                <Volume2 size={12} fill="currentColor" strokeWidth={1.5} className="queue-now__glyph" />
              </div>
            </>
          )}
          {upcoming.length > 0 && <div className="queue-panel__section">Next Up</div>}
          {upcoming.map((item) => (
            <div
              key={item.track.id}
              className={"queue-row" + (dragging === item.track.id ? " is-dragging" : "") + (target === item.track.id && dragging !== item.track.id ? " is-target" : "")}
              draggable
              onDragStart={(e) => {
                e.dataTransfer.setData("text/x-fl-queue", item.track.id);
                e.dataTransfer.effectAllowed = "move";
                setDragging(item.track.id);
              }}
              onDragEnd={() => {
                setDragging(null);
                setTarget(null);
              }}
              onDragOver={(e) => {
                if (!e.dataTransfer.types.includes("text/x-fl-queue")) return;
                e.preventDefault();
                setTarget(item.track.id);
              }}
              onDragLeave={() => setTarget((t) => (t === item.track.id ? null : t))}
              onDrop={(e) => {
                e.preventDefault();
                const src = e.dataTransfer.getData("text/x-fl-queue");
                setTarget(null);
                setDragging(null);
                if (src && src !== item.track.id) void api.moveQueueTrack(src, item.track.id);
              }}
              onDoubleClick={() => void api.jumpTo(item.index)}
              onContextMenu={contextMenu(() => upcomingMenu(item.track, item.index))}
            >
              <span className="queue-row__dot">{item.userQueued && <span />}</span>
              <ArtworkView artwork={item.track.artwork} size={36} />
              <div className="queue-row__text">
                <div className="queue-row__title">{item.track.title}</div>
                {item.track.artistDisplay && <div className="queue-row__artist">{item.track.artistDisplay}</div>}
              </div>
              <span className="queue-row__length tabular">{formatDuration(item.track.duration)}</span>
              <GripHorizontal size={12} className="queue-row__handle" />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function upcomingMenu(track: Track, index: number) {
  return [
    addToPlaylistItem([track]),
    menu.divider,
    menu.button("Remove from Queue", () => void api.removeFromQueue(index), MinusCircle),
    menu.divider,
    ...withArtistItems([viewAlbumItem(track)], linksFor(track)),
    menu.divider,
    // The library's copy carries the freshest tags.
    editTrackItem(useLibrary.getState().tracksById.get(track.id) ?? track),
  ];
}
