# FLACtastic for Windows and Linux

Desktop distribution of FLACtastic, a modern library player for audiophiles.
This repository holds the Windows and Linux builds. It is currently in
**Beta 1.0**, open for testing.

## Downloads

Installers are attached to the [v1.0 release](../../releases/tag/v1.0).

| Platform | File | Notes |
|---|---|---|
| Windows 10/11 (x64) | `FLACtastic_Windows_Beta1.0-x64-setup.exe` | Per-user install, no admin prompt |
| Windows 10/11 (x64) | `FLACtastic_Windows_Beta1.0-x64.msi` | All-users install |
| Linux (x86_64) | `FLACtastic_Linux_Beta1.0-x86_64.AppImage` | Runs on most distributions |
| Linux (Debian/Ubuntu) | `FLACtastic_Linux_Beta1.0-amd64.deb` | |
| Linux (Fedora/RHEL/openSUSE) | `FLACtastic_Linux_Beta1.0-x86_64.rpm` | |

## Installing

### Windows

1. Run the `.exe` (or the `.msi`).
2. These builds are not code-signed yet, so SmartScreen may show "Windows
   protected your PC". Click **More info**, then **Run anyway**.
3. On first launch, Windows Firewall asks about network access. Allow
   **Private networks** so Sync can find your other devices.

### Linux

AppImage:

```bash
chmod +x FLACtastic_Linux_Beta1.0-x86_64.AppImage
./FLACtastic_Linux_Beta1.0-x86_64.AppImage
```

Debian/Ubuntu:

```bash
sudo apt install ./FLACtastic_Linux_Beta1.0-amd64.deb
```

Fedora/RHEL:

```bash
sudo dnf install ./FLACtastic_Linux_Beta1.0-x86_64.rpm
```

Linux packages are unsigned. The Linux builds require WebKitGTK 4.1
(`libwebkit2gtk-4.1`), which most current distributions ship.

## What to test

- Library: import, browse (albums, artists, tracks), tag and lyrics editing,
  playlists, the Organizer.
- Playback: gapless playback, bit-perfect output when sample rates match,
  output device selection, queue and shuffle/repeat.
- Visualizer and lyrics view.
- Sync between devices on the same network.

## Known limitations

- Not code-signed on any platform (see above).
- Windows: WASAPI exclusive mode and changing the system output format have
  not been verified on real hardware.
- Linux: built and checked under WSLg only. PipeWire and ALSA output have not
  been tested on real hardware, so audio feedback from Linux testers is
  especially useful.
- Linux UI screens are covered by the same code as Windows but have had less
  manual checking. See [docs/PARITY.md](docs/PARITY.md) for the full
  feature-by-feature status.

## Reporting issues

Open an issue at <https://github.com/patronsaynt/flactastic-windows/issues>
and include:

- Operating system and version (and distribution, for Linux)
- Installer used and its version (Beta 1.0)
- Steps to reproduce, and what you expected to happen
- Your audio output device, for playback issues

## Building from source

Requires Rust, Node LTS, pnpm 9 and `cargo install tauri-cli --version "^2"`.
The version is read from `version.txt`.

Windows:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1
```

Linux (see the script header for required system packages):

```bash
scripts/linux/build-release.sh
```

Installers are written to `dist/`. Code signing setup is described in
[scripts/windows/SIGNING.md](scripts/windows/SIGNING.md).

## License

See [LICENSE](LICENSE).
