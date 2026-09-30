import { useMemo } from "react";
import { useLibrary } from "../../app/library";
import type { AudioFileFormat, Track } from "../../lib/api";
import { formatNames } from "../../lib/format";
import { ProgressBar } from "./HomeParts";

/** `LibraryFidelity` */
interface Fidelity {
  totalFiles: number;
  formats: { name: string; count: number; fraction: number }[];
  depths: { bits: number; count: number; fraction: number }[];
  rates: { rate: number; count: number; fraction: number; label: string; tier: string }[];
  losslessFraction: number;
  hiResCount: number;
  score: number;
}

const LOSSLESS: AudioFileFormat[] = ["flac", "wav", "aiff", "alac"];
const TIER_SCORE = { hiRes: 100, cd: 80, mid: 55, low: 30 } as const;

function rateLabel(rate: number) {
  const khz = rate / 1000;
  return khz === Math.round(khz) ? `${khz.toFixed(0)} kHz` : `${khz.toFixed(1)} kHz`;
}

function rateTier(rate: number) {
  if (rate >= 176400) return "Ultra Hi-Res";
  if (rate > 48000) return "Hi-Res";
  if (rate > 44100) return "Studio";
  if (rate === 44100) return "CD Quality";
  return "Standard";
}

function libraryFidelity(tracks: Track[]): Fidelity {
  const total = tracks.length;
  if (!total) return { totalFiles: 0, formats: [], depths: [], rates: [], losslessFraction: 0, hiResCount: 0, score: 0 };
  const count = <K,>(key: (t: Track) => K | null | undefined) => {
    const m = new Map<K, number>();
    for (const t of tracks) {
      const k = key(t);
      if (k != null) m.set(k, (m.get(k) ?? 0) + 1);
    }
    return m;
  };
  const formats = [...count((t) => t.fileFormat)]
    .map(([f, n]) => ({ name: formatNames[f], count: n, fraction: n / total }))
    .sort((a, b) => b.count - a.count);
  const depthCounts = count((t) => t.bitDepth);
  const depthTotal = [...depthCounts.values()].reduce((a, b) => a + b, 0);
  const depths = [...depthCounts]
    .map(([bits, n]) => ({ bits, count: n, fraction: depthTotal > 0 ? n / depthTotal : 0 }))
    .sort((a, b) => b.bits - a.bits);
  const rateCounts = count((t) => (t.sampleRate && t.sampleRate > 0 ? t.sampleRate : null));
  const maxRate = Math.max(1, ...rateCounts.values());
  const rates = [...rateCounts]
    .map(([rate, n]) => ({ rate, count: n, fraction: n / maxRate, label: rateLabel(rate), tier: rateTier(rate) }))
    .sort((a, b) => b.rate - a.rate);
  const lossless = tracks.filter((t) => LOSSLESS.includes(t.fileFormat)).length;
  const hiResCount = tracks.filter((t) => (t.bitDepth ?? 0) > 16 || (t.sampleRate ?? 0) > 48000).length;
  const score = Math.round(tracks.reduce((s, t) => s + TIER_SCORE[t.quality], 0) / total);
  return { totalFiles: total, formats, depths, rates, losslessFraction: lossless / total, hiResCount, score };
}

const STEPS = [0.32, 0.2, 0.55, 0.42, 0.3, 0.24];
const shade = (idx: number) =>
  idx === 0
    ? "var(--quality-hires)"
    : `color-mix(in srgb, var(--text-primary) ${STEPS[Math.min(Math.max(idx - 1, 0), STEPS.length - 1)] * 100}%, transparent)`;
const percent = (f: number) => `${Math.round(f * 100)}%`;

/** `FidelidexView`: format donut, bit-depth bar, sample-rate list. */
export function Fidelidex() {
  const tracks = useLibrary((s) => s.tracks);
  const f = useMemo(() => libraryFidelity(tracks), [tracks]);
  return (
    <section className="fidelidex">
      <div className="fidelidex__header">
        <div className="fidelidex__heading">
          <div className="home-section-title">FIDELIDEX</div>
          <div className="fidelidex__subtitle">Audio fidelity breakdown across your library</div>
        </div>
        <div className="fidelidex__score">
          <span className="fidelidex__score-label">SCORE</span>
          <span className="fidelidex__score-value tabular">{f.score}</span>
          <span className="fidelidex__score-max">/ 100</span>
        </div>
      </div>
      {f.totalFiles === 0 ? (
        <div className="fidelidex__empty">No audio files in your library yet.</div>
      ) : (
        <div className="fidelidex__cards">
          <div className="home-card fidelidex__card">
            <div className="home-card__title">File Format Distribution</div>
            <div className="home-card__caption" style={{ marginTop: 3 }}>
              {f.totalFiles.toLocaleString()} total files
            </div>
            <div className="fidelidex__format">
              <Donut slices={f.formats} />
              <div className="fidelidex__legend">
                {f.formats.map((s, i) => (
                  <div key={s.name} className="fidelidex__legend-row">
                    <span className="dot" style={{ background: shade(i) }} />
                    <span className="fidelidex__legend-name">{s.name}</span>
                    <span className="fidelidex__legend-pct tabular">{percent(s.fraction)}</span>
                  </div>
                ))}
              </div>
              <div style={{ flex: 1, minWidth: 12 }} />
              <div className="fidelidex__mini-stats">
                <MiniStat value={percent(f.losslessFraction)} label="Lossless" tint="var(--text-primary)" />
                <MiniStat value={f.hiResCount.toLocaleString()} label="Hi-Res files" tint="var(--quality-hires)" />
              </div>
            </div>
          </div>

          <div className="home-card fidelidex__card">
            <div className="home-card__title" style={{ marginBottom: 16 }}>
              Bit Depth
            </div>
            {f.depths.length === 0 ? (
              <div className="home-card__caption">No bit-depth metadata</div>
            ) : (
              <>
                <div className="segmented-bar">
                  {f.depths.map((d, i) => (
                    <span key={d.bits} style={{ flex: `${d.fraction} 0 0`, background: shade(i) }} />
                  ))}
                </div>
                <div className="fidelidex__depth-legend">
                  {f.depths.map((d, i) => (
                    <span key={d.bits} className="fidelidex__depth-item">
                      <span className="dot" style={{ background: shade(i) }} />
                      <span className="fidelidex__depth-name">{d.bits}-bit</span>
                      <span className="fidelidex__depth-pct tabular">{percent(d.fraction)}</span>
                    </span>
                  ))}
                </div>
              </>
            )}
          </div>

          <div className="home-card fidelidex__card">
            <div className="home-card__title" style={{ marginBottom: 16 }}>
              Sample Rates
            </div>
            <div className="fidelidex__rates">
              {f.rates.map((r, i) => (
                <div key={r.rate} className="fidelidex__rate">
                  <div className="fidelidex__rate-text">
                    <span className="fidelidex__rate-label">{r.label}</span>
                    <span className="fidelidex__rate-tier">{r.tier}</span>
                  </div>
                  <ProgressBar
                    fraction={r.fraction}
                    tint={i === 0 ? "var(--quality-hires)" : "color-mix(in srgb, var(--text-primary) 50%, transparent)"}
                  />
                  <span className="fidelidex__rate-count tabular">{r.count.toLocaleString()}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

function MiniStat({ value, label, tint }: { value: string; label: string; tint: string }) {
  return (
    <div className="mini-stat">
      <div className="mini-stat__value tabular" style={{ color: tint }}>
        {value}
      </div>
      <div className="mini-stat__label">{label.toUpperCase()}</div>
    </div>
  );
}

/** `FormatDonut`: 116 pt ring, 11 pt stroke, starting at 12 o'clock. */
function Donut({ slices }: { slices: { name: string; fraction: number }[] }) {
  const size = 116;
  const stroke = 11;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  let cursor = 0;
  const top = slices[0];
  return (
    <div className="donut" style={{ width: size, height: size }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--divider)" strokeWidth={stroke} />
        {slices.map((s, i) => {
          const start = cursor;
          const end = Math.min(cursor + s.fraction, 1);
          cursor += s.fraction;
          return (
            <circle
              key={s.name}
              cx={size / 2}
              cy={size / 2}
              r={r}
              fill="none"
              stroke={shade(i)}
              strokeWidth={stroke}
              strokeDasharray={`${(end - start) * c} ${c}`}
              strokeDashoffset={-start * c}
              transform={`rotate(-90 ${size / 2} ${size / 2})`}
            />
          );
        })}
      </svg>
      {top && (
        <div className="donut__center">
          <span className="donut__pct tabular">{percent(top.fraction)}</span>
          <span className="donut__name">{top.name.toUpperCase()}</span>
        </div>
      )}
    </div>
  );
}
