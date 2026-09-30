import {
  Check,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  CopyPlus,
  File,
  Folder,
  FolderPlus,
  GalleryVerticalEnd,
  Menu as Grip,
  Pencil,
  Plus,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useLibrary } from "../../app/library";
import { setSetting, useSettingsStore } from "../../app/settings";
import { on } from "../../lib/api";
import { invoke } from "../../lib/native";
import { useReorder } from "../../lib/reorder";
import { confirmDialog } from "../../components/sheet/ConfirmDialog";
import { Modal } from "../../components/sheet/Sheet";
import { menu, openMenuAt, type MenuItem } from "../../components/menu/ContextMenu";
import "./Organizer.css";

interface Level {
  id: string;
  groupBy: string;
  name: string;
  nameTemplate: string;
}

interface Profile {
  id: string;
  name: string;
  levels: Level[];
  fileTemplate: string;
  usePrimaryArtistOnly: boolean;
  deleteEmptyOriginals: boolean;
}

interface Row {
  id: string;
  depth: number;
  label: string;
  isFolder: boolean;
  badge: string | null;
  isConflict: boolean;
  isUnchanged: boolean;
}

interface Plan {
  moveCount: number;
  unchangedCount: number;
  conflictCount: number;
  rows: Row[];
  hiddenTrackCount: number;
  trackCount: number;
}

const GROUP_NAMES: Record<string, string> = {
  albumArtist: "Album Artist",
  artist: "Artist",
  album: "Album",
  genre: "Genre",
  year: "Year",
  decade: "Decade",
  format: "Format",
  firstLetterOfArtist: "Artist Initial",
};

const labelOf = (l: Level) => l.name.trim() || GROUP_NAMES[l.groupBy] || l.groupBy;

type Target = "filename" | { level: string };
const sameTarget = (a: Target, b: Target) => (a === "filename" ? b === "filename" : b !== "filename" && a.level === b.level);

/** `OrganizerProfilesStore`, backed by the Mac's two settings keys. */
function useProfiles() {
  const raw = useSettingsStore((s) => s.raw);
  const profiles = (raw["flactastic.organizer.profiles"] as Profile[] | undefined) ?? [];
  const selectedId = raw["flactastic.organizer.selectedProfileID"] as string | undefined;
  const selected = profiles.find((p) => p.id === selectedId) ?? profiles[0];
  const setProfiles = (p: Profile[]) => setSetting("flactastic.organizer.profiles", p);
  const select = (id: string) => setSetting("flactastic.organizer.selectedProfileID", id);
  const update = (next: Profile) => setProfiles(profiles.map((p) => (p.id === next.id ? next : p)));
  const add = (p: Profile) => {
    setProfiles([...profiles, p]);
    select(p.id);
  };
  const removeSelected = () => {
    if (profiles.length <= 1 || !selected) return;
    const idx = profiles.findIndex((p) => p.id === selected.id);
    const rest = profiles.filter((p) => p.id !== selected.id);
    setProfiles(rest);
    select(rest[Math.max(0, idx - 1)].id);
  };
  return { profiles, selected, select, update, add, removeSelected };
}

/**
 * `OrganizerView`: a rule builder on the left (folder hierarchy, file name
 * template, options, tag guide) and a live destination preview on the right.
 */
export function OrganizerView() {
  const { profiles, selected, select, update, add, removeSelected } = useProfiles();
  const root = useLibrary((s) => s.root);
  const revision = useLibrary((s) => s.revision);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [recomputing, setRecomputing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resultMessage, setResultMessage] = useState<string | null>(null);
  const [applying, setApplying] = useState<{ label: string; progress: number } | null>(null);
  const [target, setTarget] = useState<Target>("filename");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [tokens, setTokens] = useState<{ placeholder: string; description: string }[]>([]);

  useEffect(() => {
    void invoke<{ placeholder: string; description: string }[]>("organizer_tokens").then((t) => t && setTokens(t));
  }, []);

  // `schedulePreview`: re-plan 300 ms after the rules or library settle.
  const planKey = selected ? JSON.stringify(selected) : "";
  useEffect(() => {
    if (!selected) return;
    if (!root) {
      setPlan(null);
      setRecomputing(false);
      setError("Choose a source folder in Settings before organizing.");
      return;
    }
    setError(null);
    setRecomputing(true);
    let live = true;
    const t = setTimeout(() => {
      void invoke<Plan>("organizer_plan", { profile: selected })
        .then((p) => live && p && setPlan(p))
        .catch((e) => live && setError(String(e)))
        .finally(() => live && setRecomputing(false));
    }, 300);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [planKey, root, revision]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(
    () =>
      on<{ phase: string; completed: number; total: number }>("organizer://progress", (p) => {
        const label = p.phase === "moving" ? "Moving files" : p.phase === "cleaningUp" ? "Cleaning up empty folders" : "Validating";
        setApplying({
          label: p.total > 0 ? `${label} (${p.completed}/${p.total})` : label,
          progress: p.total > 0 ? p.completed / p.total : 0,
        });
      }),
    [],
  );

  if (!selected) return null;
  const mutate = (fn: (p: Profile) => Profile) => update(fn(selected));

  const apply = async () => {
    setApplying({ label: "Preparing…", progress: 0 });
    setError(null);
    try {
      const r = await invoke<{ error: string | null; message: string | null }>("organizer_apply");
      setError(r?.error ?? null);
      setResultMessage(r?.message ?? null);
      setPlan(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setApplying(null);
    }
  };

  const confirmApply = () => {
    if (!plan) return;
    const c = plan.conflictCount;
    confirmDialog({
      title: "Apply organization?",
      message: `This will move files in place inside your source folder. ${c} conflict${c === 1 ? "" : "s"} will be resolved by appending a numeric suffix.`,
      buttons: [{ title: `Move ${plan.moveCount} files`, destructive: true, action: () => void apply() }],
    });
  };

  const insert = (placeholder: string) =>
    mutate((p) =>
      target === "filename"
        ? { ...p, fileTemplate: p.fileTemplate + placeholder }
        : { ...p, levels: p.levels.map((l) => (l.id === target.level ? { ...l, nameTemplate: l.nameTemplate + placeholder } : l)) },
    );

  return (
    <div className="organizer">
      <header className="organizer__header">
        <span className="organizer__icon">
          <FolderPlus size={17} />
        </span>
        <div className="organizer__heading">
          <div className="organizer__title">Organizer</div>
          <div className="organizer__subtitle">Define how your library gets sorted into folders and named on disk.</div>
        </div>
        <ProfilePicker
          profiles={profiles}
          selected={selected}
          onSelect={select}
          onAdd={add}
          onRename={() => setRenaming(selected.name)}
          onDelete={removeSelected}
        />
      </header>
      <div className="organizer__divider" />
      <div className="organizer__body">
        <Builder profile={selected} mutate={mutate} target={target} setTarget={setTarget} tokens={tokens} insert={insert} />
        <div className="organizer__vdivider" />
        <Preview
          plan={plan}
          root={root}
          recomputing={recomputing}
          applying={applying != null}
          error={error}
          resultMessage={resultMessage}
          onApply={confirmApply}
        />
      </div>
      {applying && (
        <div className="organizer__overlay">
          <div className="organizer__overlay-card">
            <div className="organizer__progress">
              <div style={{ width: `${applying.progress * 100}%` }} />
            </div>
            <div className="organizer__overlay-label">{applying.label}</div>
            <div className="organizer__overlay-hint">Don't quit the app until this finishes.</div>
          </div>
        </div>
      )}
      <Modal open={renaming != null} onClose={() => setRenaming(null)}>
        {renaming != null && (
          <RenameSheet
            initial={renaming}
            onCancel={() => setRenaming(null)}
            onSave={(n) => {
              if (n.trim()) mutate((p) => ({ ...p, name: n.trim() }));
              setRenaming(null);
            }}
          />
        )}
      </Modal>
    </div>
  );
}

function ProfilePicker({
  profiles,
  selected,
  onSelect,
  onAdd,
  onRename,
  onDelete,
}: {
  profiles: Profile[];
  selected: Profile;
  onSelect: (id: string) => void;
  onAdd: (p: Profile) => void;
  onRename: () => void;
  onDelete: () => void;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  const openMenu = async () => {
    const presets = (await invoke<Profile[]>("organizer_presets")) ?? [];
    const r = ref.current!.getBoundingClientRect();
    const items: MenuItem[] = [
      ...profiles.map((p) => menu.button(p.name, () => onSelect(p.id), p.id === selected.id ? Check : undefined)),
      menu.divider,
      menu.label("New from preset"),
      ...presets.map((p) => menu.button(p.name, () => onAdd(p))),
    ];
    openMenuAt(items, r.left, r.bottom + 6);
  };
  return (
    <div className="profile-picker">
      <button ref={ref} className="profile-picker__menu" onClick={() => void openMenu()}>
        <span>{selected.name}</span>
        <ChevronDown size={10} strokeWidth={3} />
      </button>
      <span className="profile-picker__rule" />
      <IconButton help="Rename profile" circular onClick={onRename}>
        <Pencil size={13} />
      </IconButton>
      <IconButton
        help="Duplicate profile"
        circular
        onClick={() => void invoke<Profile>("organizer_duplicate", { profile: selected }).then((p) => p && onAdd(p))}
      >
        <CopyPlus size={13} />
      </IconButton>
      <IconButton help="Delete profile" circular danger enabled={profiles.length > 1} onClick={onDelete}>
        <Trash2 size={13} />
      </IconButton>
    </div>
  );
}

/** `OrganizerIconButton`: borderless, filled on hover. */
function IconButton({
  help,
  circular,
  danger,
  enabled = true,
  onClick,
  children,
}: {
  help: string;
  circular?: boolean;
  danger?: boolean;
  enabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      className={"org-icon" + (circular ? " is-circular" : "") + (danger ? " is-danger" : "")}
      title={help}
      disabled={!enabled}
      onClick={onClick}
    >
      {children}
    </button>
  );
}

function RenameSheet({ initial, onCancel, onSave }: { initial: string; onCancel: () => void; onSave: (n: string) => void }) {
  const [v, setV] = useState(initial);
  return (
    <div className="rename-sheet">
      <div className="rename-sheet__title">Rename profile</div>
      <input
        className="rounded-field"
        style={{ width: "100%", boxSizing: "border-box" }}
        placeholder="Profile name"
        value={v}
        autoFocus
        onChange={(e) => setV(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && onSave(v)}
      />
      <div className="rename-sheet__buttons">
        <button className="alert__button" style={{ padding: "0 14px" }} onClick={onCancel}>
          Cancel
        </button>
        <button className="alert__button is-default" style={{ padding: "0 14px" }} onClick={() => onSave(v)}>
          Save
        </button>
      </div>
    </div>
  );
}

// MARK: - Builder

function Builder({
  profile,
  mutate,
  target,
  setTarget,
  tokens,
  insert,
}: {
  profile: Profile;
  mutate: (fn: (p: Profile) => Profile) => void;
  target: Target;
  setTarget: (t: Target) => void;
  tokens: { placeholder: string; description: string }[];
  insert: (placeholder: string) => void;
}) {
  const [guideOpen, setGuideOpen] = useState(false);
  const examples = useExamples(profile);
  const levels = profile.levels;

  const moveLevel = useCallback(
    (id: string, to: number) =>
      mutate((p) => {
        const from = p.levels.findIndex((l) => l.id === id);
        const dest = Math.min(Math.max(to, 0), p.levels.length - 1);
        if (from < 0 || from === dest) return p;
        const next = [...p.levels];
        const [l] = next.splice(from, 1);
        next.splice(dest, 0, l);
        return { ...p, levels: next };
      }),
    [mutate],
  );
  const reorder = useReorder({ onMove: (src, dst) => moveLevel(src, levels.findIndex((l) => l.id === dst)) });

  const setLevel = (id: string, patch: Partial<Level>) =>
    mutate((p) => ({ ...p, levels: p.levels.map((l) => (l.id === id ? { ...l, ...patch } : l)) }));

  const chipRow = (
    <div className="token-row">
      {tokens.map((t) => (
        <button key={t.placeholder} className="token-chip" title={t.description} onMouseDown={(e) => e.preventDefault()} onClick={() => insert(t.placeholder)}>
          {t.placeholder}
        </button>
      ))}
    </div>
  );

  return (
    <div className="builder">
      <SectionHeader title="Folder hierarchy" subtitle="Each level nests inside the one above — files land in the deepest folder." />
      <div className="levels">
        {levels.map((level, index) => {
          const active = sameTarget(target, { level: level.id });
          return (
            <div
              key={level.id}
              className={"level-row" + (reorder.target === level.id && reorder.dragging !== level.id ? " is-target" : "")}
              style={{ paddingLeft: index * 20, opacity: reorder.dragging === level.id ? 0.4 : 1 }}
              {...reorder.rowProps(level.id)}
            >
              {index > 0 && (
                <span className="level-rail">
                  <span className="level-rail__stem" />
                  <span className="level-rail__stub" />
                </span>
              )}
              <div className={"level-card" + (active ? " is-active" : "")}>
                <div className="level-card__head">
                  <span className="level-card__grip" title="Drag to reorder" {...reorder.handleProps(level.id, labelOf(level))}>
                    <Grip size={11} strokeWidth={2.6} />
                  </span>
                  <Folder size={13} className="level-card__folder" />
                  <input
                    className="level-card__name"
                    placeholder={GROUP_NAMES[level.groupBy]}
                    value={level.name}
                    onChange={(e) => setLevel(level.id, { name: e.target.value })}
                    onFocus={() => setTarget({ level: level.id })}
                  />
                  <span style={{ flex: 1 }} />
                  <IconButton help="Move up" enabled={index > 0} onClick={() => moveLevel(level.id, index - 1)}>
                    <ChevronUp size={12} />
                  </IconButton>
                  <IconButton help="Move down" enabled={index < levels.length - 1} onClick={() => moveLevel(level.id, index + 1)}>
                    <ChevronDown size={12} />
                  </IconButton>
                  <IconButton
                    help="Remove level"
                    danger
                    enabled={levels.length > 1}
                    onClick={() => {
                      mutate((p) => ({ ...p, levels: p.levels.filter((l) => l.id !== level.id) }));
                      if (active) setTarget("filename");
                    }}
                  >
                    <Trash2 size={12} />
                  </IconButton>
                </div>
                <div className="level-card__body">
                  <TemplateField
                    value={level.nameTemplate}
                    onChange={(v) => setLevel(level.id, { nameTemplate: v })}
                    onFocus={() => setTarget({ level: level.id })}
                  />
                  <ExampleLine text={examples[index] ?? ""} />
                  {active && chipRow}
                </div>
              </div>
            </div>
          );
        })}
        {reorder.ghost}
        <button
          className="add-level"
          title="Add another folder level below the last one"
          onClick={() =>
            void invoke<Level>("organizer_new_level").then((l) => {
              if (!l) return;
              mutate((p) => ({ ...p, levels: [...p.levels, l] }));
              setTarget({ level: l.id });
            })
          }
        >
          <Plus size={11} strokeWidth={3} /> Add level
        </button>
      </div>

      <div className="section-divider" />
      <SectionHeader title="File name" subtitle="How each track file is named inside its final folder." />
      <div className={"filename-card" + (target === "filename" ? " is-active" : "")}>
        <TemplateField
          value={profile.fileTemplate}
          onChange={(v) => mutate((p) => ({ ...p, fileTemplate: v }))}
          onFocus={() => setTarget("filename")}
        />
        <ExampleLine text={examples[levels.length] ?? ""} />
        {chipRow}
      </div>

      <div className="section-divider" />
      <div className="setting-rows">
        <SettingRow
          title="Use primary artist only"
          subtitle="For tracks credited to multiple artists (“A & B”, “A feat. B”, “A; B”), file under just the first."
          on={profile.usePrimaryArtistOnly}
          onChange={(v) => mutate((p) => ({ ...p, usePrimaryArtistOnly: v }))}
        />
        <SettingRow
          title="Delete empty original folders"
          subtitle="After moves complete, remove any source folders that no longer contain audio (cover art and other leftovers are swept up too)."
          on={profile.deleteEmptyOriginals}
          onChange={(v) => mutate((p) => ({ ...p, deleteEmptyOriginals: v }))}
        />
      </div>

      <div className="section-divider" />
      <button className="tag-guide__toggle" onClick={() => setGuideOpen((o) => !o)}>
        <ChevronRight size={11} strokeWidth={3} className={"tag-guide__chevron" + (guideOpen ? " is-open" : "")} />
        <span className="tag-guide__title">Tag guide</span>
        <span className="tag-guide__count">{tokens.length} tokens</span>
      </button>
      {guideOpen && (
        <div className="tag-guide__grid">
          {tokens.map((t) => (
            <button key={t.placeholder} className="token-card" title={`Insert ${t.placeholder}`} onClick={() => insert(t.placeholder)}>
              <span className="token-card__token">{t.placeholder}</span>
              <span className="token-card__desc">{t.description}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** Example lines for each level, then the file name. */
function useExamples(profile: Profile): string[] {
  const revision = useLibrary((s) => s.revision);
  const [out, setOut] = useState<string[]>([]);
  const key = JSON.stringify([profile.levels.map((l) => [l.nameTemplate, labelOf(l)]), profile.fileTemplate, profile.usePrimaryArtistOnly]);
  useEffect(() => {
    let live = true;
    const requests = [
      ...profile.levels.map((l) => ({ template: l.nameTemplate, fallback: labelOf(l) })),
      { template: profile.fileTemplate, fallback: "Untitled", isFilename: true },
    ];
    void invoke<string[]>("organizer_examples", { requests, primaryArtistOnly: profile.usePrimaryArtistOnly }).then(
      (r) => live && r && setOut(r),
    );
    return () => {
      live = false;
    };
  }, [key, revision]); // eslint-disable-line react-hooks/exhaustive-deps
  return out;
}

function SectionHeader({ title, subtitle }: { title: string; subtitle: string }) {
  return (
    <div className="org-section">
      <div className="org-section__title">{title}</div>
      <div className="org-section__subtitle">{subtitle}</div>
    </div>
  );
}

function TemplateField({ value, onChange, onFocus }: { value: string; onChange: (v: string) => void; onFocus: () => void }) {
  return <input className="template-field" value={value} spellCheck={false} onChange={(e) => onChange(e.target.value)} onFocus={onFocus} />;
}

function ExampleLine({ text }: { text: string }) {
  return (
    <div className="example-line">
      <span className="example-line__label">Example:</span>
      <span className="example-line__value" title={text}>
        {text}
      </span>
    </div>
  );
}

/** `OrganizerPillToggle`: 40×24 track, 18 pt knob. */
function SettingRow({ title, subtitle, on, onChange }: { title: string; subtitle: string; on: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="setting-row">
      <button className={"pill-toggle-switch" + (on ? " is-on" : "")} role="switch" aria-checked={on} onClick={() => onChange(!on)}>
        <span />
      </button>
      <div className="setting-row__text">
        <div className="setting-row__title">{title}</div>
        <div className="setting-row__subtitle">{subtitle}</div>
      </div>
    </div>
  );
}

// MARK: - Preview

function Preview({
  plan,
  root,
  recomputing,
  applying,
  error,
  resultMessage,
  onApply,
}: {
  plan: Plan | null;
  root: string | null;
  recomputing: boolean;
  applying: boolean;
  error: string | null;
  resultMessage: string | null;
  onApply: () => void;
}) {
  const trackCount = useLibrary((s) => s.tracks.length);
  const subtitle = useMemo(() => {
    if (!root) return "Choose a source folder in Settings to see a preview.";
    if (recomputing && !plan) return "Working out where everything lands…";
    return `Updates live as you edit the rules. Showing how ${trackCount} file${trackCount === 1 ? "" : "s"} would land.`;
  }, [root, recomputing, plan, trackCount]);

  const moveCount = plan?.moveCount ?? 0;
  const enabled = !applying && !recomputing && moveCount > 0 && root != null;
  const footer = !root
    ? "Choose a source folder in Settings first."
    : recomputing
      ? "Recalculating…"
      : !plan || plan.rows.length === 0
        ? "No files will move until you apply."
        : moveCount === 0
          ? "Everything is already where these rules want it."
          : `${moveCount} file${moveCount === 1 ? "" : "s"} to move · ${plan.unchangedCount} already in place. Nothing moves until you apply.`;

  return (
    <div className="preview">
      <div className="org-section preview__head">
        <div className="org-section__title">Preview</div>
        <div className="org-section__subtitle">{subtitle}</div>
      </div>
      <div className="preview__scroll">
        {error ? (
          <Notice color="var(--quality-low)" text={error} />
        ) : resultMessage && !plan?.rows.length ? (
          <Notice color="var(--quality-cd)" text={resultMessage} />
        ) : null}
        {plan && plan.conflictCount > 0 && (
          <Notice
            color="var(--quality-mid)"
            text={`${plan.conflictCount} destination${plan.conflictCount === 1 ? "" : "s"} collide — a numeric suffix will be appended unless you adjust the file name.`}
          />
        )}
        {!plan || plan.rows.length === 0 ? (
          <div className="preview__empty">
            <GalleryVerticalEnd size={32} />
            <div className="preview__empty-title">{recomputing ? "Building preview…" : "Nothing to preview yet"}</div>
            <div className="preview__empty-hint">
              {!root ? "Choose a source folder in Settings, then come back." : "Scan a library folder to see how your rules reshape it."}
            </div>
          </div>
        ) : (
          <div className="preview__tree">
            {plan.rows.map((r) => (
              <div key={r.id} className="tree-row" style={{ paddingLeft: 10 + r.depth * 22 }}>
                {r.isFolder ? <Folder size={12} className="tree-row__folder" /> : <File size={12} className="tree-row__file" />}
                <span
                  className="tree-row__label"
                  style={{
                    color: r.isConflict
                      ? "var(--quality-mid)"
                      : r.isFolder
                        ? "var(--text-primary)"
                        : r.isUnchanged
                          ? "var(--text-tertiary)"
                          : "var(--text-secondary)",
                  }}
                >
                  {r.label}
                </span>
                {r.badge && <span className={"tree-row__badge" + (r.isConflict ? " is-conflict" : "")}>{r.badge}</span>}
              </div>
            ))}
            {plan.hiddenTrackCount > 0 && (
              <div className="preview__more">
                + {plan.hiddenTrackCount} more track{plan.hiddenTrackCount === 1 ? "" : "s"} organized the same way
              </div>
            )}
          </div>
        )}
      </div>
      <div className="organizer__divider" />
      <div className="preview__footer">
        <span className="preview__footer-text">{footer}</span>
        <button className={"apply-button" + (enabled ? " is-enabled" : "")} disabled={!enabled} onClick={onApply}>
          {applying ? <span className="spinner" /> : <Check size={11} strokeWidth={3.5} />}
          {applying ? "Applying…" : "Apply"}
        </button>
      </div>
    </div>
  );
}

function Notice({ color, text }: { color: string; text: string }) {
  return (
    <div className="notice" style={{ background: `color-mix(in srgb, ${color} 12%, transparent)` }}>
      <span className="notice__dot" style={{ background: color }} />
      <span className="notice__text">{text}</span>
    </div>
  );
}
