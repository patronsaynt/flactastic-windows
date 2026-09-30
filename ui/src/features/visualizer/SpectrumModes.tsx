import { useEffect, useRef } from "react";
import { usePlayer } from "../../app/player";
import type { Track } from "../../lib/api";
import { ArtworkView } from "../../components/ArtworkView";
import { MarqueeText, PlaybackTimeReadout, TechSpecLabel } from "./Chrome";
import type { VisualizerMode } from "./modes";
import { BIN_COUNT, spectrum, useSpectrumFeed } from "./spectrum";

const artistOf = (t: Track | null) => t?.artistDisplay ?? "Unknown Artist";

/** The theme's mark colour (white on dark, black on light) as "r,g,b". */
function markRGB(): string {
  return document.documentElement.dataset.theme === "light" ? "0,0,0" : "255,255,255";
}

/**
 * A canvas redrawn on every analyzer display tick. `draw` gets a 2D context
 * already scaled to CSS pixels.
 */
function useCanvasLoop(draw: (ctx: CanvasRenderingContext2D, w: number, h: number) => void, everyFrame = false) {
  const ref = useRef<HTMLCanvasElement>(null);
  const drawRef = useRef(draw);
  drawRef.current = draw;
  useEffect(() => {
    const c = ref.current!;
    let raf = 0;
    let seen = -1;
    let dims = "";
    const loop = () => {
      raf = requestAnimationFrame(loop);
      const dpr = window.devicePixelRatio || 1;
      const w = c.clientWidth;
      const h = c.clientHeight;
      const d = `${w}x${h}@${dpr}`;
      if (d !== dims) {
        dims = d;
        c.width = Math.max(1, Math.round(w * dpr));
        c.height = Math.max(1, Math.round(h * dpr));
        seen = -1;
      }
      if (!everyFrame && spectrum.frame === seen) return;
      seen = spectrum.frame;
      const ctx = c.getContext("2d")!;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      drawRef.current(ctx, w, h);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [everyFrame]);
  return ref;
}

export function SpectrumMode({ mode }: { mode: VisualizerMode }) {
  useSpectrumFeed(true);
  if (mode === "spectrumRadial") return <RadialLayout />;
  if (mode === "spectrumHorizontal") return <HorizontalLayout />;
  return <SpectrogramLayout />;
}

// MARK: - Radial: details left, radial spectrum right.

function RadialLayout() {
  const track = usePlayer((s) => s.currentTrack);
  const album = track?.album?.trim();
  return (
    <div className="vis-stage vis-split">
      <div className="vis-split__half">
        <div className="radial-details">
          <ArtworkView artwork={track?.artwork} size={140} />
          <div className="radial-details__text">
            <MarqueeText text={track?.title ?? "—"} style={{ fontSize: 30, fontWeight: 700, color: "var(--text-primary)" }} />
            <MarqueeText text={artistOf(track)} style={{ fontSize: 17, fontWeight: 600, color: "var(--text-secondary)" }} />
            {album && <MarqueeText text={album} style={{ fontSize: 13, color: "var(--text-tertiary)" }} />}
            <div style={{ paddingTop: "var(--space-sm)" }}>
              <TechSpecLabel track={track} />
            </div>
          </div>
        </div>
      </div>
      <div className="vis-split__half" style={{ padding: "var(--space-xl)" }}>
        <RadialSpectrum />
      </div>
    </div>
  );
}

/** Mirrored spokes around a ring, between two faint guide rings. */
function RadialSpectrum() {
  const ref = useCanvasLoop((ctx, w, h) => {
    ctx.clearRect(0, 0, w, h);
    const m = spectrum.magnitudes;
    const mark = markRGB();
    const minSide = Math.min(w, h);
    const cx = w / 2;
    const cy = h / 2;
    const inner = minSide * 0.24;
    const maxBar = minSide * 0.13;
    ctx.lineWidth = 1;
    ctx.strokeStyle = `rgba(${mark},0.10)`;
    for (const r of [inner - 10, inner + maxBar + 14]) {
      ctx.beginPath();
      ctx.arc(cx, cy, Math.max(0, r), 0, Math.PI * 2);
      ctx.stroke();
    }
    const spokes = BIN_COUNT * 2;
    ctx.lineWidth = 2;
    ctx.lineCap = "round";
    for (let i = 0; i < spokes; i++) {
      const mag = m[i < BIN_COUNT ? i : spokes - 1 - i];
      const a = (i / spokes) * Math.PI * 2 - Math.PI / 2;
      const len = inner + maxBar * mag;
      ctx.strokeStyle = `rgba(${mark},${0.35 + mag * 0.6})`;
      ctx.beginPath();
      ctx.moveTo(cx + Math.cos(a) * inner, cy + Math.sin(a) * inner);
      ctx.lineTo(cx + Math.cos(a) * len, cy + Math.sin(a) * len);
      ctx.stroke();
    }
  });
  return <canvas ref={ref} className="vis-canvas" />;
}

// MARK: - Horizontal: spectrum above, details below.

function HorizontalLayout() {
  const track = usePlayer((s) => s.currentTrack);
  const album = track?.album?.trim();
  return (
    <div className="vis-stage vis-center">
      <div className="horizontal-layout">
        <div style={{ height: 200 }}>
          <HorizontalSpectrum />
        </div>
        <div className="horizontal-details">
          <ArtworkView artwork={track?.artwork} size={96} />
          <div className="horizontal-details__text">
            <MarqueeText text={track?.title ?? "—"} style={{ fontSize: 22, fontWeight: 600, color: "var(--text-primary)" }} />
            <MarqueeText text={artistOf(track)} style={{ fontSize: 13, fontWeight: 500, color: "var(--text-secondary)" }} />
            {album && <MarqueeText text={album} style={{ fontSize: 11, color: "var(--text-tertiary)" }} />}
            <div style={{ paddingTop: "var(--space-sm)" }}>
              <TechSpecLabel track={track} />
            </div>
          </div>
          <div style={{ flex: 1 }} />
          <PlaybackTimeReadout />
        </div>
      </div>
    </div>
  );
}

const REFLECTION = 28;

/** Rounded bars on a baseline, with short reflection stubs below it. */
function HorizontalSpectrum() {
  const ref = useCanvasLoop((ctx, w, h) => {
    ctx.clearRect(0, 0, w, h);
    const m = spectrum.magnitudes;
    const mark = markRGB();
    const gap = 4;
    const bw = Math.max(2, (w - gap * (BIN_COUNT - 1)) / BIN_COUNT);
    const base = h - REFLECTION;
    for (let i = 0; i < BIN_COUNT; i++) {
      const mag = m[i];
      const bh = Math.max(2, mag * base);
      const x = i * (bw + gap);
      ctx.fillStyle = `rgba(${mark},${0.42 + mag * 0.53})`;
      ctx.beginPath();
      ctx.roundRect(x, base - bh, bw, bh, bw / 2);
      ctx.fill();
      ctx.fillStyle = `rgba(${mark},0.14)`;
      ctx.beginPath();
      ctx.roundRect(x, base, bw, Math.min(REFLECTION, bh * 0.35), bw / 2);
      ctx.fill();
    }
    ctx.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue("--divider") || `rgba(${mark},0.1)`;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, base + 0.5);
    ctx.lineTo(w, base + 0.5);
    ctx.stroke();
  });
  return <canvas ref={ref} className="vis-canvas" />;
}

// MARK: - Spectrogram: full-canvas stipple with corner chrome.

function SpectrogramLayout() {
  const track = usePlayer((s) => s.currentTrack);
  return (
    <div className="vis-stage">
      <SpectrogramStipple />
      <div className="spectrogram-fade" />
      <div className="spectrogram-caption">
        <span className="spectrogram-caption__title">
          {track?.title ?? "—"} — {artistOf(track)}
        </span>
        <span className="vis-rule" />
        <PlaybackTimeReadout showsTotal={false} size={11} />
      </div>
    </div>
  );
}

const STEP = 1.6;

/**
 * Scrolling stipple. History lives in an offscreen canvas used as a ring
 * buffer: each tick stipples one column at the write head, and display
 * draws the buffer twice so the newest column sits at the right edge.
 */
function SpectrogramStipple() {
  const buf = useRef<{ canvas: HTMLCanvasElement; w: number; h: number; dpr: number; light: boolean; head: number } | null>(null);
  const ref = useCanvasLoop((ctx, w, h) => {
    const dpr = window.devicePixelRatio || 1;
    const light = document.documentElement.dataset.theme === "light";
    let b = buf.current;
    if (!b || b.w !== w || b.h !== h || b.dpr !== dpr || b.light !== light) {
      const canvas = document.createElement("canvas");
      canvas.width = Math.max(1, Math.round(w * dpr));
      canvas.height = Math.max(1, Math.round(h * dpr));
      b = buf.current = { canvas, w, h, dpr, light, head: 0 };
    }
    const bctx = b.canvas.getContext("2d")!;
    bctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    b.head += STEP;
    if (b.head >= w) b.head -= w;
    bctx.clearRect(b.head, 0, STEP, h);
    const over = b.head + STEP - w;
    if (over > 0) bctx.clearRect(0, 0, over, h);

    const m = spectrum.magnitudes;
    const rowH = h / BIN_COUNT;
    const mark = light ? "0,0,0" : "255,255,255";
    for (let i = 0; i < BIN_COUNT; i++) {
      const mag = m[i];
      if (mag < 0.05) continue;
      // Bin 0 (lowest) at the bottom; squeezed and inset clear of the chrome.
      const bandBottom = h - ((i + 1) * rowH * 0.94 + 14);
      const r = Math.max(0.6, mag * 1.6);
      bctx.fillStyle = `rgba(${mark},${0.12 + mag * 0.6})`;
      const dots = 1 + Math.round(mag * 4);
      for (let d = 0; d < dots; d++) {
        let x = b.head + Math.random() * STEP;
        if (x >= w) x -= w;
        const y = bandBottom + Math.random() * rowH;
        bctx.beginPath();
        bctx.arc(x, y, r, 0, Math.PI * 2);
        bctx.fill();
      }
    }

    ctx.clearRect(0, 0, w, h);
    const dx = w - (b.head + STEP);
    ctx.drawImage(b.canvas, dx, 0, w, h);
    ctx.drawImage(b.canvas, dx - w, 0, w, h);
  });
  return <canvas ref={ref} className="vis-canvas" />;
}
