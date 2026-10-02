# version.txt holds the Mac's version strings (Scripts/build-release.sh):
# MAJOR.MINOR.PATCH[-PRERELEASE] or beta-N[.N]. Installers need a plain
# numeric version (MSI: major.minor.patch, each field capped), so:
#   1.3.0-beta.2 -> 1.3.0   (the Mac's CFBundleShortVersionString does the same)
#   beta-6       -> 0.6.0
#   beta-6.1     -> 0.6.1
# The app itself still shows the full version.txt string (About, Sync).
# Mirrored in scripts/linux/build-release.sh.

function ConvertTo-BundleVersion([string]$v) {
    if ($v -match '^(\d+)\.(\d+)\.(\d+)(-[A-Za-z0-9.]+)?$') {
        return "$($Matches[1]).$($Matches[2]).$($Matches[3])"
    }
    if ($v -match '^beta-(\d+)(\.(\d+))?$') {
        $patch = if ($Matches[3]) { $Matches[3] } else { '0' }
        return "0.$($Matches[1]).$patch"
    }
    throw "invalid version '$v' in version.txt (expected MAJOR.MINOR.PATCH[-PRERELEASE] or beta-N[.N])"
}
