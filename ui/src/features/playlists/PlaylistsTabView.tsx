import { AnimatePresence, motion } from "motion/react";
import { LayoutGrid, List, ListMusic, Pencil, Plus } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { playlistTracks, usePlaylists } from "../../app/playlists";
import { setSetting, useSetting } from "../../app/settings";
import { useUI } from "../../app/store";
import { api, artworkUrl, type Playlist } from "../../lib/api";
import { playlistSummary, coarseDuration } from "../../lib/format";
import { standardCompare, containsCI, plural } from "../../lib/text";
import { PageHeader } from "../../components/PageHeader";
import { ActionPill, PillToggle, SortMenu } from "../../components/chrome/Chrome";
import { SearchBar } from "../../components/chrome/SearchBar";
import { contextMenu, menu } from "../../components/menu/ContextMenu";
import { RiseFadeIn } from "../../components/RiseFadeIn";
import { ArtworkView } from "../../components/ArtworkView";
import { Modal } from "../../components/sheet/Sheet";
import { playbackItems } from "../collection/menus";
import { NewPlaylistSheet } from "./NewPlaylistSheet";
import { PlaylistDetailView } from "./PlaylistDetailView";
import { PlaylistEditorView } from "./PlaylistEditorView";
import "../collection/AlbumCard.css";
import "../collection/CollectionView.css";

type SortOption = "A → Z" | "Z → A" | "Newest";

const fade = { initial: { opacity: 0 }, animate: { opacity: 1 }, exit: { opacity: 0 }, transition: { duration: 0.22, ease: "easeInOut" } } as const;

/** `PlaylistsTabView`: the playlists root, or the playlist on top of the path. */
export function PlaylistsTabView() {
  const top = useUI((s) => s.playlistsPath.at(-1));
  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <motion.div key={top ?? "root"} className="collection-page" {...fade}>
        {top == null ? <PlaylistsRoot /> : <PlaylistDetailView playlistId={top} />}
      </motion.div>
    </AnimatePresence>
  );
}

function PlaylistsRoot() {
  const all = usePlaylists((s) => s.playlists);
  const listLayout = useSetting("flactastic.useListLayout");
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortOption>("A → Z");
  const [showNew, setShowNew] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const closeNew = useCallback(() => setShowNew(false), []);
  const closeEditor = useCallback(() => setEditingId(null), []);

  const playlists = useMemo(() => {
    const sorted = [...all];
    if (sort === "A → Z") sorted.sort((a, b) => standardCompare(a.name, b.name));
    else if (sort === "Z → A") sorted.sort((a, b) => standardCompare(b.name, a.name));
    else sorted.sort((a, b) => b.dateCreated - a.dateCreated);
    return search ? sorted.filter((p) => containsCI(p.name, search)) : sorted;
  }, [all, sort, search]);

  const menuFor = (p: Playlist) =>
    contextMenu(() => [
      ...playbackItems(playlistTracks(p)),
      menu.divider,
      menu.button("Edit…", () => setEditingId(p.id), Pencil),
      menu.divider,
      menu.button("Delete", () => void api.deletePlaylist(p.id), undefined, true),
    ]);
  const push = (id: string) => useUI.getState().navigateToPlaylist(id);

  return (
    <div className="collection-root">
      <div className="collection-root__head">
        <div style={{ paddingTop: 30 }}>
          <PageHeader eyebrow="Library" title="Playlists" />
        </div>
        <div className="collection-controls">
          <ActionPill primary height={34} icon={Plus} onClick={() => setShowNew(true)}>
            New Playlist
          </ActionPill>
          <PillToggle<boolean>
            selection={listLayout}
            onChange={(v) => setSetting("flactastic.useListLayout", v)}
            segments={[
              { value: false, icon: LayoutGrid, help: "Grid" },
              { value: true, icon: List, help: "List" },
            ]}
          />
          <div style={{ flex: 1 }} />
          <SortMenu<SortOption> selection={sort} options={["A → Z", "Z → A", "Newest"]} label={(o) => o} onChange={setSort} />
          <SearchBar value={search} onChange={setSearch} />
        </div>
      </div>
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.div key={String(listLayout)} className="collection-body" {...fade}>
          <div className="collection-scroll">
            <div className="collection-scroll__inner">
              {listLayout ? (
                <div className="album-list">
                  {playlists.map((p, i) => (
                    <RiseFadeIn key={p.id} index={i} onClick={() => push(p.id)} onContextMenu={menuFor(p)}>
                      <PlaylistRow playlist={p} />
                    </RiseFadeIn>
                  ))}
                </div>
              ) : (
                <div className="album-grid">
                  {playlists.map((p, i) => (
                    <RiseFadeIn key={p.id} index={i} onClick={() => push(p.id)} onContextMenu={menuFor(p)}>
                      <PlaylistCard playlist={p} />
                    </RiseFadeIn>
                  ))}
                </div>
              )}
            </div>
          </div>
        </motion.div>
      </AnimatePresence>
      <Modal open={showNew} onClose={closeNew}>
        <NewPlaylistSheet onClose={closeNew} />
      </Modal>
      <Modal open={editingId != null} onClose={closeEditor}>
        {editingId && <PlaylistEditorView playlistId={editingId} onClose={closeEditor} />}
      </Modal>
    </div>
  );
}

/** `PlaylistCardView` */
function PlaylistCard({ playlist }: { playlist: Playlist }) {
  const rounded = useSetting("flactastic.roundedArtwork");
  const shadow = useSetting("flactastic.showArtworkShadow");
  const zoom = useSetting("flactastic.uiScale");
  const [failed, setFailed] = useState<string | null>(null);
  const radius = rounded ? 8 : 0;
  const art = playlist.artwork && failed !== playlist.artwork ? playlist.artwork : null;
  return (
    <div className="album-card">
      <div className={"album-card__cover" + (shadow ? " has-shadow" : "")} style={{ borderRadius: radius }}>
        {art ? (
          <img
            src={artworkUrl(art, 180, zoom)}
            alt=""
            draggable={false}
            loading="lazy"
            decoding="async"
            style={{ borderRadius: radius }}
            onError={() => setFailed(art)}
          />
        ) : (
          <div className="album-card__placeholder" style={{ borderRadius: radius }}>
            <ListMusic size={40} strokeWidth={0.75} />
          </div>
        )}
      </div>
      <div className="album-card__title">{playlist.name}</div>
      <div className="album-card__artist">{playlistSummary(playlist.trackIds.length, playlist.totalDuration)}</div>
    </div>
  );
}

/** `PlaylistRowView` */
function PlaylistRow({ playlist }: { playlist: Playlist }) {
  return (
    <div className="album-row fl-row">
      <ArtworkView artwork={playlist.artwork} size={46} />
      <div className="album-row__text">
        <div className="album-row__title">{playlist.name}</div>
        <div className="album-row__artist">{plural(playlist.trackIds.length, "track")}</div>
      </div>
      <span className="album-row__meta tabular" style={{ width: 54, textAlign: "right" }}>
        {coarseDuration(playlist.totalDuration)}
      </span>
    </div>
  );
}
