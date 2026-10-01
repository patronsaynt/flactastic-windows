import { AnimatePresence, motion } from "motion/react";
import { Music } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Modal } from "./components/sheet/Sheet";
import { SettingsView } from "./features/settings/SettingsView";
import { startLibrarySync, useLibrary } from "./app/library";
import { startArtistsSync } from "./app/artists";
import { startPlaylistsSync } from "./app/playlists";
import { startDownloadsSync } from "./app/downloads";
import { DownloadView } from "./features/download/DownloadView";
import { LucidaChallengeHost } from "./features/download/LucidaChallenge";
import { LucidaDebugPanel } from "./features/download/LucidaDebug";
import { SyncWindowHost } from "./features/sync/SyncWindow";
import { ConfirmHost } from "./components/sheet/ConfirmDialog";
import { EditorHost } from "./features/editors/EditorHost";
import { OnboardingView } from "./features/onboarding/OnboardingView";
import { VisualizerView } from "./features/visualizer/VisualizerView";
import { OrganizerView } from "./features/organizer/OrganizerView";
import { PlaylistsTabView } from "./features/playlists/PlaylistsTabView";
import { player, startPlayerSync, usePlayer } from "./app/player";
import { startSettingsSync, useSetting, useSettingsStore } from "./app/settings";
import { useUI } from "./app/store";
import type { AppTab } from "./app/tabs";
import { api } from "./lib/api";
import { PageHeader } from "./components/PageHeader";
import { TopBar } from "./components/shell/TopBar";
import { ContextMenuHost } from "./components/menu/ContextMenu";
import { FloatingPlayerBar } from "./components/transport/FloatingPlayerBar";
import { QueuePanel } from "./components/transport/QueuePanel";
import { ArtworkView } from "./components/ArtworkView";
import { CollectionView } from "./features/collection/CollectionView";
import { HomeView } from "./features/home/HomeView";
import { durations } from "./theme/motion";
import "./App.css";

/** Longest the loading cover may stay up (`maxCoverDuration`). */
const MAX_COVER_MS = 8000;

/** `ContentView`: top bar over the selected tab, which cross-fades on switch. */
export function App() {
  const tab = useUI((s) => s.selectedTab);
  const lightMode = useSetting("flactastic.useLightMode");
  const uiScale = useSetting("flactastic.uiScale");
  const showDownloadTab = useSetting("flactastic.showDownloadTab");
  const settingsLoaded = useSettingsStore((s) => s.loaded);
  const onboarded = useSetting("flactastic.hasCompletedOnboarding");
  const hasTrack = usePlayer((s) => s.currentTrack != null);
  const zoomArt = useUI((s) => s.artworkZoom);
  const queueVisible = usePlayer((s) => s.isQueueVisible);
  const showSettings = useUI((s) => s.showSettings);
  const closeSettings = useCallback(() => useUI.getState().setShowSettings(false), []);

  useEffect(() => {
    const stops = [startSettingsSync(), startLibrarySync(), startPlayerSync(), startArtistsSync(), startPlaylistsSync(), startDownloadsSync()];
    void api.bootstrapLibrary().then((opened) => {
      // Nothing to scan: reveal the UI at once so the empty state shows.
      if (opened === false) useLibrary.setState({ hasCompletedInitialLoad: true });
    });
    return () => stops.forEach((s) => s());
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = lightMode ? "light" : "dark";
  }, [lightMode]);

  useEffect(() => {
    const s = Math.min(1.35, Math.max(0.9, Number(uiScale) || 1));
    document.documentElement.style.setProperty("--ui-scale", String(s));
  }, [uiScale]);

  // Show the window once the first themed frame is ready (no white flash).
  useEffect(() => {
    if (!settingsLoaded) return;
    requestAnimationFrame(() => void api.appReady());
    // First run: raise the OS firewall prompt for sync now (once), not
    // mid-pairing later.
    const t = setTimeout(() => void api.syncPrimeNetwork(), 1500);
    return () => clearTimeout(t);
  }, [settingsLoaded]);

  // The Download tab vanished from the bar: don't strand the user on it.
  useEffect(() => {
    if (!showDownloadTab && tab === "Download") useUI.getState().select("Home");
  }, [showDownloadTab, tab]);

  useKeyboardShortcuts();

  // First run: onboarding stands in for the whole window until finished.
  if (settingsLoaded && !onboarded) {
    return (
      <>
        <OnboardingView />
        <ConfirmHost />
        <ContextMenuHost />
      </>
    );
  }

  const showBar = tab !== "Visualizer" && tab !== "Download";
  return (
    <div className="app">
      <TopBar showDownloadTab={showDownloadTab} />
      <main className="app__main">
        <AnimatePresence mode="popLayout" initial={false}>
          <motion.div
            key={tab}
            className="app__page"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: durations.tabSwitch, ease: "easeInOut" }}
          >
            <Page tab={tab} />
          </motion.div>
        </AnimatePresence>
        <LoadingCover />
        <AnimatePresence>
          {queueVisible && (
            <motion.div
              className="app__queue"
              initial={{ opacity: 0, x: 60 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 60 }}
              transition={{ duration: durations.queuePanel, ease: "easeInOut" }}
            >
              <QueuePanel />
            </motion.div>
          )}
        </AnimatePresence>
        <AnimatePresence>
          {showBar && hasTrack && (
            <motion.div
              className="app__player-bar"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1, x: queueVisible ? -180 : 0 }}
              exit={{ opacity: 0 }}
              transition={{ duration: durations.queuePanel, ease: "easeInOut" }}
            >
              <FloatingPlayerBar />
            </motion.div>
          )}
        </AnimatePresence>
        <AnimatePresence>
          {zoomArt && (
            <motion.div
              className="artwork-zoom"
              initial={{ opacity: 0, scale: 0.96 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.96 }}
              transition={{ duration: durations.artworkZoom, ease: "easeInOut" }}
            >
              <ArtworkZoom id={zoomArt} />
            </motion.div>
          )}
        </AnimatePresence>
      </main>
      <Modal open={showSettings} onClose={closeSettings}>
        <SettingsView onClose={closeSettings} />
      </Modal>
      <EditorHost />
      <LucidaChallengeHost />
      <LucidaDebugPanel />
      <SyncWindowHost />
      <ConfirmHost />
      <ContextMenuHost />
    </div>
  );
}

function Page({ tab }: { tab: AppTab }) {
  switch (tab) {
    case "Home":
      return <HomeView />;
    case "Collection":
      return <CollectionView />;
    case "Playlists":
      return <PlaylistsTabView />;
    case "Visualizer":
      return <VisualizerView />;
    case "Organizer":
      return <OrganizerView />;
    case "Download":
      return <DownloadView />;
    default:
      return (
        <div className="page-pad">
          <PageHeader eyebrow="Tools" title={tab} />
        </div>
      );
  }
}

/** `LoadingCoverView`: opaque until the first load resolves (≤ 8 s). */
function LoadingCover() {
  const done = useLibrary((s) => s.hasCompletedInitialLoad);
  const [timedOut, setTimedOut] = useState(false);
  const [gone, setGone] = useState(false);
  useEffect(() => {
    const t = setTimeout(() => setTimedOut(true), MAX_COVER_MS);
    return () => clearTimeout(t);
  }, []);
  const fading = done || timedOut;
  useEffect(() => {
    if (!fading) return;
    const t = setTimeout(() => setGone(true), 400);
    return () => clearTimeout(t);
  }, [fading]);
  if (gone) return null;
  return (
    <div className={"loading-cover" + (fading ? " is-fading" : "")}>
      <Music size={46} strokeWidth={0.75} className="loading-cover__icon" />
      <div className="loading-cover__text">Loading collection…</div>
    </div>
  );
}

function ArtworkZoom({ id }: { id: string }) {
  const close = () => useUI.getState().setArtworkZoom(null);
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);
  const [size, setSize] = useState(() => Math.min(window.innerWidth, window.innerHeight - 140) * 0.85);
  useEffect(() => {
    const r = () => setSize(Math.min(window.innerWidth, window.innerHeight - 140) * 0.85);
    window.addEventListener("resize", r);
    return () => window.removeEventListener("resize", r);
  }, []);
  return (
    <>
      <div className="artwork-zoom__art">
        <ArtworkView artwork={id} size={size} fullResolution />
      </div>
      <button className="pill-button" onClick={close}>
        Back
      </button>
    </>
  );
}

function isTyping(e: KeyboardEvent) {
  const t = e.target as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
}

/**
 * The Mac's Playback/Collection menu shortcuts on Ctrl, and the app-level
 * spacebar monitor (not while typing).
 */
function useKeyboardShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (e.code === "Space" && !mod && !e.altKey && !isTyping(e) && !useUI.getState().lyricsSyncActive) {
        e.preventDefault();
        void player.toggle();
        return;
      }
      if (!mod || e.altKey || e.shiftKey) return;
      switch (e.key) {
        case "ArrowRight":
          e.preventDefault();
          void player.next();
          break;
        case "ArrowLeft":
          e.preventDefault();
          void player.previous();
          break;
        case "ArrowUp":
          e.preventDefault();
          void api.transport("volumeUp");
          break;
        case "ArrowDown":
          e.preventDefault();
          void api.transport("volumeDown");
          break;
        case "r":
        case "R":
          e.preventDefault();
          void api.refreshLibrary();
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
