import { useState } from "react";
import { usePlaylists } from "../../app/playlists";
import { api, type LoadedImage } from "../../lib/api";
import { ArtworkView } from "../../components/ArtworkView";
import { FLSheet, Modal } from "../../components/sheet/Sheet";
import { PillButton } from "../../components/settings/Primitives";
import { ImageCropper, pickImage } from "../../components/editors/ImageCropper";
import "../../components/editors/Editor.css";
import "./Playlists.css";

/** `Playlist.descriptionMaxLength` */
const DESCRIPTION_MAX = 200;

/** `PlaylistEditorView`: cover (1:1 crop), name, description. */
export function PlaylistEditorView({ playlistId, onClose }: { playlistId: string; onClose: () => void }) {
  const playlist = usePlaylists((s) => s.byId.get(playlistId));
  const [name, setName] = useState(playlist?.name ?? "");
  const [description, setDescription] = useState(playlist?.description ?? "");
  const [artwork, setArtwork] = useState<string | null>(playlist?.customArtwork ?? null);
  const [crop, setCrop] = useState<LoadedImage | null>(null);
  const [error, setError] = useState<string | null>(null);

  const pick = async () => {
    setError(null);
    try {
      const img = await pickImage("Choose a playlist cover image");
      if (img) setCrop(img);
    } catch (e) {
      setError(String(e));
    }
  };

  const save = async () => {
    const n = name.trim();
    if (!n) return;
    const d = description.trim();
    await api.updatePlaylistMetadata(playlistId, n, d || null, artwork);
    onClose();
  };

  return (
    <FLSheet
      title="Edit Playlist"
      width={480}
      height={420}
      onClose={onClose}
      footer={
        <div className="editor-footer">
          <div style={{ flex: 1 }} />
          <PillButton onClick={onClose}>Cancel</PillButton>
          <PillButton primary onClick={() => void save()} disabled={!name.trim()}>
            Save
          </PillButton>
        </div>
      }
    >
      <div className="playlist-editor">
        <div className="playlist-editor__art">
          <button className="playlist-editor__art-button" title="Click to choose a custom cover image" onClick={() => void pick()}>
            <ArtworkView artwork={artwork} size={130} />
            {!artwork && <span className="playlist-editor__add">Click to add</span>}
          </button>
          {artwork && (
            <button className="editor-text-button" onClick={() => setArtwork(null)}>
              Remove
            </button>
          )}
        </div>
        <div className="playlist-editor__fields">
          <label className="editor-field" style={{ gap: 3 }}>
            <span className="editor-field__label">Name</span>
            <input className="editor-input" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
          </label>
          <label className="editor-field" style={{ gap: 3 }}>
            <span className="editor-row">
              <span className="editor-field__label" style={{ flex: 1 }}>
                Description
              </span>
              <span className="playlist-editor__count">
                {description.length}/{DESCRIPTION_MAX}
              </span>
            </span>
            <textarea
              className="editor-input playlist-editor__description"
              value={description}
              onChange={(e) => setDescription(e.target.value.slice(0, DESCRIPTION_MAX))}
            />
          </label>
          {error && <div className="editor-error">{error}</div>}
        </div>
      </div>
      <Modal open={crop != null} onClose={() => setCrop(null)}>
        {crop && <ImageCropper source={crop} aspectRatio={1} title="Crop Cover" onComplete={setArtwork} onClose={() => setCrop(null)} />}
      </Modal>
    </FLSheet>
  );
}
