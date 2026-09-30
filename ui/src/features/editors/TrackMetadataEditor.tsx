import { AlignLeft, ListVideo } from "lucide-react";
import { useCallback, useState } from "react";
import { api, type ArtworkEdit, type Track } from "../../lib/api";
import { formatNames } from "../../lib/format";
import { ArtworkView } from "../../components/ArtworkView";
import { alertDialog } from "../../components/sheet/ConfirmDialog";
import { FLSheet, Modal } from "../../components/sheet/Sheet";
import { PillButton } from "../../components/settings/Primitives";
import { pickImage } from "../../components/editors/ImageCropper";
import { ArtistsField, Checkbox, GenreField, MetaField, SecondaryGenresField } from "../../components/editors/Fields";
import { TrackLyricsEditor } from "./TrackLyricsEditor";
import { TrackMarkersEditor } from "./TrackMarkersEditor";
import { artistChips, joinedChips } from "./chips";
import "./Editors.css";

/** `TrackMetadataEditorView`: writes the file's tags on Save. */
export function TrackMetadataEditor({ track, onClose }: { track: Track; onClose: () => void }) {
  const [title, setTitle] = useState(track.title);
  const [artists, setArtists] = useState(() => artistChips(track.artist));
  const [album, setAlbum] = useState(track.album ?? "");
  const [year, setYear] = useState(track.year != null ? String(track.year) : "");
  const [genre, setGenre] = useState(track.genre ?? "");
  const [secondary, setSecondary] = useState(track.secondaryGenres);
  const [trackNumber, setTrackNumber] = useState(track.trackNumber != null ? String(track.trackNumber) : "");
  const [mix, setMix] = useState(track.isMixCompilation);
  const [artwork, setArtwork] = useState<string | null>(track.artwork);
  const [artChange, setArtChange] = useState<ArtworkEdit>({ kind: "unchanged" });
  const [saving, setSaving] = useState(false);
  const [sub, setSub] = useState<"lyrics" | "markers" | null>(null);
  const closeSub = useCallback(() => setSub(null), []);

  const mixEligible = (track.duration ?? 0) > 600;
  const support = track.fileFormat === "wav" || track.fileFormat === "aiff" ? "limited" : "supported";

  const pick = async () => {
    try {
      const img = await pickImage("Choose album artwork");
      if (!img) return;
      setArtwork(img.id);
      setArtChange({ kind: "updated", id: img.id });
    } catch (e) {
      alertDialog("Couldn't Open Image", String(e));
    }
  };

  const save = async () => {
    setSaving(true);
    try {
      await api.writeTrackMetadata(track.id, {
        title: title.trim(),
        artist: joinedChips(artists),
        album: album || null,
        year: year ? Number(year) : null,
        genre: genre || null,
        secondaryGenres: secondary.filter((g) => g.toLowerCase() !== genre.toLowerCase()),
        trackNumber: trackNumber ? Number(trackNumber) : null,
        artwork: artChange,
        mixCompilation: mix === track.isMixCompilation ? null : mix,
      });
      onClose();
    } catch (e) {
      setSaving(false);
      alertDialog("Save Failed", String(e));
    }
  };

  return (
    <FLSheet
      title="Edit Track"
      width={520}
      height={540}
      onClose={onClose}
      footer={
        <div className="editor-footer">
          {mix ? (
            <PillButton onClick={() => setSub("markers")} disabled={saving}>
              <span className="pill-icon">
                <ListVideo size={11} /> Markers…
              </span>
            </PillButton>
          ) : (
            <PillButton onClick={() => setSub("lyrics")} disabled={saving}>
              <span className="pill-icon">
                <AlignLeft size={11} /> Lyrics…
              </span>
            </PillButton>
          )}
          <div style={{ flex: 1 }} />
          <PillButton onClick={onClose} disabled={saving}>
            Cancel
          </PillButton>
          <PillButton primary onClick={() => void save()} disabled={saving || !title.trim()}>
            {saving ? "Saving…" : "Save"}
          </PillButton>
        </div>
      }
    >
      <div className="meta-editor">
        <div className="meta-editor__art">
          <button className="meta-editor__art-button" title="Click to choose an image file" onClick={() => void pick()}>
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
          <span className="meta-editor__support">
            {formatNames[track.fileFormat]} · {support}
          </span>
        </div>
        <div className="meta-editor__fields">
          <MetaField label="Song Name" value={title} onChange={setTitle} required autoFocus />
          <ArtistsField artists={artists} onChange={setArtists} />
          <MetaField label="Album" value={album} onChange={setAlbum} />
          <div className="editor-row" style={{ alignItems: "flex-start" }}>
            <div style={{ flex: 1 }}>
              <MetaField label="Year" value={year} onChange={setYear} numericOnly />
            </div>
            <div style={{ flex: 1 }}>
              <MetaField label="Track #" value={trackNumber} onChange={setTrackNumber} numericOnly />
            </div>
          </div>
          <GenreField value={genre} onChange={setGenre} />
          <SecondaryGenresField genres={secondary} onChange={setSecondary} primary={genre} />
          <div style={{ paddingTop: "var(--space-xs)" }}>
            <Checkbox
              label="Mix Compilation"
              checked={mix}
              onChange={setMix}
              disabled={saving || (!mixEligible && !mix)}
              title={
                mixEligible
                  ? "Mark this track as a mix, live set, radio show, or concert recording. Disables lyrics and enables chapter markers."
                  : "Only available for tracks longer than 10 minutes"
              }
            />
          </div>
        </div>
      </div>
      <Modal open={sub === "lyrics"} onClose={closeSub}>
        <TrackLyricsEditor track={track} onClose={closeSub} />
      </Modal>
      <Modal open={sub === "markers"} onClose={closeSub}>
        <TrackMarkersEditor track={track} onClose={closeSub} />
      </Modal>
    </FLSheet>
  );
}
