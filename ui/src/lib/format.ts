/** `FormatUtils` */

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds == null || !isFinite(seconds) || seconds < 0) return "--:--";
  const total = Math.round(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const p2 = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${p2(m)}:${p2(s)}` : `${m}:${p2(s)}`;
}

export function coarseDuration(seconds: number): string {
  if (!isFinite(seconds) || seconds <= 0) return "0m";
  const total = Math.round(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  return h > 0 ? `${h}h ${String(m).padStart(2, "0")}m` : `${m}m`;
}

export function playlistSummary(trackCount: number, duration: number): string {
  const tracks = `${trackCount} track${trackCount === 1 ? "" : "s"}`;
  return duration > 0 ? `${tracks} · ${coarseDuration(duration)}` : tracks;
}

export function kilohertzString(rate: number): string {
  const khz = rate / 1000;
  return khz === Math.round(khz) ? `${khz.toFixed(0)} kHz` : `${khz.toFixed(1)} kHz`;
}

export function formatSampleRate(rate: number | null, bitDepth: number | null): string | null {
  if (rate == null) return null;
  if (bitDepth != null) return `${bitDepth}/${Math.trunc(rate / 1000)}`;
  return kilohertzString(rate);
}

export const formatNames = { flac: "FLAC", mp3: "MP3", wav: "WAV", aiff: "AIFF", alac: "ALAC", aac: "AAC" } as const;

export function techSpec(t: { fileFormat: keyof typeof formatNames; bitDepth: number | null; sampleRate: number | null } | null) {
  if (!t) return null;
  const parts: string[] = [formatNames[t.fileFormat]];
  const fid: string[] = [];
  if (t.bitDepth != null) fid.push(`${t.bitDepth}-BIT`);
  if (t.sampleRate != null) fid.push(kilohertzString(t.sampleRate));
  if (fid.length) parts.push(fid.join(" / "));
  return parts.join(" · ");
}

export const qualityLabel = { hiRes: "Hi-Res", cd: "CD", mid: "Mid", low: "Low" } as const;
