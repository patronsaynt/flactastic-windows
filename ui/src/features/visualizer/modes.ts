/** `VisualizerMode`, in the wheel's order (the Mac's raw values). */
export const MODES = [
  "albumArtLarge",
  "albumArtLargeDetails",
  "albumArtSmallDetails",
  "albumArtWheel",
  "lyrics",
  "spectrumRadial",
  "spectrumHorizontal",
  "spectrogram",
] as const;

export type VisualizerMode = (typeof MODES)[number];

export const wheelLabel: Record<VisualizerMode, string> = {
  albumArtLarge: "Large Art",
  albumArtLargeDetails: "Large Art + Details",
  albumArtSmallDetails: "Small Art + Details",
  albumArtWheel: "Cover Wheel",
  lyrics: "Lyrics",
  spectrumRadial: "Radial Spectrum",
  spectrumHorizontal: "Horizontal Spectrum",
  spectrogram: "Spectrogram",
};

export const requiresAudioTap = (m: VisualizerMode) =>
  m === "spectrumRadial" || m === "spectrumHorizontal" || m === "spectrogram";

export function asMode(v: unknown): VisualizerMode {
  return (MODES as readonly string[]).includes(v as string) ? (v as VisualizerMode) : "albumArtLargeDetails";
}
