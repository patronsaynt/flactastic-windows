/**
 * `VisualizerChrome` + `MarqueeText`: tech-spec caption, glowing progress
 * bar, time readout, film grain, and the sliding single-line marquee.
 */
import { useLayoutEffect, useRef, type CSSProperties } from "react";
import { usePlaybackTime, usePlayer } from "../../app/player";
import type { Track } from "../../lib/api";
import { formatDuration, techSpec } from "../../lib/format";

/** `FLAC · 24-BIT / 96 kHz`, or nothing. */
export function TechSpecLabel({ track, color }: { track: Track | null; color?: string }) {
  const spec = techSpec(track);
  if (!spec) return null;
  return (
    <div className="tech-spec" style={color ? { color } : undefined}>
      {spec}
    </div>
  );
}

/** `GlowProgressBar` driven by the playback clock (`PlaybackProgressBar`). */
export function PlaybackProgressBar({
  showsLabels = false,
  track = "var(--divider)",
  fill = "var(--accent)",
  glow = true,
  labelColor,
}: {
  showsLabels?: boolean;
  track?: string;
  fill?: string;
  glow?: boolean;
  labelColor?: string;
}) {
  const t = usePlaybackTime();
  const total = usePlayer((s) => s.duration) ?? 0;
  const f = total > 0 ? Math.max(0, Math.min(1, t / total)) : 0;
  return (
    <div className="vis-progress">
      {showsLabels && (
        <span className="vis-time" style={labelColor ? { color: labelColor } : undefined}>
          {formatDuration(t)}
        </span>
      )}
      <div className="vis-progress__track" style={{ background: track }}>
        <div
          className="vis-progress__fill"
          style={{
            width: `${f * 100}%`,
            background: fill,
            boxShadow: glow ? `0 0 10px color-mix(in srgb, ${fill} 45%, transparent)` : undefined,
          }}
        />
      </div>
      {showsLabels && (
        <span className="vis-time" style={labelColor ? { color: labelColor } : undefined}>
          {formatDuration(total > 0 ? total : null)}
        </span>
      )}
    </div>
  );
}

/** `PlaybackTimeReadout`: `0:42 / 3:15` or just the elapsed time. */
export function PlaybackTimeReadout({ showsTotal = true, size = 13 }: { showsTotal?: boolean; size?: number }) {
  const t = usePlaybackTime();
  const total = usePlayer((s) => s.duration) ?? 0;
  return (
    <span className="vis-time" style={{ fontSize: size }}>
      {showsTotal ? `${formatDuration(t)} / ${formatDuration(total > 0 ? total : null)}` : formatDuration(t)}
    </span>
  );
}

let grainTile: string | null = null;

/** 128² of mid-grey noise, generated once (`VisualizerFilmGrain`). */
function grain(): string {
  if (grainTile) return grainTile;
  const c = document.createElement("canvas");
  c.width = c.height = 128;
  const ctx = c.getContext("2d")!;
  const img = ctx.createImageData(128, 128);
  for (let i = 0; i < img.data.length; i += 4) {
    const v = 96 + Math.floor(Math.random() * 65);
    img.data[i] = img.data[i + 1] = img.data[i + 2] = v;
    img.data[i + 3] = 255;
  }
  ctx.putImageData(img, 0, 0);
  grainTile = c.toDataURL();
  return grainTile;
}

export function FilmGrain({ opacity = 0.1 }: { opacity?: number }) {
  return <div className="film-grain" style={{ backgroundImage: `url(${grain()})`, opacity }} />;
}

/**
 * `MarqueeText`: one line that, when it overflows, slides to its end and
 * back (30 pt/s, 1 s pauses); otherwise static with the given alignment.
 */
export function MarqueeText({
  text,
  style,
  align = "left",
  speed = 30,
  pause = 1,
}: {
  text: string;
  style?: CSSProperties;
  align?: "left" | "center";
  speed?: number;
  pause?: number;
}) {
  const box = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLSpanElement>(null);

  useLayoutEffect(() => {
    const b = box.current;
    const s = inner.current;
    if (!b || !s) return;
    let anim: Animation | null = null;
    const restart = () => {
      anim?.cancel();
      anim = null;
      const overflow = s.scrollWidth - b.clientWidth;
      b.dataset.overflowing = overflow > 0.5 ? "1" : "0";
      if (overflow <= 0.5) return;
      const distance = overflow + 16;
      const scroll = Math.max(0.5, distance / speed);
      const total = 2 * (pause + scroll);
      anim = s.animate(
        [
          { transform: "translateX(0)", offset: 0 },
          { transform: "translateX(0)", offset: pause / total },
          { transform: `translateX(${-distance}px)`, offset: (pause + scroll) / total },
          { transform: `translateX(${-distance}px)`, offset: (2 * pause + scroll) / total },
          { transform: "translateX(0)", offset: 1 },
        ],
        { duration: total * 1000, iterations: Infinity, easing: "linear" },
      );
    };
    restart();
    const ro = new ResizeObserver(restart);
    ro.observe(b);
    return () => {
      ro.disconnect();
      anim?.cancel();
    };
  }, [text, speed, pause]);

  return (
    <div ref={box} className={"marquee" + (align === "center" ? " is-center" : "")} style={style}>
      <span ref={inner} className="marquee__text">
        {text}
      </span>
    </div>
  );
}
