# FLACtastic desktop - Windows developer setup.
# Idempotent: re-running skips anything already installed.
#
# Only VS Build Tools needs elevation (one UAC prompt, via winget). Everything
# else installs per-user under %LOCALAPPDATA%\flactastic-dev and is added to the
# user PATH, so no further prompts appear.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Tools = Join-Path $env:LOCALAPPDATA 'flactastic-dev'
New-Item -ItemType Directory -Force $Tools | Out-Null

function Have($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }

function Refresh-Path {
    $env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
                [Environment]::GetEnvironmentVariable('Path', 'User') + ';' +
                "$env:USERPROFILE\.cargo\bin"
}

function Add-UserPath($dir) {
    $cur = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $cur) { $cur = '' }
    if (($cur -split ';') -notcontains $dir) {
        [Environment]::SetEnvironmentVariable('Path', ($cur.TrimEnd(';') + ';' + $dir).TrimStart(';'), 'User')
        Write-Host "   added to user PATH: $dir"
    }
    Refresh-Path
}

function Fetch($url, $dest) {
    Write-Host "   downloading $url"
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $dest
}

function Expand-Portable($url, $name) {
    $dir = Join-Path $Tools $name
    if (Test-Path $dir) { return $dir }
    $zip = Join-Path $env:TEMP "$name.zip"
    Fetch $url $zip
    $staging = "$dir.staging"
    if (Test-Path $staging) { Remove-Item -Recurse -Force $staging }
    Expand-Archive -Path $zip -DestinationPath $staging
    # Flatten a single top-level folder if the archive has one.
    $items = @(Get-ChildItem $staging)
    if ($items.Count -eq 1 -and $items[0].PSIsContainer) { Move-Item $items[0].FullName $dir; Remove-Item $staging }
    else { Move-Item $staging $dir }
    Remove-Item $zip
    return $dir
}

# 1. MSVC toolchain + Windows 11 SDK (rustc, TagLib, vendored OpenSSL). Needs UAC once.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vsPath = $null
if (Test-Path $vswhere) {
    $vsPath = & $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
}
if (-not $vsPath) {
    Write-Host '-> installing VS 2022 Build Tools (C++ workload, Windows 11 SDK) - accept the UAC prompt'
    winget install --id Microsoft.VisualStudio.2022.BuildTools -e --accept-package-agreements --accept-source-agreements --disable-interactivity `
        --override '--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --add Microsoft.VisualStudio.Component.Windows11SDK.22621'
    if ($LASTEXITCODE -ne 0) { throw "VS Build Tools install failed ($LASTEXITCODE)" }
    $vsPath = & $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
} else { Write-Host '[ok] MSVC build tools present' }

# 2. CMake - the copy bundled with VS Build Tools.
if (-not (Have cmake)) {
    $vsCmake = Join-Path $vsPath 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin'
    if (Test-Path (Join-Path $vsCmake 'cmake.exe')) { Add-UserPath $vsCmake }
    else { throw 'cmake not found; re-run the VS installer with the "C++ CMake tools" component' }
}
Write-Host '[ok] cmake'

# 3. Perl (openssl-src's Configure) - Strawberry Perl portable.
if (-not (Have perl) -or ((Get-Command perl).Source -like '*\Git\*')) {
    $rel = Invoke-RestMethod -UseBasicParsing 'https://api.github.com/repos/StrawberryPerl/Perl-Dist-Strawberry/releases/latest'
    $asset = $rel.assets | Where-Object { $_.name -like '*64bit*portable.zip' } | Select-Object -First 1
    if (-not $asset) { throw 'no Strawberry Perl portable zip in the latest release' }
    $dir = Expand-Portable $asset.browser_download_url 'strawberry-perl'
    Add-UserPath (Join-Path $dir 'perl\bin')
    Add-UserPath (Join-Path $dir 'c\bin')
}
Write-Host '[ok] perl'

# 4. NASM (OpenSSL assembly).
if (-not (Have nasm)) {
    $dir = Expand-Portable 'https://www.nasm.us/pub/nasm/releasebuilds/2.16.03/win64/nasm-2.16.03-win64.zip' 'nasm'
    Add-UserPath $dir
}
Write-Host '[ok] nasm'

# 5. Rust (user scope, MSVC host).
if (-not (Have rustup)) {
    $init = Join-Path $env:TEMP 'rustup-init.exe'
    Fetch 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' $init
    & $init -y --default-toolchain stable --profile minimal -c clippy,rustfmt
    if ($LASTEXITCODE -ne 0) { throw "rustup-init failed ($LASTEXITCODE)" }
    Remove-Item $init
    Refresh-Path
}
rustup toolchain install stable --profile minimal -c clippy,rustfmt | Out-Null
Write-Host '[ok] rust'

# 6. Node LTS (portable zip) + pnpm.
if (-not (Have node)) {
    $index = Invoke-RestMethod -UseBasicParsing 'https://nodejs.org/dist/index.json'
    $lts = $index | Where-Object { $_.lts } | Select-Object -First 1
    $dir = Expand-Portable "https://nodejs.org/dist/$($lts.version)/node-$($lts.version)-win-x64.zip" 'node'
    Add-UserPath $dir
}
if (-not (Have pnpm)) {
    npm install -g pnpm@9 --prefix (Join-Path $Tools 'node')
    Refresh-Path
}
Write-Host '[ok] node + pnpm'

# 7. Tauri CLI.
if (-not (Have cargo-tauri)) { cargo install tauri-cli --version '^2' --locked }
Write-Host '[ok] tauri-cli'

# 8. ffmpeg (test fixtures only: MP3/AAC gapless tests encode with it).
if (-not (Have ffmpeg)) {
    winget install --id Gyan.FFmpeg.Essentials -e --scope user --accept-package-agreements --accept-source-agreements --disable-interactivity
}
Write-Host '[ok] ffmpeg'

Refresh-Path
Write-Host ''
Write-Host 'Toolchain (open a new shell to pick up PATH changes):'
foreach ($c in 'rustc', 'cargo', 'node', 'pnpm', 'cmake', 'perl', 'nasm') {
    if (Have $c) { Write-Host ("  {0,-6} {1}" -f $c, ((& $c --version 2>&1) | Select-Object -First 1)) }
    else { Write-Host "  $c MISSING" }
}
