import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect } from "react";
import { player, usePlayer } from "../../app/player";
import { setSetting, useSetting } from "../../app/settings";
import { isModalOpen } from "../../components/sheet/Sheet";
import { AlbumArtMode } from "./AlbumArtModes";
import { LyricsMode } from "./LyricsMode";
import { ModeWheel } from "./ModeWheel";
import { asMode, type VisualizerMode } from "./modes";
import { SpectrumMode } from "./SpectrumModes";
import "./Visualizer.css";

/**
 * `VisualizerView`: the selected mode (cross-fading on change, never
 * hit-testable), the hidden mode wheel, and Q to toggle the queue.
 */
export function VisualizerView() {
  const mode = asMode(useSetting("flactastic.visualizerMode"));
  const setMode = useCallback((m: VisualizerMode) => setSetting("flactastic.visualizerMode", m), []);

  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      if (e.key.toLowerCase() === "q" && !e.ctrlKey && !e.metaKey && !e.altKey && !isModalOpen()) {
        player.setQueueVisible(!usePlayer.getState().isQueueVisible);
      }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);

  return (
    <div className="visualizer">
      <AnimatePresence initial={false}>
        <motion.div
          key={mode}
          className="visualizer__content"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.3, ease: "easeInOut" }}
        >
          {mode === "lyrics" ? (
            <LyricsMode />
          ) : mode === "spectrumRadial" || mode === "spectrumHorizontal" || mode === "spectrogram" ? (
            <SpectrumMode mode={mode} />
          ) : (
            <AlbumArtMode mode={mode} />
          )}
        </motion.div>
      </AnimatePresence>
      <ModeWheel mode={mode} onChange={setMode} />
    </div>
  );
}
