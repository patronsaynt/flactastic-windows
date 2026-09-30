import { SquareUser, Users } from "lucide-react";
import { Fragment, type CSSProperties } from "react";
import { useUI } from "../../app/store";
import { useLibrary } from "../../app/library";
import type { ArtistLinkPiece, Track } from "../../lib/api";
import { menu, type MenuItem } from "../../components/menu/ContextMenu";

/**
 * `ArtistLink`: a credit rendered as one link per resolved artist, joined
 * with ", " — or "Unknown Artist" when there's no credit.
 */
export function ArtistLink({ links, style }: { links: ArtistLinkPiece[]; style?: CSSProperties }) {
  const go = useUI((s) => s.navigateToArtist);
  if (!links.length) return <span style={style}>Unknown Artist</span>;
  return (
    <span style={style}>
      {links.map((l, i) => (
        <Fragment key={i}>
          {i > 0 && ", "}
          <button
            className="artist-link"
            title={links.length > 1 ? `View ${l.name}` : "View artist"}
            onClick={(e) => {
              e.stopPropagation();
              go(l.key);
            }}
          >
            {l.name}
          </button>
        </Fragment>
      ))}
    </span>
  );
}

/**
 * `artistContextMenuItems`: "View artist" for one, a "View artists…"
 * submenu for several, nothing for none.
 */
export function artistMenuItems(links: ArtistLinkPiece[] | undefined): MenuItem[] {
  if (!links?.length) return [];
  const go = (key: string) => useUI.getState().navigateToArtist(key);
  if (links.length === 1) return [menu.button("View artist", () => go(links[0].key), SquareUser)];
  return [
    menu.submenu(
      "View artists…",
      links.map((l) => menu.button(l.name, () => go(l.key))),
      Users,
    ),
  ];
}

/** Appends artist items after a divider, as every Mac call site does. */
export function withArtistItems(items: MenuItem[], links: ArtistLinkPiece[] | undefined): MenuItem[] {
  const extra = artistMenuItems(links);
  return extra.length ? [...items, menu.divider, ...extra] : items;
}

/** Links for any track: queue copies carry none, so use the library's. */
export function linksFor(track: Track): ArtistLinkPiece[] | undefined {
  return track.artistLinks ?? useLibrary.getState().tracksById.get(track.id)?.artistLinks;
}
