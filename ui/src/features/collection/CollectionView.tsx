import { AnimatePresence, motion } from "motion/react";
import { ArrowDown, ArrowUp, LayoutGrid, List, RotateCw } from "lucide-react";
import { useMemo, useState } from "react";
import { useLibrary } from "../../app/library";
import { setSetting, useSetting } from "../../app/settings";
import { route, useUI } from "../../app/store";
import { api, type Album } from "../../lib/api";
import { standardCompare, containsCI, plural } from "../../lib/text";
import { PageHeader } from "../../components/PageHeader";
import { CircleIconButton, Eyebrow, PillToggle, SortMenu } from "../../components/chrome/Chrome";
import { SearchBar } from "../../components/chrome/SearchBar";
import { contextMenu } from "../../components/menu/ContextMenu";
import { RiseFadeIn } from "../../components/RiseFadeIn";
import { AlbumCard, AlbumRow } from "./AlbumCard";
import { AlbumDetailView } from "./AlbumDetailView";
import { AllTracksView } from "./AllTracksView";
import { playbackItems } from "./menus";
import { ArtistDetailView } from "../artist/ArtistDetailView";
import { ArtistsCollectionView } from "../artist/ArtistsCollectionView";
import { withArtistItems } from "../artist/ArtistLink";
import "./CollectionView.css";

type SortOption = "Album" | "Artist" | "Year" | "Genre";
type TracksSort = "Date Added" | "Song Name" | "Artist";
type Mode = "albums" | "artists" | "tracks";

const fade = { initial: { opacity: 0 }, animate: { opacity: 1 }, exit: { opacity: 0 }, transition: { duration: 0.22, ease: "easeInOut" } } as const;

export function CollectionView() {
  const path = useUI((s) => s.collectionPath);
  const top = path.at(-1);
  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <motion.div key={top ?? "root"} className="collection-page" {...fade}>
        {top == null ? (
          <CollectionRoot />
        ) : route.artistKey(top) != null ? (
          <ArtistDetailView artistKey={route.artistKey(top)!} />
        ) : (
          <AlbumDetailView albumId={top} />
        )}
      </motion.div>
    </AnimatePresence>
  );
}

function CollectionRoot() {
  const albums = useLibrary((s) => s.albums);
  const tracksById = useLibrary((s) => s.tracksById);
  const scanState = useLibrary((s) => s.scanState);
  const sortOption = useSetting("flactastic.collectionSort") as SortOption;
  const tracksSort = useSetting("flactastic.allTracksSort") as TracksSort;
  const ascending = useSetting("flactastic.allTracksAscending");
  const listLayout = useSetting("flactastic.useListLayout");
  const groupByArtist = useSetting("flactastic.groupByArtist");
  const [search, setSearch] = useState("");
  const [mode, setMode] = useState<Mode>("albums");
  const refreshing = scanState.state === "refreshing" || scanState.state === "scanning";

  const { filtered, groups } = useMemo(() => {
    const sorted = [...albums];
    switch (sortOption) {
      case "Artist":
        sorted.sort((a, b) => standardCompare(a.artist ?? "", b.artist ?? ""));
        break;
      case "Year":
        sorted.sort((a, b) => (b.year ?? 0) - (a.year ?? 0));
        break;
      case "Genre":
        sorted.sort((a, b) => standardCompare(a.genre ?? "Unknown", b.genre ?? "Unknown"));
        break;
      default:
        sorted.sort((a, b) => standardCompare(a.name, b.name));
    }
    const q = search;
    const filtered = q
      ? sorted.filter(
          (a) =>
            containsCI(a.name, q) ||
            containsCI(a.artist, q) ||
            a.trackIds.some((id) => containsCI(tracksById.get(id)?.title, q)),
        )
      : sorted;
    let groups: { key: string; albums: Album[] }[] = [];
    if (sortOption === "Artist" || sortOption === "Genre") {
      const m = new Map<string, Album[]>();
      for (const a of filtered) {
        const k = sortOption === "Artist" ? (a.artist ?? "Unknown Artist") : (a.genre ?? "Unknown");
        m.set(k, [...(m.get(k) ?? []), a]);
      }
      groups = [...m.entries()].map(([key, albums]) => ({ key, albums })).sort((x, y) => standardCompare(x.key, y.key));
    }
    return { filtered, groups };
  }, [albums, tracksById, sortOption, search]);

  const shouldGroup = sortOption === "Genre" || (sortOption === "Artist" && groupByArtist);
  const title = mode === "albums" ? "Albums" : mode === "artists" ? "Artists" : "All Tracks";

  return (
    <div className="collection-root">
      <div className="collection-root__head">
        <div style={{ paddingTop: 30 }}>
          <PageHeader
            eyebrow="Library"
            title={title}
            accessory={
              <CircleIconButton onClick={() => void api.refreshLibrary()} title="Refresh Library" disabled={refreshing}>
                <RotateCw size={14} strokeWidth={2} className={refreshing ? "spin" : undefined} />
              </CircleIconButton>
            }
          />
        </div>
        <div className="collection-controls">
          <PillToggle<Mode>
            selection={mode}
            onChange={setMode}
            segments={[
              { value: "albums", title: "Albums" },
              { value: "artists", title: "Artists" },
              { value: "tracks", title: "Tracks" },
            ]}
          />
          {mode === "albums" && (
            <PillToggle<boolean>
              selection={listLayout}
              onChange={(v) => setSetting("flactastic.useListLayout", v)}
              segments={[
                { value: false, icon: LayoutGrid, help: "Grid" },
                { value: true, icon: List, help: "List" },
              ]}
            />
          )}
          <div style={{ flex: 1 }} />
          {mode === "albums" && (
            <SortMenu<SortOption>
              selection={sortOption}
              options={["Album", "Artist", "Year", "Genre"]}
              label={(o) => o}
              onChange={(o) => setSetting("flactastic.collectionSort", o)}
            />
          )}
          {mode === "tracks" && (
            <>
              <SortMenu<TracksSort>
                selection={tracksSort}
                options={["Date Added", "Song Name", "Artist"]}
                label={(o) => o}
                onChange={(o) => {
                  setSetting("flactastic.allTracksSort", o);
                  // Reset direction to the sensible default for the sort.
                  setSetting("flactastic.allTracksAscending", o !== "Date Added");
                }}
              />
              <CircleIconButton
                icon={ascending ? ArrowUp : ArrowDown}
                title={ascending ? "Ascending" : "Descending"}
                onClick={() => setSetting("flactastic.allTracksAscending", !ascending)}
              />
            </>
          )}
          <SearchBar value={search} onChange={setSearch} />
        </div>
      </div>

      <AnimatePresence mode="popLayout" initial={false}>
        <motion.div key={mode + String(listLayout)} className="collection-body" {...fade}>
          {mode === "albums" && (
            <div className="collection-scroll">
              <div className="collection-scroll__inner">
                {shouldGroup ? (
                  <div className="album-groups">
                    {groups.map((g) => (
                      <section key={g.key} className="album-group">
                        <div className="album-group__head">
                          <Eyebrow>{g.key}</Eyebrow>
                          <span className="album-group__rule" />
                          <span className="album-group__count">{plural(g.albums.length, "album")}</span>
                        </div>
                        {listLayout ? <AlbumList albums={g.albums} /> : <AlbumGrid albums={g.albums} />}
                      </section>
                    ))}
                  </div>
                ) : listLayout ? (
                  <AlbumList albums={filtered} />
                ) : (
                  <AlbumGrid albums={filtered} />
                )}
              </div>
            </div>
          )}
          {mode === "artists" && <ArtistsCollectionView search={search} />}
          {mode === "tracks" && <AllTracksView search={search} sort={tracksSort} ascending={ascending} />}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}

function albumMenu(a: Album) {
  return contextMenu(() => {
    const byId = useLibrary.getState().tracksById;
    const tracks = a.trackIds.map((id) => byId.get(id)!).filter(Boolean);
    const items = playbackItems(tracks);
    return a.isCompilation ? items : withArtistItems(items, a.albumArtistLinks);
  });
}

function AlbumGrid({ albums }: { albums: Album[] }) {
  const push = useUI((s) => s.pushCollection);
  return (
    <div className="album-grid">
      {albums.map((a, i) => (
        <RiseFadeIn key={a.id} index={i} id={"album:" + a.id} onClick={() => push(a.id)} onContextMenu={albumMenu(a)}>
          <AlbumCard album={a} />
        </RiseFadeIn>
      ))}
    </div>
  );
}

function AlbumList({ albums }: { albums: Album[] }) {
  const push = useUI((s) => s.pushCollection);
  return (
    <div className="album-list">
      {albums.map((a, i) => (
        <RiseFadeIn key={a.id} index={i} id={"album:" + a.id} onClick={() => push(a.id)} onContextMenu={albumMenu(a)}>
          <AlbumRow album={a} />
        </RiseFadeIn>
      ))}
    </div>
  );
}
