import { usePlayer } from "../../app/player";
import type { Track } from "../../lib/api";
import { ArtworkView } from "../../components/ArtworkView";
import { MarqueeText, PlaybackProgressBar, TechSpecLabel } from "./Chrome";
import type { VisualizerMode } from "./modes";
import { useStageSize } from "./useStageSize";

const artistOf = (t: Track | null) => t?.artistDisplay ?? "Unknown Artist";
const albumOf = (t: Track | null) => t?.album?.trim() || null;

/** `AlbumArtVisualizerView` */
export function AlbumArtMode({ mode }: { mode: VisualizerMode }) {
  const [ref, size] = useStageSize();
  return (
    <div ref={ref} className="vis-stage">
      {size.w > 0 && mode === "albumArtLarge" && <LargeArt w={size.w} h={size.h} />}
      {size.w > 0 && mode === "albumArtLargeDetails" && <LargeArtDetails w={size.w} h={size.h} />}
      {size.w > 0 && mode === "albumArtSmallDetails" && <SmallArtDetails h={size.h} />}
      {size.w > 0 && mode === "albumArtWheel" && <CoverWheel w={size.w} h={size.h} />}
    </div>
  );
}

/** The artwork breathes (0.985 ↔ 1) while playing. */
function BreathingArt({ track, size }: { track: Track | null; size: number }) {
  const playing = usePlayer((s) => s.isPlaying);
  return (
    <div className={"breathing" + (playing ? " is-playing" : "")}>
      <ArtworkView artwork={track?.artwork} size={size} />
    </div>
  );
}

/** Artwork alone, in a soft halo: 46% of the height, 320 floor. */
function LargeArt({ w, h }: { w: number; h: number }) {
  const track = usePlayer((s) => s.currentTrack);
  const size = Math.min(Math.max(h * 0.46, 320), w * 0.9, h * 0.9);
  const halo = size * 1.28;
  return (
    <div className="vis-center">
      <div
        className="vis-halo"
        style={{
          width: halo,
          height: halo,
          background: `radial-gradient(circle closest-side, color-mix(in srgb, var(--vis-mark) 10%, transparent), transparent 68%)`,
        }}
      />
      <BreathingArt track={track} size={size} />
    </div>
  );
}

function CenteredDetails({ track }: { track: Track | null }) {
  const album = albumOf(track);
  return (
    <div className="vis-centered-details">
      <MarqueeText text={track?.title ?? "—"} align="center" style={{ fontSize: 17, fontWeight: 600, color: "var(--text-primary)" }} />
      <MarqueeText text={artistOf(track)} align="center" style={{ fontSize: 13, color: "var(--text-secondary)" }} />
      {album && <MarqueeText text={album} align="center" style={{ fontSize: 11, color: "var(--text-tertiary)" }} />}
      <div style={{ paddingTop: 14 }}>
        <TechSpecLabel track={track} />
      </div>
      <div style={{ paddingTop: "var(--space-md)", width: "100%", maxWidth: 360 }}>
        <PlaybackProgressBar />
      </div>
    </div>
  );
}

function LargeArtDetails({ w, h }: { w: number; h: number }) {
  const track = usePlayer((s) => s.currentTrack);
  const size = Math.min(Math.max(h * 0.4, 280), w * 0.85, h * 0.66);
  return (
    <div className="vis-center vis-column" style={{ gap: "var(--space-xl)" }}>
      <BreathingArt track={track} size={size} />
      <CenteredDetails track={track} />
    </div>
  );
}

function SmallArtDetails({ h }: { h: number }) {
  const track = usePlayer((s) => s.currentTrack);
  const album = albumOf(track);
  const art = Math.min(160, h * 0.35);
  return (
    <div className="vis-center">
      <div className="vis-banner">
        <ArtworkView artwork={track?.artwork} size={art} />
        <div className="vis-banner__text">
          <MarqueeText text={track?.title ?? "—"} style={{ fontSize: 30, fontWeight: 700, color: "var(--text-primary)" }} />
          <MarqueeText text={artistOf(track)} style={{ fontSize: 17, fontWeight: 600, color: "var(--text-secondary)" }} />
          {album && <MarqueeText text={album} style={{ fontSize: 13, color: "var(--text-tertiary)" }} />}
          <div className="vis-banner__spec">
            <span className="vis-rule" />
            <TechSpecLabel track={track} />
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * `AlbumArtWheelView`: the current cover flanked by two either side, which
 * shrink, fade, desaturate and blur with distance.
 */
function CoverWheel({ w, h }: { w: number; h: number }) {
  const queue = usePlayer((s) => s.queue);
  const cur = usePlayer((s) => s.currentIndex);
  const track = usePlayer((s) => s.currentTrack);
  const album = albumOf(track);
  const center = Math.min(300, Math.min(w, h) * 0.42);
  const spacing = center * (186 / 300);
  // Keyed by queue position identity (track id + occurrence), which stays
  // put as the current index moves, so covers slide rather than remount.
  const entries: { track: Track; rel: number; key: string }[] = [];
  for (let d = -2; d <= 2; d++) {
    const i = cur + d;
    const q = queue[i];
    if (!q) continue;
    let nth = 0;
    for (let j = 0; j < i; j++) if (queue[j].track.id === q.track.id) nth++;
    entries.push({ track: q.track, rel: d, key: `${q.track.id}#${nth}` });
  }
  return (
    <div className="vis-center vis-column" style={{ gap: 40 }}>
      <div className="cover-wheel" style={{ height: center, width: "100%" }}>
        {entries.map((e) => {
          const dist = Math.abs(e.rel);
          return (
            <div
              key={e.key}
              className="cover-wheel__item"
              style={{
                transform: `translateX(calc(-50% + ${e.rel * spacing}px)) scale(${Math.max(0.55, 1 - dist * 0.18)})`,
                opacity: Math.max(0.18, 1 - dist * 0.3),
                filter: `grayscale(${e.rel === 0 ? 0 : 0.45}) blur(${dist * 1.6}px)`,
                zIndex: 10 - dist,
              }}
            >
              <ArtworkView artwork={e.track.artwork} size={center} />
            </div>
          );
        })}
      </div>
      <div className="vis-centered-details" style={{ maxWidth: 520 }}>
        <MarqueeText text={track?.title ?? "—"} align="center" style={{ fontSize: 17, fontWeight: 600, color: "var(--text-primary)" }} />
        <MarqueeText text={artistOf(track)} align="center" style={{ fontSize: 13, color: "var(--text-secondary)" }} />
        {album && <MarqueeText text={album} align="center" style={{ fontSize: 11, color: "var(--text-tertiary)" }} />}
        <div className="queue-dots">
          {entries.map((e) => (
            <span key={e.key} className={"queue-dots__dot" + (e.rel === 0 ? " is-current" : "")} />
          ))}
        </div>
      </div>
    </div>
  );
}
