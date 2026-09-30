import { GripHorizontal, Volume2 } from "lucide-react";
import type { Track } from "../../lib/api";
import { formatDuration, formatSampleRate, qualityLabel } from "../../lib/format";
import { ArtworkView } from "../ArtworkView";
import { trackColumns } from "../chrome/Chrome";
import "./TrackRow.css";

/** `TrackRow`: number or playing glyph, title/subtitle, FORMAT, QUALITY, LENGTH. */
export function TrackRow({
  track,
  isPlaying,
  displayNumber,
  showDragHandle = false,
  showAlbumArt = false,
  showAlbumInSubtitle = false,
}: {
  track: Track;
  isPlaying: boolean;
  displayNumber?: number;
  showDragHandle?: boolean;
  showAlbumArt?: boolean;
  showAlbumInSubtitle?: boolean;
}) {
  const artist = track.artistDisplay;
  const subtitle =
    showAlbumInSubtitle && track.album ? (artist ? `${artist} — ${track.album}` : track.album) : artist;
  const num = displayNumber ?? track.trackNumber;
  const numberCell = (
    <span className="track-row__num">
      {isPlaying ? (
        <Volume2 size={12} fill="currentColor" strokeWidth={1.5} className="track-row__playing" />
      ) : num != null ? (
        String(num).padStart(2, "0")
      ) : (
        ""
      )}
    </span>
  );
  const detail = formatSampleRate(track.sampleRate, track.bitDepth);
  const qLabel = detail ? `${qualityLabel[track.quality]} · ${detail}` : qualityLabel[track.quality];

  return (
    <div className="track-row">
      {showAlbumArt && displayNumber != null ? (
        <>
          {numberCell}
          <ArtworkView artwork={track.artwork} size={34} />
        </>
      ) : showAlbumArt ? (
        <span className="track-row__art">
          <ArtworkView artwork={track.artwork} size={34} />
          {isPlaying && <Volume2 size={12} fill="white" color="white" strokeWidth={1.5} className="track-row__art-glyph" />}
        </span>
      ) : (
        numberCell
      )}
      <div className="track-row__text">
        <div className={"track-row__title" + (isPlaying ? " is-playing" : "")}>{track.title}</div>
        {subtitle && <div className="track-row__subtitle">{subtitle}</div>}
      </div>
      <span className="badge badge--format" style={{ minWidth: trackColumns.format }}>
        {track.fileFormat === "aac" ? "AAC" : track.fileFormat.toUpperCase()}
      </span>
      <span className={`badge badge--quality q-${track.quality}`} style={{ minWidth: trackColumns.quality }}>
        {qLabel}
      </span>
      <span className="track-row__length tabular" style={{ minWidth: trackColumns.length }}>
        {formatDuration(track.duration)}
      </span>
      {showDragHandle && <GripHorizontal size={12} className="track-row__handle" />}
    </div>
  );
}
