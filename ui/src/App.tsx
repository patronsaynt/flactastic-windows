import { AnimatePresence, motion } from "motion/react";
import { useEffect } from "react";
import { useUI } from "./app/store";
import type { AppTab } from "./app/tabs";
import { PageHeader } from "./components/PageHeader";
import { TopBar } from "./components/shell/TopBar";
import { durations } from "./theme/motion";
import "./App.css";

/** `ContentView`: top bar over the selected tab, which cross-fades on switch. */
export function App() {
  const tab = useUI((s) => s.selectedTab);

  useEffect(() => {
    document.documentElement.dataset.theme = "dark";
  }, []);

  return (
    <div className="app">
      <TopBar showDownloadTab={false} />
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
            <Placeholder tab={tab} />
          </motion.div>
        </AnimatePresence>
      </main>
    </div>
  );
}

const eyebrows: Record<AppTab, string> = {
  Home: "Good evening",
  Collection: "Library",
  Playlists: "Library",
  Download: "Tools",
  Organizer: "Tools",
  Visualizer: "Now Playing",
};

function Placeholder({ tab }: { tab: AppTab }) {
  return (
    <div className="page-pad">
      <PageHeader eyebrow={eyebrows[tab]} title={tab} />
    </div>
  );
}
