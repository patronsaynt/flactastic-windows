import { useEffect, useMemo, useRef } from "react";
import { useArtists } from "../../app/artists";
import { useUI } from "../../app/store";
import { api } from "../../lib/api";
import { RiseFadeIn } from "../../components/RiseFadeIn";
import { ArtistGridCell } from "./ArtistGridCell";

/**
 * `ArtistsCollectionView`: the Artists grid. Each cell asks for its Deezer
 * picture as it mounts (gated on `autoFetchArtistImages` in Rust).
 */
export function ArtistsCollectionView({ search }: { search: string }) {
  const all = useArtists((s) => s.artists);
  const push = useUI((s) => s.navigateToArtist);
  const artists = useMemo(() => {
    if (!search) return all;
    const q = search.toLowerCase();
    return all.filter((a) => a.displayName.toLowerCase().includes(q));
  }, [all, search]);

  return (
    <div className="collection-scroll">
      <div className="collection-scroll__inner">
        <div className="album-grid">
          {artists.map((a, i) => (
            <RiseFadeIn key={a.id} index={i} id={"artist:" + a.id} onClick={() => push(a.id)}>
              <EnsureImage id={a.id} name={a.displayName} />
              <ArtistGridCell artist={a} />
            </RiseFadeIn>
          ))}
        </div>
      </div>
    </div>
  );
}

/** `.task(id: summary.id) { ensureImage }` */
function EnsureImage({ id, name }: { id: string; name: string }) {
  const asked = useRef<string | null>(null);
  useEffect(() => {
    if (asked.current === id) return;
    asked.current = id;
    void api.ensureArtistImage(id, name);
  }, [id, name]);
  return null;
}
