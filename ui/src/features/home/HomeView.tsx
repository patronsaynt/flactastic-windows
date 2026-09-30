import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen } from "lucide-react";
import { useLibrary } from "../../app/library";
import { api } from "../../lib/api";
import { plural } from "../../lib/text";
import { PageHeader } from "../../components/PageHeader";
import { ActionPill } from "../../components/chrome/Chrome";

export async function chooseLibraryFolder() {
  const dir = await open({ directory: true, multiple: false, title: "Choose your music folder" });
  if (typeof dir === "string") await api.openLibrary(dir);
}

function greeting(): string {
  const h = new Date().getHours();
  return h < 12 ? "Good morning" : h < 18 ? "Good afternoon" : "Good evening";
}

/** Home (interim): library summary and folder chooser. */
export function HomeView() {
  const root = useLibrary((s) => s.root);
  const tracks = useLibrary((s) => s.tracks.length);
  const albums = useLibrary((s) => s.albums.length);
  return (
    <div className="page-pad">
      <PageHeader eyebrow={greeting()} title="Home" />
      <div style={{ marginTop: 24, display: "flex", flexDirection: "column", gap: 12, alignItems: "flex-start" }}>
        {root ? (
          <div style={{ color: "var(--text-secondary)" }}>
            {root} — {plural(albums, "album")}, {plural(tracks, "track")}
          </div>
        ) : (
          <div style={{ color: "var(--text-secondary)" }}>Choose the folder that holds your music to get started.</div>
        )}
        <ActionPill primary={!root} icon={FolderOpen} onClick={() => void chooseLibraryFolder()}>
          {root ? "Change Library Folder…" : "Choose Library Folder…"}
        </ActionPill>
      </div>
    </div>
  );
}
