import { useEffect, useRef, useState } from "react";
import { playbackTimeNow, usePlayer } from "../../app/player";
import { api, artworkUrl, on, type LyricLine } from "../../lib/api";
import { ArtworkView } from "../../components/ArtworkView";
import { linksFor } from "../artist/ArtistLink";
import { FilmGrain, MarqueeText, PlaybackProgressBar, TechSpecLabel } from "./Chrome";

type LyricsState =
  | { state: "mix" | "disabled" | "loading" | "notFound" }
  | { state: "ready"; lines: LyricLine[]; isSynced: boolean };

/**
 * `LyricsVisualizerView`: a baked, blurred and tinted artist photo behind
 * karaoke lyrics, with the track details and progress along the bottom.
 */
export function LyricsMode() {
  const track = usePlayer((s) => s.currentTrack);
  const queue = usePlayer((s) => s.queue);
  const cur = usePlayer((s) => s.currentIndex);
  const [lyrics, setLyrics] = useState<LyricsState>({ state: "loading" });
  const trackId = track?.id ?? null;

  // Fetch (and re-read when a lookup lands); warm the neighbours.
  useEffect(() => {
    if (!trackId) return;
    const neighbours = [-1, 1, 2].map((d) => queue[cur + d]?.track).filter((t) => t && !t.isMixCompilation).map((t) => t!.id);
    let live = true;
    const load = () => void api.visualizerLyrics(trackId, neighbours).then((s) => live && s && setLyrics(s as LyricsState));
    setLyrics({ state: "loading" });
    load();
    // `ensureArtistImage`: the lead artist's photo becomes the backdrop.
    const lead = track ? linksFor(track)?.[0] : undefined;
    if (lead) void api.ensureArtistImage(lead.key, lead.name);
    const off = on("lyrics://changed", load);
    return () => {
      live = false;
      off();
    };
  }, [trackId]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="vis-stage lyrics-mode">
      <Backdrop trackId={trackId} />
      <div className="lyrics-mode__content">
        <div className="lyrics-mode__lyrics">
          {!track ? null : lyrics.state === "ready" ? (
            <LyricsScroller lines={lyrics.lines} />
          ) : lyrics.state === "loading" ? (
            <span className="spinner spinner--light" />
          ) : (
            <div className="lyrics-mode__message">
              {lyrics.state === "mix"
                ? "Lyrics aren't available for mix compilations"
                : lyrics.state === "disabled"
                  ? "Lyrics lookup disabled in Settings"
                  : "Unable to find lyrics for this song"}
            </div>
          )}
        </div>
        <div className="lyrics-mode__bottom">
          <div className="lyrics-mode__footer">
            <ArtworkView artwork={track?.artwork} size={96} />
            <div className="lyrics-mode__details">
              <MarqueeText text={track?.title ?? "—"} style={{ fontSize: 28, fontWeight: 700, color: "white" }} />
              <MarqueeText text={track?.artistDisplay ?? "Unknown Artist"} style={{ fontSize: 16, color: "rgb(255 255 255 / 0.85)" }} />
              <TechSpecLabel track={track} color="rgb(255 255 255 / 0.5)" />
            </div>
          </div>
          <div className="lyrics-mode__progress">
            <PlaybackProgressBar
              showsLabels
              track="rgb(255 255 255 / 0.25)"
              fill="white"
              glow={false}
              labelColor="rgb(255 255 255 / 0.85)"
            />
          </div>
        </div>
      </div>
    </div>
  );
}

/** Two layers: the old backdrop stays under while the new one fades in (0.6 s). */
function Backdrop({ trackId }: { trackId: string | null }) {
  const [layers, setLayers] = useState<{ prev: string | null; cur: string | null; key: string; n: number }>({
    prev: null,
    cur: null,
    key: "",
    n: 0,
  });
  useEffect(() => {
    if (!trackId) return;
    let live = true;
    const load = () =>
      void api.visualizerBackdrop(trackId).then((b) => {
        if (!live || !b) return;
        setLayers((l) => (l.key === b.key && l.cur === b.image ? l : { prev: l.cur, cur: b.image, key: b.key, n: l.n + 1 }));
      });
    load();
    // A freshly fetched artist photo replaces the album-art stand-in.
    const off = on("artists://changed", load);
    return () => {
      live = false;
      off();
    };
  }, [trackId]);

  return (
    <div className="lyrics-backdrop">
      {layers.prev && <img className="lyrics-backdrop__img" src={artworkUrl(layers.prev, 0)} alt="" />}
      {layers.cur && (
        <img
          key={layers.n}
          className={"lyrics-backdrop__img" + (layers.n > 1 ? " is-fading-in" : "")}
          src={artworkUrl(layers.cur, 0)}
          alt=""
        />
      )}
      <div className="lyrics-backdrop__shade" />
      <FilmGrain />
    </div>
  );
}

/** `Lyrics.currentLineIndex`: the last line whose timestamp ≤ t. */
function lineIndex(lines: LyricLine[], t: number): number {
  let idx = 0;
  for (let i = 0; i < lines.length; i++) {
    const ts = lines[i].timestamp;
    if (ts == null) continue;
    if (ts <= t) idx = i;
    else break;
  }
  return idx;
}

const WINDOW = 4;

/** Per-line falloff (`LyricsScrollerView.metrics`). */
function metrics(d: number) {
  const ad = Math.abs(d);
  if (ad === 0) return { y: -8, scale: 1, opacity: 1, blur: 0 };
  const step = ad - 1;
  return {
    y: d * 74 - 8,
    scale: Math.max(0.46, 0.58 - step * 0.03),
    opacity: Math.max(0, 0.5 - step * 0.13),
    blur: Math.min(3.5, 0.6 + step * 0.8),
  };
}

/**
 * `LyricsScrollerView`: the active line large and sharp, the others shrinking,
 * fading and blurring with distance; the stack slides as time advances.
 */
function LyricsScroller({ lines }: { lines: LyricLine[] }) {
  const [idx, setIdx] = useState(() => lineIndex(lines, playbackTimeNow()));
  const idxRef = useRef(idx);
  useEffect(() => {
    // 10 Hz while playing, like the Mac's TimelineView.
    const iv = setInterval(() => {
      const i = lineIndex(lines, playbackTimeNow());
      if (i !== idxRef.current) {
        idxRef.current = i;
        setIdx(i);
      }
    }, 100);
    return () => clearInterval(iv);
  }, [lines]);

  const lo = Math.max(0, idx - WINDOW);
  const hi = Math.min(lines.length - 1, idx + WINDOW);
  const visible: number[] = [];
  for (let i = lo; i <= hi; i++) visible.push(i);

  return (
    <div className="lyrics-scroller">
      {visible.map((i) => {
        const d = i - idx;
        const m = metrics(d);
        return (
          <div
            key={i}
            className={"lyrics-line" + (d === 0 ? " is-active" : "")}
            style={{
              transform: `translateY(calc(-50% + ${m.y}px)) scale(${m.scale})`,
              opacity: m.opacity,
              filter: m.blur ? `blur(${m.blur}px)` : undefined,
            }}
          >
            {lines[i].text || " "}
          </div>
        );
      })}
    </div>
  );
}
