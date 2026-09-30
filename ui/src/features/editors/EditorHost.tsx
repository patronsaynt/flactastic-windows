import { editors, useEditors } from "../../app/editors";
import { Modal } from "../../components/sheet/Sheet";
import { ImportAlbumView, ImportPlaylistView, ImportTrackView } from "../import/ImportViews";
import { AlbumMetadataEditor } from "./AlbumMetadataEditor";
import { MergeTracksSheet } from "./MergeTracksSheet";
import { TrackMetadataEditor } from "./TrackMetadataEditor";

/** Presents whichever editor or import sheet `editors.*` asked for. */
export function EditorHost() {
  const sheet = useEditors((s) => s.sheet);
  return (
    <Modal open={sheet != null} onClose={editors.close}>
      {sheet?.kind === "track" && <TrackMetadataEditor track={sheet.track} onClose={editors.close} />}
      {sheet?.kind === "album" && <AlbumMetadataEditor album={sheet.album} onClose={editors.close} />}
      {sheet?.kind === "merge" && <MergeTracksSheet tracks={sheet.tracks} onDone={sheet.onDone} onClose={editors.close} />}
      {sheet?.kind === "import" && sheet.mode === "track" && <ImportTrackView onClose={editors.close} />}
      {sheet?.kind === "import" && sheet.mode === "album" && <ImportAlbumView onClose={editors.close} />}
      {sheet?.kind === "import" && sheet.mode === "playlist" && <ImportPlaylistView onClose={editors.close} />}
    </Modal>
  );
}
