/**
 * Thin wrappers over the Tauri API that degrade gracefully when the UI runs
 * in a plain browser (`pnpm dev` without the shell).
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T | undefined> {
  if (!isTauri) return undefined;
  return tauriInvoke<T>(cmd, args);
}

export const windowControls = {
  minimize: () => isTauri && getCurrentWindow().minimize(),
  toggleMaximize: () => isTauri && getCurrentWindow().toggleMaximize(),
  close: () => isTauri && getCurrentWindow().close(),
  startDragging: () => isTauri && getCurrentWindow().startDragging(),
  async isMaximized(): Promise<boolean> {
    return isTauri ? getCurrentWindow().isMaximized() : false;
  },
  onResized(cb: () => void): () => void {
    if (!isTauri) return () => {};
    let un: (() => void) | undefined;
    void getCurrentWindow()
      .onResized(cb)
      .then((u) => (un = u));
    return () => un?.();
  },
};

export const platform: "windows" | "linux" | "other" = (() => {
  const ua = typeof navigator !== "undefined" ? navigator.userAgent : "";
  if (ua.includes("Windows")) return "windows";
  if (ua.includes("Linux")) return "linux";
  return "other";
})();
