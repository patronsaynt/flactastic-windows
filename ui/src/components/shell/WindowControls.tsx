import { useEffect, useState } from "react";
import { platform, windowControls } from "../../lib/native";
import "./WindowControls.css";

/**
 * Caption buttons for the frameless window. Windows gets the 46×32 Fluent
 * glyph buttons (Segoe Fluent Icons when available); Linux gets round
 * GNOME-style buttons. macOS draws its own traffic lights, so nothing here.
 */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const refresh = () => void windowControls.isMaximized().then(setMaximized);
    refresh();
    return windowControls.onResized(refresh);
  }, []);

  if (platform === "other") return null;

  const cls = platform === "windows" ? "caption caption--win" : "caption caption--linux";
  return (
    <div className={cls}>
      <button className="caption__btn" aria-label="Minimize" onClick={windowControls.minimize}>
        <Glyph kind="min" />
      </button>
      <button
        className="caption__btn"
        aria-label={maximized ? "Restore" : "Maximize"}
        onClick={windowControls.toggleMaximize}
      >
        <Glyph kind={maximized ? "restore" : "max"} />
      </button>
      <button className="caption__btn caption__btn--close" aria-label="Close" onClick={windowControls.close}>
        <Glyph kind="close" />
      </button>
    </div>
  );
}

function Glyph({ kind }: { kind: "min" | "max" | "restore" | "close" }) {
  // 10×10 hairline glyphs matching the Windows 11 caption icons.
  const p = { fill: "none", stroke: "currentColor", strokeWidth: 1 } as const;
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      {kind === "min" && <path d="M0 5.5h10" {...p} />}
      {kind === "max" && <rect x="0.5" y="0.5" width="9" height="9" rx="1" {...p} />}
      {kind === "restore" && (
        <>
          <rect x="0.5" y="2.5" width="7" height="7" rx="1" {...p} />
          <path d="M2.5 2.5v-1a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1" {...p} />
        </>
      )}
      {kind === "close" && <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" {...p} />}
    </svg>
  );
}
