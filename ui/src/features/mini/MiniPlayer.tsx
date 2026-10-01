import { LogicalSize, getCurrentWindow } from "@tauri-apps/api/window";
import { Music, Pause, Play, SkipBack, SkipForward } from "lucide-react";
import { useEffect, useLayoutEffect, useRef } from "react";
import { player, startPlayerSync, usePlayer } from "../../app/player";
import { startSettingsSync, useSetting } from "../../app/settings";
import { ArtworkView } from "../../components/ArtworkView";
import { SeekBar } from "../../components/transport/SeekBar";
import { invoke, isTauri } from "../../lib/native";
import "./MiniPlayer.css";

/**
 * `MenuBarPlayerView`: the tray mini-player window. Shares the main window's
 * player state, artwork and seek bar.
 */
export function MiniPlayer() {
  const track = usePlayer((s) => s.currentTrack);
  const isPlaying = usePlayer((s) => s.isPlaying);
  const light = useSetting("flactastic.useLightMode");
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const stops = [startSettingsSync(), startPlayerSync()];
    return () => stops.forEach((s) => s());
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = light ? "light" : "dark";
  }, [light]);

  // The window hugs the content.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !isTauri) return;
    const fit = () => void getCurrentWindow().setSize(new LogicalSize(320, Math.ceil(el.getBoundingClientRect().height)));
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  return (
    <div className="mini" ref={ref}>
      {track ? (
        <>
          <div className="mini__header">
            <ArtworkView artwork={track.artwork} size={72} />
            <div className="mini__text">
              <div className="mini__title">{track.title}</div>
              {track.artistDisplay && <div className="mini__artist">{track.artistDisplay}</div>}
              {track.album && <div className="mini__album">{track.album}</div>}
            </div>
          </div>
          <SeekBar />
          <div className="mini__transport">
            <button className="mini__btn" onClick={() => void player.previous()}>
              <SkipBack size={18} fill="currentColor" strokeWidth={1.5} />
            </button>
            <button className="mini__btn is-play" onClick={() => void player.toggle()}>
              {isPlaying ? <Pause size={26} fill="currentColor" strokeWidth={0} /> : <Play size={26} fill="currentColor" strokeWidth={0} />}
            </button>
            <button className="mini__btn" onClick={() => void player.next()}>
              <SkipForward size={18} fill="currentColor" strokeWidth={1.5} />
            </button>
          </div>
        </>
      ) : (
        <div className="mini__empty">
          <span className="mini__empty-art">
            <Music size={28} strokeWidth={0.8} />
          </span>
          <div>
            <div className="mini__empty-title">Nothing playing</div>
            <div className="mini__empty-sub">Open FLACtastic to pick a track</div>
          </div>
        </div>
      )}
      <div className="mini__divider" />
      <div className="mini__footer">
        <button className="mini__link" onClick={() => void invoke("mini_open_main")}>
          Open FLACtastic
        </button>
        <span style={{ flex: 1 }} />
        <button className="mini__link is-quit" onClick={() => void invoke("mini_quit")}>
          Quit
        </button>
      </div>
    </div>
  );
}
