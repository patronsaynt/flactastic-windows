import { useCallback, useEffect, useRef, useState } from "react";
import { MODES, wheelLabel, type VisualizerMode } from "./modes";

const PITCH = 46;
const PANEL_WIDTH = 340;
const HOT_STRIP = 18;

/** Per-row falloff, verbatim from the prototype's `renderVals()`. */
function rowMetrics(d: number) {
  const ad = Math.abs(d);
  return {
    x: -Math.min(30, ad * ad * 3),
    y: d * PITCH,
    scale: Math.max(0.74, 1 - ad * 0.055),
    opacity: Math.max(0.1, 1 - ad * 0.19),
    blur: Math.min(2.4, ad * 0.45),
  };
}

/** `SPARKS`: position %, dot size, glow radius, cycle and delay. */
const SPARKS: [number, number, number, number, number, number][] = [
  [3, 68, 2.5, 9, 6.2, 0.0], [9, 82, 1.5, 6, 7.8, 0.9], [16, 58, 2.0, 8, 5.6, 1.7], [24, 74, 1.5, 5, 8.4, 0.4],
  [32, 44, 1.8, 7, 6.9, 2.6], [40, 80, 2.2, 9, 7.2, 1.2], [48, 30, 1.4, 5, 9.1, 3.4], [55, 88, 2.6, 11, 5.9, 2.1],
  [63, 62, 1.6, 6, 8.8, 0.6], [71, 22, 2.0, 8, 7.4, 4.2], [78, 52, 1.5, 5, 6.6, 3.0], [85, 18, 1.8, 7, 9.6, 1.5],
  [91, 46, 1.4, 5, 8.2, 2.9], [12, 92, 2.3, 10, 6.4, 3.8], [60, 12, 1.6, 6, 7.0, 1.9], [95, 76, 1.9, 8, 8.6, 0.2],
  [5, 20, 1.4, 5, 9.3, 2.4], [68, 66, 2.1, 9, 6.0, 3.1], [44, 55, 1.5, 6, 7.6, 1.0], [27, 10, 1.7, 7, 8.9, 4.5],
];

const reducedMotion = () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

/**
 * `VisualizerModeWheel`: an 18 pt hot strip on the leading edge opens a
 * 340 pt panel over a dimmed, blurred canvas with a halo and drifting
 * sparkles. Scroll, ↑/↓ and click change the mode.
 */
export function ModeWheel({ mode, onChange }: { mode: VisualizerMode; onChange: (m: VisualizerMode) => void }) {
  const [open, setOpen] = useState(false);
  const selected = Math.max(0, MODES.indexOf(mode));
  const acc = useRef(0);
  const selRef = useRef(selected);
  selRef.current = selected;

  const step = useCallback(
    (d: number) => {
      const next = Math.min(MODES.length - 1, Math.max(0, selRef.current + d));
      if (next !== selRef.current) onChange(MODES[next]);
    },
    [onChange],
  );

  // ↑/↓ while open; the wheel swallows scrolling so the canvas stays put.
  useEffect(() => {
    if (!open) return;
    acc.current = 0;
    const key = (e: KeyboardEvent) => {
      if (e.key === "ArrowUp" || e.key === "ArrowDown") {
        e.preventDefault();
        step(e.key === "ArrowUp" ? -1 : 1);
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [open, step]);

  const onWheel = (e: React.WheelEvent) => {
    acc.current += e.deltaY;
    while (Math.abs(acc.current) >= PITCH) {
      step(acc.current > 0 ? 1 : -1);
      acc.current -= Math.sign(acc.current) * PITCH;
    }
  };

  return (
    <div className={"mode-wheel" + (open ? " is-open" : "")}>
      <div className="mode-wheel__scrim" />
      <Sparkles running={open && !reducedMotion()} />
      <div
        className="mode-wheel__panel"
        style={{ width: PANEL_WIDTH }}
        onMouseLeave={() => setOpen(false)}
        onMouseEnter={() => setOpen(true)}
        onWheel={onWheel}
      >
        <div className="mode-wheel__edge" />
        <div className="mode-wheel__title">VISUALIZER</div>
        <div className="mode-wheel__rows">
          {MODES.map((m, i) => {
            const d = i - selected;
            const r = rowMetrics(d);
            const isSel = d === 0;
            return (
              <button
                key={m}
                className={"mode-wheel__row" + (isSel ? " is-selected" : "")}
                style={{
                  transform: `translate(${r.x}px, calc(-50% + ${r.y}px)) scale(${r.scale})`,
                  opacity: r.opacity,
                  filter: r.blur ? `blur(${r.blur}px)` : undefined,
                }}
                onClick={() => onChange(m)}
              >
                <span className={"mode-wheel__label" + (isSel && !reducedMotion() ? " is-shimmering" : "")}>{wheelLabel[m]}</span>
              </button>
            );
          })}
        </div>
      </div>
      {!open && <div className="mode-wheel__hot" style={{ width: HOT_STRIP }} onMouseEnter={() => setOpen(true)} />}
    </div>
  );
}

/** The halo and drifting motes, one canvas at 30 fps while open. */
function Sparkles({ running }: { running: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current!;
    let raf = 0;
    let last = 0;
    const draw = (now: number) => {
      raf = requestAnimationFrame(draw);
      if (now - last < 1000 / 30) return;
      last = now;
      const dpr = window.devicePixelRatio || 1;
      const w = c.clientWidth;
      const h = c.clientHeight;
      if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) {
        c.width = Math.round(w * dpr);
        c.height = Math.round(h * dpr);
      }
      const ctx = c.getContext("2d")!;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      const mark = document.documentElement.dataset.theme === "light" ? "0,0,0" : "255,255,255";
      const t = now / 1000;

      // `fl-halo`: 4.6 s ease-in-out, opacity 0.42 ↔ 0.85, scale 1 ↔ 1.14.
      const f = (t / 4.6) % 1;
      const pp = f < 0.5 ? f * 2 : (1 - f) * 2;
      const phase = pp < 0.5 ? 2 * pp * pp : 1 - Math.pow(-2 * pp + 2, 2) / 2;
      const haloOpacity = 0.42 + (0.85 - 0.42) * phase;
      const diameter = 420 * (1 + 0.14 * phase);
      const hx = w * 0.06;
      const hy = h * 0.5;
      const hg = ctx.createRadialGradient(hx, hy, 0, hx, hy, diameter * 0.34);
      hg.addColorStop(0, `rgba(${mark},${(0.14 * haloOpacity) / 0.85})`);
      hg.addColorStop(1, `rgba(${mark},0)`);
      ctx.fillStyle = hg;
      ctx.beginPath();
      ctx.arc(hx, hy, diameter / 2, 0, Math.PI * 2);
      ctx.fill();

      // `fl-drift`: rise (26, −120), scale 0.6 → 1.1, opacity 0 → 1 → 0.7 → 0.
      for (const [left, top, size, glow, dur, delay] of SPARKS) {
        const p = ((t + delay) % dur) / dur;
        const op = p < 0.18 ? p / 0.18 : p < 0.7 ? 1 - ((p - 0.18) / 0.52) * 0.3 : 0.7 * (1 - (p - 0.7) / 0.3);
        if (op <= 0.01) continue;
        const x = (w * left) / 100 + 26 * p;
        const y = (h * top) / 100 - 120 * p;
        const r = (size / 2) * (0.6 + 0.5 * p);
        const g = ctx.createRadialGradient(x, y, 0, x, y, glow);
        g.addColorStop(0, `rgba(${mark},${0.55 * op})`);
        g.addColorStop(1, `rgba(${mark},0)`);
        ctx.fillStyle = g;
        ctx.beginPath();
        ctx.arc(x, y, glow, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = `rgba(${mark},${op})`;
        ctx.beginPath();
        ctx.arc(x, y, r, 0, Math.PI * 2);
        ctx.fill();
      }
    };
    if (running) raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, [running]);
  return <canvas ref={ref} className="mode-wheel__sparkles" />;
}
