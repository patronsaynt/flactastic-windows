import { open } from "@tauri-apps/plugin-dialog";
import {
  Check as CheckIcon,
  ChevronDown,
  Clock,
  FolderOpen,
  Headphones,
  LayoutGrid,
  Layers,
  ListMusic,
  Music,
  Pin,
  Play,
  Star,
  TrendingUp,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { albumTracks, useLibrary } from "../../app/library";
import { player } from "../../app/player";
import { playlistTracks, usePlaylists } from "../../app/playlists";
import { setSetting, useSetting } from "../../app/settings";
import { useUI } from "../../app/store";
import { api, artworkUrl, on, type Album, type HomeHighlight, type HomeMetrics, type Playlist, type RecentContext } from "../../lib/api";
import { ArtworkView } from "../../components/ArtworkView";
import { ActionPill } from "../../components/chrome/Chrome";
import { contextMenu, menu, openMenuAt, type MenuItem } from "../../components/menu/ContextMenu";
import wordmark from "../../assets/Wordmark.png";
import { artistMenuItems } from "../artist/ArtistLink";
import { addToPlaylistItem, playbackItems, removeFromLibraryItem } from "../collection/menus";
import { displayCredit } from "../collection/AlbumCard";
import { Fidelidex } from "./Fidelidex";
import { abbreviated, minutesLabel, ProgressBar, StatCard, WeeklyChart } from "./HomeParts";
import "./Home.css";

export async function chooseLibraryFolder() {
  const dir = await open({ directory: true, multiple: false, title: "Choose your music folder" });
  if (typeof dir === "string") await api.openLibrary(dir);
}

type StatsRange = "allTime" | "year" | "month" | "week";
const RANGES: { value: StatsRange; label: string }[] = [
  { value: "allTime", label: "All time" },
  { value: "year", label: "Past year" },
  { value: "month", label: "Past month" },
  { value: "week", label: "Past week" },
];

/**
 * `HomeView`: hero (wordmark, date, lyric highlight), Recently Played,
 * listening stats, Top Albums This Week, Fidelidex, footer. History-driven
 * sections stay hidden until there is history.
 */
export function HomeView() {
  const range = (useSetting("flactastic.home.statsRange") as StatsRange) ?? "allTime";
  const revision = useLibrary((s) => s.revision);
  const loaded = useLibrary((s) => s.hasCompletedInitialLoad);
  const root = useLibrary((s) => s.root);
  const [metrics, setMetrics] = useState<HomeMetrics | null>(null);
  const [highlight, setHighlight] = useState<HomeHighlight | null>(null);

  const recompute = useCallback(() => {
    void api.homeMetrics(range).then((m) => m && setMetrics(m));
  }, [range]);
  useEffect(recompute, [recompute, revision]);
  useEffect(() => on("listening://changed", recompute), [recompute]);

  useEffect(() => {
    if (loaded) void api.homeHighlight().then((h) => setHighlight(h ?? null));
  }, [loaded]);

  return (
    <div className="home-scroll">
      <div className="home">
        <Hero highlight={highlight} onPinToggled={setHighlight} />
        {!root && loaded && (
          <div>
            <ActionPill primary icon={FolderOpen} onClick={() => void chooseLibraryFolder()}>
              Choose Library Folder…
            </ActionPill>
          </div>
        )}
        {metrics && <RecentlyPlayed items={metrics.recentlyPlayed} />}
        {metrics?.hasHistory && <ListeningStats metrics={metrics} range={range} />}
        {metrics && <TopAlbums metrics={metrics} />}
        <Fidelidex />
        {metrics && (
          <div className="home-footer">
            {metrics.footerAlbumCount.toLocaleString()} albums · {metrics.footerHours.toLocaleString()} hours of music
          </div>
        )}
      </div>
    </div>
  );
}

// MARK: - Hero

function Hero({ highlight, onPinToggled }: { highlight: HomeHighlight | null; onPinToggled: (h: HomeHighlight | null) => void }) {
  const date = new Date()
    .toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" })
    .toUpperCase();
  return (
    <div className="hero">
      {highlight && <HeroBanner image={highlight.image} />}
      <div className="hero__wordmark" style={{ maskImage: `url(${wordmark})`, WebkitMaskImage: `url(${wordmark})` }} />
      <div className="hero__date">{date}</div>
      {highlight ? (
        <>
          <FitLyric text={`“${highlight.lyric}”`} />
          <div className="hero__credit">
            <Music size={12} />
            <span>{highlight.songTitle}</span>
            {highlight.artistDisplay && (
              <>
                <span>—</span>
                <span>{highlight.artistDisplay}</span>
              </>
            )}
          </div>
          <button
            className={"hero__pin" + (highlight.isPinned ? " is-pinned" : "")}
            title={highlight.isPinned ? "Unpin lyric" : "Pin lyric"}
            onClick={() => void api.toggleHighlightPin().then((h) => onPinToggled(h ?? null))}
          >
            <Pin size={13} fill={highlight.isPinned ? "currentColor" : "none"} />
          </button>
        </>
      ) : (
        <div className="hero__headline">Welcome to your library.</div>
      )}
    </div>
  );
}

/**
 * 42 pt bold italic, at most two lines, shrinking to 55% before it would
 * need a third (`lineLimit(2).minimumScaleFactor(0.55)`).
 */
function FitLyric({ text }: { text: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState(42);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = () => {
      let s = 42;
      el.style.fontSize = `${s}px`;
      while (s > 42 * 0.55 && el.scrollHeight > s * 1.2 * 2 + 2) {
        s -= 1;
        el.style.fontSize = `${s}px`;
      }
      setSize(s);
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el.parentElement!);
    return () => ro.disconnect();
  }, [text]);
  return (
    <div ref={ref} className="hero__headline is-lyric" style={{ fontSize: size }}>
      {text}
    </div>
  );
}

/** Blurred artist image pushed right, fading out toward the headline. */
function HeroBanner({ image }: { image: string | null }) {
  const zoom = useSetting("flactastic.uiScale");
  return (
    <div className="hero__banner" aria-hidden>
      <div className="hero__banner-image">
        {image ? <img src={artworkUrl(image, 640, zoom)} alt="" /> : <div className="hero__blob" />}
      </div>
      <div className="hero__banner-fade" />
    </div>
  );
}

// MARK: - Recently Played

function RecentlyPlayed({ items }: { items: RecentContext[] }) {
  const albumsById = useLibrary((s) => s.albumsById);
  const playlistsById = usePlaylists((s) => s.byId);
  if (!items.length) return null;
  return (
    <section className="home-section">
      <div className="home-section-title">RECENTLY PLAYED</div>
      <div className="recent-row">
        {items.map((item) => {
          if (item.kind === "album") {
            const album = albumsById.get(item.targetID);
            const subtitle = album?.isMixCompilation
              ? "Mix Compilation"
              : album?.artist
                ? displayCredit(album.artist)
                : displayCredit(item.subtitle);
            return (
              <Tile
                key={"a:" + item.targetID}
                artwork={album?.artwork}
                title={item.title}
                subtitle={subtitle}
                enabled={!!album}
                onOpen={() => album && useUI.getState().navigateToAlbum(album.id)}
                menu={() => (album ? albumMenu(album) : [])}
              />
            );
          }
          const playlist = findPlaylist(playlistsById, item.targetID);
          return (
            <Tile
              key={"p:" + item.targetID}
              artwork={playlist?.artwork}
              title={item.title}
              subtitle={item.subtitle}
              enabled={!!playlist}
              onOpen={() => playlist && useUI.getState().navigateToPlaylist(playlist.id)}
              menu={() => (playlist ? playlistMenu(playlist) : [])}
            />
          );
        })}
      </div>
    </section>
  );
}

/** Playlist ids are UUID strings; match case-insensitively. */
function findPlaylist(byId: Map<string, Playlist>, id: string) {
  const hit = byId.get(id);
  if (hit) return hit;
  const want = id.toLowerCase();
  for (const [k, p] of byId) if (k.toLowerCase() === want) return p;
  return undefined;
}

function Tile({
  artwork,
  title,
  subtitle,
  enabled,
  onOpen,
  menu: items,
}: {
  artwork: string | null | undefined;
  title: string;
  subtitle: string;
  enabled: boolean;
  onOpen: () => void;
  menu: () => MenuItem[];
}) {
  return (
    <button className="recent-tile" disabled={!enabled} onClick={onOpen} onContextMenu={contextMenu(items)}>
      <ArtworkView artwork={artwork} size={148} />
      <div className="recent-tile__title">{title}</div>
      <div className="recent-tile__subtitle">{subtitle}</div>
    </button>
  );
}

function playAlbum(album: Album) {
  void player.play(albumTracks(album), 0, album.name);
  void api.recordAlbumPlay(album.id);
}

/** Home's album menu: Play Album, queue, View Album, Add to Playlist, artists, Remove. */
function albumMenu(album: Album): MenuItem[] {
  const tracks = albumTracks(album);
  const items: MenuItem[] = [
    menu.button("Play Album", () => playAlbum(album), Play),
    ...playbackItems(tracks),
    menu.divider,
    menu.button("View Album", () => useUI.getState().navigateToAlbum(album.id), LayoutGrid),
    addToPlaylistItem(tracks),
  ];
  if (!album.isCompilation) {
    const artists = artistMenuItems(album.albumArtistLinks);
    if (artists.length) items.push(menu.divider, ...artists);
  }
  items.push(menu.divider, removeFromLibraryItem(album.name, tracks));
  return items;
}

function playlistMenu(playlist: Playlist): MenuItem[] {
  const tracks = playlistTracks(playlist);
  return [
    menu.button(
      "Play",
      () => {
        void player.play(tracks, 0, playlist.name, false);
        void api.recordPlaylistPlay(playlist.id);
      },
      Play,
    ),
    ...playbackItems(tracks),
    menu.divider,
    menu.button("View Playlist", () => useUI.getState().navigateToPlaylist(playlist.id), ListMusic),
  ];
}

// MARK: - Listening stats

function ListeningStats({ metrics: m, range }: { metrics: HomeMetrics; range: StatsRange }) {
  const cards = [
    <StatCard key="h" icon={Clock} value={`${m.hoursListened}h`} label="Hours Listened" accent />,
    <StatCard key="t" icon={Music} value={abbreviated(m.tracksPlayed)} label="Tracks Played" />,
    <StatCard key="a" icon={Layers} value={m.albumsPlayed.toLocaleString()} label="Albums" />,
    <StatCard key="s" icon={Headphones} value={m.sessions.toLocaleString()} label="Sessions" />,
  ];
  if (m.topGenre) {
    cards.push(
      <StatCard
        key="g"
        icon={Star}
        value={m.topGenre.name}
        label="Top Genre"
        detail={`${Math.round(m.topGenre.share * 100)}% of plays`}
      />,
    );
  }
  cards.push(
    <StatCard
      key="k"
      icon={TrendingUp}
      value={`${m.streakDays} day${m.streakDays === 1 ? "" : "s"}`}
      label="Current Streak"
      accent={m.streakDays > 0}
    />,
  );
  const maxMinutes = Math.max(m.topArtists[0]?.minutes ?? 1, 0.0001);
  const label = RANGES.find((r) => r.value === range)?.label ?? "All time";

  return (
    <section className="home-section">
      <div className="home-section__head">
        <div className="home-section-title">YOUR LISTENING STATS</div>
        <button
          className="range-picker"
          onClick={(e) => {
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            openMenuAt(
              RANGES.map((x) => menu.button(x.label, () => setSetting("flactastic.home.statsRange", x.value), x.value === range ? CheckIcon : undefined)),
              r.left,
              r.bottom + 4,
            );
          }}
        >
          {label}
          <ChevronDown size={9} strokeWidth={3} />
        </button>
      </div>
      <div className="stat-grid">{cards}</div>
      <div className="stats-row">
        <WeeklyChart minutes={m.weeklyMinutes} labels={m.weeklyDayLabels} />
        <div className="home-card top-artists">
          <div className="home-card__title">Top Artists</div>
          {m.topArtists.length === 0 ? (
            <div className="home-card__caption">Not enough plays yet</div>
          ) : (
            <div className="top-artists__list">
              {m.topArtists.map((a, i) => (
                <div key={a.name} className="top-artists__row">
                  <span className="top-artists__rank tabular">{i + 1}</span>
                  <div className="top-artists__body">
                    <div className="top-artists__line">
                      <span className="top-artists__name" style={{ color: i === 0 ? "var(--text-primary)" : "var(--text-secondary)" }}>
                        {displayCredit(a.name)}
                      </span>
                      <span className="top-artists__minutes tabular">{minutesLabel(a.minutes)}</span>
                    </div>
                    <ProgressBar
                      fraction={a.minutes / maxMinutes}
                      tint={i === 0 ? "var(--quality-hires)" : "color-mix(in srgb, var(--text-primary) 45%, transparent)"}
                    />
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}


// MARK: - Top albums

function TopAlbums({ metrics }: { metrics: HomeMetrics }) {
  const albumsById = useLibrary((s) => s.albumsById);
  const albums = metrics.topAlbums;
  if (!albums.length) return null;
  const maxMinutes = Math.max(albums[0]?.minutes ?? 1, 0.0001);
  return (
    <section className="home-section">
      <div className="home-section-title">TOP ALBUMS THIS WEEK</div>
      <div className="top-albums">
        {albums.map((rank, i) => {
          const album = albumsById.get(rank.albumId);
          const artist = album?.artist ? displayCredit(album.artist) : displayCredit(rank.artist);
          return (
            <button
              key={rank.albumId}
              className="top-album"
              onClick={() => album && playAlbum(album)}
              onContextMenu={contextMenu(() => (album ? albumMenu(album) : []))}
            >
              <span className="top-album__rank tabular" style={{ color: i === 0 ? "var(--quality-hires)" : "var(--text-tertiary)" }}>
                {i + 1}
              </span>
              <ArtworkView artwork={album?.artwork} size={40} />
              <div className="top-album__text">
                <div className="top-album__title">{rank.album}</div>
                <div className="top-album__artist">{artist}</div>
              </div>
              <div style={{ flex: 1 }} />
              <div style={{ width: 150 }}>
                <ProgressBar
                  fraction={rank.minutes / maxMinutes}
                  tint={i === 0 ? "var(--quality-hires)" : "color-mix(in srgb, var(--text-primary) 35%, transparent)"}
                />
              </div>
              <div className="top-album__nums">
                <span className="top-album__plays tabular">{rank.plays} plays</span>
                <span className="top-album__minutes tabular">{Math.round(rank.minutes)} min</span>
              </div>
            </button>
          );
        })}
      </div>
    </section>
  );
}
