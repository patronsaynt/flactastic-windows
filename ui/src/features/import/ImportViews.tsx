import { Equal, ListMusic } from "lucide-react";
import { useEffect, useState } from "react";
import { api, on, type ArtworkEdit, type LoadedImage, type Track, type TrackEdit } from "../../lib/api";
import { useReorder } from "../../lib/reorder";
import { ArtworkView } from "../../components/ArtworkView";
import { alertDialog } from "../../components/sheet/ConfirmDialog";
import { FLSheet, Modal } from "../../components/sheet/Sheet";
import { PillButton } from "../../components/settings/Primitives";
import { ImageCropper, pickImage } from "../../components/editors/ImageCropper";
import { FieldLabel, MetaField } from "../../components/editors/Fields";
import { ImportDropView } from "./ImportDropView";
import "../../components/editors/Editor.css";
import "../editors/Editors.css";
import "../playlists/Playlists.css";
import "./Import.css";

type Stage = "dropping" | "loading" | "editing";

function mostCommon<T>(values: (T | null | undefined)[]): T | null {
  const counts = new Map<T, number>();
  for (const v of values) if (v != null && v !== "") counts.set(v, (counts.get(v) ?? 0) + 1);
  let best: T | null = null;
  let n = 0;
  for (const [v, c] of counts) if (c > n) [best, n] = [v, c];
  return best;
}

/** Stage 1 → 2 plumbing shared by the three sheets. */
function useLoader(onLoaded: (tracks: Track[]) => void) {
  const [stage, setStage] = useState<Stage>("dropping");
  const [progress, setProgress] = useState({ loaded: 0, total: 0 });
  useEffect(() => on<{ loaded: number; total: number }>("import://progress", setProgress), []);
  const load = async (paths: string[]) => {
    setStage("loading");
    setProgress({ loaded: 0, total: paths.length });
    try {
      const tracks = (await api.importLoad(paths)) ?? [];
      onLoaded(tracks);
      setStage("editing");
    } catch (e) {
      setStage("dropping");
      alertDialog("Import Failed", String(e));
    }
  };
  return { stage, progress, load };
}

function Loading({ text }: { text: string }) {
  return (
    <div className="import-loading">
      <span className="spinner" />
      <span className="editor-caption">{text}</span>
    </div>
  );
}

function ArtworkColumn({
  artwork,
  onPick,
  onRemove,
  title,
}: {
  artwork: string | null;
  onPick: () => void;
  onRemove: () => void;
  title: string;
}) {
  return (
    <div className="meta-editor__art">
      <button className="meta-editor__art-button" title={title} onClick={onPick}>
        <ArtworkView artwork={artwork} size={130} />
        {!artwork && <span className="meta-editor__add">Click to add</span>}
      </button>
      {artwork && (
        <button className="editor-text-button" onClick={onRemove}>
          Remove
        </button>
      )}
    </div>
  );
}

// MARK: - Import Track

/** `ImportTrackView`: one file, its tags pre-filled, copied into the root. */
export function ImportTrackView({ onClose }: { onClose: () => void }) {
  const [source, setSource] = useState<Track | null>(null);
  const [title, setTitle] = useState("");
  const [artist, setArtist] = useState("");
  const [album, setAlbum] = useState("");
  const [year, setYear] = useState("");
  const [genre, setGenre] = useState("");
  const [trackNumber, setTrackNumber] = useState("");
  const [artwork, setArtwork] = useState<string | null>(null);
  const [artChange, setArtChange] = useState<ArtworkEdit>({ kind: "unchanged" });
  const [saving, setSaving] = useState(false);

  const { stage, load } = useLoader(([t]) => {
    setSource(t);
    setTitle(t.title);
    setArtist(t.artist ?? "");
    setAlbum(t.album ?? "");
    setYear(t.year != null ? String(t.year) : "");
    setGenre(t.genre ?? "");
    setTrackNumber(t.trackNumber != null ? String(t.trackNumber) : "");
    setArtwork(t.artwork);
    setArtChange({ kind: "unchanged" });
  });

  const pick = async () => {
    try {
      const img = await pickImage("Choose cover artwork");
      if (!img) return;
      setArtwork(img.id);
      setArtChange({ kind: "updated", id: img.id });
    } catch (e) {
      alertDialog("Couldn't Open Image", String(e));
    }
  };

  const save = async () => {
    if (!source) return;
    setSaving(true);
    const edit: TrackEdit = {
      title: title.trim(),
      artist: artist || null,
      album: album || null,
      year: year ? Number(year) : null,
      genre: genre || null,
      // Keep the file's own secondary genres (the Mac drops them here).
      secondaryGenres: source.secondaryGenres,
      trackNumber: trackNumber ? Number(trackNumber) : null,
      artwork: artChange,
    };
    try {
      const r = await api.importCommit([{ token: source.id, edit }], null);
      if (r?.error) throw r.error;
      onClose();
    } catch (e) {
      setSaving(false);
      alertDialog("Import Failed", String(e));
    }
  };

  return (
    <FLSheet
      title="Import Track"
      width={520}
      height={500}
      onClose={onClose}
      footer={
        <div className="editor-footer">
          <div style={{ flex: 1 }} />
          <PillButton onClick={onClose} disabled={saving}>
            Cancel
          </PillButton>
          <PillButton primary onClick={() => void save()} disabled={stage !== "editing" || saving || !title.trim()}>
            {saving ? "Importing…" : "Import"}
          </PillButton>
        </div>
      }
    >
      {stage === "dropping" && (
        <div className="import-drop-pad">
          <ImportDropView title="Import Track" allowsMultiple={false} onFiles={(p) => void load(p)} />
        </div>
      )}
      {stage === "loading" && <Loading text="Reading metadata…" />}
      {stage === "editing" && (
        <div className="meta-editor">
          <ArtworkColumn
            artwork={artwork}
            title="Click to choose cover artwork"
            onPick={() => void pick()}
            onRemove={() => {
              setArtwork(null);
              setArtChange({ kind: "removed" });
            }}
          />
          <div className="meta-editor__fields" style={{ gap: "var(--space-sm)" }}>
            <MetaField label="Song Name" value={title} onChange={setTitle} required autoFocus />
            <MetaField label="Artist" value={artist} onChange={setArtist} />
            <MetaField label="Album" value={album} onChange={setAlbum} />
            <div className="editor-row">
              <MetaField label="Year" value={year} onChange={setYear} numericOnly width={80} />
              <MetaField label="Track #" value={trackNumber} onChange={setTrackNumber} numericOnly width={80} />
            </div>
            <MetaField label="Genre" value={genre} onChange={setGenre} />
          </div>
        </div>
      )}
    </FLSheet>
  );
}

// MARK: - Import Album

interface Row {
  token: string;
  title: string;
  source: Track;
}

/**
 * `ImportAlbumView`: album fields for every file plus per-track titles and
 * order; files land in `<root>/<artist> - <album>/`, numbered by position.
 */
export function ImportAlbumView({ onClose }: { onClose: () => void }) {
  const [albumName, setAlbumName] = useState("");
  const [artist, setArtist] = useState("");
  const [albumArtist, setAlbumArtist] = useState("");
  const [year, setYear] = useState("");
  const [genre, setGenre] = useState("");
  const [artwork, setArtwork] = useState<string | null>(null);
  const [artChange, setArtChange] = useState<ArtworkEdit>({ kind: "unchanged" });
  const [crop, setCrop] = useState<LoadedImage | null>(null);
  const [rows, setRows] = useState<Row[]>([]);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(0);

  useEffect(() => on<{ saved: number }>("import://saved", (p) => setSaved(p.saved)), []);

  const { stage, progress, load } = useLoader((tracks) => {
    const sorted = [...tracks].sort((a, b) => (a.trackNumber ?? Infinity) - (b.trackNumber ?? Infinity));
    setRows(sorted.map((t) => ({ token: t.id, title: t.title, source: t })));
    setAlbumName(mostCommon(sorted.map((t) => t.album)) ?? "");
    setArtist(mostCommon(sorted.map((t) => t.artist)) ?? "");
    setAlbumArtist(mostCommon(sorted.map((t) => t.albumArtist)) ?? "");
    const y = mostCommon(sorted.map((t) => t.year));
    setYear(y != null ? String(y) : "");
    setGenre(mostCommon(sorted.map((t) => t.genre)) ?? "");
    setArtwork(sorted.find((t) => t.artwork)?.artwork ?? null);
    setArtChange({ kind: "unchanged" });
  });

  const reorder = useReorder({
    live: true,
    onMove: (src, dst) =>
      setRows((r) => {
        const from = r.findIndex((x) => x.token === src);
        const to = r.findIndex((x) => x.token === dst);
        if (from < 0 || to < 0) return r;
        const next = [...r];
        const [item] = next.splice(from, 1);
        next.splice(to, 0, item);
        return next;
      }),
  });

  const pick = async () => {
    try {
      const img = await pickImage("Choose album artwork");
      if (img) setCrop(img);
    } catch (e) {
      alertDialog("Couldn't Open Image", String(e));
    }
  };

  const save = async () => {
    setSaving(true);
    setSaved(0);
    const album = albumName.trim();
    const aa = albumArtist.trim();
    const newArtist = artist || null;
    const items = rows.map((r, i) => ({
      token: r.token,
      edit: {
        title: r.title.trim() || r.source.title,
        artist: newArtist,
        album,
        year: year ? Number(year) : null,
        genre: genre || null,
        secondaryGenres: r.source.secondaryGenres,
        trackNumber: i + 1,
        artwork: artChange,
        albumArtist: aa || null,
      } satisfies TrackEdit,
    }));
    // Prefer the album artist for the folder so a compilation files together.
    const folderArtist = aa || newArtist || "";
    try {
      const r = await api.importCommit(items, { artist: folderArtist, album });
      if (r?.error) throw r.error;
      onClose();
    } catch (e) {
      setSaving(false);
      alertDialog("Import Failed", String(e));
    }
  };

  return (
    <FLSheet
      title="Import Album"
      width={520}
      height={660}
      onClose={onClose}
      footer={
        <div className="editor-footer">
          <div style={{ flex: 1 }} />
          <PillButton onClick={onClose} disabled={saving}>
            Cancel
          </PillButton>
          <PillButton
            primary
            onClick={() => void save()}
            disabled={stage !== "editing" || saving || !albumName.trim() || rows.length === 0}
          >
            {saving ? "Importing…" : "Import Album"}
          </PillButton>
        </div>
      }
    >
      {stage === "dropping" && (
        <div className="import-drop-pad">
          <ImportDropView title="Import Album" allowsMultiple onFiles={(p) => void load(p)} />
        </div>
      )}
      {stage === "loading" && <Loading text={`Reading metadata ${progress.loaded} of ${progress.total}…`} />}
      {stage === "editing" && (
        <div className="album-editor">
          <div className="meta-editor">
            <ArtworkColumn
              artwork={artwork}
              title="Click to choose album artwork"
              onPick={() => void pick()}
              onRemove={() => {
                setArtwork(null);
                setArtChange({ kind: "removed" });
              }}
            />
            <div className="meta-editor__fields" style={{ gap: "var(--space-sm)", alignSelf: "stretch" }}>
              <MetaField label="Album Name" value={albumName} onChange={setAlbumName} required autoFocus />
              <MetaField label="Artist" value={artist} onChange={setArtist} />
              <MetaField label="Album Artist" value={albumArtist} onChange={setAlbumArtist} />
              <div className="editor-row" style={{ alignItems: "flex-start" }}>
                <MetaField label="Year" value={year} onChange={setYear} numericOnly width={80} />
                <div style={{ flex: 1 }}>
                  <MetaField label="Genre" value={genre} onChange={setGenre} />
                </div>
              </div>
              <div style={{ flex: 1 }} />
              <div className="import-count">
                <ListMusic size={11} />
                {saving
                  ? `Imported ${saved} of ${rows.length}…`
                  : `Applies to ${rows.length} track${rows.length === 1 ? "" : "s"}`}
              </div>
            </div>
          </div>
          <div className="sync-divider" />
          <div className="album-editor__tracks-title">TRACKS</div>
          <div className="import-tracks">
            {rows.map((r, i) => (
              <div
                key={r.token}
                className={"import-track" + (reorder.dragging === r.token ? " is-dragging" : "")}
                {...reorder.rowProps(r.token)}
              >
                <span className="import-track__number">{i + 1}</span>
                <input
                  className="editor-input"
                  style={{ flex: 1, minWidth: 0, padding: "5px 8px" }}
                  value={r.title}
                  onChange={(e) => setRows((all) => all.map((x, j) => (j === i ? { ...x, title: e.target.value } : x)))}
                />
                <span
                  className={"album-editor__handle" + (reorder.dragging === r.token ? " is-active" : "")}
                  {...reorder.handleProps(r.token)}
                >
                  <Equal size={12} strokeWidth={2.2} />
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
      <Modal open={crop != null} onClose={() => setCrop(null)}>
        {crop && (
          <ImageCropper
            source={crop}
            title="Crop Cover"
            onComplete={(id) => {
              setArtwork(id);
              setArtChange({ kind: "updated", id });
            }}
            onClose={() => setCrop(null)}
          />
        )}
      </Modal>
    </FLSheet>
  );
}

// MARK: - Import Files as Playlist

const DESCRIPTION_MAX = 200;

/**
 * `ImportPlaylistView`: files copied into the root untouched, then gathered
 * into a new playlist with a name, description and cover.
 */
export function ImportPlaylistView({ onClose }: { onClose: () => void }) {
  const [tracks, setTracks] = useState<Track[]>([]);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [artwork, setArtwork] = useState<string | null>(null);
  const [crop, setCrop] = useState<LoadedImage | null>(null);
  const [saving, setSaving] = useState(false);

  const { stage, progress, load } = useLoader((loaded) => {
    setTracks(loaded);
    const first = loaded[0]?.album;
    setName((n) => n || (first && loaded.every((t) => (t.album ?? "") === first) ? first : "New Playlist"));
  });

  const pick = async () => {
    try {
      const img = await pickImage("Choose a playlist cover image");
      if (img) setCrop(img);
    } catch (e) {
      alertDialog("Couldn't Open Image", String(e));
    }
  };

  const save = async () => {
    setSaving(true);
    try {
      const r = await api.importCommit(
        tracks.map((t) => ({ token: t.id, edit: null })),
        null,
      );
      if (r && r.trackIds.length) {
        const n = name.trim();
        const id = await api.createPlaylist(n);
        if (id) {
          await api.addToPlaylist(id, r.trackIds, false);
          await api.updatePlaylistMetadata(id, n, description.trim() || null, artwork);
        }
      }
      if (r?.error) throw r.error;
      onClose();
    } catch (e) {
      setSaving(false);
      alertDialog("Import Failed", String(e));
    }
  };

  return (
    <FLSheet
      title="Import Files as Playlist"
      width={480}
      height={460}
      onClose={onClose}
      footer={
        <div className="editor-footer">
          <div style={{ flex: 1 }} />
          <PillButton onClick={onClose} disabled={saving}>
            Cancel
          </PillButton>
          <PillButton
            primary
            onClick={() => void save()}
            disabled={stage !== "editing" || saving || !name.trim() || tracks.length === 0}
          >
            {saving ? "Creating…" : "Create Playlist"}
          </PillButton>
        </div>
      }
    >
      {stage === "dropping" && (
        <div className="import-drop-pad">
          <ImportDropView title="Import Files as Playlist" allowsMultiple onFiles={(p) => void load(p)} />
        </div>
      )}
      {stage === "loading" && <Loading text={`Reading metadata ${progress.loaded} of ${progress.total}…`} />}
      {stage === "editing" && (
        <div className="meta-editor">
          <ArtworkColumn
            artwork={artwork}
            title="Click to choose a cover image"
            onPick={() => void pick()}
            onRemove={() => setArtwork(null)}
          />
          <div className="meta-editor__fields">
            <label className="editor-field" style={{ gap: 3 }}>
              <FieldLabel label="Name" />
              <input className="editor-input" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
            </label>
            <label className="editor-field" style={{ gap: 3 }}>
              <FieldLabel
                label="Description"
                trailing={
                  <span className="playlist-editor__count">
                    {description.length}/{DESCRIPTION_MAX}
                  </span>
                }
              />
              <textarea
                className="editor-input playlist-editor__description"
                style={{ height: 100 }}
                value={description}
                onChange={(e) => setDescription(e.target.value.slice(0, DESCRIPTION_MAX))}
              />
            </label>
            <div className="import-count">
              <ListMusic size={11} />
              {tracks.length} track{tracks.length === 1 ? "" : "s"}
            </div>
          </div>
        </div>
      )}
      <Modal open={crop != null} onClose={() => setCrop(null)}>
        {crop && <ImageCropper source={crop} title="Crop Cover" onComplete={setArtwork} onClose={() => setCrop(null)} />}
      </Modal>
    </FLSheet>
  );
}
