import { open } from "@tauri-apps/plugin-dialog";
import { ZoomIn, ZoomOut } from "lucide-react";
import { useRef, useState } from "react";
import { api, artworkUrl, type LoadedImage } from "../../lib/api";
import { isTauri } from "../../lib/native";
import { FLSheet } from "../sheet/Sheet";
import { PillButton, Slider } from "../settings/Primitives";
import "./ImageCropper.css";

/**
 * `NSOpenPanel` for images, read into the artwork store. Returns null on
 * cancel; throws with a readable message for an unreadable file.
 */
export async function pickImage(title: string): Promise<LoadedImage | null> {
  if (!isTauri) return null;
  const path = await open({
    title,
    multiple: false,
    directory: false,
    filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png", "webp"] }],
  });
  if (typeof path !== "string") return null;
  return (await api.loadImageFile(path)) ?? null;
}

const MIN_SCALE = 1;
const MAX_SCALE = 4;
/** Output keeps 1200 px across regardless of ratio. */
const OUTPUT_WIDTH = 1200;

type Size = { w: number; h: number };

/**
 * `ImageCropperView`: pan and zoom inside a fixed crop window of any aspect
 * ratio; "Use" returns the id of the cropped PNG.
 */
export function ImageCropper({
  source,
  aspectRatio = 1,
  title = "Crop Image",
  onComplete,
  onClose,
}: {
  source: LoadedImage;
  aspectRatio?: number;
  title?: string;
  onComplete: (id: string) => void;
  onClose: () => void;
}) {
  const cropW = aspectRatio >= 1 ? 380 : 320;
  const cropH = cropW / aspectRatio;
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState<Size>({ w: 0, h: 0 });
  const [busy, setBusy] = useState(false);
  const drag = useRef<{ x: number; y: number; start: Size } | null>(null);

  // Aspect-fill into the crop window, before the user's zoom.
  const fill = Math.max(cropW / source.width, cropH / source.height);
  const filled: Size = { w: source.width * fill, h: source.height * fill };

  const clamp = (o: Size, s: number): Size => {
    const maxX = Math.max(0, (filled.w * s - cropW) / 2);
    const maxY = Math.max(0, (filled.h * s - cropH) / 2);
    return { w: Math.min(Math.max(o.w, -maxX), maxX), h: Math.min(Math.max(o.h, -maxY), maxY) };
  };

  const commit = async () => {
    const perPoint = source.width / filled.w / scale;
    const renderedW = filled.w * scale;
    const renderedH = filled.h * scale;
    const rect: [number, number, number, number] = [
      (renderedW / 2 - cropW / 2 - offset.w) * perPoint,
      (renderedH / 2 - cropH / 2 - offset.h) * perPoint,
      cropW * perPoint,
      cropH * perPoint,
    ];
    setBusy(true);
    try {
      const id = await api.cropImage(source.id, rect, OUTPUT_WIDTH, Math.round(OUTPUT_WIDTH / aspectRatio));
      if (id) {
        onComplete(id);
        onClose();
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <FLSheet
      title={title}
      width={Math.max(440, cropW + 80)}
      height={cropH + 220}
      onClose={onClose}
      footer={
        <div className="cropper__footer">
          <PillButton onClick={onClose}>Cancel</PillButton>
          <PillButton primary onClick={() => void commit()} disabled={busy}>
            Use
          </PillButton>
        </div>
      }
    >
      <div
        className="cropper"
        onKeyDown={(e) => {
          if (e.key === "Enter") void commit();
        }}
      >
        <div
          className="cropper__window"
          style={{ width: cropW, height: cropH }}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            drag.current = { x: e.clientX, y: e.clientY, start: offset };
          }}
          onPointerMove={(e) => {
            const d = drag.current;
            if (!d) return;
            // clientX is in window pixels; undo the UI zoom.
            const z = Number(getComputedStyle(document.documentElement).getPropertyValue("--ui-scale")) || 1;
            setOffset(clamp({ w: d.start.w + (e.clientX - d.x) / z, h: d.start.h + (e.clientY - d.y) / z }, scale));
          }}
          onPointerUp={() => (drag.current = null)}
        >
          <img
            src={artworkUrl(source.id, 0)}
            alt=""
            draggable={false}
            style={{
              width: filled.w,
              height: filled.h,
              left: (cropW - filled.w) / 2,
              top: (cropH - filled.h) / 2,
              transform: `translate(${offset.w}px, ${offset.h}px) scale(${scale})`,
            }}
          />
        </div>
        <div className="cropper__zoom">
          <ZoomOut size={12} />
          <Slider
            value={scale}
            min={MIN_SCALE}
            max={MAX_SCALE}
            onChange={(s) => {
              setScale(s);
              setOffset((o) => clamp(o, s));
            }}
          />
          <ZoomIn size={12} />
        </div>
      </div>
    </FLSheet>
  );
}
