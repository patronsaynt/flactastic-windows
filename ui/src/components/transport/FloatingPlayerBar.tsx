import { ListMusic, Pause, Play, Plus, Repeat, Repeat1, Rewind, FastForward, Shuffle } from "lucide-react";
import { player, usePlayer } from "../../app/player";
import { ArtworkView } from "../ArtworkView";
import { SeekBar } from "./SeekBar";
import { VolumeSlider } from "./VolumeSlider";
import { contextMenu } from "../menu/ContextMenu";
import { viewAlbumItem } from "../../features/collection/menus";
import { linksFor, withArtistItems } from "../../features/artist/ArtistLink";
import "./FloatingPlayerBar.css";

/**
 * `FloatingPlayerBar`: track info left, transport centred on the full bar
 * width, add-to-playlist / queue / volume right, prominent seek bar below.
 */
export function FloatingPlayerBar({ onAddToPlaylist }: { onAddToPlaylist?: (e: React.MouseEvent) => void }) {
  const track = usePlayer((s) => s.currentTrack);
  const isPlaying = usePlayer((s) => s.isPlaying);
  const shuffle = usePlayer((s) => s.shuffle);
  const repeat = usePlayer((s) => s.repeat);
  const queueVisible = usePlayer((s) => s.isQueueVisible);
  if (!track) return null;

  return (
    <div className="player-bar">
      <div className="player-bar__row">
        <div className="player-bar__transport">
          <button className={"pb-icon" + (shuffle ? " pb-icon--on" : "")} onClick={player.toggleShuffle} title="Shuffle">
            <Shuffle size={14} strokeWidth={2} />
          </button>
          <button className="pb-main" style={{ width: 36, height: 36 }} onClick={player.previous} title="Previous">
            <Rewind size={21} fill="currentColor" strokeWidth={0} />
          </button>
          <button className="pb-main" style={{ width: 50, height: 50 }} onClick={player.toggle} title={isPlaying ? "Pause" : "Play"}>
            {isPlaying ? (
              <Pause size={27} fill="currentColor" strokeWidth={0} />
            ) : (
              <Play size={27} fill="currentColor" strokeWidth={0} style={{ marginLeft: 3 }} />
            )}
          </button>
          <button className="pb-main" style={{ width: 36, height: 36 }} onClick={player.next} title="Next">
            <FastForward size={21} fill="currentColor" strokeWidth={0} />
          </button>
          <button className={"pb-icon" + (repeat !== "off" ? " pb-icon--on" : "")} onClick={player.cycleRepeat} title="Repeat">
            {repeat === "one" ? <Repeat1 size={14} strokeWidth={2} /> : <Repeat size={14} strokeWidth={2} />}
          </button>
        </div>

        <div
          className="player-bar__info"
          onContextMenu={contextMenu(() => withArtistItems([viewAlbumItem(track)], linksFor(track)))}
        >
          <ArtworkView artwork={track.artwork} size={48} />
          <div className="player-bar__text">
            <div className="player-bar__title">{track.title}</div>
            {track.artistDisplay && <div className="player-bar__artist">{track.artistDisplay}</div>}
          </div>
        </div>
        <div className="player-bar__spacer" />
        <button className="pb-side" onClick={onAddToPlaylist} title="Add to playlist">
          <Plus size={15} strokeWidth={2} />
        </button>
        <button
          className={"pb-side" + (queueVisible ? " pb-side--on" : "")}
          onClick={() => player.setQueueVisible(!queueVisible)}
          title={queueVisible ? "Hide queue" : "Show queue"}
        >
          <ListMusic size={16} strokeWidth={2} />
        </button>
        <VolumeSlider />
      </div>
      <SeekBar prominent />
    </div>
  );
}
