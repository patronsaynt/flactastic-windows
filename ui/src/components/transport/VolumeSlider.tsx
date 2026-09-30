import { Volume1, Volume2 } from "lucide-react";
import { useRef } from "react";
import { player, usePlayer } from "../../app/player";
import "./VolumeSlider.css";

/** `VolumeSliderView`: speaker · 64pt slider · loud speaker. */
export function VolumeSlider() {
  const volume = usePlayer((s) => s.volume);
  const ref = useRef<HTMLDivElement>(null);
  const set = (clientX: number) => {
    const r = ref.current!.getBoundingClientRect();
    player.setVolume(Math.max(0, Math.min(1, (clientX - r.left) / r.width)));
  };
  return (
    <div className="volume">
      <Volume1 size={13} fill="currentColor" strokeWidth={1.5} />
      <div
        ref={ref}
        className="volume__slider"
        role="slider"
        aria-valuemin={0}
        aria-valuemax={1}
        aria-valuenow={volume}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          set(e.clientX);
        }}
        onPointerMove={(e) => {
          if (e.buttons & 1) set(e.clientX);
        }}
      >
        <div className="volume__track" />
        <div className="volume__fill" style={{ width: `${volume * 100}%` }} />
        <div className="volume__knob" style={{ left: `calc(${volume * 100}% - ${volume * 14}px)` }} />
      </div>
      <Volume2 size={13} fill="currentColor" strokeWidth={1.5} />
    </div>
  );
}
