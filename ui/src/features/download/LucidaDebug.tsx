import { X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useDebug, useDownloads } from "../../app/downloads";
import { PillButton } from "../../components/settings/Primitives";
import { api, on, type LucidaLogEntry, type LucidaPhase } from "../../lib/api";
import "./LucidaDebug.css";

const phaseLabel = (p: LucidaPhase) => (p.kind === "failed" ? `failed — ${p.message}` : p.kind);
const phaseColor = (p: LucidaPhase) =>
  p.kind === "ready" ? "var(--quality-cd)" : p.kind === "failed" ? "var(--quality-low)" : "var(--text-secondary)";
const kindColor: Record<LucidaLogEntry["kind"], string> = {
  nav: "rgb(80 140 255)",
  bridge: "rgb(180 110 255)",
  ok: "var(--quality-cd)",
  error: "var(--quality-low)",
  info: "var(--text-secondary)",
};

const time = (ms: number) => {
  const d = new Date(ms);
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
};

/**
 * `LucidaDebugView`: phase, controls and the navigation/bridge log. The live
 * page itself opens as its own window ("Show Web View").
 */
export function LucidaDebugPanel() {
  const enabled = useDebug((s) => s.lucidaDebugEnabled);
  const phase = useDownloads((s) => s.lucida.phase);
  const [log, setLog] = useState<LucidaLogEntry[]>([]);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!enabled) return;
    const load = () => void api.lucidaLog().then((l) => l && setLog(l));
    load();
    return on("lucida://log", load);
  }, [enabled]);

  useEffect(() => {
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [log.length]);

  if (!enabled) return null;
  return (
    <div className="lucida-debug">
      <div className="lucida-debug__bar">
        <span className="lucida-debug__label">Phase:</span>
        <span className="lucida-debug__phase" style={{ color: phaseColor(phase) }}>
          {phaseLabel(phase)}
        </span>
        <span style={{ flex: 1 }} />
        <PillButton small onClick={() => void api.lucidaShowWebview()}>
          Show Web View
        </PillButton>
        <PillButton small onClick={() => void api.lucidaReload()}>
          Reload
        </PillButton>
        <PillButton small onClick={() => void api.lucidaClearSiteData()}>
          Clear Site Data
        </PillButton>
        <PillButton small onClick={() => void api.lucidaClearLog()}>
          Clear log
        </PillButton>
        <button className="lucida-debug__close" title="Close" onClick={() => useDebug.setState({ lucidaDebugEnabled: false })}>
          <X size={13} strokeWidth={2.4} />
        </button>
      </div>
      <div className="lucida-debug__log" ref={listRef}>
        {log.map((e) => (
          <div key={e.id} className="lucida-debug__entry">
            <span className="lucida-debug__time">{time(e.timestamp)}</span>
            <span className="lucida-debug__kind" style={{ color: kindColor[e.kind] }}>
              {e.kind}
            </span>
            <span className="lucida-debug__msg">{e.message}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
