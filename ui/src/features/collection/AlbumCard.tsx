import { Music } from "lucide-react";
import { useState } from "react";
import type { Album } from "../../lib/api";
import { artworkUrl } from "../../lib/api";
import { formatDuration } from "../../lib/format";
import { plural } from "../../lib/text";
import { useSetting } from "../../app/settings";
import { ArtworkView } from "../../components/ArtworkView";
import "./AlbumCard.css";

export const albumSubtitle = (a: Album) => (a.isCompilation ? "Compilation" : (a.artist ? displayCredit(a.artist) : "Unknown Artist"));

/** `ArtistResolver.displayString`: explicit " ; " lists read "A, B". */
export function displayCredit(raw: string): string {
  return raw.includes(" ; ") ? raw.split(" ; ").map((s) => s.trim()).filter(Boolean).join(", ") : raw;
}

/**
 * `AlbumCardView`: square cover (8pt radius) that lifts 3pt and magnifies
 * 1.03 with a hairline + glow on hover; title and artist below.
 */
export function AlbumCard({ album }: { album: Album }) {
  const rounded = useSetting("flactastic.roundedArtwork");
  const shadow = useSetting("flactastic.showArtworkShadow");
  const zoom = useSetting("flactastic.uiScale");
  const [failed, setFailed] = useState(false);
  const radius = rounded ? 8 : 0;
  return (
    <div className="album-card">
      <div
        className={"album-card__cover" + (shadow ? " has-shadow" : "")}
        style={{ borderRadius: radius }}
      >
        {album.artwork && !failed ? (
          <img
            src={artworkUrl(album.artwork, 180, zoom)}
            alt=""
            draggable={false}
            loading="lazy"
            decoding="async"
            style={{ borderRadius: radius }}
            onError={() => setFailed(true)}
          />
        ) : (
          <div className="album-card__placeholder" style={{ borderRadius: radius }}>
            <Music size={40} strokeWidth={0.75} />
          </div>
        )}
      </div>
      <div className="album-card__title">{album.name}</div>
      <div className="album-card__artist">{albumSubtitle(album)}</div>
    </div>
  );
}

/** `AlbumRowView`: 46pt art, name/artist, track count, duration. */
export function AlbumRow({ album }: { album: Album }) {
  return (
    <div className="album-row fl-row">
      <ArtworkView artwork={album.artwork} size={46} />
      <div className="album-row__text">
        <div className="album-row__title">{album.name}</div>
        <div className="album-row__artist">{albumSubtitle(album)}</div>
      </div>
      <span className="album-row__meta">{plural(album.trackIds.length, "track")}</span>
      <span className="album-row__meta tabular" style={{ width: 54, textAlign: "right" }}>
        {formatDuration(album.totalDuration)}
      </span>
    </div>
  );
}
