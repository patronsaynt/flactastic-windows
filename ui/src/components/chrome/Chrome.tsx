/**
 * `CollectionChrome.swift`: pill toggles, capsule sort menu / buttons,
 * action pills, back link, track-list header.
 */
import { motion } from "motion/react";
import { Check, ChevronDown, ChevronLeft, type LucideIcon } from "lucide-react";
import { useId, type ReactNode } from "react";
import { springs } from "../../theme/motion";
import { openMenuAt } from "../menu/ContextMenu";
import "./Chrome.css";

export interface PillSegment<V> {
  value: V;
  title?: string;
  icon?: LucideIcon;
  help?: string;
}

/** `FLPillToggle`: the active segment slides between options. */
export function PillToggle<V extends string | number | boolean>({
  selection,
  segments,
  onChange,
}: {
  selection: V;
  segments: PillSegment<V>[];
  onChange: (v: V) => void;
}) {
  const group = useId();
  return (
    <div className="pill-toggle">
      {segments.map((s) => {
        const active = s.value === selection;
        const Icon = s.icon;
        return (
          <button
            key={String(s.value)}
            className={"pill-toggle__seg" + (active ? " is-active" : "") + (Icon ? " is-icon" : "")}
            title={s.help ?? s.title}
            onClick={() => onChange(s.value)}
          >
            {active && <motion.span className="pill-toggle__bg" layoutId={group} transition={springs.pill} />}
            <span className="pill-toggle__label">{Icon ? <Icon size={13} strokeWidth={2} /> : s.title}</span>
          </button>
        );
      })}
    </div>
  );
}

/** `FLSortMenu`: capsule dropdown showing the current option. */
export function SortMenu<V extends string>({
  selection,
  options,
  label,
  onChange,
}: {
  selection: V;
  options: V[];
  label: (v: V) => string;
  onChange: (v: V) => void;
}) {
  return (
    <button
      className="capsule sort-menu"
      onClick={(e) => {
        const r = e.currentTarget.getBoundingClientRect();
        openMenuAt(
          options.map((o) => ({
            kind: "button" as const,
            title: label(o),
            icon: o === selection ? Check : undefined,
            action: () => onChange(o),
          })),
          r.left,
          r.bottom + 4,
        );
      }}
    >
      <span className="sort-menu__label">{label(selection)}</span>
      <ChevronDown size={11} strokeWidth={3} className="sort-menu__chevron" />
    </button>
  );
}

/** `FLCircleIconButton`: 34pt circle on the capsule surface. */
export function CircleIconButton({
  icon: Icon,
  onClick,
  title,
  disabled,
  children,
}: {
  icon?: LucideIcon;
  onClick: () => void;
  title?: string;
  disabled?: boolean;
  children?: ReactNode;
}) {
  return (
    <button className="capsule circle-button" onClick={onClick} title={title} disabled={disabled}>
      {Icon ? <Icon size={14} strokeWidth={2} /> : children}
    </button>
  );
}

/** `FLActionPillStyle`: Play / Shuffle / New Playlist pills. */
export function ActionPill({
  primary = false,
  height = 36,
  icon: Icon,
  children,
  onClick,
  disabled,
}: {
  primary?: boolean;
  height?: number;
  icon?: LucideIcon;
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      className={"action-pill" + (primary ? " is-primary" : "")}
      style={{ height, padding: `0 ${primary ? 20 : 18}px` }}
      onClick={onClick}
      disabled={disabled}
    >
      {Icon && <Icon size={12} strokeWidth={2.2} fill={primary ? "currentColor" : "none"} />}
      <span>{children}</span>
    </button>
  );
}

/** `FLBackLink` */
export function BackLink({ title, onClick }: { title: string; onClick: () => void }) {
  return (
    <button className="back-link" onClick={onClick}>
      <ChevronLeft size={14} strokeWidth={2.5} />
      <span>{title}</span>
    </button>
  );
}

/** `FLEyebrow` */
export function Eyebrow({ children }: { children: ReactNode }) {
  return <div className="eyebrow">{children}</div>;
}

/** Column widths shared with `TrackRow`. */
export const trackColumns = { format: 46, quality: 92, length: 44 } as const;

/** `FLTrackListHeader`: `# / TITLE / FORMAT / QUALITY / LENGTH`. */
export function TrackListHeader({ showDragHandle = false }: { showDragHandle?: boolean }) {
  return (
    <div className="track-list-header">
      <span style={{ width: 26, textAlign: "center", letterSpacing: 1 }}>#</span>
      <span style={{ flex: 1 }}>TITLE</span>
      <span style={{ minWidth: trackColumns.format, textAlign: "center" }}>FORMAT</span>
      <span style={{ minWidth: trackColumns.quality, textAlign: "center" }}>QUALITY</span>
      <span style={{ minWidth: trackColumns.length, textAlign: "right" }}>LENGTH</span>
      {showDragHandle && <span style={{ width: 18 }} />}
    </div>
  );
}
