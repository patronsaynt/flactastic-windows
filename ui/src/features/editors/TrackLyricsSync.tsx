import { Hand, Pause, Play, Redo2, RotateCcw, RotateCw, SkipBack, Undo2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { player, usePlaybackTime, usePlayer } from "../../app/player";
import { useUI } from "../../app/store";
import { api, type Track } from "../../lib/api";
import { FLSheet } from "../../components/sheet/Sheet";
import { PillButton } from "../../components/settings/Primitives";
import "./Editors.css";

/** `m:ss.ss` */
function formatTime(s: number | null | undefined): string {
  if (s == null || !isFinite(s) || s < 0) return "—";
  const m = Math.floor(s / 60);
  const r = s - m * 60;
  return `${m}:${r.toFixed(2).padStart(5, "0")}`;
}

/**
 * `TrackLyricsSyncView`: Space (or Tap) stamps the current playback time on
 * the next line; Save re-emits the lines as LRC.
 */
export function TrackLyricsSync({
  track,
  lyrics,
  onSave,
  onClose,
}: {
  track: Track;
  lyrics: string;
  onSave: (lrc: string) => void;
  onClose: () => void;
}) {
  const [lines, setLines] = useState<string[]>([]);
  const [stamps, setStamps] = useState<(number | null)[]>([]);
  const [cursor, setCursor] = useState(0);
  const isPlaying = usePlayer((s) => s.isPlaying);
  const duration = usePlayer((s) => s.duration);
  const currentId = usePlayer((s) => s.currentTrackId);
  const now = usePlaybackTime();
  const nowRef = useRef(0);
  nowRef.current = now;
  const listRef = useRef<HTMLDivElement>(null);

  // Seed from the text; start this track if something else is current.
  useEffect(() => {
    void api.parseLyricsForSync(lyrics).then((parsed) => {
      if (!parsed) return;
      setLines(parsed.map((l) => l.text));
      const ts = parsed.map((l) => l.timestamp);
      setStamps(ts);
      const first = ts.findIndex((t) => t == null);
      setCursor(first < 0 ? parsed.length : first);
    });
    if (usePlayer.getState().currentTrackId !== track.id) void api.playTracks([track.id], 0, null);
    useUI.getState().setLyricsSyncActive(true);
    return () => useUI.getState().setLyricsSyncActive(false);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const tap = () => {
    if (cursor >= lines.length) return;
    const t = nowRef.current;
    setStamps((s) => s.map((v, i) => (i === cursor ? t : v)));
    setCursor(cursor + 1);
  };
  const back = () => {
    if (cursor <= 0) return;
    setStamps((s) => s.map((v, i) => (i === cursor - 1 ? null : v)));
    setCursor(cursor - 1);
  };
  const skip = () => setCursor((c) => Math.min(c + 1, lines.length));

  // Spacebar stamps the next line.
  const tapRef = useRef(tap);
  tapRef.current = tap;
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.code === "Space" && !e.ctrlKey && !e.metaKey && !e.altKey) {
        e.preventDefault();
        e.stopPropagation();
        tapRef.current();
      }
    };
    window.addEventListener("keydown", k, true);
    return () => window.removeEventListener("keydown", k, true);
  }, []);

  useEffect(() => {
    const el = listRef.current?.children[Math.min(cursor, lines.length - 1)] as HTMLElement | undefined;
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [cursor, lines.length]);

  const seek = (t: number) => void api.seek(Math.max(0, t));
  const known = stamps.filter((s): s is number => s != null);
  const outOfOrder = known.some((s, i) => i > 0 && known[i - 1] > s);
  const playingThis = currentId === track.id;

  const save = async () => {
    const lrc = await api.serializeLrc(lines.map((text, i) => ({ timestamp: stamps[i] ?? null, text })));
    if (lrc != null) onSave(lrc);
    onClose();
  };

  return (
    <FLSheet
      title="Sync Lyrics"
      width={620}
      height={620}
      onClose={onClose}
      footer={
        <div className="sync-footer">
          {outOfOrder && <div className="editor-caption">Some timestamps are out of order — saving anyway.</div>}
          <div className="editor-footer">
            <div style={{ flex: 1 }} />
            <PillButton onClick={onClose}>Cancel</PillButton>
            <PillButton primary onClick={() => void save()}>
              Save
            </PillButton>
          </div>
        </div>
      }
    >
      <div className="lyrics-sync">
        <div>
          <div className="editor-caption" style={{ color: "var(--text-secondary)" }}>
            Press the spacebar — or click <b>Tap</b> — when each line begins.
          </div>
          <div className="editor-caption">Use Back to undo a stamp; Skip leaves a line untimed.</div>
        </div>
        <div className="editor-row">
          <SyncButton title="Restart" onClick={() => seek(0)} icon={<SkipBack size={14} fill="currentColor" />} />
          <SyncButton title="Back 3s" onClick={() => seek(now - 3)} icon={<RotateCcw size={14} />} />
          <SyncButton
            title={isPlaying ? "Pause" : "Play"}
            onClick={() => void player.toggle()}
            icon={isPlaying ? <Pause size={14} fill="currentColor" /> : <Play size={14} fill="currentColor" />}
          />
          <SyncButton title="Forward 3s" onClick={() => seek(Math.min(duration ?? Infinity, now + 3))} icon={<RotateCw size={14} />} />
          <div style={{ flex: 1 }} />
          <span className="sync-time">
            {formatTime(playingThis ? now : 0)} / {formatTime(playingThis ? (duration ?? 0) : (track.duration ?? 0))}
          </span>
        </div>
        <div className="sync-divider" />
        <div className="sync-lines" ref={listRef}>
          {lines.map((line, i) => {
            const active = i === cursor;
            return (
              <div key={i} className={"sync-line" + (active ? " is-active" : "")} onClick={() => setCursor(i)}>
                <span className="sync-line__dot" />
                <span className={"sync-line__time" + (stamps[i] != null ? " is-set" : "")}>{formatTime(stamps[i])}</span>
                <span className="sync-line__text">{line.trim() ? line : " "}</span>
              </div>
            );
          })}
        </div>
        <div className="editor-row">
          <PillButton onClick={back} disabled={cursor === 0 && stamps.every((s) => s == null)}>
            <span className="pill-icon">
              <Undo2 size={12} /> Back
            </span>
          </PillButton>
          <PillButton onClick={skip} disabled={cursor >= lines.length}>
            <span className="pill-icon">
              <Redo2 size={12} /> Skip
            </span>
          </PillButton>
          <PillButton primary onClick={tap} disabled={cursor >= lines.length}>
            <span className="pill-icon">
              <Hand size={12} fill="currentColor" /> Tap
            </span>
          </PillButton>
          <div style={{ flex: 1 }} />
          <button
            className="editor-text-button"
            style={{ fontSize: "var(--font-caption)" }}
            onClick={() => {
              setStamps(lines.map(() => null));
              setCursor(0);
            }}
          >
            Reset all
          </button>
        </div>
      </div>
    </FLSheet>
  );
}

function SyncButton({ title, onClick, icon }: { title: string; onClick: () => void; icon: React.ReactNode }) {
  return (
    <button className="sync-button" title={title} onClick={onClick}>
      {icon}
    </button>
  );
}
