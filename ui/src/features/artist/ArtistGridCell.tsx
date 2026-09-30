import { MicVocal } from "lucide-react";
import { useState } from "react";
import { useSetting } from "../../app/settings";
import { artworkUrl, type Artist } from "../../lib/api";

const ART_SIZE = 180;

/**
 * `ArtistGridCell`: the preferred image (profile, banner, or Deezer), else
 * the first album cover; the album card's hover lift and highlight.
 */
export function ArtistGridCell({ artist }: { artist: Artist }) {
  const rounded = useSetting("flactastic.roundedArtwork");
  const shadow = useSetting("flactastic.showArtworkShadow");
  const zoom = useSetting("flactastic.uiScale");
  const [failed, setFailed] = useState<Set<string>>(new Set());
  const radius = rounded ? 8 : 0;
  // An undecodable preferred image falls through to the artwork sample.
  const src = [artist.image, artist.artworkSample].find((id): id is string => !!id && !failed.has(id));

  const releases = artist.albumIds.length + artist.singleIds.length;
  const appears = artist.appearsOnIds.length;
  const count = appears === 0 ? `${releases} release${releases === 1 ? "" : "s"}` : `${releases} · appears on ${appears}`;

  return (
    <div className="album-card">
      <div className={"album-card__cover" + (shadow ? " has-shadow" : "")} style={{ borderRadius: radius }}>
        {src ? (
          <img
            src={artworkUrl(src, ART_SIZE, zoom)}
            alt=""
            draggable={false}
            loading="lazy"
            decoding="async"
            style={{ borderRadius: radius }}
            onError={() => setFailed((f) => new Set(f).add(src))}
          />
        ) : (
          <div className="album-card__placeholder" style={{ borderRadius: radius }}>
            <MicVocal size={40} strokeWidth={0.75} />
          </div>
        )}
      </div>
      <div className="album-card__title">{artist.displayName}</div>
      <div className="album-card__artist">{count}</div>
    </div>
  );
}
