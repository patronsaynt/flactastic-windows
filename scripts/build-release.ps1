# FLACtastic desktop - Windows release build (the Mac's Scripts/build-release.sh).
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1 [-SkipTests] [-SkipClean]
#
# Reads version.txt, runs the tests, builds the NSIS installer and the MSI, and
# copies them to dist\ as FLACtastic-<version>-windows-x64-setup.exe / .msi
# together with a RELEASE_NOTES.md stub.

param([switch]$SkipTests, [switch]$SkipClean)

$ErrorActionPreference = 'Stop'
$Root = Resolve-Path (Join-Path $PSScriptRoot '..')

$env:Path = (Join-Path $env:LOCALAPPDATA 'flactastic-dev\node') + ';' +
            [Environment]::GetEnvironmentVariable('Path', 'User') + ';' +
            [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
            "$env:USERPROFILE\.cargo\bin"

# Validate version
$Version = (Get-Content (Join-Path $Root 'version.txt') -Raw).Trim()
. (Join-Path $PSScriptRoot 'bundle-version.ps1')
$BundleVersion = ConvertTo-BundleVersion $Version

Write-Host '==============================================='
Write-Host "  FLACtastic release build - v$Version (bundle $BundleVersion)"
Write-Host '==============================================='

$Dist = Join-Path $Root 'dist'
if (-not $SkipClean -and (Test-Path $Dist)) { Remove-Item -Recurse -Force $Dist }
New-Item -ItemType Directory -Force $Dist | Out-Null

# UI dependencies
$ui = Join-Path $Root 'ui'
if (-not (Test-Path (Join-Path $ui 'node_modules\.bin\vite.cmd'))) {
    pnpm --dir $ui install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { throw 'pnpm install failed' }
}

# Tests (like the Mac: a failure warns but doesn't stop a beta build)
if (-not $SkipTests) {
    Write-Host '> Running tests...'
    Push-Location $Root
    try {
        cargo test --workspace
        $rust = $LASTEXITCODE
        pnpm --dir $ui test
        $js = $LASTEXITCODE
    } finally { Pop-Location }
    if ($rust -ne 0 -or $js -ne 0) {
        Write-Warning 'tests failed. Use -SkipTests to bypass. (continuing anyway for beta release)'
    }
} else { Write-Host '> Skipping tests (-SkipTests)' }

# Build. The version goes in through a config overlay (a file: PowerShell 5.1
# mangles JSON quotes on native command lines).
$overlay = Join-Path $env:TEMP 'flactastic-release.json'
@{ version = $BundleVersion; bundle = @{ targets = @('nsis', 'msi') } } |
    ConvertTo-Json -Depth 4 | Set-Content -Encoding ascii $overlay

Write-Host '> Building installers...'
Push-Location (Join-Path $Root 'app\src-tauri')
try {
    cargo tauri build --config $overlay
    if ($LASTEXITCODE -ne 0) { throw "cargo tauri build failed ($LASTEXITCODE)" }
} finally { Pop-Location; Remove-Item $overlay -ErrorAction SilentlyContinue }

$bundle = Join-Path $Root 'target\release\bundle'
$nsis = Get-ChildItem (Join-Path $bundle 'nsis') -Filter "*_${BundleVersion}_*.exe" | Select-Object -First 1
$msi = Get-ChildItem (Join-Path $bundle 'msi') -Filter "*_${BundleVersion}_*.msi" | Select-Object -First 1
if (-not $nsis -or -not $msi) { throw "installers for $BundleVersion not found under $bundle" }
Copy-Item $nsis.FullName (Join-Path $Dist "FLACtastic-$Version-windows-x64-setup.exe")
Copy-Item $msi.FullName (Join-Path $Dist "FLACtastic-$Version-windows-x64.msi")

# Release notes stub
$notes = Join-Path $Dist 'RELEASE_NOTES.md'
if (-not (Test-Path $notes)) {
    @"
# FLACtastic $Version

**Release date:** $(Get-Date -Format yyyy-MM-dd)
**Platforms:** Windows 10/11 (x64)

## What's new
- _(fill in changes for this beta)_

## Known issues
- _(fill in known issues)_

## Installation
1. Download ``FLACtastic-$Version-windows-x64-setup.exe`` (installs for your
   user, no administrator prompt) or the ``.msi`` (installs for all users).
2. Run it. Windows SmartScreen may say it "protected your PC" - this build is
   not code-signed yet, so Windows has no publisher to show. It is not a
   malware detection. Click **More info**, then **Run anyway**.
3. The first time FLACtastic starts, Windows Firewall asks whether it may
   communicate on networks. Allow **Private networks** so Sync can find and
   reach your other devices.
"@ | ForEach-Object { [IO.File]::WriteAllText($notes, $_ + "`n", (New-Object Text.UTF8Encoding $false)) }
    Write-Host "> Wrote release notes stub: $notes"
}

Write-Host ''
Write-Host '==============================================='
Write-Host "  Release build complete: v$Version"
Write-Host '==============================================='
Get-ChildItem $Dist | Format-Table Name, Length -AutoSize
