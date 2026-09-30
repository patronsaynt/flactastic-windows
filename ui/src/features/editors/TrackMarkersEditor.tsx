import { GripHorizontal, X } from "lucide-react";
import { useEffect, useState } from "react";
import { usePlaybackTime, usePlayer } from "../../app/player";
import { api, type Marker, type Track } from "../../lib/api";
import { formatDuration } from "../../lib/format";
import { alertDialog } from "../../components/sheet/ConfirmDialog";
import { FLSheet } from "../../components/sheet/Sheet";
import { PillButton } from "../../components/settings/Primitives";
import "./Editors.css";

const MIME = "text/x-fl-marker";

/** `TrackMarkersEditorView`: chapter markers stored as an embedded CUESHEET. */
export function TrackMarkersEditor({ track, onClose }: { track: Track; onClose: () => void }) {
  const [markers, setMarkers] = useState<Marker[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [stampText, setStampText] = useState("");
  const [titleText, setTitleText] = useState("");
  const [parsed, setParsed] = useState<number | null>(null);
  const currentId = usePlayer((s) => s.currentTrackId);
  const now = usePlaybackTime();

  useEffect(() => {
    void api
      .readTrackMarkers(track.id)
      .then((m) => setMarkers(m ?? []))
      .finally(() => setLoading(false));
  }, [track.id]);

  useEffect(() => {
    let live = true;
    void api.parseMarkerTimestamp(stampText).then((v) => live && setParsed(v ?? null));
    return () => {
      live = false;
    };
  }, [stampText]);

  const add = () => {
    if (parsed == null) return;
    setMarkers((m) => [...m, { id: crypto.randomUUID(), timestamp: parsed, title: titleText }].sort((a, b) => a.timestamp - b.timestamp));
    setStampText("");
    setTitleText("");
  };

  const move = (src: string, dst: string) =>
    setMarkers((m) => {
      const from = m.findIndex((x) => x.id === src);
      const to = m.findIndex((x) => x.id === dst);
      if (from < 0 || to < 0 || from === to) return m;
      const next = [...m];
      const [item] = next.splice(from, 1);
      next.splice(to, 0, item);
      return next;
    });

  const save = async () => {
    setSaving(true);
    try {
      await api.writeTrackMarkers(track.id, markers);
      onClose();
    } catch (e) {
      setSaving(false);
      alertDialog("Save Failed", String(e));
    }
  };

  return (
    <FLSheet
      title="Chapter Markers"
      width={480}
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
      <div className="markers-editor">
        <div className="editor-caption">Add timestamps to mark sections of this mix, live set, or recording.</div>
        {loading ? (
          <div className="markers-editor__empty">
            <span className="spinner" />
          </div>
        ) : markers.length === 0 ? (
          <div className="markers-editor__empty editor-caption">No markers yet</div>
        ) : (
          <div className="markers-editor__list">
            {markers.map((m) => (
              <div
                key={m.id}
                className="marker-row"
                draggable
                onDragStart={(e) => e.dataTransfer.setData(MIME, m.id)}
                onDragOver={(e) => e.dataTransfer.types.includes(MIME) && e.preventDefault()}
                onDrop={(e) => {
                  e.preventDefault();
                  move(e.dataTransfer.getData(MIME), m.id);
                }}
              >
                <span className="marker-row__time">{formatDuration(m.timestamp)}</span>
                <input
                  className="marker-row__title"
                  placeholder="Marker title"
                  value={m.title}
                  onChange={(e) => setMarkers((all) => all.map((x) => (x.id === m.id ? { ...x, title: e.target.value } : x)))}
                />
                <button className="marker-row__delete" title="Delete" onClick={() => setMarkers((all) => all.filter((x) => x.id !== m.id))}>
                  <X size={11} />
                </button>
                <GripHorizontal size={13} className="marker-row__grip" />
              </div>
            ))}
          </div>
        )}
        {!loading && (
          <div className="editor-row" style={{ gap: "var(--space-sm)" }}>
            <input
              className="editor-input mono"
              style={{ width: 80 }}
              placeholder="mm:ss"
              value={stampText}
              onChange={(e) => setStampText(e.target.value)}
            />
            <input
              className="editor-input"
              style={{ flex: 1, minWidth: 0 }}
              placeholder="Title"
              value={titleText}
              onChange={(e) => setTitleText(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && add()}
            />
            <button
              className="editor-text-button"
              style={{ fontSize: "var(--font-caption)" }}
              disabled={currentId !== track.id}
              onClick={() => setStampText(formatDuration(now))}
            >
              At Playhead
            </button>
            <PillButton onClick={add} disabled={parsed == null}>
              Add
            </PillButton>
          </div>
        )}
      </div>
    </FLSheet>
  );
}
