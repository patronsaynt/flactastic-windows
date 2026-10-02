# FLACtastic desktop - run the app in development mode.
#
# Reloads PATH from the registry first, so it works from any terminal, even one
# opened before dev-setup.ps1 (or anything else) changed the user PATH.
#
#   powershell -ExecutionPolicy Bypass -File scripts\dev.ps1

$ErrorActionPreference = 'Stop'

$env:Path = (Join-Path $env:LOCALAPPDATA 'flactastic-dev\node') + ';' +
            [Environment]::GetEnvironmentVariable('Path', 'User') + ';' +
            [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
            "$env:USERPROFILE\.cargo\bin"

foreach ($c in 'cargo', 'pnpm') {
    if (-not (Get-Command $c -ErrorAction SilentlyContinue)) {
        throw "$c not found - run scripts\dev-setup.ps1 first"
    }
}

$ui = Join-Path $PSScriptRoot '..\ui'
if (-not (Test-Path (Join-Path $ui 'node_modules\.bin\vite.cmd'))) {
    pnpm --dir $ui install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Push-Location (Join-Path $PSScriptRoot '..\app\src-tauri')
try { cargo tauri dev @args } finally { Pop-Location }
exit $LASTEXITCODE
