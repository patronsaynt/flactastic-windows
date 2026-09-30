import type { LucideIcon } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";

/** `minutesLabel`: "0m", "47m", or "1.5h" from an hour up. */
export function minutesLabel(minutes: number): string {
  return minutes >= 60 ? `${(minutes / 60).toFixed(1)}h` : `${Math.round(minutes)}m`;
}

/** `Int.abbreviated()`: "12.4k" from a thousand up. */
export function abbreviated(n: number): string {
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k` : n.toLocaleString();
}

/** `ProgressBar`: a capsule track with a tinted fill. */
export function ProgressBar({ fraction, tint, height = 3 }: { fraction: number; tint: string; height?: number }) {
  const f = Math.min(Math.max(fraction, 0), 1);
  return (
    <div className="progress-bar" style={{ height }}>
      <div className="progress-bar__fill" style={{ width: `${f * 100}%`, background: tint }} />
    </div>
  );
}

/** `HomeStatCard` */
export function StatCard({
  icon: Icon,
  value,
  label,
  detail,
  accent = false,
}: {
  icon: LucideIcon;
  value: string;
  label: string;
  detail?: string;
  accent?: boolean;
}) {
  return (
    <div className="stat-card">
      <span className="stat-card__icon" style={{ color: accent ? "var(--quality-hires)" : "var(--text-tertiary)" }}>
        <Icon size={13} strokeWidth={1.8} />
      </span>
      <div className="stat-card__value" title={value}>
        {value}
      </div>
      <div className="stat-card__label">{label.toUpperCase()}</div>
      {detail && <div className="stat-card__detail">{detail}</div>}
    </div>
  );
}

/** `WeeklyListeningChart`: minutes per day, area + line in the Hi-Res tint. */
export function WeeklyChart({ minutes, labels }: { minutes: number[]; labels: string[] }) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    setWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);
  const H = 132;
  const maxV = Math.max(...minutes, 1);
  const step = minutes.length > 1 ? width / (minutes.length - 1) : width;
  const pts = minutes.map((v, i) => [i * step, H - (v / maxV) * H] as const);
  const line = pts.map(([x, y], i) => `${i ? "L" : "M"}${x},${y}`).join(" ");
  const area = pts.length > 1 ? `${line} L${pts.at(-1)![0]},${H} L${pts[0][0]},${H} Z` : "";

  return (
    <div className="home-card weekly-chart">
      <div className="weekly-chart__head">
        <div>
          <div className="home-card__title">Weekly Listening</div>
          <div className="home-card__caption" style={{ marginTop: 3 }}>
            Minutes per day
          </div>
        </div>
        <div className="home-card__caption">This week</div>
      </div>
      <div className="weekly-chart__body">
        <div className="weekly-chart__axis">
          <span>{minutesLabel(maxV)}</span>
          <span>{minutesLabel(maxV / 2)}</span>
          <span>{minutesLabel(0)}</span>
        </div>
        <div ref={ref} className="weekly-chart__plot">
          {width > 0 && (
            <svg width={width} height={H} style={{ overflow: "visible" }}>
              <defs>
                <linearGradient id="weekly-fill" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0" stopColor="var(--quality-hires)" stopOpacity="0.26" />
                  <stop offset="1" stopColor="var(--quality-hires)" stopOpacity="0" />
                </linearGradient>
              </defs>
              {[0, 0.5, 1].map((f) => (
                <rect key={f} x={0} y={H * (1 - f) - 0.5} width={width} height={1} className="weekly-chart__grid" />
              ))}
              {pts.length > 1 && (
                <>
                  <path d={area} fill="url(#weekly-fill)" />
                  <path d={line} fill="none" stroke="var(--quality-hires)" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" />
                </>
              )}
            </svg>
          )}
        </div>
      </div>
      <div className="weekly-chart__labels">
        {labels.map((l, i) => (
          <span key={i}>{l}</span>
        ))}
      </div>
    </div>
  );
}
