# Feature parity: macOS → Windows / Linux

Every macOS feature and view (from `references/macos-main/Sources/flactastic`)
with its desktop status. Release needs every row green.

**Legend:** ✅ ported and checked in the running app · 🧪 ported, covered by
automated tests only · ⏳ ported, needs a manual check (account, device or
hardware) · ❌ missing

Linux column: the same code; ✅ there means checked under WSLg (Ubuntu 24.04).
Real-hardware audio (PipeWire/ALSA) still needs a Linux box.

## Shell and design system

| Feature (Mac source) | Windows | Linux | Notes |
|---|---|---|---|
| Top bar, tab bar with sliding pill (`TabBarView`, `TopBarItems`) | ✅ | ⏳ | Frameless window, custom window controls |
| Search (`SearchBarView`) | ✅ | ⏳ | |
| Floating player bar, seek, volume (`Transport/*`) | ✅ | ⏳ | |
| Queue panel with drag-reorder (`QueuePanelView`) | ✅ | ⏳ | |
| Context menus with submenus (`FLContextMenu`) | ✅ | ⏳ | |
| Sheets (`FLSheet`), confirmation dialogs | ✅ | ⏳ | |
| Artwork view, shadow and corner toggles, zoom overlay | ✅ | ⏳ | Rust thumbnail cache over `flart://` |
| Loading cover (`LoadingCoverView`) | ✅ | ⏳ | |
| Light/dark theme, UI scale 0.9–1.35 | ✅ | ⏳ | |
| Keyboard shortcuts (Space, ⌘R/←/→/↑/↓ on Ctrl, Esc) | ✅ | ⏳ | |
| Full-screen observer (`WindowFullScreenObserver`) | ✅ | ⏳ | |

## Library and screens

| Feature | Windows | Linux | Notes |
|---|---|---|---|
| Home: hero, recently played, stats, chart, top albums | ✅ | ⏳ | |
| Fidelidex / library fidelity | ✅ | ⏳ | |
| Collection: albums, artists, tracks; grid/list; grouping; sort | ✅ | ⏳ | Resolvers ported with the Mac's golden tests |
| Album detail, artist detail, artist editor | ✅ | ⏳ | |
| Remove from library confirmation | ✅ | ⏳ | |
| Playlists tab, detail, editor, cover cropper | ✅ | ⏳ | `playlists.json` read/written compatibly |
| Track, album, lyrics, lyrics-sync, markers editors | ✅ | ⏳ | TagLib 2 (vendored) |
| Merge tracks into album; artist/genre chip fields | ✅ | ⏳ | |
| Import: track, album, playlist; OS drag-and-drop | ✅ | ⏳ | Windows reserved names sanitised |
| Organizer: template, planner, preview tree, executor, profiles | ✅ | ⏳ | Conflicts included in the path map |
| Visualizer: 8 modes, mode wheel, lyrics scroller, backdrop cache | ✅ | ⏳ | |
| Onboarding: all pages | ✅ | ⏳ | |
| Settings: all panes, About | ✅ | ⏳ | Audio pane adds Windows Exclusive mode |
| Debug: Lucida inspector, Debug Onboarding preview | ✅ | ⏳ | Preview is a 640 × 720 sheet (the Mac opens a window) |

## Audio

| Feature | Windows | Linux | Notes |
|---|---|---|---|
| Gapless playback across files (FLAC, MP3/LAME, AAC) | 🧪 | 🧪 | Null-sink render tests; listen on your hardware |
| Bit-perfect passthrough when rates match at volume 1.0 | 🧪 | 🧪 | |
| High-quality resampling (soxr VHQ) for mismatched rates | 🧪 | 🧪 | |
| Output device choice, pinned with fallback, rate/bit-depth lists | ✅ | ⏳ | |
| Changing the system output format (IPolicyConfig) | ⏳ | n/a | Needs your go-ahead to test |
| WASAPI exclusive mode | ⏳ | n/a | Needs your go-ahead to test |
| PipeWire / ALSA `hw:` output | n/a | ⏳ | Real Linux hardware |
| Shuffle, repeat one/all, user queue, gapless queue edits | 🧪 ✅ | 🧪 | |
| Spectrum tap and smoothing | ✅ | ⏳ | |
| Listening tracker (90% rule, repeats) | 🧪 | 🧪 | |
| Media keys / OS media controls | ✅ SMTC | ⏳ MPRIS | |

## Connections and downloads

| Feature | Windows | Linux | Notes |
|---|---|---|---|
| Download tab: chooser, albums/tracks, playlists, drawer, options | ✅ | ⏳ | |
| Lucida bridge and challenge sheet | ⏳ | ⏳ | Bridge loads; Lucida's workers were returning errors (service side) |
| Spotify playlist resolve (public links) | ✅ | ⏳ | |
| Spotify login (PKCE, `flactastic://` callback, keyring) | ⏳ | ⏳ | Needs your account |
| Playlist rebuild (Odesli/Amazon matching) | ⏳ | ⏳ | Depends on Lucida |
| VPN notice, artwork accent | ✅ | ⏳ | |
| Discord Rich Presence | 🧪 | 🧪 | Mock IPC tested; needs Discord running |
| Lyrics fetch and write-back, Deezer artist images | ⏳ | ⏳ | Ported; not rechecked against the live services |
| Menu-bar player → tray mini-player | ✅ | ⏳ | Linux tray needs an AppIndicator host |

## Sync

| Feature | Windows | Linux | Notes |
|---|---|---|---|
| Wire protocol, framing, TLS-PSK channel, pairing crypto | 🧪 | 🧪 | Byte-exact rows in `SYNC-INTEROP.md` |
| Manifest, tag fingerprint, plan hash, diff, selection | 🧪 | 🧪 | Fingerprint matches the Mac golden value |
| File transfer: staging, resume, verify, MAC-ISSUES #6 recovery | 🧪 | 🧪 | Loopback runs |
| mDNS discovery and advertising | ✅ | ⏳ | |
| Sync window, pairing sheet, plan sheet, Devices pane | ✅ | ⏳ | |
| First-run firewall prompt | ✅ | n/a | |
| Interop with a real Mac / iPhone | ⏳ | ⏳ | Pair, push, pull, conflict, selection, resume, revoke, lockout |

## Packaging

| Item | Windows | Linux | Notes |
|---|---|---|---|
| Installers | NSIS (per user), MSI | AppImage, .deb, .rpm | `scripts/build-release.ps1`, `scripts/linux/build-release.sh` |
| Version from `version.txt` | ✅ | ✅ | `beta-N` → bundle version `0.N.0`; the app shows the full string |
| `flactastic://` registered at install | 🧪 | 🧪 | In the generated NSIS/WiX scripts and the .desktop entry (`%u`); not yet installed and clicked |
| `.flac` etc. file associations | 🧪 | 🧪 | As on the Mac: declared, the app opens without acting on the file |
| CI (`.github/workflows`) | idle | idle | Until hosting is chosen |
| Code signing | ❌ | n/a | See `scripts/windows/SIGNING.md` |
| Visual parity screenshots against the Mac (1440 × 900, both themes) | ⏳ | ⏳ | Needs Mac screenshots |
