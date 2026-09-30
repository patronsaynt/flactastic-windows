/**
 * `PlayerState` mirror. The backend publishes structural changes at once and
 * the clock every 100 ms while playing; `usePlaybackTime` interpolates
 * between updates so the seek bar moves at display rate.
 */
import { useEffect, useState } from "react";
import { create } from "zustand";
import { api, on, type PlayerSnapshot, type QueueItem, type Track } from "../lib/api";

interface PlayerState extends Omit<PlayerSnapshot, "queue"> {
  queue: QueueItem[];
  currentTrack: Track | null;
  /** performance.now() when currentTime was last reported. */
  stampedAt: number;
}

export const usePlayer = create<PlayerState>(() => ({
  currentTrackId: null,
  currentIndex: 0,
  isPlaying: false,
  currentTime: 0,
  duration: null,
  volume: 0.75,
  shuffle: false,
  repeat: "off",
  playbackSource: null,
  isQueueVisible: false,
  queueRevision: -1,
  outputSampleRate: null,
  lastError: null,
  queue: [],
  currentTrack: null,
  stampedAt: 0,
}));

function apply(s: PlayerSnapshot) {
  const prev = usePlayer.getState();
  const queue = s.queue ?? prev.queue;
  const currentTrack =
    queue[s.currentIndex]?.track.id === s.currentTrackId
      ? queue[s.currentIndex].track
      : (queue.find((q) => q.track.id === s.currentTrackId)?.track ?? null);
  usePlayer.setState({ ...s, queue, currentTrack, stampedAt: performance.now() });
}

export function startPlayerSync(): () => void {
  void api.playerSnapshot().then((s) => s && apply(s));
  return on<PlayerSnapshot>("player://state", apply);
}

/** The interpolated position right now, for animation loops outside React. */
export function playbackTimeNow(): number {
  const { currentTime, isPlaying, stampedAt, duration } = usePlayer.getState();
  if (!isPlaying) return currentTime;
  const t = currentTime + Math.min(0.25, Math.max(0, (performance.now() - stampedAt) / 1000));
  return duration ? Math.min(t, duration) : t;
}

/** Current position, advancing smoothly between backend ticks. */
export function usePlaybackTime(): number {
  const { currentTime, isPlaying, stampedAt, duration } = usePlayer();
  const [now, setNow] = useState(() => performance.now());
  useEffect(() => {
    if (!isPlaying) return;
    let raf = 0;
    const loop = () => {
      setNow(performance.now());
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [isPlaying]);
  if (!isPlaying) return currentTime;
  // Never extrapolate more than one backend interval ahead.
  const t = currentTime + Math.min(0.25, Math.max(0, (now - stampedAt) / 1000));
  return duration ? Math.min(t, duration) : t;
}

export const player = {
  toggle: () => api.transport("toggle"),
  next: () => api.transport("next"),
  previous: () => api.transport("previous"),
  toggleShuffle: () => api.transport("shuffle"),
  cycleRepeat() {
    const r = usePlayer.getState().repeat;
    void api.setRepeat(r === "off" ? "all" : r === "all" ? "one" : "off");
  },
  seek(t: number) {
    // Optimistic: the bar shouldn't jump back while the engine re-buffers.
    usePlayer.setState({ currentTime: t, stampedAt: performance.now() });
    void api.seek(t);
  },
  setVolume(v: number) {
    usePlayer.setState({ volume: v });
    void api.setVolume(v);
  },
  setQueueVisible(v: boolean) {
    usePlayer.setState({ isQueueVisible: v });
    void api.setQueueVisible(v);
  },
  play: (tracks: Track[], start = 0, source?: string | null, shuffle?: boolean) =>
    api.playTracks(
      tracks.map((t) => t.id),
      start,
      source,
      shuffle,
    ),
  playNext: (tracks: Track[]) => api.playNext(tracks.map((t) => t.id)),
  addToQueue: (tracks: Track[]) => api.addToQueue(tracks.map((t) => t.id)),
};
