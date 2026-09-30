/**
 * `FLContextMenu`: the app's own menu, drawn in-window. Right-click routing
 * falls out of DOM bubbling: the innermost element with a menu handles the
 * event and stops it, which is the Mac's "highest priority wins".
 */
import { ChevronRight, type LucideIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { create } from "zustand";
import "./ContextMenu.css";

export type MenuItem =
  | { kind: "button"; title: string; icon?: LucideIcon; destructive?: boolean; action: () => void }
  | { kind: "divider" }
  | { kind: "submenu"; title: string; icon?: LucideIcon; items: MenuItem[] }
  | { kind: "label"; title: string }
  | { kind: "textField"; placeholder: string; icon?: LucideIcon; onSubmit: (text: string) => void };

export const menu = {
  button: (title: string, action: () => void, icon?: LucideIcon, destructive = false): MenuItem => ({
    kind: "button",
    title,
    action,
    icon,
    destructive,
  }),
  divider: { kind: "divider" } as MenuItem,
  submenu: (title: string, items: MenuItem[], icon?: LucideIcon): MenuItem => ({ kind: "submenu", title, items, icon }),
  label: (title: string): MenuItem => ({ kind: "label", title }),
  textField: (placeholder: string, onSubmit: (t: string) => void, icon?: LucideIcon): MenuItem => ({
    kind: "textField",
    placeholder,
    onSubmit,
    icon,
  }),
};

interface MenuState {
  open: { items: MenuItem[]; x: number; y: number } | null;
}

const useMenu = create<MenuState>(() => ({ open: null }));

export function openMenuAt(items: MenuItem[], x: number, y: number) {
  if (!items.length) return;
  useMenu.setState({ open: { items, x, y } });
}

export function closeMenu() {
  useMenu.setState({ open: null });
}

/** `.flContextMenu { … }` for any element. */
export function contextMenu(provider: () => MenuItem[]) {
  return (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const items = provider();
    // `zoom` on #root: client coordinates are already in zoomed CSS px.
    openMenuAt(items, e.clientX, e.clientY);
  };
}

/** Host rendered once at the app root. */
export function ContextMenuHost() {
  const open = useMenu((s) => s.open);
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        closeMenu();
      }
    };
    const onBlur = () => closeMenu();
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("resize", onBlur);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("resize", onBlur);
    };
  }, [open]);
  if (!open) return null;
  return (
    <div
      className="menu-layer"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) closeMenu();
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        if (e.target === e.currentTarget) closeMenu();
      }}
    >
      <MenuPanel items={open.items} x={open.x} y={open.y} />
    </div>
  );
}

function MenuPanel({ items, x, y, isSub = false }: { items: MenuItem[]; x: number; y: number; isSub?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });
  const [child, setChild] = useState<{ index: number; x: number; y: number } | null>(null);

  // Keep within the window, like the Mac's screen clamping.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const vw = document.documentElement.clientWidth;
    const vh = document.documentElement.clientHeight;
    let left = x;
    let top = y;
    if (left + r.width > vw - 4) left = isSub ? Math.max(4, x - r.width - 224) : Math.max(4, vw - r.width - 4);
    if (top + r.height > vh - 4) top = Math.max(4, vh - r.height - 4);
    setPos({ left, top });
  }, [x, y, isSub]);

  return (
    <>
      <div ref={ref} className="menu" style={{ left: pos.left, top: pos.top }}>
        {items.map((it, i) => {
          switch (it.kind) {
            case "divider":
              return <div key={i} className="menu__divider" />;
            case "label":
              return (
                <div key={i} className="menu__label">
                  {it.title}
                </div>
              );
            case "textField":
              return <TextFieldRow key={i} item={it} onHover={() => setChild(null)} />;
            case "button": {
              const Icon = it.icon;
              return (
                <div
                  key={i}
                  className={"menu__row" + (it.destructive ? " is-destructive" : "")}
                  onPointerEnter={() => !isSub && setChild(null)}
                  onClick={() => {
                    closeMenu();
                    it.action();
                  }}
                >
                  <span className="menu__icon">{Icon && <Icon size={13} strokeWidth={2} />}</span>
                  <span className="menu__title">{it.title}</span>
                </div>
              );
            }
            case "submenu": {
              const Icon = it.icon;
              const openChild = (el: HTMLElement) => {
                const r = el.getBoundingClientRect();
                const panel = ref.current!.getBoundingClientRect();
                setChild({ index: i, x: panel.right + 2, y: r.top - 4 });
              };
              return (
                <div
                  key={i}
                  className={"menu__row" + (child?.index === i ? " is-open" : "")}
                  onPointerEnter={(e) => openChild(e.currentTarget)}
                  onClick={(e) => openChild(e.currentTarget)}
                >
                  <span className="menu__icon">{Icon && <Icon size={13} strokeWidth={2} />}</span>
                  <span className="menu__title">{it.title}</span>
                  <ChevronRight size={11} strokeWidth={3} className="menu__chevron" />
                </div>
              );
            }
          }
        })}
      </div>
      {child && items[child.index]?.kind === "submenu" && (
        <MenuPanel items={(items[child.index] as { items: MenuItem[] }).items} x={child.x} y={child.y} isSub />
      )}
    </>
  );
}

function TextFieldRow({ item, onHover }: { item: Extract<MenuItem, { kind: "textField" }>; onHover: () => void }) {
  const [text, setText] = useState("");
  const Icon = item.icon;
  return (
    <div className="menu__field" onPointerEnter={onHover}>
      <span className="menu__icon">{Icon && <Icon size={13} strokeWidth={2} />}</span>
      <input
        autoFocus
        placeholder={item.placeholder}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            const t = text.trim();
            if (!t) return;
            closeMenu();
            item.onSubmit(t);
          }
        }}
      />
    </div>
  );
}
