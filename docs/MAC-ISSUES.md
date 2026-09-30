# Issues found in the macOS code while porting

Behaviour that affects interop or shared on-disk data is **mirrored** in the
desktop port. Local-only issues are **fixed** in the port. Each is listed here
so it can be raised on the Mac repo.

| # | Area | Mac location | Issue | Desktop port |
|---|------|--------------|-------|--------------|
| 1 | Artists | `ArtistResolver.init` + `split` | Pass 2 records every fragment of a multi-artist string ("Earth", "Wind", "Fire") as a known name, and `split` then treats all fragments as known. So "Earth, Wind & Fire" splits even when it only ever appears as itself, and `normaliseArtistTags` rewrites it in memory to "Earth ; Wind ; Fire". | Mirrored (display/navigation must match the Mac). |
| 2 | Organizer | `OrganizerModel.apply` | Rebuilding moved tracks drops `secondaryGenres` and `isMixCompilation` from the in-memory `Track` until the next rescan. | Fixed: all fields kept. |
| 3 | Import | `Track.relocated(to:)` | Resets `secondaryGenres`, `isCompilation` and `isMixCompilation` on imported copies until the next rescan. | Fixed: all fields kept. |
| 4 | Library | `LibraryStore.applyStableIDs` | Rebuilds `Track` without `secondaryGenres`/`isMixCompilation`. Harmless today (only called on metadata-free stubs). | Not applicable (copies the whole struct). |
| 5 | Sync | responder | Responder ignores its own per-peer filter. | Fixed. |
| 6 | Sync | `FileTransfer` | `hashMismatch` is sent after `fileEnd`, which puts the two sides out of step. | Fixed (see SYNC-INTEROP.md for the wire impact). |
| 7 | Sync | receive path | A content-hash match never adopts the sender's track ID. | Fixed. |
| 8 | Playback | `PlayerState.updateListeningTracker` | The repeat check ("last position within 2 s of the end, now under 2 s") has no backward-jump test, so on tracks shorter than 2 s it re-records a play on every 50 ms tick. | Fixed: also requires the position to have moved backwards. |
| 9 | Metadata | `MergeTracksIntoAlbumView.save` | Calls `MetadataWriter.write` without `secondaryGenres`, which defaults to `[]`, so merging rewrites each file's GENRE with the primary genre only and drops its secondary genres. | Fixed: each track keeps its own secondary genres. |
