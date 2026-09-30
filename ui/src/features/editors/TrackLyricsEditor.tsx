import { Timer } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api, type Track } from "../../lib/api";
import { alertDialog } from "../../components/sheet/ConfirmDialog";
import { FLSheet, Modal } from "../../components/sheet/Sheet";
import { PillButton } from "../../components/settings/Primitives";
import { TrackLyricsSync } from "./TrackLyricsSync";
import "./Editors.css";

/** `TrackLyricsEditorView`: the file's LYRICS tag (or cached lrclib text). */
export function TrackLyricsEditor({ track, onClose }: { track: Track; onClose: () => void }) {
  const [text, setText] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const closeSync = useCallback(() => setSyncing(false), []);

  useEffect(() => {
    void api
      .readTrackLyrics(track.id)
      .then((t) => setText(t ?? ""))
      .finally(() => setLoading(false));
  }, [track.id]);

  const save = async () => {
    setSaving(true);
    try {
      await api.writeTrackLyrics(track.id, text);
      onClose();
    } catch (e) {
      setSaving(false);
      alertDialog("Save Failed", String(e));
    }
  };

  return (
    <FLSheet
      title="Edit Lyrics"
      width={560}
      height={560}
      onClose={onClose}
      footer={
        <div className="editor-footer">
          <div style={{ flex: 1 }} />
          <PillButton onClick={onClose} disabled={saving}>
            Cancel
          </PillButton>
          <PillButton primary onClick={() => void save()} disabled={saving || loading}>
            {saving ? "Saving…" : "Save"}
          </PillButton>
        </div>
      }
    >
      <div className="lyrics-editor">
        <div className="editor-caption">
          Paste plain lyrics or LRC-formatted synced lyrics. Synced lines look like [01:23.45]Lyric.
        </div>
        <div className="lyrics-editor__box">
          {loading ? (
            <span className="spinner" />
          ) : (
            <textarea className="lyrics-editor__text" value={text} spellCheck={false} onChange={(e) => setText(e.target.value)} />
          )}
        </div>
        <div className="editor-row">
          <PillButton onClick={() => setSyncing(true)} disabled={!text.trim()}>
            <span className="pill-icon" title="Tap on the beat to add timestamps to each line">
              <Timer size={11} /> Sync…
            </span>
          </PillButton>
          <div style={{ flex: 1 }} />
          {text && (
            <button className="editor-text-button" style={{ fontSize: "var(--font-caption)" }} onClick={() => setText("")}>
              Clear
            </button>
          )}
        </div>
      </div>
      <Modal open={syncing} onClose={closeSync}>
        <TrackLyricsSync track={track} lyrics={text} onSave={setText} onClose={closeSync} />
      </Modal>
    </FLSheet>
  );
}
