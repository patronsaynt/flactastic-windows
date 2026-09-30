/**
 * The editor sheets' field kit: `metaField`, `ArtistsFieldView`,
 * `GenreFieldView`, `SecondaryGenresFieldView`, and the macOS checkbox.
 */
import { Check, Tag, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { setSetting, useSettingsStore } from "../../app/settings";
import { PillButton } from "../settings/Primitives";
import { Popover } from "./Popover";
import "./Editor.css";
import "./Fields.css";

export function FieldLabel({ label, required, trailing }: { label: string; required?: boolean; trailing?: ReactNode }) {
  return (
    <span className="editor-row" style={{ gap: 3 }}>
      <span className="editor-field__label">{label}</span>
      {required && <span className="field-required">*</span>}
      {trailing && (
        <>
          <span style={{ flex: 1 }} />
          {trailing}
        </>
      )}
    </span>
  );
}

/** `metaField`: label (with a required star), plain field on the elevated surface. */
export function MetaField({
  label,
  value,
  onChange,
  required,
  numericOnly,
  width,
  hint,
  autoFocus,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  required?: boolean;
  numericOnly?: boolean;
  width?: number;
  hint?: string;
  autoFocus?: boolean;
}) {
  return (
    <label className="editor-field" style={{ gap: 3, width, flex: width ? "none" : undefined }}>
      <FieldLabel label={label} required={required} />
      <input
        className="editor-input"
        value={value}
        autoFocus={autoFocus}
        inputMode={numericOnly ? "numeric" : undefined}
        onChange={(e) => onChange(numericOnly ? e.target.value.replace(/\D/g, "") : e.target.value)}
      />
      {hint && <span className="editor-field__note">{hint}</span>}
    </label>
  );
}

/** `.toggleStyle(.checkbox)` */
export function Checkbox({
  checked,
  onChange,
  label,
  disabled,
  title,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <label className={"checkbox" + (disabled ? " is-disabled" : "")} title={title}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      <span className={"checkbox__box" + (checked ? " is-on" : "")}>{checked && <Check size={10} strokeWidth={3.5} />}</span>
      <span className="checkbox__label">{label}</span>
    </label>
  );
}

function Chip({ label, onRemove }: { label: string; onRemove: () => void }) {
  return (
    <span className="chip">
      <span>{label}</span>
      <button className="chip__remove" onClick={onRemove} tabIndex={-1}>
        <X size={8} strokeWidth={3.5} />
      </button>
    </span>
  );
}

/**
 * `ArtistsFieldView`: one chip per artist; the inline field commits on
 * Return or a typed `,` / `;`.
 */
export function ArtistsField({
  artists,
  onChange,
  label = "Artist",
  placeholder = "Add artist…",
  compact = false,
}: {
  artists: string[];
  onChange: (a: string[]) => void;
  label?: string | null;
  placeholder?: string;
  compact?: boolean;
}) {
  const [draft, setDraft] = useState("");
  const commit = () => {
    const t = draft.trim();
    if (!t) return;
    onChange([...artists, t]);
    setDraft("");
  };
  return (
    <div className="editor-field" style={{ gap: 4 }}>
      {label && !compact && (
        <FieldLabel
          label={label}
          trailing={artists.length > 1 ? <span className="editor-field__note">{artists.length} artists</span> : undefined}
        />
      )}
      <div className={"chip-field" + (compact ? " is-compact" : "")}>
        {artists.map((a, i) => (
          <Chip key={i} label={a} onRemove={() => onChange(artists.filter((_, j) => j !== i))} />
        ))}
        <input
          className="chip-field__input"
          style={{ minWidth: compact ? 80 : 110 }}
          value={draft}
          placeholder={artists.length ? "Add another" : placeholder}
          onChange={(e) => {
            const v = e.target.value;
            const last = v.at(-1);
            if (last === "," || last === ";") {
              const t = v.slice(0, -1).trim();
              if (t) onChange([...artists, t]);
              setDraft("");
            } else {
              setDraft(v);
            }
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commit();
            }
          }}
        />
      </div>
    </div>
  );
}

export const PREDEFINED_GENRES = [
  "Alternative", "Ambient", "Blues", "Classical", "Country",
  "Electronic", "Folk", "Hip-Hop", "Indie", "Jazz",
  "Latin", "Metal", "Pop", "Punk", "R&B",
  "Reggae", "Rock", "Soul", "Soundtrack", "World",
];

function useCustomGenres(): [string[], (g: string[]) => void] {
  const custom = useSettingsStore((s) => (s.raw["flactastic.customGenres"] as string[] | undefined) ?? []);
  return [custom, (g) => setSetting("flactastic.customGenres", g)];
}

function TagButton({
  anchor,
  active,
  onClick,
  disabled,
  title,
}: {
  anchor: React.RefObject<HTMLButtonElement | null>;
  active: boolean;
  onClick: () => void;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button ref={anchor} className={"tag-button" + (active ? " is-active" : "")} onClick={onClick} disabled={disabled} title={title}>
      <Tag size={11} />
    </button>
  );
}

/** `GenreFieldView`: text field plus a tag button opening the genre picker. */
export function GenreField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const [custom, setCustom] = useCustomGenres();
  const close = useCallback(() => setOpen(false), []);
  return (
    <div className="editor-field" style={{ gap: 3, flex: 1 }}>
      <FieldLabel label="Genre" />
      <div className="editor-row" style={{ gap: 4 }}>
        <input className="editor-input" style={{ flex: 1, minWidth: 0 }} value={value} onChange={(e) => onChange(e.target.value)} />
        <TagButton anchor={anchor} active={open} onClick={() => setOpen((o) => !o)} />
      </div>
      <Popover anchor={anchor} open={open} onClose={close}>
        <GenrePicker
          genres={[...PREDEFINED_GENRES, ...custom]}
          custom={custom}
          isSelected={(g) => value.toLowerCase() === g.toLowerCase()}
          onSelect={onChange}
          onRemoveCustom={(g) => setCustom(custom.filter((x) => x !== g))}
          onAddCustom={(g) => setCustom([...custom, g])}
        />
      </Popover>
    </div>
  );
}

const CAP = 3;

/** `SecondaryGenresFieldView`: up to three, never the primary. */
export function SecondaryGenresField({
  genres,
  onChange,
  primary,
}: {
  genres: string[];
  onChange: (g: string[]) => void;
  primary: string;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const [custom, setCustom] = useCustomGenres();
  const close = useCallback(() => setOpen(false), []);

  // Keep primary and secondary disjoint as the primary changes.
  useEffect(() => {
    const key = primary.toLowerCase();
    if (genres.some((g) => g.toLowerCase() === key)) onChange(genres.filter((g) => g.toLowerCase() !== key));
  }, [primary]); // eslint-disable-line react-hooks/exhaustive-deps

  const toggle = (g: string) => {
    const i = genres.findIndex((x) => x.toLowerCase() === g.toLowerCase());
    if (i >= 0) onChange(genres.filter((_, j) => j !== i));
    else if (genres.length < CAP) onChange([...genres, g]);
  };
  const atCap = genres.length >= CAP;
  const pickable = [...PREDEFINED_GENRES, ...custom].filter((g) => g.toLowerCase() !== primary.toLowerCase());

  return (
    <div className="editor-field" style={{ gap: 3 }}>
      <FieldLabel
        label="Secondary Genres"
        trailing={
          <span className="editor-field__note">
            {genres.length}/{CAP}
          </span>
        }
      />
      <div className="chip-field" style={{ alignItems: "flex-start" }}>
        <div className="chip-field__chips">
          {genres.map((g) => (
            <Chip key={g} label={g} onRemove={() => onChange(genres.filter((x) => x.toLowerCase() !== g.toLowerCase()))} />
          ))}
          {!genres.length && <span className="chip-field__none">None</span>}
        </div>
        <TagButton
          anchor={anchor}
          active={open}
          onClick={() => setOpen((o) => !o)}
          disabled={atCap}
          title={atCap ? `Up to ${CAP} secondary genres` : "Add a secondary genre"}
        />
      </div>
      <Popover anchor={anchor} open={open} onClose={close}>
        <GenrePicker
          note={atCap ? `Up to ${CAP} secondary genres` : undefined}
          genres={pickable}
          custom={custom}
          isSelected={(g) => genres.some((x) => x.toLowerCase() === g.toLowerCase())}
          isDisabled={(_g, sel) => !sel && atCap}
          onSelect={toggle}
          onRemoveCustom={(g) => setCustom(custom.filter((x) => x !== g))}
          onAddCustom={(g) => {
            setCustom([...custom, g]);
            toggle(g);
          }}
        />
      </Popover>
    </div>
  );
}

/** The picker popover: chip grid, then a "Custom" add field. */
function GenrePicker({
  genres,
  custom,
  isSelected,
  isDisabled,
  onSelect,
  onRemoveCustom,
  onAddCustom,
  note,
}: {
  genres: string[];
  custom: string[];
  isSelected: (g: string) => boolean;
  isDisabled?: (g: string, selected: boolean) => boolean;
  onSelect: (g: string) => void;
  onRemoveCustom: (g: string) => void;
  onAddCustom: (g: string) => void;
  note?: string;
}) {
  const [text, setText] = useState("");
  const t = text.trim();
  const addDisabled = !t || PREDEFINED_GENRES.includes(t) || custom.includes(t);
  const add = () => {
    if (addDisabled) return;
    onAddCustom(t);
    setText("");
  };
  return (
    <div className="genre-picker">
      {note && <div className="editor-field__note">{note}</div>}
      <div className="genre-picker__grid">
        {genres.map((g) => {
          const sel = isSelected(g);
          const dis = isDisabled?.(g, sel) ?? false;
          const isCustom = custom.includes(g);
          return (
            <span key={g} className={"genre-chip" + (sel ? " is-selected" : "")} style={{ opacity: dis ? 0.4 : 1 }}>
              <button className="genre-chip__label" disabled={dis} onClick={() => onSelect(g)}>
                {g}
              </button>
              {isCustom && (
                <button className="genre-chip__remove" onClick={() => onRemoveCustom(g)}>
                  <X size={8} strokeWidth={2.5} />
                </button>
              )}
            </span>
          );
        })}
      </div>
      <div className="genre-picker__divider" />
      <div className="genre-picker__section">Custom</div>
      <div className="editor-row" style={{ gap: "var(--space-sm)" }}>
        <input
          className="editor-input"
          style={{ flex: 1, padding: "5px 8px" }}
          placeholder="Genre name…"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <PillButton onClick={add} disabled={addDisabled}>
          Add
        </PillButton>
      </div>
    </div>
  );
}
