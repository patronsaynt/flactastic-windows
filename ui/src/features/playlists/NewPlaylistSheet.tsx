import { useState } from "react";
import { api } from "../../lib/api";
import { PillButton } from "../../components/settings/Primitives";
import "./Playlists.css";

/** `PlaylistsTabView.newPlaylistSheet`: 340×160, name field, Cancel / Create. */
export function NewPlaylistSheet({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState("");
  const trimmed = name.trim();
  const commit = async () => {
    if (!trimmed) return;
    await api.createPlaylist(trimmed);
    onClose();
  };
  return (
    <div className="new-playlist">
      <div className="new-playlist__title">New Playlist</div>
      <input
        className="rounded-field"
        value={name}
        placeholder="Playlist name"
        autoFocus
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && void commit()}
      />
      <div className="new-playlist__buttons">
        <PillButton onClick={onClose}>Cancel</PillButton>
        <PillButton primary onClick={() => void commit()} disabled={!trimmed}>
          Create
        </PillButton>
      </div>
    </div>
  );
}
