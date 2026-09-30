import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import "./Popover.css";

/**
 * `.popover(arrowEdge: .bottom)`: a panel under `anchor` with an arrow,
 * dismissed by a click outside or Esc. Flips above when there's no room.
 */
export function Popover({
  anchor,
  open,
  onClose,
  children,
}: {
  anchor: RefObject<HTMLElement | null>;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number; arrow: number; above: boolean } | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    const place = () => {
      const a = anchor.current?.getBoundingClientRect();
      const p = panel.current;
      if (!a || !p) return;
      // #root is zoomed; client rects are in zoomed pixels.
      const z = Number(getComputedStyle(document.documentElement).getPropertyValue("--ui-scale")) || 1;
      const w = p.offsetWidth;
      const h = p.offsetHeight;
      const vw = window.innerWidth / z;
      const vh = window.innerHeight / z;
      const cx = (a.left + a.width / 2) / z;
      const left = Math.min(Math.max(8, cx - w / 2), vw - w - 8);
      const below = a.bottom / z + 10;
      const above = below + h > vh - 8 && a.top / z - 10 - h > 8;
      setPos({ left, top: above ? a.top / z - 10 - h : below, arrow: cx - left, above });
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [open, anchor]);

  useEffect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => {
      const t = e.target as Node;
      if (panel.current?.contains(t) || anchor.current?.contains(t)) return;
      onClose();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopImmediatePropagation();
        onClose();
      }
    };
    window.addEventListener("pointerdown", down, true);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("pointerdown", down, true);
      window.removeEventListener("keydown", key, true);
    };
  }, [open, onClose, anchor]);

  if (!open) return null;
  return createPortal(
    <div
      ref={panel}
      className={"popover" + (pos?.above ? " is-above" : "")}
      style={{ left: pos?.left ?? -9999, top: pos?.top ?? -9999, visibility: pos ? "visible" : "hidden" }}
    >
      <span className="popover__arrow" style={{ left: (pos?.arrow ?? 0) - 7 }} />
      {children}
    </div>,
    document.getElementById("root") ?? document.body,
  );
}
