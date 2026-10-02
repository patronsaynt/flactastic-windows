# Windows code signing (not set up yet)

Release builds are unsigned, like the Mac's ad-hoc DMG. Windows SmartScreen
shows "Windows protected your PC" on first run; users click **More info →
Run anyway** (the release-notes stub says so).

## Turning it on

Tauri signs the app exe, the NSIS installer and the MSI when one of these is
set in `app/src-tauri/tauri.conf.json` under `bundle.windows`:

- **Certificate in the Windows store** (OV/EV from DigiCert, Sectigo, …):

  ```json
  "certificateThumbprint": "<SHA-1 thumbprint>",
  "digestAlgorithm": "sha256",
  "timestampUrl": "http://timestamp.digicert.com"
  ```

- **Cloud signing** (Azure Trusted Signing, SignPath, a hardware token through
  a CLI): a command run once per file, `%1` being the file:

  ```json
  "signCommand": "trusted-signing-cli -e https://<region>.codesigning.azure.net -a <account> -c <profile> %1"
  ```

For CI, keep the secret material in the hosting provider's secret store and
inject it in `release.yml` before `scripts/build-release.ps1` runs. Never
commit certificates or keys.

## Notes

- EV certificates get SmartScreen reputation immediately; OV certificates
  build it over downloads.
- Azure Trusted Signing is the cheapest route (~$10/month) for individuals
  in supported regions.
- Linux packages are unsigned; an AppImage can carry a GPG signature via
  `SIGN=1` in the AppImage tooling if wanted later.
