import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Import } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { isTauri } from "../../lib/native";
import { PillButton } from "../../components/settings/Primitives";
import "./Import.css";

const AUDIO_EXTENSIONS = ["flac", "mp3", "wav", "aiff", "aif", "m4a", "aac", "mp4", "alac"];

const isAudio = (p: string) => AUDIO_EXTENSIONS.includes(p.split(".").pop()?.toLowerCase() ?? "");

/**
 * `ImportDropView`: the dashed drop zone every import starts with. Takes
 * files dropped from the OS file manager (Tauri's native drop, which gives
 * real paths) or picked with the open dialog.
 */
export function ImportDropView({
  title,
  allowsMultiple,
  onFiles,
}: {
  title: string;
  allowsMultiple: boolean;
  onFiles: (paths: string[]) => void;
}) {
  const [targeted, setTargeted] = useState(false);
  const zone = useRef<HTMLDivElement>(null);
  const cb = useRef(onFiles);
  cb.current = onFiles;

  const deliver = (paths: string[]) => {
    const audio = paths.filter(isAudio);
    if (!audio.length) return;
    cb.current(allowsMultiple ? audio : audio.slice(0, 1));
  };

  useEffect(() => {
    if (!isTauri) return;
    let un: (() => void) | undefined;
    let dead = false;
    const inside = (pos: { x: number; y: number }) => {
      const r = zone.current?.getBoundingClientRect();
      if (!r) return false;
      // Drop positions are physical pixels relative to the webview.
      const x = pos.x / window.devicePixelRatio;
      const y = pos.y / window.devicePixelRatio;
      return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
    };
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "leave") setTargeted(false);
        else if (p.type === "enter" || p.type === "over") setTargeted(inside(p.position));
        else if (p.type === "drop") {
          setTargeted(false);
          if (inside(p.position)) deliver(p.paths);
        }
      })
      .then((u) => (dead ? u() : (un = u)));
    return () => {
      dead = true;
      un?.();
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const pick = async () => {
    if (!isTauri) return;
    const r = await open({
      title,
      multiple: allowsMultiple,
      directory: false,
      filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }],
    });
    if (r == null) return;
    deliver(Array.isArray(r) ? r : [r]);
  };

  return (
    <div ref={zone} className={"drop-zone" + (targeted ? " is-targeted" : "")} onClick={() => void pick()}>
      <Import size={44} strokeWidth={0.8} className="drop-zone__icon" />
      <div className="drop-zone__title">Drag and drop here, or select files</div>
      <div className="drop-zone__formats">FLAC · MP3 · WAV · AIFF · M4A · AAC</div>
      <div style={{ paddingTop: "var(--space-sm)" }} onClick={(e) => e.stopPropagation()}>
        <PillButton primary onClick={() => void pick()}>
          Select Files…
        </PillButton>
      </div>
    </div>
  );
}
