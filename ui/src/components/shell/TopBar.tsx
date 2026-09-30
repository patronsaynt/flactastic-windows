import { motion } from "motion/react";
import { Settings as Gear } from "lucide-react";
import { useUI } from "../../app/store";
import { libraryTabs, tabIcons, toolTabs, type AppTab } from "../../app/tabs";
import { springs } from "../../theme/motion";
import { WindowControls } from "./WindowControls";
import "./TopBar.css";

/**
 * `TopBarView`: 52px bar, centred tab group, Settings pill on the right.
 * Empty areas drag the window (`data-tauri-drag-region`); the caption buttons
 * sit where the macOS traffic lights would be mirrored on Windows/Linux.
 */
export function TopBar({ showDownloadTab }: { showDownloadTab: boolean }) {
  const setShowSettings = useUI((s) => s.setShowSettings);
  return (
    <header className="top-bar" data-tauri-drag-region>
      <div className="top-bar__center" data-tauri-drag-region>
        <TabBar showDownloadTab={showDownloadTab} />
      </div>
      <div className="top-bar__trailing">
        <motion.button
          className="bar-pill"
          onClick={() => setShowSettings(true)}
          whileTap={{ scale: 0.94, opacity: 0.8 }}
          transition={springs.press}
        >
          <span className="bar-pill__inner">
            <Gear size={12} strokeWidth={1.75} />
            <span>Settings</span>
          </span>
        </motion.button>
        <WindowControls />
      </div>
    </header>
  );
}

function TabBar({ showDownloadTab }: { showDownloadTab: boolean }) {
  const tools = toolTabs.filter((t) => t !== "Download" || showDownloadTab);
  return (
    <nav className="tab-bar">
      {libraryTabs.map((t) => (
        <TabButton key={t} tab={t} />
      ))}
      <span className="tab-bar__divider" />
      {tools.map((t) => (
        <TabButton key={t} tab={t} />
      ))}
    </nav>
  );
}

function TabButton({ tab }: { tab: AppTab }) {
  const selected = useUI((s) => s.selectedTab === tab);
  const select = useUI((s) => s.select);
  const Icon = tabIcons[tab];
  return (
    <motion.button
      className={"tab" + (selected ? " tab--selected" : "")}
      onClick={() => select(tab)}
      whileTap={{ scale: 0.94, opacity: 0.8 }}
      transition={springs.press}
    >
      {selected && <motion.span className="tab__active" layoutId="activeTab" transition={springs.tab} />}
      <motion.span
        className="tab__icon"
        animate={{ scale: selected ? 1.05 : 1 }}
        transition={springs.tabLabel}
      >
        <Icon size={12} strokeWidth={selected ? 2.25 : 1.6} />
      </motion.span>
      <span className="tab__label">{tab}</span>
    </motion.button>
  );
}
