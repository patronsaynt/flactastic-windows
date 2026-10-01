import { motion } from "motion/react";
import { Menu } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { editors } from "../../app/editors";
import { player } from "../../app/player";
import { api } from "../../lib/api";
import { springs } from "../../theme/motion";
import wordmark from "../../assets/Wordmark.png";
import { menu, openMenuAt } from "../menu/ContextMenu";
import { Modal } from "../sheet/Sheet";
import { openSyncWindow } from "../../features/sync/SyncWindow";

/**
 * The Mac's menu-bar commands (application, File, Collection, Playback
 * menus), which a frameless window has nowhere else to put. Sits where the
 * traffic lights are on macOS.
 */
export function AppMenuButton() {
  const ref = useRef<HTMLButtonElement>(null);
  const [about, setAbout] = useState(false);
  const closeAbout = useCallback(() => setAbout(false), []);
  const open = () => {
    const r = ref.current!.getBoundingClientRect();
    openMenuAt(
      [
        menu.button("About FLACtastic", () => setAbout(true)),
        menu.divider,
        menu.button("Refresh Collection", () => void api.refreshLibrary()),
        menu.divider,
        menu.button("Sync…", openSyncWindow),
        menu.divider,
        menu.button("Import Track…", () => editors.import("track")),
        menu.button("Import Album…", () => editors.import("album")),
        menu.button("Import Files as Playlist…", () => editors.import("playlist")),
        menu.divider,
        menu.submenu("Playback", [
          menu.button("Play / Pause", () => void player.toggle()),
          menu.button("Next", () => void player.next()),
          menu.button("Previous", () => void player.previous()),
          menu.divider,
          menu.button("Volume Up", () => void api.transport("volumeUp")),
          menu.button("Volume Down", () => void api.transport("volumeDown")),
        ]),
      ],
      r.left,
      r.bottom + 6,
    );
  };
  return (
    <>
      <motion.button
        ref={ref}
        className="bar-pill bar-pill--icon"
        title="FLACtastic"
        onClick={open}
        whileTap={{ scale: 0.94, opacity: 0.8 }}
        transition={springs.press}
      >
        <span className="bar-pill__inner">
          <Menu size={13} strokeWidth={1.75} />
        </span>
      </motion.button>
      <Modal open={about} onClose={closeAbout}>
        <AboutView />
      </Modal>
    </>
  );
}

/** `AboutView` */
function AboutView() {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    void api.appVersion().then((v) => setVersion(v ?? "beta"));
  }, []);
  return (
    <div className="about">
      <div className="about__wordmark" style={{ maskImage: `url(${wordmark})`, WebkitMaskImage: `url(${wordmark})` }} />
      <div className="about__version">Version {version ?? ""}</div>
      <div className="about__tagline">Lossless music, beautifully organized.</div>
    </div>
  );
}
