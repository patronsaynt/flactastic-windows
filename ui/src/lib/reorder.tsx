/**
 * Pointer-driven drag-to-reorder. HTML5 drag-and-drop can't be used: Tauri's
 * native file-drop handling (needed for Import, which wants real paths)
 * swallows in-page HTML5 drags on Windows.
 *
 * Rows carry `data-reorder-id`; a press that moves 4 px starts a drag. The
 * row under the pointer is the target. `live` mode reports every target
 * change (the album editor's move-on-enter); otherwise one move on release.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import "./reorder.css";

const THRESHOLD = 4;

export interface Reorder {
  dragging: string | null;
  target: string | null;
  /** Spread on each row. */
  rowProps: (id: string) => { "data-reorder-id": string; "data-reorder-group": string };
  /** Spread on whatever starts a drag (the row itself, or a grip). */
  handleProps: (id: string, label?: string) => { onPointerDown: (e: React.PointerEvent) => void };
  ghost: React.ReactNode;
}

let groupSeq = 0;

export function useReorder({
  onMove,
  live = false,
}: {
  /** `source` should be placed where `target` is. */
  onMove: (source: string, target: string) => void;
  live?: boolean;
}): Reorder {
  const group = useRef(`g${++groupSeq}`).current;
  const [dragging, setDragging] = useState<string | null>(null);
  const [target, setTarget] = useState<string | null>(null);
  const [ghost, setGhost] = useState<{ x: number; y: number; label: string } | null>(null);
  const moveRef = useRef(onMove);
  moveRef.current = onMove;
  const state = useRef<{ id: string; label: string; x: number; y: number; started: boolean; target: string | null } | null>(null);

  const zoom = () => Number(getComputedStyle(document.documentElement).getPropertyValue("--ui-scale")) || 1;

  const onPointerMove = useCallback(
    (e: PointerEvent) => {
      const s = state.current;
      if (!s) return;
      if (!s.started) {
        if (Math.hypot(e.clientX - s.x, e.clientY - s.y) < THRESHOLD) return;
        s.started = true;
        setDragging(s.id);
        document.body.classList.add("is-reordering");
      }
      const z = zoom();
      setGhost({ x: e.clientX / z, y: e.clientY / z, label: s.label });
      const el = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>(`[data-reorder-group="${group}"]`);
      const t = el?.dataset.reorderId ?? null;
      if (t === s.target) return;
      s.target = t;
      if (live) {
        if (t && t !== s.id) moveRef.current(s.id, t);
      } else {
        setTarget(t);
      }
    },
    [group, live],
  );

  const end = useCallback(() => {
    const s = state.current;
    state.current = null;
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerup", end);
    window.removeEventListener("pointercancel", end);
    document.body.classList.remove("is-reordering");
    if (s?.started && !live && s.target && s.target !== s.id) moveRef.current(s.id, s.target);
    if (s?.started) {
      // Swallow the click that follows a drag.
      const stop = (ev: MouseEvent) => {
        ev.stopPropagation();
        ev.preventDefault();
      };
      window.addEventListener("click", stop, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", stop, { capture: true }), 0);
    }
    setDragging(null);
    setTarget(null);
    setGhost(null);
  }, [onPointerMove, live]);

  useEffect(() => end, [end]);

  const handleProps = (id: string, label = "") => ({
    onPointerDown: (e: React.PointerEvent) => {
      if (e.button !== 0) return;
      const t = e.target as HTMLElement;
      if (t.closest("input, textarea, button:not([data-reorder-grip])")) return;
      state.current = { id, label, x: e.clientX, y: e.clientY, started: false, target: null };
      window.addEventListener("pointermove", onPointerMove);
      window.addEventListener("pointerup", end);
      window.addEventListener("pointercancel", end);
    },
  });

  const rowProps = (id: string) => ({ "data-reorder-id": id, "data-reorder-group": group });

  return {
    dragging,
    target,
    rowProps,
    handleProps,
    ghost:
      ghost && ghost.label
        ? createPortal(
            <div className="reorder-ghost" style={{ left: ghost.x + 12, top: ghost.y + 10 }}>
              {ghost.label}
            </div>,
            document.getElementById("root") ?? document.body,
          )
        : null,
  };
}
