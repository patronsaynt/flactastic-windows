/**
 * `SpectrumAnalyzer`'s UI half. The backend analyzer publishes a 64-bin
 * target about ten times a second; a 60 Hz loop eases the displayed bars
 * toward it (60% attack, 3% decay per frame) so they move fluidly.
 *
 * Magnitudes live in a shared Float32Array read by the canvases' own
 * animation loops, never in React state.
 */
import { useEffect } from "react";
import { api, on } from "../../lib/api";

export const BIN_COUNT = 64;

export const spectrum = {
  magnitudes: new Float32Array(BIN_COUNT),
  target: new Float32Array(BIN_COUNT),
  /** Incremented each display tick, so consumers can skip unchanged frames. */
  frame: 0,
};

let users = 0;
let raf = 0;
let offFrames: (() => void) | null = null;
let last = 0;

function tick(now: number) {
  raf = requestAnimationFrame(tick);
  // Smooth at 60 Hz regardless of the display's refresh rate.
  if (now - last < 1000 / 60 - 1) return;
  last = now;
  const m = spectrum.magnitudes;
  const t = spectrum.target;
  // Settled (silent long enough that bars and the spectrogram's visible
  // history are blank): stop publishing frames so nothing redraws.
  let silent = true;
  for (let i = 0; i < BIN_COUNT; i++) {
    if (t[i] >= IDLE_EPSILON || m[i] >= IDLE_EPSILON) {
      silent = false;
      break;
    }
  }
  idleFrames = silent ? idleFrames + 1 : 0;
  if (idleFrames > HISTORY_FRAMES) return;
  for (let i = 0; i < BIN_COUNT; i++) {
    if (t[i] > m[i]) m[i] += (t[i] - m[i]) * 0.6;
    else m[i] *= 0.97;
  }
  spectrum.frame++;
}

const IDLE_EPSILON = 0.001;
/** Frames for a silent column to cross the widest spectrogram. */
const HISTORY_FRAMES = 1600;
let idleFrames = 0;

/** `attach(to:)` / `detach()`, reference-counted across spectrum modes. */
export function useSpectrumFeed(active: boolean) {
  useEffect(() => {
    if (!active) return;
    if (users++ === 0) {
      void api.setSpectrum(true);
      offFrames = on<number[]>("spectrum://frame", (f) => spectrum.target.set(f.slice(0, BIN_COUNT)));
      raf = requestAnimationFrame(tick);
    }
    return () => {
      if (--users === 0) {
        void api.setSpectrum(false);
        offFrames?.();
        offFrames = null;
        cancelAnimationFrame(raf);
        spectrum.target.fill(0);
        spectrum.magnitudes.fill(0);
      }
    };
  }, [active]);
}
