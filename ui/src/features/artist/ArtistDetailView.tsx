import { ChevronLeft, Pencil, Play, Shuffle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useArtists } from "../../app/artists";
import { albumTracks, useLibrary } from "../../app/library";
import { player } from "../../app/player";
import { useUI } from "../../app/store";
import { api, artworkUrl, type Album, type ArtistDetail } from "../../lib/api";
import { RiseFadeIn } from "../../components/RiseFadeIn";
import { Modal } from "../../components/sheet/Sheet";
import { AlbumCard } from "../collection/AlbumCard";
import { ArtistEditorView } from "./ArtistEditorView";
import "./Artist.css";

const BANNER_HEIGHT = 320;

/** `ArtistDetailView`: banner, then Albums / Singles & EPs / Appears On. */
export function ArtistDetailView({ artistKey }: { artistKey: string }) {
  const revision = useArtists((s) => s.revision);
  const albumsById = useLibrary((s) => s.albumsById);
  const goBack = useUI((s) => s.goBackInCollection);
  const [detail, setDetail] = useState<ArtistDetail | null | undefined>(undefined);
  const [editing, setEditing] = useState(false);
  const closeEditor = useCallback(() => setEditing(false), []);

  useEffect(() => {
    let live = true;
    void api.artistDetail(artistKey).then((d) => live && setDetail(d ?? null));
    return () => {
      live = false;
    };
  }, [artistKey, revision]);

  const name = detail?.artist.displayName;
  useEffect(() => {
    if (name) void api.ensureArtistImage(artistKey, name);
  }, [artistKey, name]);

  if (detail === undefined) return <div className="artist-page" />;
  if (detail === null) return <div className="not-found">Artist not found</div>;

  const a = detail.artist;
  const pick = (ids: string[]) => ids.map((id) => albumsById.get(id)).filter((x): x is Album => !!x);
  const albums = pick(a.albumIds);
  const singles = pick(a.singleIds);
  const appearsOn = pick(a.appearsOnIds);

  const playAll = (shuffle: boolean) => {
    const tracks = [...albums, ...singles, ...appearsOn].flatMap(albumTracks);
    if (!tracks.length) return;
    const start = shuffle ? Math.floor(Math.random() * tracks.length) : 0;
    void player.play(tracks, start, a.displayName, shuffle);
  };

  const releases = a.albumIds.length + a.singleIds.length;
  const parts: string[] = [];
  if (releases > 0) parts.push(`${releases} release${releases === 1 ? "" : "s"}`);
  if (a.appearsOnIds.length) parts.push(`${a.appearsOnIds.length} appearance${a.appearsOnIds.length === 1 ? "" : "s"}`);
  parts.push(`${a.trackCount} track${a.trackCount === 1 ? "" : "s"}`);

  // `dominantColor ?? surfaceElevated`, faded 0 → 0.4 → background.
  const rgb = detail.baseColor ? detail.baseColor.join(" ") : null;
  const tint = (alpha: number) =>
    rgb ? `rgb(${rgb} / ${alpha})` : `color-mix(in srgb, var(--surface-elevated) ${alpha * 100}%, transparent)`;

  return (
    <div className="artist-page">
      <div className="artist-page__scroll">
        <div className="artist-banner" style={{ height: BANNER_HEIGHT }}>
          {detail.banner ? (
            <img
              className={"artist-banner__image" + (detail.bannerIsTrue ? "" : " is-blurred")}
              src={artworkUrl(detail.banner, 0)}
              alt=""
              draggable={false}
            />
          ) : (
            <div className="artist-banner__image" style={{ background: rgb ? `rgb(${rgb})` : "var(--surface-elevated)" }} />
          )}
          <div
            className="artist-banner__fade"
            style={{ background: `linear-gradient(to bottom, ${tint(0)}, ${tint(0.4)}, var(--background))` }}
          />
          <div className="artist-banner__content">
            <div className="artist-banner__text">
              <div className="artist-banner__eyebrow">ARTIST</div>
              <h1 className="artist-banner__name">{a.displayName}</h1>
              <div className="artist-banner__subtitle">{parts.join(" · ")}</div>
              <div className="artist-banner__actions">
                <button className="pill-btn is-primary" onClick={() => playAll(false)}>
                  <Play size={11} fill="currentColor" /> Play
                </button>
                <button className="pill-btn" onClick={() => playAll(true)}>
                  <Shuffle size={12} /> Shuffle
                </button>
              </div>
            </div>
            <button className="pill-btn" onClick={() => setEditing(true)}>
              <Pencil size={12} /> Edit
            </button>
          </div>
        </div>
        <Section title="Albums" albums={albums} />
        <Section title="Singles & EPs" albums={singles} />
        <Section title="Appears On" albums={appearsOn} />
        <div style={{ height: 100, flex: "none" }} />
      </div>
      <button className="detail-back-button" onClick={goBack} title="Back">
        <ChevronLeft size={15} strokeWidth={2.6} />
      </button>
      <Modal open={editing} onClose={closeEditor}>
        <ArtistEditorView artistKey={a.id} fallbackName={a.displayName} fallbackArtwork={a.artworkSample} onClose={closeEditor} />
      </Modal>
    </div>
  );
}

function Section({ title, albums }: { title: string; albums: Album[] }) {
  const push = useUI((s) => s.pushCollection);
  if (!albums.length) return null;
  return (
    <section className="artist-section">
      <div className="artist-section__title">{title}</div>
      <div className="artist-section__grid">
        {albums.map((al, i) => (
          <RiseFadeIn key={al.id} index={i} onClick={() => push(al.id)}>
            <AlbumCard album={al} />
          </RiseFadeIn>
        ))}
      </div>
    </section>
  );
}
