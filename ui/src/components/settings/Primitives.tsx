/**
 * `SettingsPrimitives.swift` + the rows SettingsView builds from them, and the
 * macOS-style controls they use (switch, pop-up menu, slider, pill button).
 */
import { Check, ChevronsUpDown } from "lucide-react";
import { useRef, type ReactNode } from "react";
import { openMenuAt, type MenuItem } from "../menu/ContextMenu";
import "./Primitives.css";

export function SettingsGroup({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <div className="settings-group">
      {title && <div className="settings-group__title">{title}</div>}
      <div className="settings-group__body">{children}</div>
    </div>
  );
}

export function GroupDivider() {
  return <div className="group-divider" />;
}

/** The `.switch` toggle, tinted with the accent. */
export function Switch({ on, onChange, disabled }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      className={"switch" + (on ? " is-on" : "")}
      disabled={disabled}
      onClick={() => onChange(!on)}
    >
      <span className="switch__knob" />
    </button>
  );
}

export function ToggleRow({
  label,
  subtitle,
  on,
  onChange,
  enabled = true,
}: {
  label: string;
  subtitle?: string;
  on: boolean;
  onChange: (v: boolean) => void;
  enabled?: boolean;
}) {
  return (
    <div className={"settings-row" + (subtitle ? " is-top" : "")} style={{ opacity: enabled ? 1 : 0.4 }}>
      <RowLabel label={label} subtitle={subtitle} />
      <Switch on={on} onChange={onChange} disabled={!enabled} />
    </div>
  );
}

export function RowLabel({ label, subtitle, dim }: { label: string; subtitle?: string; dim?: boolean }) {
  return (
    <div className="settings-row__label">
      <div className="settings-row__title" style={dim ? { color: "var(--text-tertiary)" } : undefined}>
        {label}
      </div>
      {subtitle && <div className="settings-row__subtitle">{subtitle}</div>}
    </div>
  );
}

export interface PickerOption<V> {
  value: V;
  label: string;
  /** Divider after this option. */
  divider?: boolean;
}

/** `.pickerStyle(.menu)`: a pop-up button with a checkmarked menu. */
export function MenuPicker<V>({
  value,
  options,
  onChange,
  disabled,
}: {
  value: V;
  options: PickerOption<V>[];
  onChange: (v: V) => void;
  disabled?: boolean;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  const current = options.find((o) => o.value === value)?.label ?? "";
  return (
    <button
      ref={ref}
      className="popup-button"
      disabled={disabled}
      onClick={() => {
        const r = ref.current!.getBoundingClientRect();
        const items: MenuItem[] = [];
        for (const o of options) {
          items.push({ kind: "button", title: o.label, icon: o.value === value ? Check : undefined, action: () => onChange(o.value) });
          if (o.divider) items.push({ kind: "divider" });
        }
        openMenuAt(items, r.left, r.bottom + 4);
      }}
    >
      <span>{current}</span>
      <ChevronsUpDown size={11} strokeWidth={2.2} />
    </button>
  );
}

export function PickerRow({ label, subtitle, children }: { label: string; subtitle?: string; children: ReactNode }) {
  return (
    <div className={"settings-row" + (subtitle ? " is-top" : "")}>
      <RowLabel label={label} subtitle={subtitle} />
      <div style={{ flex: "none" }}>{children}</div>
    </div>
  );
}

/** macOS slider: thin track, accent fill, round knob; optional step. */
export function Slider({
  value,
  min = 0,
  max = 1,
  step,
  onChange,
  disabled,
}: {
  value: number;
  min?: number;
  max?: number;
  step?: number;
  onChange: (v: number) => void;
  disabled?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const frac = (value - min) / (max - min);
  const set = (x: number) => {
    const r = ref.current!.getBoundingClientRect();
    let v = min + Math.max(0, Math.min(1, (x - r.left - 10) / (r.width - 20))) * (max - min);
    if (step) v = Math.round(v / step) * step;
    onChange(Math.min(max, Math.max(min, +v.toFixed(6))));
  };
  return (
    <div
      ref={ref}
      className={"slider" + (disabled ? " is-disabled" : "")}
      onPointerDown={(e) => {
        if (disabled) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        set(e.clientX);
      }}
      onPointerMove={(e) => !disabled && e.buttons & 1 && set(e.clientX)}
    >
      <div className="slider__track" />
      <div className="slider__fill" style={{ width: `calc(${frac} * (100% - 20px) + 10px)` }} />
      <div className="slider__knob" style={{ left: `calc(${frac} * (100% - 20px))` }} />
    </div>
  );
}

/** `PillButtonStyle` */
export function PillButton({
  primary = false,
  children,
  onClick,
  disabled,
  small,
  muted,
}: {
  primary?: boolean;
  /** Tertiary text, for secondary actions like "Reset to Default". */
  muted?: boolean;
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  small?: boolean;
}) {
  return (
    <button
      className={"pill-btn" + (primary ? " is-primary" : "") + (small ? " is-small" : "")}
      style={muted ? { color: "var(--text-tertiary)" } : undefined}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </button>
  );
}
