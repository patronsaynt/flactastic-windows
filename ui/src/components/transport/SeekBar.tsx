import { useRef, useState } from "react";
import { player, usePlaybackTime, usePlayer } from "../../app/player";
import { formatDuration } from "../../lib/format";
import "./SeekBar.css";

/** `SeekBarView`: capsule track, 5pt fill, thumb; seeks on release. */
export function SeekBar({ prominent = false }: { prominent?: boolean }) {
  const duration = usePlayer((s) => s.duration) ?? 1;
  const time = usePlaybackTime();
  const [drag, setDrag] = useState<number | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const shown = drag ?? time;
  const progress = duration > 0 ? Math.min(1, Math.max(0, shown / duration)) : 0;
  const thumb = prominent ? 14 : 12;

  const at = (clientX: number) => {
    const r = ref.current!.getBoundingClientRect();
    return Math.max(0, Math.min(1, (clientX - r.left) / r.width)) * duration;
  };

  return (
    <div className={"seek" + (prominent ? " seek--prominent" : "")}>
      <div
        ref={ref}
        className="seek__hit"
        style={{ height: prominent ? 18 : 16 }}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          setDrag(at(e.clientX));
        }}
        onPointerMove={(e) => {
          if (drag !== null) setDrag(at(e.clientX));
        }}
        onPointerUp={(e) => {
          if (drag === null) return;
          player.seek(at(e.clientX));
          setDrag(null);
        }}
        onPointerCancel={() => setDrag(null)}
      >
        <div className="seek__track" style={{ height: prominent ? 5 : 4 }} />
        <div className="seek__fill" style={{ width: `${progress * 100}%` }} />
        <div
          className="seek__thumb"
          style={{
            width: thumb,
            height: thumb,
            left: `clamp(0px, calc(${progress * 100}% - ${thumb / 2}px), calc(100% - ${thumb}px))`,
          }}
        />
      </div>
      <div className="seek__times tabular">
        <span>{formatDuration(shown)}</span>
        <span>-{formatDuration(Math.max(0, duration - shown))}</span>
      </div>
    </div>
  );
}
