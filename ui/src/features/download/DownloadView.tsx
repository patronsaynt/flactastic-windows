import {
  AlertCircle,
  ArrowDownToLine,
  ArrowRight,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronLeft,
  ChevronUp,
  CircleDot,
  Disc3,
  Heart,
  Link,
  ListFilter,
  ListMusic,
  RefreshCw,
  ShieldHalf,
  TriangleAlert,
  UserCheck,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState, type CSSProperties, type ReactNode } from "react";
import {
  defaultOptions,
  FORMATS,
  isTerminal,
  qualitiesFor,
  useDownloads,
} from "../../app/downloads";
import { setSetting, useSetting } from "../../app/settings";
import { MenuPicker } from "../../components/settings/Primitives";
import { Checkbox } from "../../components/editors/Fields";
import { Modal } from "../../components/sheet/Sheet";
import {
  api,
  artworkUrl,
  LIKED_SONGS_ID,
  type DownloadJob,
  type JobStatus,
  type LucidaOptions,
  type RebuildSummary,
  type RemoteAlbum,
  type RemoteCoverArt,
  type RemotePlaylist,
  type RemoteResolve,
  type RemoteTrack,
  type SpotifyPlaylistSummary,
} from "../../lib/api";
import { formatDuration } from "../../lib/format";
import "./Download.css";

type Mode = "chooser" | "albumsTracks" | "playlists";

/** The VPN advisory pops once per app session. */
let offeredVpnNotice = false;

const SPOTIFY_TRACK_CAP = 100;

function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}

/** `RemoteCoverArt.best`: largest by area, first of equals. */
function bestCover(arts: RemoteCoverArt[] | undefined): RemoteCoverArt | undefined {
  let best: RemoteCoverArt | undefined;
  const area = (a: RemoteCoverArt) => (a.width ?? 0) * (a.height ?? 0);
  for (const a of arts ?? []) if (!best || area(a) > area(best)) best = a;
  return best;
}

function isSpotifyPlaylist(url: string): boolean {
  const u = url.trim();
  if (/^spotify:playlist:[^:]+$/.test(u)) return true;
  const m = /^[a-z]+:\/\/(?:[^/@]*@)?([^/:?#]+)[^/?#]*(\/[^?#]*)?/i.exec(u);
  if (!m) return false;
  const host = m[1].toLowerCase();
  if (host !== "open.spotify.com" && host !== "spotify.com") return false;
  const comps = (m[2] ?? "").split("/").filter(Boolean);
  const i = comps.indexOf("playlist");
  return i >= 0 && i + 1 < comps.length;
}

/** `ArtworkAccent.load`: the cover (via the artwork store) and its accent. */
function useRemoteArtwork(url: string | undefined) {
  const [art, setArt] = useState<{ id: string; accent: string | null } | null>(null);
  useEffect(() => {
    setArt(null);
    if (!url) return;
    let live = true;
    api
      .remoteArtwork(url)
      .then((r) => live && r && setArt({ id: r.id, accent: r.accent ? `rgb(${r.accent.join(" ")})` : null }))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [url]);
  return art;
}

/**
 * `DownloadTabView`: a two-panel chooser that drills into Albums & Tracks
 * (paste any service link) or Playlists (Spotify), with a docked downloads
 * drawer.
 */
export function DownloadView() {
  const [mode, setMode] = useState<Mode>("chooser");
  const showVpnNotice = useSetting("flactastic.showVpnNotice");
  const [vpnOpen, setVpnOpen] = useState(false);
  const [options, setOptions] = useState<LucidaOptions>(defaultOptions);

  // Albums & Tracks
  const [pasteURL, setPasteURL] = useState("");
  const [resolved, setResolved] = useState<RemoteResolve | null>(null);
  const [isWorking, setIsWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Playlists
  const [playlistURL, setPlaylistURL] = useState("");
  const [resolvedPlaylist, setResolvedPlaylist] = useState<RemotePlaylist | null>(null);
  const [playlistTruncated, setPlaylistTruncated] = useState(false);
  const [isResolvingPlaylist, setIsResolvingPlaylist] = useState(false);
  const [playlistError, setPlaylistError] = useState<string | null>(null);

  useEffect(() => {
    void api.lucidaWarmUp();
    if (!offeredVpnNotice && showVpnNotice) {
      offeredVpnNotice = true;
      setVpnOpen(true);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const resolvePlaylist = useCallback((raw: string) => {
    const trimmed = raw.trim();
    if (!trimmed) return;
    setResolvedPlaylist(null);
    setPlaylistTruncated(false);
    setPlaylistError(null);
    setIsResolvingPlaylist(true);
    // Fresh paste: back to the highest-quality defaults.
    setOptions(defaultOptions());
    api
      .spotifyResolvePlaylist(trimmed, false)
      .then((r) => {
        if (!r) return;
        setResolvedPlaylist(r.playlist);
        setPlaylistTruncated(r.wasTruncated);
      })
      .catch((e) => setPlaylistError(errorText(e)))
      .finally(() => setIsResolvingPlaylist(false));
  }, []);

  const resolve = useCallback(() => {
    const trimmed = pasteURL.trim();
    if (!trimmed) return;
    // Spotify playlists belong to the Playlists screen (Lucida's metadata
    // endpoint fails on them).
    if (isSpotifyPlaylist(trimmed)) {
      setPlaylistURL(trimmed);
      setPasteURL("");
      setMode("playlists");
      resolvePlaylist(trimmed);
      return;
    }
    setResolved(null);
    setError(null);
    setIsWorking(true);
    setOptions(defaultOptions());
    api
      .downloadResolve(trimmed)
      .then((r) => setResolved(r ?? null))
      .catch((e) => setError(errorText(e)))
      .finally(() => setIsWorking(false));
  }, [pasteURL, resolvePlaylist]);

  const openPlaylist = useCallback((p: SpotifyPlaylistSummary) => {
    setPlaylistError(null);
    setResolvedPlaylist(null);
    setPlaylistTruncated(false);
    setIsResolvingPlaylist(true);
    setOptions(defaultOptions());
    api
      .spotifyResolvePlaylist(p.externalUrl, true, p.id === LIKED_SONGS_ID)
      .then((r) => {
        if (!r) return;
        setResolvedPlaylist(r.playlist);
        setPlaylistTruncated(r.wasTruncated);
      })
      .catch((e) => setPlaylistError(errorText(e)))
      .finally(() => setIsResolvingPlaylist(false));
  }, []);

  const enqueue = useCallback((tracks: RemoteTrack[]) => void api.downloadEnqueue(tracks, options), [options]);

  let body: ReactNode;
  if (mode === "chooser") {
    body = <Chooser onPick={setMode} />;
  } else if (mode === "albumsTracks") {
    body = (
      <AlbumsTracks
        onBack={() => setMode("chooser")}
        pasteURL={pasteURL}
        setPasteURL={setPasteURL}
        resolve={resolve}
        resolved={resolved}
        isWorking={isWorking}
        error={error}
        clear={() => {
          setError(null);
          setResolved(null);
        }}
        options={options}
        setOptions={setOptions}
        enqueue={enqueue}
      />
    );
  } else {
    body = (
      <Playlists
        onBack={() => (resolvedPlaylist ? setResolvedPlaylist(null) : setMode("chooser"))}
        playlistURL={playlistURL}
        setPlaylistURL={setPlaylistURL}
        resolvePlaylist={() => resolvePlaylist(playlistURL)}
        resolvedPlaylist={resolvedPlaylist}
        truncated={playlistTruncated}
        resolving={isResolvingPlaylist}
        error={playlistError}
        clear={() => {
          setPlaylistError(null);
          setResolvedPlaylist(null);
        }}
        openPlaylist={openPlaylist}
        options={options}
        setOptions={setOptions}
      />
    );
  }

  return (
    <div className="dl">
      {body}
      <Modal open={vpnOpen} onClose={() => setVpnOpen(false)}>
        <VpnNotice onClose={() => setVpnOpen(false)} />
      </Modal>
    </div>
  );
}

// MARK: - Chooser

function Chooser({ onPick }: { onPick: (m: Mode) => void }) {
  const [hovered, setHovered] = useState<"left" | "right" | null>(null);
  const panel = (side: "left" | "right", p: { icon: ReactNode; eyebrow: string; title: string; subtitle: string; cta: string; accent: string; mode: Mode }) => {
    const isHovered = hovered === side;
    const dimmed = hovered != null && !isHovered;
    return (
      <button
        className={"dl-choice" + (isHovered ? " is-hovered" : "") + (dimmed ? " is-dimmed" : "")}
        style={{ "--choice-accent": p.accent } as CSSProperties}
        onPointerEnter={() => setHovered(side)}
        onPointerLeave={() => setHovered((h) => (h === side ? null : h))}
        onClick={() => {
          setHovered(null);
          onPick(p.mode);
        }}
      >
        <div className="dl-choice__top">
          <div className="dl-choice__icon">{p.icon}</div>
          <span className="dl-eyebrow">{p.eyebrow}</span>
        </div>
        <div className="dl-choice__spacer" />
        <div className="dl-choice__title">{p.title}</div>
        <div className="dl-choice__subtitle">{p.subtitle}</div>
        <div className="dl-choice__cta">
          {p.cta}
          <ArrowRight size={14} strokeWidth={2.4} />
        </div>
      </button>
    );
  };
  return (
    <div className="dl-chooser">
      <div className={"dl-chooser__header" + (hovered ? " is-dimmed" : "")}>
        <div className="dl-chooser__brand">
          <CircleDot size={14} strokeWidth={2} />
          <span>FLACtastic</span>
        </div>
        <div className="dl-chooser__title">Download</div>
        <div className="dl-chooser__subtitle">
          Choose what you'd like to pull down — every file arrives in its original lossless quality.
        </div>
      </div>
      <div className="dl-chooser__panels">
        {panel("left", {
          icon: <Disc3 size={30} strokeWidth={1.6} />,
          eyebrow: "Individual",
          title: "Tracks & Albums",
          subtitle: "Hand-pick singles or grab a complete album. Each file in full studio fidelity, exactly as it was mastered.",
          cta: "Browse library",
          accent: "var(--quality-hires)",
          mode: "albumsTracks",
        })}
        {panel("right", {
          icon: <ListMusic size={30} strokeWidth={1.6} />,
          eyebrow: "Collections",
          title: "Playlists",
          subtitle: "Download a whole playlist in a single pass. We bundle every track at the highest fidelity it's available in.",
          cta: "View playlists",
          accent: "var(--quality-cd)",
          mode: "playlists",
        })}
      </div>
    </div>
  );
}

// MARK: - Shared pieces

function ScreenHeader({ title, subtitle, onBack, backHelp, trailing }: { title: string; subtitle: string; onBack: () => void; backHelp: string; trailing?: ReactNode }) {
  return (
    <div className="dl-header">
      <button className="dl-back" onClick={onBack} title={backHelp}>
        <ChevronLeft size={18} strokeWidth={2.4} />
      </button>
      <div className="dl-header__text">
        <div className="dl-header__title">{title}</div>
        <div className="dl-header__subtitle">{subtitle}</div>
      </div>
      <div className="dl-header__spacer" />
      {trailing}
    </div>
  );
}

function PasteBar({ large, value, onChange, onSubmit, disabled, placeholder }: { large: boolean; value: string; onChange: (v: string) => void; onSubmit: () => void; disabled?: boolean; placeholder: string }) {
  return (
    <div className={"dl-paste" + (large ? " is-large" : "")}>
      <Link size={large ? 18 : 15} strokeWidth={1.8} className="dl-paste__icon" />
      <input
        className="dl-paste__input"
        value={value}
        placeholder={placeholder}
        disabled={disabled}
        spellCheck={false}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && onSubmit()}
      />
      <button className="dl-paste__fetch" onClick={onSubmit}>
        Fetch
        {large && <ArrowRight size={13} strokeWidth={3} />}
      </button>
    </div>
  );
}

function StateIcon({ icon, tint, glow }: { icon: ReactNode; tint: string; glow?: boolean }) {
  return (
    <div className={"dl-state-icon" + (glow ? " has-glow" : "")} style={{ "--tint": tint } as CSSProperties}>
      {icon}
    </div>
  );
}

function LoadingState() {
  return (
    <div className="dl-state">
      <div className="dl-spinner" />
      <div className="dl-state__loading">Fetching…</div>
    </div>
  );
}

function ErrorState({ title, message, action, actionIcon, onAction }: { title: string; message: string; action: string; actionIcon: ReactNode; onAction: () => void }) {
  return (
    <div className="dl-state">
      <StateIcon icon={<AlertCircle size={30} strokeWidth={1.6} />} tint="var(--quality-low)" />
      <div className="dl-state__title is-small">{title}</div>
      <div className="dl-state__body">{message}</div>
      <button className="dl-capsule" onClick={onAction}>
        {actionIcon}
        {action}
      </button>
    </div>
  );
}

// MARK: - Albums & Tracks

function AlbumsTracks(p: {
  onBack: () => void;
  pasteURL: string;
  setPasteURL: (v: string) => void;
  resolve: () => void;
  resolved: RemoteResolve | null;
  isWorking: boolean;
  error: string | null;
  clear: () => void;
  options: LucidaOptions;
  setOptions: (o: LucidaOptions) => void;
  enqueue: (t: RemoteTrack[]) => void;
}) {
  const coverURL = useMemo(() => {
    const r = p.resolved;
    if (!r) return undefined;
    if (r.kind === "album") return bestCover(r.album.coverArt)?.url;
    if (r.kind === "track") return bestCover(r.track.coverArt)?.url;
    if (r.kind === "playlist") return bestCover(r.playlist.coverArt)?.url;
    return undefined;
  }, [p.resolved]);
  const art = useRemoteArtwork(coverURL);
  const accent = art?.accent ?? "var(--quality-hires)";
  const jobs = useDownloads((s) => s.jobs);

  let body: ReactNode;
  if (p.isWorking) body = <LoadingState />;
  else if (p.error != null)
    body = (
      <ErrorState
        title="Couldn't fetch that link"
        message={p.error || "That URL doesn't look valid, or the content is region-locked or private. Check the link and try again."}
        action="Try another link"
        actionIcon={<RefreshCw size={15} strokeWidth={2.2} />}
        onAction={p.clear}
      />
    );
  else if (p.resolved) body = <div className="dl-scroll"><div className="dl-resolved">{resolvedBody(p.resolved, art?.id ?? null, p.options, p.setOptions, p.enqueue)}</div></div>;
  else
    body = (
      <div className="dl-state">
        <StateIcon icon={<Link size={32} strokeWidth={1.6} />} tint={accent} glow />
        <div className="dl-state__title">Paste a link to get started</div>
        <div className="dl-state__body is-wide">
          Drop in a track or album URL and we'll resolve every track at its highest available fidelity.
        </div>
        <div className="dl-state__paste">
          <PasteBar large value={p.pasteURL} onChange={p.setPasteURL} onSubmit={p.resolve} disabled={p.isWorking} placeholder="Paste a track or album URL" />
        </div>
        <div className="dl-works-with">
          <span className="dl-eyebrow">Works with</span>
          {["Spotify", "Tidal", "Qobuz", "Amazon Music", "SoundCloud"].map((s) => (
            <span key={s} className="dl-chip">
              {s}
            </span>
          ))}
        </div>
      </div>
    );

  return (
    <div className="dl-screen" style={{ "--dl-accent": accent } as CSSProperties}>
      <ScreenHeader
        title="Albums & Tracks"
        subtitle="Paste a link from Spotify, Tidal, Qobuz, Amazon Music, or SoundCloud."
        onBack={p.onBack}
        backHelp="Back to Download"
        trailing={
          (p.resolved != null || p.error != null) && (
            <div style={{ width: 340 }}>
              <PasteBar large={false} value={p.pasteURL} onChange={p.setPasteURL} onSubmit={p.resolve} disabled={p.isWorking} placeholder="Paste a track or album URL" />
            </div>
          )
        }
      />
      <div className="dl-body">{body}</div>
      {jobs.length > 0 && <DownloadsDrawer jobs={jobs} />}
    </div>
  );
}

function albumMeta(a: RemoteAlbum): string {
  const parts = [a.artists.map((x) => x.name).join(", ")];
  if (a.releaseYear != null) parts.push(String(a.releaseYear));
  const count = a.trackCount ?? a.tracks.length;
  parts.push(`${count} track${count === 1 ? "" : "s"}`);
  const total = a.tracks.reduce((s, t) => s + (t.durationSeconds ?? 0), 0);
  if (total > 0) parts.push(`${Math.floor(total / 60)} min`);
  return parts.join(" · ");
}

function resolvedBody(r: RemoteResolve, artId: string | null, options: LucidaOptions, setOptions: (o: LucidaOptions) => void, enqueue: (t: RemoteTrack[]) => void) {
  const common = { artId, options, setOptions, onTrack: (t: RemoteTrack) => enqueue([t]) };
  switch (r.kind) {
    case "album":
      return <Collection kind="Album" title={r.album.title} meta={albumMeta(r.album)} tracks={r.album.tracks} downloadLabel="Download album" onDownloadAll={() => enqueue(r.album.tracks)} {...common} />;
    case "track":
      return <Collection kind="Track" title={r.track.title} meta={r.track.artists.map((a) => a.name).join(", ")} tracks={[r.track]} downloadLabel="Download track" onDownloadAll={() => enqueue([r.track])} {...common} />;
    case "playlist":
      return (
        <Collection
          kind="Playlist"
          title={r.playlist.title}
          meta={r.playlist.creator ? `by ${r.playlist.creator}` : `${r.playlist.tracks.length} tracks`}
          tracks={r.playlist.tracks}
          downloadLabel="Download all"
          onDownloadAll={() => enqueue(r.playlist.tracks)}
          {...common}
        />
      );
    case "artist":
      return (
        <div className="dl-artist">
          <div className="dl-artist__title">Artist: {r.artist.name}</div>
          <div className="dl-artist__body">Paste a track or album URL to download.</div>
        </div>
      );
  }
}

/** The hero, collapsible options and tracklist shared by every resolved kind. */
function Collection(p: {
  kind: string;
  title: string;
  meta: string;
  tracks: RemoteTrack[];
  downloadLabel: string;
  onDownloadAll: () => void;
  artId: string | null;
  options: LucidaOptions;
  setOptions: (o: LucidaOptions) => void;
  onTrack: ((t: RemoteTrack) => void) | null;
}) {
  const [optionsOpen, setOptionsOpen] = useState(false);
  const allLossless = p.tracks.length > 0 && p.tracks.every((t) => t.isLossless);
  return (
    <div>
      <div className="dl-hero">
        <div className="dl-hero__art">
          <div className="dl-hero__glow" />
          <div className="dl-hero__cover">
            {p.artId ? (
              <img src={artworkUrl(p.artId, 176)} alt="" draggable={false} />
            ) : (
              <div className="dl-hero__placeholder">
                <Disc3 size={40} strokeWidth={0.8} />
              </div>
            )}
          </div>
        </div>
        <div className="dl-hero__info">
          <div className="dl-hero__kind">
            <span className={"dl-quality" + (allLossless ? " is-lossless" : "")}>{allLossless ? "FLAC" : "Lossy"}</span>
            <span className="dl-eyebrow">{p.kind}</span>
          </div>
          <div className="dl-hero__title">{p.title}</div>
          <div className="dl-hero__meta">{p.meta}</div>
          <div className="dl-hero__actions">
            <button className="dl-primary" onClick={p.onDownloadAll}>
              <ArrowDownToLine size={16} strokeWidth={2.4} />
              {p.downloadLabel}
            </button>
            <button className="dl-options-toggle" onClick={() => setOptionsOpen((o) => !o)}>
              <ListFilter size={15} strokeWidth={1.8} className="dl-options-toggle__icon" />
              <span className="dl-options-toggle__summary">{optionsSummary(p.options)}</span>
              <span className="dl-options-toggle__rule" />
              {optionsOpen ? <ChevronUp size={13} strokeWidth={2.6} /> : <ChevronDown size={13} strokeWidth={2.6} />}
            </button>
          </div>
        </div>
      </div>
      {optionsOpen && <OptionsPanel options={p.options} setOptions={p.setOptions} />}
      <TrackList tracks={p.tracks} onTrack={p.onTrack} />
    </div>
  );
}

function optionsSummary(o: LucidaOptions): string {
  const parts: string[] = [];
  parts.push(o.format === "original" ? "Original quality" : FORMATS.find((f) => f.value === o.format)!.label);
  const q = qualitiesFor(o.format).find((x) => x.value === o.quality);
  if (q) parts.push(q.label);
  parts.push(`Region ${o.region || "auto"}`);
  return parts.join("   ·   ");
}

function OptionsPanel({ options, setOptions }: { options: LucidaOptions; setOptions: (o: LucidaOptions) => void }) {
  const qualities = qualitiesFor(options.format);
  return (
    <div className="dl-options">
      <div className="dl-options__head">
        <span className="dl-eyebrow">Download options</span>
        <span className="dl-options__rule" />
        <span className="dl-options__note">Applies to every track you download</span>
      </div>
      <div className="dl-options__row">
        <div className="dl-options__field" style={{ width: 200 }}>
          <div className="dl-options__label">Format</div>
          <MenuPicker
            value={options.format}
            options={FORMATS}
            onChange={(format) => setOptions({ ...options, format, quality: qualitiesFor(format)[0]?.value ?? null })}
          />
        </div>
        {qualities.length > 0 && (
          <div className="dl-options__field" style={{ width: 170 }}>
            <div className="dl-options__label">Quality</div>
            <MenuPicker
              value={options.quality ?? qualities[0].value}
              options={qualities}
              onChange={(quality) => setOptions({ ...options, quality })}
            />
          </div>
        )}
        <div className="dl-options__field">
          <div className="dl-options__label">Region</div>
          <input
            className="dl-options__region"
            value={options.region}
            placeholder="auto or country code"
            spellCheck={false}
            onChange={(e) => setOptions({ ...options, region: e.target.value })}
          />
        </div>
      </div>
      <div className="dl-options__checks">
        <OptionCheck on={options.addMetadata} onToggle={() => setOptions({ ...options, addMetadata: !options.addMetadata })} title="Embed metadata + cover art" subtitle="Tags & artwork written into each file" />
        <OptionCheck on={options.compatibility} onToggle={() => setOptions({ ...options, compatibility: !options.compatibility })} title="Player compatibility" subtitle="Smaller cover, ID3v2.3 for older hardware" />
      </div>
    </div>
  );
}

function OptionCheck({ on, onToggle, title, subtitle }: { on: boolean; onToggle: () => void; title: string; subtitle: string }) {
  return (
    <button className="dl-check" onClick={onToggle}>
      <span className={"dl-check__box" + (on ? " is-on" : "")}>{on && <Check size={11} strokeWidth={3.4} />}</span>
      <span className="dl-check__text">
        <span className="dl-check__title">{title}</span>
        <span className="dl-check__subtitle">{subtitle}</span>
      </span>
    </button>
  );
}

function TrackList({ tracks, onTrack }: { tracks: RemoteTrack[]; onTrack: ((t: RemoteTrack) => void) | null }) {
  return (
    <div className="dl-tracks">
      <div className="dl-tracks__head">
        <span className="dl-tracks__num">#</span>
        <span className="dl-eyebrow">Title</span>
        <span className="dl-header__spacer" />
        <span className="dl-eyebrow" style={{ paddingRight: 64 }}>
          Length
        </span>
      </div>
      {tracks.map((t) => (
        <div key={t.id} className="dl-track">
          <span className="dl-track__num">{t.trackNumber != null ? String(t.trackNumber).padStart(2, "0") : "–"}</span>
          <div className="dl-track__text">
            <div className="dl-track__title">{t.title}</div>
            <div className="dl-track__artist">{t.artists.map((a) => a.name).join(", ")}</div>
          </div>
          {t.isLossless ? <span className="dl-track__flac">FLAC</span> : <span className="dl-track__lossy">Lossy</span>}
          <span className="dl-track__duration">{formatDuration(t.durationSeconds)}</span>
          {onTrack ? (
            <button className="dl-track__download" title="Download track" onClick={() => onTrack(t)}>
              <ArrowDownToLine size={15} strokeWidth={1.8} />
            </button>
          ) : (
            <span className="dl-track__download" />
          )}
        </div>
      ))}
    </div>
  );
}

// MARK: - Downloads drawer

function jobProgress(s: JobStatus): number {
  switch (s.kind) {
    case "downloading":
      return s.totalBytes ? s.receivedBytes / s.totalBytes : 0.05;
    case "tagging":
    case "finishing":
    case "completed":
      return 1;
    default:
      return 0;
  }
}

function jobColor(s: JobStatus): string {
  switch (s.kind) {
    case "downloading":
    case "tagging":
    case "finishing":
      return "var(--dl-accent, var(--quality-hires))";
    case "completed":
      return "var(--quality-cd)";
    case "failed":
      return "var(--quality-low)";
    default:
      return "var(--text-tertiary)";
  }
}

function jobLabel(s: JobStatus): string {
  switch (s.kind) {
    case "queued":
      return "Queued";
    case "downloading":
      return s.totalBytes ? `Downloading… ${Math.floor((s.receivedBytes / s.totalBytes) * 100)}%` : `Downloading… ${Math.floor(s.receivedBytes / 1024)} KB`;
    case "tagging":
      return "Tagging…";
    case "finishing":
      return "Moving into library…";
    case "completed":
      return "Done";
    case "failed":
      return `Failed — ${s.message}`;
    case "cancelled":
      return "Cancelled";
    case "skipped":
      return "Already in library";
  }
}

function DownloadsDrawer({ jobs }: { jobs: DownloadJob[] }) {
  const [open, setOpen] = useState(false);
  const active = jobs.filter((j) => !isTerminal(j)).length;
  const done = jobs.filter((j) => j.status.kind === "completed").length;
  const summary = active > 0 ? `${active} active · ${done} done` : `${jobs.length} complete`;
  return (
    <div className="dl-drawer">
      <div className="dl-drawer__bar" onClick={() => setOpen((o) => !o)}>
        <span className="dl-drawer__icon">
          <ArrowDownToLine size={14} strokeWidth={1.8} />
        </span>
        <span className="dl-drawer__title">Downloads</span>
        <span className="dl-drawer__summary">{summary}</span>
        <span className="dl-header__spacer" />
        {active > 0 && (
          <button className="dl-link" onClick={(e) => (e.stopPropagation(), void api.downloadCancelAll())}>
            Cancel all
          </button>
        )}
        <button className="dl-link" onClick={(e) => (e.stopPropagation(), void api.downloadClearCompleted())}>
          Clear completed
        </button>
        {open ? <ChevronDown size={14} strokeWidth={2.4} className="dl-drawer__chevron" /> : <ChevronUp size={14} strokeWidth={2.4} className="dl-drawer__chevron" />}
      </div>
      {open && (
        <div className="dl-drawer__list">
          {jobs.map((j) => (
            <div key={j.id} className="dl-job">
              <span className="dl-job__dot" style={{ background: jobColor(j.status) }} />
              <div className="dl-job__main">
                <div className="dl-job__names">
                  <span className="dl-job__title">{j.track.title}</span>
                  <span className="dl-job__artist">{j.track.artists.map((a) => a.name).join(", ")}</span>
                </div>
                <div className="dl-job__track">
                  <div className="dl-job__fill" style={{ width: `${jobProgress(j.status) * 100}%`, background: jobColor(j.status) }} />
                </div>
              </div>
              <span className="dl-job__status" title={jobLabel(j.status)}>
                {jobLabel(j.status)}
              </span>
              {!isTerminal(j) ? (
                <button className="dl-job__action" title="Cancel download" onClick={() => void api.downloadCancel(j.id)}>
                  <X size={14} strokeWidth={2.6} />
                </button>
              ) : j.status.kind === "completed" ? (
                <span className="dl-job__action is-done">
                  <Check size={15} strokeWidth={2.6} />
                </span>
              ) : (
                <span className="dl-job__action" />
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// MARK: - Playlists

function Playlists(p: {
  onBack: () => void;
  playlistURL: string;
  setPlaylistURL: (v: string) => void;
  resolvePlaylist: () => void;
  resolvedPlaylist: RemotePlaylist | null;
  truncated: boolean;
  resolving: boolean;
  error: string | null;
  clear: () => void;
  openPlaylist: (s: SpotifyPlaylistSummary) => void;
  options: LucidaOptions;
  setOptions: (o: LucidaOptions) => void;
}) {
  const spotify = useDownloads((s) => s.spotify);
  const rebuild = useDownloads((s) => s.rebuild);
  const jobs = useDownloads((s) => s.jobs);
  const connected = spotify.connection.kind === "connected";
  const art = useRemoteArtwork(bestCover(p.resolvedPlaylist?.coverArt)?.url);
  const accent = art?.accent ?? "var(--quality-hires)";

  const state = p.resolving ? "resolving" : p.error != null ? "error" : p.resolvedPlaylist ? "resolved" : connected ? "grid" : "notConnected";
  const sub =
    state === "resolved"
      ? "Downloading a full playlist — every track at its highest available fidelity."
      : state === "grid" || state === "resolving"
        ? "Your Spotify library — pick a playlist to download in full."
        : "Connect your account, or paste a public Spotify playlist link.";

  let body: ReactNode;
  switch (state) {
    case "resolving":
      body = <LoadingState />;
      break;
    case "error":
      body = (
        <ErrorState
          title="Couldn't read that playlist"
          message={p.error || "It may be private, or the link isn't a Spotify playlist."}
          action={connected ? "Back to playlists" : "Back"}
          actionIcon={<ChevronLeft size={15} strokeWidth={2.4} />}
          onAction={p.clear}
        />
      );
      break;
    case "resolved": {
      const pl = p.resolvedPlaylist!;
      body = (
        <div className="dl-scroll">
          <div className="dl-resolved is-playlist">
            {connected && (
              <button className="dl-all-playlists" onClick={p.clear}>
                <ChevronLeft size={12} strokeWidth={2.6} />
                All playlists
              </button>
            )}
            {p.truncated && (
              <div className="dl-truncated">
                <TriangleAlert size={15} strokeWidth={2.2} />
                <span>
                  Showing the first {SPOTIFY_TRACK_CAP} tracks. Spotify's public preview caps longer playlists — connect your account to fetch every track.
                </span>
              </div>
            )}
            <Collection
              kind="Playlist"
              title={pl.title}
              meta={playlistMeta(pl)}
              tracks={pl.tracks}
              downloadLabel="Download playlist"
              onDownloadAll={() => void api.rebuildStart(pl, p.options)}
              artId={art?.id ?? null}
              options={p.options}
              setOptions={p.setOptions}
              onTrack={(t) => void api.downloadEnqueue([t], p.options)}
            />
          </div>
        </div>
      );
      break;
    }
    case "grid":
      body = <PlaylistGrid playlists={spotify.playlists} error={spotify.playlistsError} onOpen={p.openPlaylist} />;
      break;
    default:
      body = (
        <div className="dl-state">
          <StateIcon icon={<ListMusic size={32} strokeWidth={1.6} />} tint="var(--quality-cd)" glow />
          <div className="dl-state__title">Connect your Spotify account</div>
          <div className="dl-state__body is-wide">
            Log in to list your own playlists here and download any of them in full lossless quality.
          </div>
          <SpotifyConnectButton />
          <div className="dl-or">
            <span className="dl-or__rule" />
            <span className="dl-eyebrow">or paste a public link</span>
            <span className="dl-or__rule" />
          </div>
          <div className="dl-pl-paste">
            <Link size={16} strokeWidth={1.8} className="dl-paste__icon" />
            <input
              className="dl-paste__input"
              value={p.playlistURL}
              placeholder="open.spotify.com/playlist/…"
              disabled={p.resolving}
              spellCheck={false}
              onChange={(e) => p.setPlaylistURL(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && p.resolvePlaylist()}
            />
            <button className="dl-paste__fetch" onClick={p.resolvePlaylist}>
              Fetch
            </button>
          </div>
        </div>
      );
  }

  return (
    <div className="dl-screen" style={{ "--dl-accent": accent } as CSSProperties}>
      <ScreenHeader
        title="Playlists"
        subtitle={sub}
        onBack={p.onBack}
        backHelp="Back"
        trailing={
          spotify.connection.kind === "connected" && (
            <div className="dl-connected-chip">
              <span className="dl-connected-chip__dot" />
              <span>
                Spotify connected · <b>{spotify.connection.displayName}</b>
              </span>
            </div>
          )
        }
      />
      {rebuild.kind !== "idle" && <RebuildBar />}
      <div className="dl-body">{body}</div>
      {jobs.length > 0 && <DownloadsDrawer jobs={jobs} />}
    </div>
  );
}

function playlistMeta(p: RemotePlaylist): string {
  const parts: string[] = [];
  if (p.creator) parts.push(`by ${p.creator}`);
  parts.push(`${p.tracks.length} track${p.tracks.length === 1 ? "" : "s"}`);
  const total = p.tracks.reduce((s, t) => s + (t.durationSeconds ?? 0), 0);
  if (total > 0) parts.push(`${Math.floor(total / 60)} min`);
  return parts.join(" · ");
}

export function SpotifyConnectButton() {
  const connection = useDownloads((s) => s.spotify.connection);
  const connecting = connection.kind === "connecting";
  return (
    <div className="dl-connect">
      <button className="dl-connect__button" disabled={connecting} onClick={() => void api.spotifyConnect().catch(() => {})}>
        {connecting ? <span className="dl-spinner is-small is-dark" /> : <UserCheck size={17} strokeWidth={2.2} />}
        {connecting ? "Connecting…" : "Connect Spotify"}
      </button>
      {connecting && (
        <button className="dl-link" onClick={() => void api.spotifyCancelConnect()}>
          Cancel
        </button>
      )}
    </div>
  );
}

function PlaylistGrid({ playlists, error, onOpen }: { playlists: SpotifyPlaylistSummary[]; error: string | null; onOpen: (p: SpotifyPlaylistSummary) => void }) {
  const showLiked = useSetting("flactastic.showSpotifyLikedSongs");
  useEffect(() => {
    if (playlists.length === 0) void api.spotifyLoadPlaylists();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const liked: SpotifyPlaylistSummary = {
    id: LIKED_SONGS_ID,
    name: "Liked Songs",
    owner: null,
    trackCount: -1,
    coverArtUrl: null,
    externalUrl: "https://open.spotify.com/collection/tracks",
  };
  const cards = showLiked ? [liked, ...playlists] : playlists;
  return (
    <div className="dl-scroll">
      <div className="dl-grid-wrap">
        <div className="dl-grid-head">
          <span className="dl-eyebrow">Your playlists</span>
          <span className="dl-grid-head__count">{playlists.length} from Spotify</span>
        </div>
        {error && playlists.length === 0 && <div className="dl-grid-error">{error}</div>}
        <div className="dl-grid">
          {cards.map((p) => (
            <button key={p.id} className="dl-card" onClick={() => onOpen(p)}>
              <div className={"dl-card__art" + (p.id === LIKED_SONGS_ID ? " is-liked" : "")}>
                {p.coverArtUrl ? (
                  <img src={p.coverArtUrl} alt="" draggable={false} referrerPolicy="no-referrer" />
                ) : p.id === LIKED_SONGS_ID ? (
                  <Heart size={28} strokeWidth={1} fill="currentColor" />
                ) : (
                  <ListMusic size={28} strokeWidth={0.9} />
                )}
              </div>
              <div className="dl-card__name">{p.name}</div>
              <div className="dl-card__meta">{p.trackCount < 0 ? "your saved tracks" : `${p.owner ?? "you"} · ${p.trackCount} tracks`}</div>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function RebuildBar() {
  const phase = useDownloads((s) => s.rebuild);
  const cancel = (
    <button className="dl-link is-secondary" onClick={() => void api.rebuildCancel()}>
      Cancel
    </button>
  );
  let content: ReactNode = null;
  if (phase.kind === "fetchingArtwork") {
    content = (
      <div className="dl-rebuild__row">
        <span className="dl-spinner is-small" />
        <span className="dl-rebuild__title is-medium">Preparing playlist…</span>
        <span className="dl-header__spacer" />
        {cancel}
      </div>
    );
  } else if (phase.kind === "running") {
    content = (
      <>
        <div className="dl-rebuild__row">
          <span className="dl-rebuild__title">Rebuilding playlist</span>
          <span className="dl-header__spacer" />
          <span className="dl-rebuild__count">
            {phase.current} of {phase.total} done
          </span>
          {cancel}
        </div>
        <div className="dl-job__track">
          <div className="dl-job__fill" style={{ width: `${(phase.current / Math.max(phase.total, 1)) * 100}%`, background: "var(--dl-accent)" }} />
        </div>
      </>
    );
  } else if (phase.kind === "finished") {
    content = (
      <div className="dl-rebuild__row">
        <CheckCircle2 size={16} className="dl-rebuild__check" />
        <span className="dl-rebuild__title">{phase.summary.isResume ? "Playlist updated" : "Playlist rebuilt"}</span>
        <span className="dl-rebuild__summary">{summaryLine(phase.summary)}</span>
        <span className="dl-header__spacer" />
        <button className="dl-link is-secondary" onClick={() => void api.rebuildDismiss()}>
          Dismiss
        </button>
      </div>
    );
  }
  return <div className="dl-rebuild">{content}</div>;
}

function summaryLine(s: RebuildSummary): string {
  const parts = [`${s.downloaded} downloaded`, `${s.reused.length} already in library`];
  if (s.alreadyInPlaylist > 0) parts.push(`${s.alreadyInPlaylist} already in playlist`);
  parts.push(`${s.failures.length} failed`);
  return "· " + parts.join(" · ");
}

// MARK: - VPN notice

function VpnNotice({ onClose }: { onClose: () => void }) {
  const showAgain = useSetting("flactastic.showVpnNotice");
  return (
    <div className="dl-vpn">
      <div className="dl-vpn__head">
        <ShieldHalf size={28} strokeWidth={2.2} />
        <span>Protect your connection!</span>
      </div>
      <div className="dl-vpn__body">
        Always use a VPN when downloading files, and only download files you already own or have a license to.
      </div>
      <div className="dl-vpn__happy">Happy listening!</div>
      <div className="dl-vpn__check">
        <Checkbox
          label="Don't show this again"
          checked={!showAgain}
          onChange={(v) => {
            setSetting("flactastic.showVpnNotice", !v);
            if (v) onClose();
          }}
        />
      </div>
      <button className="pill-btn is-primary" onClick={onClose} autoFocus>
        Got it
      </button>
    </div>
  );
}
