import { Music } from "lucide-react";
import { useEffect, useState } from "react";
import { api, on, type Track } from "../../lib/api";
import { alertDialog } from "../../components/sheet/ConfirmDialog";
import { FLSheet } from "../../components/sheet/Sheet";
import { PillButton } from "../../components/settings/Primitives";
import { MetaField } from "../../components/editors/Fields";
import "./Editors.css";

function mostCommon(values: (string | null)[]): string {
  const counts = new Map<string, number>();
  for (const v of values) if (v != null) counts.set(v, (counts.get(v) ?? 0) + 1);
  let best = "";
  let n = 0;
  for (const [v, c] of counts) if (c > n) [best, n] = [v, c];
  return best;
}

/**
 * `MergeTracksIntoAlbumView`: one album name (and optionally artist /
 * album artist) written to every selected track so they group together.
 */
export function MergeTracksSheet({ tracks, onDone, onClose }: { tracks: Track[]; onDone?: () => void; onClose: () => void }) {
  const [albumName, setAlbumName] = useState(() => mostCommon(tracks.map((t) => t.album)));
  const [artist, setArtist] = useState(() => mostCommon(tracks.map((t) => t.artist)));
  const [albumArtist, setAlbumArtist] = useState(() => mostCommon(tracks.map((t) => t.albumArtist)));
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(0);

  useEffect(() => on<{ saved: number }>("metadata://progress", (p) => setSaved(p.saved)), []);

  const save = async () => {
    setSaving(true);
    setSaved(0);
    const newAlbum = albumName.trim();
    const newArtist = artist.trim();
    const aa = albumArtist.trim();
    const edits = tracks.map((t) => ({
      id: t.id,
      edit: {
        title: t.title,
        artist: newArtist || t.artist,
        album: newAlbum,
        year: t.year,
        genre: t.genre,
        // The Mac passes no secondary genres here, which the writer treats
        // as none; keep the track's own so a merge never drops them.
        secondaryGenres: t.secondaryGenres,
        trackNumber: t.trackNumber,
        artwork: { kind: "unchanged" as const },
        albumArtist: aa || null,
      },
    }));
    try {
      await api.writeTracksMetadata(null, edits);
      onDone?.();
      onClose();
    } catch (e) {
      setSaving(false);
      alertDialog("Save Failed", String(e));
    }
  };

  return (
    <FLSheet
      title="Merge into Album"
      width={460}
      height={510}
      onClose={onClose}
      footer={
        <div className="editor-footer">
          {saving && (
            <span className="editor-caption">
              Saving {saved} of {tracks.length}…
            </span>
          )}
          <div style={{ flex: 1 }} />
          <PillButton onClick={onClose} disabled={saving}>
            Cancel
          </PillButton>
          <PillButton primary onClick={() => void save()} disabled={saving || !albumName.trim()}>
            {saving ? "Saving…" : "Merge"}
          </PillButton>
        </div>
      }
    >
      <div className="editor-form" style={{ gap: "var(--space-md)" }}>
        <div className="editor-caption" style={{ color: "var(--text-secondary)" }}>
          Set these fields on all {tracks.length} selected tracks.
        </div>
        <MetaField label="Album Name" value={albumName} onChange={setAlbumName} required autoFocus />
        <MetaField
          label="Artist"
          value={artist}
          onChange={setArtist}
          hint="Applied to every track. Leave blank to keep each track's existing artist."
        />
        <MetaField
          label="Album Artist"
          value={albumArtist}
          onChange={setAlbumArtist}
          hint="Shared album-level artist. Ensures tracks merge under one album."
        />
      </div>
      <div className="sync-divider" />
      <div className="album-editor__tracks-title">TRACKS TO MERGE</div>
      <div className="merge-list">
        {tracks.map((t) => (
          <div key={t.id} className="merge-list__row">
            <Music size={11} className="merge-list__icon" />
            <span className="merge-list__title">{t.title}</span>
            {t.artist && (
              <>
                <span className="merge-list__dash">–</span>
                <span className="merge-list__artist">{t.artist}</span>
              </>
            )}
          </div>
        ))}
      </div>
    </FLSheet>
  );
}
