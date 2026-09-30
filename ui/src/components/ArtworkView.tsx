import { Music } from "lucide-react";
import { useState } from "react";
import { artworkUrl } from "../lib/api";
import { useSetting } from "../app/settings";
import "./ArtworkView.css";

/**
 * `ArtworkView`: a square cover at `size` points with the user's rounded
 * corners / shadow preferences, or the music-note placeholder.
 */
export function ArtworkView({
  artwork,
  size,
  fullResolution = false,
  className,
  onClick,
}: {
  artwork: string | null | undefined;
  size: number;
  fullResolution?: boolean;
  className?: string;
  onClick?: () => void;
}) {
  const rounded = useSetting("flactastic.roundedArtwork");
  const shadow = useSetting("flactastic.showArtworkShadow");
  const zoom = useSetting("flactastic.uiScale");
  const [failed, setFailed] = useState<string | null>(null);
  const radius = rounded ? 12 : 0;
  // ArtworkShadow: blur max(2, min(14, size·0.06)), y max(1, min(6, size·0.025)).
  const blur = Math.max(2, Math.min(14, size * 0.06)) * 2;
  const y = Math.max(1, Math.min(6, size * 0.025));
  const alpha = size < 80 ? 0.3 : 0.22;
  const style = {
    width: size,
    height: size,
    borderRadius: radius,
    boxShadow: shadow ? `0 ${y}px ${blur}px rgb(0 0 0 / ${alpha})` : undefined,
  };
  const src = artwork && failed !== artwork ? artworkUrl(artwork, fullResolution ? 0 : size, zoom) : null;
  return (
    <div className={"artwork" + (className ? " " + className : "")} style={style} onClick={onClick}>
      {src ? (
        <img src={src} alt="" draggable={false} style={{ borderRadius: radius }} onError={() => setFailed(artwork!)} />
      ) : (
        <div className="artwork__placeholder" style={{ borderRadius: radius }}>
          <Music size={size * 0.3} strokeWidth={1} />
        </div>
      )}
    </div>
  );
}
