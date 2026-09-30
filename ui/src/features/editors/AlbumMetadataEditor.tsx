import { Equal, ListMusic } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { albumTracks } from "../../app/library";
import { api, on, type Album, type ArtworkEdit, type LoadedImage, type Track } from "../../lib/api";
import { ArtworkView } from "../../components/ArtworkView";
import { alertDialog } from "../../components/sheet/ConfirmDialog";
import { FLSheet, Modal } from "../../components/sheet/Sheet";
import { PillButton } from "../../components/settings/Primitives";
import { ImageCropper, pickImage } from "../../components/editors/ImageCropper";
import { ArtistsField, Checkbox, GenreField, MetaField, SecondaryGenresField } from "../../components/editors/Fields";
import { artistChips, joinedChips } from "./chips";
import { useReorder } from "../../lib/reorder";
import "./Editors.css";

interface EditableTrack {
  id: string;
  title: string;
  artists: string[];
  trackNumber: number;
  original: Track;
}

const sameList = (a: string[], b: string[]) => a.length === b.length && a.every((x, i) => x === b[i]);

/**
 * `AlbumMetadataEditorView`: shared album fields plus per-track titles,
 * artists and order, written to every track on "Save All".
 */
export function AlbumMetadataEditor({ album, onClose }: { album: Album; onClose: () => void }) {
  // The chip field seeds from ALBUMARTIST, else the per-track roll-up. A
  // roll-up ("Various Artists") is display-only and never written unedited.
  const seeded = useRef(artistChips(album.albumArtist ?? album.artist)).current;
  const seededFromRollUp = album.albumArtist == null;

  const [name, setName] = useState(album.name);
  const [albumArtists, setAlbumArtists] = useState(seeded);
  const [year, setYear] = useState(album.year != null ? String(album.year) : "");
  const [genre, setGenre] = useState(album.genre ?? "");
  const [secondary, setSecondary] = useState(album.secondaryGenres);
  const [compilation, setCompilation] = useState(album.isCompilation);
  const [mix, setMix] = useState(album.isMixCompilation);
  const [artwork, setArtwork] = useState<string | null>(album.artwork);
  const [artChange, setArtChange] = useState<ArtworkEdit>({ kind: "unchanged" });
  const [crop, setCrop] = useState<LoadedImage | null>(null);
  const [rows, setRows] = useState<EditableTrack[]>(() =>
    [...albumTracks(album)]
      .sort((a, b) => (a.trackNumber ?? Infinity) - (b.trackNumber ?? Infinity))
      .map((t, i) => ({ id: t.id, title: t.title, artists: artistChips(t.artist), trackNumber: t.trackNumber ?? i + 1, original: t })),
  );
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(0);

  const tracks = albumTracks(album);
  const mixEligible = tracks.length === 1 && (tracks[0]?.duration ?? 0) > 600;

  useEffect(() => on<{ saved: number }>("metadata://progress", (p) => setSaved(p.saved)), []);

  const pick = async () => {
    try {
      const img = await pickImage("Choose album artwork");
      if (img) setCrop(img);
    } catch (e) {
      alertDialog("Couldn't Open Image", String(e));
    }
  };

  // Dragging over a row moves the dragged row there and renumbers everyone
  // (an explicit resequence; numbers are otherwise left as tagged).
  const reorder = useReorder({
    live: true,
    onMove: (src, dst) =>
      setRows((r) => {
        const from = r.findIndex((x) => x.id === src);
        const to = r.findIndex((x) => x.id === dst);
        if (from < 0 || to < 0) return r;
        const next = [...r];
        const [item] = next.splice(from, 1);
        next.splice(to, 0, item);
        return next.map((x, i) => ({ ...x, trackNumber: i + 1 }));
      }),
  });

  const save = async () => {
    setSaving(true);
    setSaved(0);
    const newAlbumArtist = joinedChips(albumArtists);
    const existing = album.albumArtist ?? "";
    const aaUnchanged =
      existing === (newAlbumArtist ?? "") || (seededFromRollUp && sameList(albumArtists, seeded));
    const edits = rows.map((r) => {
      const title = r.title.trim();
      return {
        id: r.id,
        edit: {
          title: title || r.original.title,
          artist: joinedChips(r.artists) ?? newAlbumArtist,
          album: name.trim(),
          year: year ? Number(year) : null,
          genre: genre || null,
          secondaryGenres: secondary.filter((g) => g.toLowerCase() !== genre.toLowerCase()),
          trackNumber: r.trackNumber,
          artwork: artChange,
          ...(aaUnchanged ? {} : { albumArtist: newAlbumArtist }),
          compilation: compilation === album.isCompilation ? null : compilation,
          mixCompilation: mix === album.isMixCompilation ? null : mix,
        },
      };
    });
    try {
      await api.writeTracksMetadata(album.id, edits);
      onClose();
    } catch (e) {
      setSaving(false);
      alertDialog("Save Failed", String(e));
    }
  };

  return (
    <FLSheet
      title="Edit Album"
      width={540}
      height={720}
      onClose={onClose}
      footer={
        <div className="editor-footer">
          <div style={{ flex: 1 }} />
          <PillButton onClick={onClose} disabled={saving}>
            Cancel
          </PillButton>
          <PillButton primary onClick={() => void save()} disabled={saving || !name.trim()}>
            {saving ? "Saving…" : "Save All"}
          </PillButton>
        </div>
      }
    >
      <div className="album-editor">
        <div className="meta-editor">
          <div className="meta-editor__art">
            <button className="meta-editor__art-button" title="Click to choose album artwork" onClick={() => void pick()}>
              <ArtworkView artwork={artwork} size={130} />
              {!artwork && <span className="meta-editor__add">Click to add</span>}
            </button>
            {artwork && (
              <button
                className="editor-text-button"
                onClick={() => {
                  setArtwork(null);
                  setArtChange({ kind: "removed" });
                }}
              >
                Remove
              </button>
            )}
          </div>
          <div className="meta-editor__fields" style={{ gap: "var(--space-sm)" }}>
            <MetaField label="Album Name" value={name} onChange={setName} required autoFocus />
            <ArtistsField artists={albumArtists} onChange={setAlbumArtists} label="Album Artist" />
            <Checkbox
              label="Compilation"
              checked={compilation}
              disabled={saving}
              onChange={(on) => {
                setCompilation(on);
                if (on) setMix(false);
              }}
            />
            <Checkbox
              label="Mix Compilation"
              checked={mix}
              disabled={saving || (!mixEligible && !mix)}
              title={
                mixEligible
                  ? "Mark this album's single track as a mix, live set, radio show, or concert recording. Disables lyrics and enables chapter markers."
                  : "Only available for single-track albums longer than 10 minutes"
              }
              onChange={(on) => {
                setMix(on);
                if (on) setCompilation(false);
              }}
            />
            <div className="editor-row" style={{ alignItems: "flex-start" }}>
              <MetaField label="Year" value={year} onChange={setYear} numericOnly width={80} />
              <GenreField value={genre} onChange={setGenre} />
            </div>
            <SecondaryGenresField genres={secondary} onChange={setSecondary} primary={genre} />
            {saving && (
              <div className="editor-row editor-caption" style={{ gap: "var(--space-xs)" }}>
                <ListMusic size={11} /> Saved {saved} of {rows.length}…
              </div>
            )}
          </div>
        </div>
        <div className="sync-divider" />
        <div className="album-editor__tracks-title">TRACKS</div>
        <div className="album-editor__tracks">
          {rows.map((r, i) => (
            <div
              key={r.id}
              className={"album-editor__track" + (reorder.dragging === r.id ? " is-dragging" : "")}
              {...reorder.rowProps(r.id)}
            >
              <div className="editor-row" style={{ gap: "var(--space-sm)" }}>
                <input
                  className="album-editor__number"
                  value={String(r.trackNumber)}
                  onChange={(e) => {
                    const n = Number(e.target.value.replace(/\D/g, "")) || 0;
                    setRows((all) => all.map((x, j) => (j === i ? { ...x, trackNumber: n } : x)));
                  }}
                />
                <input
                  className="editor-input"
                  style={{ flex: 1, minWidth: 0, padding: "5px 8px" }}
                  value={r.title}
                  onChange={(e) => setRows((all) => all.map((x, j) => (j === i ? { ...x, title: e.target.value } : x)))}
                />
                <span
                  className={"album-editor__handle" + (reorder.dragging === r.id ? " is-active" : "")}
                  {...reorder.handleProps(r.id)}
                >
                  <Equal size={12} strokeWidth={2.2} />
                </span>
              </div>
              <div className="editor-row" style={{ gap: "var(--space-sm)", alignItems: "flex-start" }}>
                <span style={{ width: 28, flex: "none" }} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <ArtistsField
                    artists={r.artists}
                    onChange={(a) => setRows((all) => all.map((x, j) => (j === i ? { ...x, artists: a } : x)))}
                    label={null}
                    placeholder="Artists for this track…"
                    compact
                  />
                </div>
                <span style={{ width: 18, flex: "none" }} />
              </div>
            </div>
          ))}
        </div>
      </div>
      <Modal open={crop != null} onClose={() => setCrop(null)}>
        {crop && (
          <ImageCropper
            source={crop}
            aspectRatio={1}
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
