import { describe, expect, it } from "vitest";
import { coarseDuration, formatDuration, formatSampleRate, kilohertzString, playlistSummary, techSpec } from "./format";
import { byteCount } from "../app/sync";

// Expected strings follow the Mac's FormatUtils / ByteCountFormatter output.

describe("formatDuration", () => {
  it("formats minutes and hours", () => {
    expect(formatDuration(0)).toBe("0:00");
    expect(formatDuration(59.5)).toBe("1:00");
    expect(formatDuration(245)).toBe("4:05");
    expect(formatDuration(3725)).toBe("1:02:05");
  });
  it("shows dashes for missing or invalid values", () => {
    expect(formatDuration(null)).toBe("--:--");
    expect(formatDuration(undefined)).toBe("--:--");
    expect(formatDuration(-1)).toBe("--:--");
    expect(formatDuration(Infinity)).toBe("--:--");
  });
});

describe("coarseDuration / playlistSummary", () => {
  it("drops seconds", () => {
    expect(coarseDuration(0)).toBe("0m");
    expect(coarseDuration(38 * 60 + 20)).toBe("38m");
    expect(coarseDuration(4 * 3600 + 12 * 60)).toBe("4h 12m");
    expect(coarseDuration(3600 + 5 * 60)).toBe("1h 05m");
  });
  it("summarises playlists", () => {
    expect(playlistSummary(1, 0)).toBe("1 track");
    expect(playlistSummary(64, 4 * 3600 + 12 * 60)).toBe("64 tracks · 4h 12m");
  });
});

describe("sample rates", () => {
  it("prints whole kHz without a decimal", () => {
    expect(kilohertzString(96000)).toBe("96 kHz");
    expect(kilohertzString(44100)).toBe("44.1 kHz");
    expect(kilohertzString(88200)).toBe("88.2 kHz");
  });
  it("formats rate with bit depth", () => {
    expect(formatSampleRate(null, 16)).toBeNull();
    expect(formatSampleRate(44100, 16)).toBe("16/44");
    expect(formatSampleRate(96000, null)).toBe("96 kHz");
  });
  it("builds the visualizer caption", () => {
    expect(techSpec(null)).toBeNull();
    expect(techSpec({ fileFormat: "flac", bitDepth: 24, sampleRate: 96000 })).toBe("FLAC · 24-BIT / 96 kHz");
    expect(techSpec({ fileFormat: "mp3", bitDepth: null, sampleRate: 44100 })).toBe("MP3 · 44.1 kHz");
    expect(techSpec({ fileFormat: "aac", bitDepth: null, sampleRate: null })).toBe("AAC");
  });
});

describe("byteCount (ByteCountFormatter .file)", () => {
  it("handles small counts", () => {
    expect(byteCount(0)).toBe("Zero KB");
    expect(byteCount(1)).toBe("1 byte");
    expect(byteCount(999)).toBe("999 bytes");
  });
  it("uses adaptive precision per unit", () => {
    expect(byteCount(1500)).toBe("2 KB");
    expect(byteCount(999_499)).toBe("999 KB");
    expect(byteCount(999_999)).toBe("1 MB");
    expect(byteCount(1_234_567)).toBe("1.2 MB");
    expect(byteCount(123_456_789)).toBe("123.5 MB");
    expect(byteCount(1_000_000_000)).toBe("1 GB");
    expect(byteCount(1_234_567_890)).toBe("1.23 GB");
    expect(byteCount(12_345_678_901)).toBe("12.35 GB");
  });
});
