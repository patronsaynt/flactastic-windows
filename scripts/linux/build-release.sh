#!/usr/bin/env bash
# FLACtastic desktop - Linux release build (the Mac's Scripts/build-release.sh).
#
# Usage: scripts/linux/build-release.sh [--skip-tests] [--skip-clean]
#
# Reads version.txt, runs the tests, builds the AppImage, .deb and .rpm, and
# copies them to dist/ as FLACtastic-<version>-linux-<arch>.<ext> together
# with a RELEASE_NOTES.md stub.
#
# Build host packages (Debian/Ubuntu):
#   libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
#   libasound2-dev libdbus-1-dev libssl-dev patchelf file rpm
#   cmake perl nasm build-essential
# plus Rust (rustup), Node LTS, pnpm 9 and `cargo install tauri-cli --version '^2'`.

set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT="$( cd "$SCRIPT_DIR/../.." && pwd )"

SKIP_TESTS=0
SKIP_CLEAN=0
for arg in "$@"; do
    case "$arg" in
        --skip-tests) SKIP_TESTS=1 ;;
        --skip-clean) SKIP_CLEAN=1 ;;
        -h|--help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "Unknown argument: $arg" >&2; exit 2 ;;
    esac
done

# ── Validate version (mirrors scripts/bundle-version.ps1) ───────────────────
VERSION="$(tr -d '[:space:]' < "$ROOT/version.txt")"
if [[ "$VERSION" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)(-[a-zA-Z0-9.]+)?$ ]]; then
    BUNDLE_VERSION="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.${BASH_REMATCH[3]}"
elif [[ "$VERSION" =~ ^beta-([0-9]+)(\.([0-9]+))?$ ]]; then
    BUNDLE_VERSION="0.${BASH_REMATCH[1]}.${BASH_REMATCH[3]:-0}"
else
    echo "ERROR: invalid version '$VERSION' in version.txt" >&2
    echo "       expected MAJOR.MINOR.PATCH[-PRERELEASE] or beta-N[.N]" >&2
    exit 1
fi

echo "═══════════════════════════════════════════════"
echo "  FLACtastic release build — v$VERSION (bundle $BUNDLE_VERSION)"
echo "═══════════════════════════════════════════════"

DIST="$ROOT/dist"
[[ $SKIP_CLEAN -eq 0 ]] && rm -rf "$DIST"
mkdir -p "$DIST"

[[ -x "$ROOT/ui/node_modules/.bin/vite" ]] || pnpm --dir "$ROOT/ui" install --frozen-lockfile

# ── Tests (a failure warns but doesn't stop a beta build, like the Mac) ─────
if [[ $SKIP_TESTS -eq 0 ]]; then
    echo "▶ Running tests..."
    status=0
    (cd "$ROOT" && cargo test --workspace) || status=1
    pnpm --dir "$ROOT/ui" test || status=1
    if [[ $status -ne 0 ]]; then
        echo "WARNING: tests failed. Use --skip-tests to bypass." >&2
        echo "         (continuing anyway for beta release)" >&2
    fi
else
    echo "▶ Skipping tests (--skip-tests)"
fi

# ── Build ───────────────────────────────────────────────────────────────────
echo "▶ Building packages..."
OVERLAY="$(mktemp --suffix=.json)"
trap 'rm -f "$OVERLAY"' EXIT
# productName is lowercased here because the bundler derives the deb/rpm
# package name from it ("FLACtastic" would become "fla-ctastic"); the desktop
# entry (app/src-tauri/linux/flactastic.desktop) and the window title still
# say FLACtastic.
printf '{"productName":"flactastic","version":"%s","bundle":{"targets":["appimage","deb","rpm"]}}\n' "$BUNDLE_VERSION" > "$OVERLAY"
(cd "$ROOT/app/src-tauri" && cargo tauri build --config "$OVERLAY")

BUNDLE="${CARGO_TARGET_DIR:-$ROOT/target}/release/bundle"
shopt -s nullglob
copy_one() { # <glob> <dest name>
    local files=( $1 )
    [[ ${#files[@]} -gt 0 ]] || { echo "ERROR: no match for $1" >&2; exit 1; }
    cp "${files[0]}" "$DIST/$2"
}
copy_one "$BUNDLE/appimage/*_${BUNDLE_VERSION}_*.AppImage" "FLACtastic-$VERSION-linux-x86_64.AppImage"
copy_one "$BUNDLE/deb/*_${BUNDLE_VERSION}_*.deb"           "FLACtastic-$VERSION-linux-amd64.deb"
copy_one "$BUNDLE/rpm/*-${BUNDLE_VERSION}-*.rpm"           "FLACtastic-$VERSION-linux-x86_64.rpm"

# ── Release notes stub ──────────────────────────────────────────────────────
NOTES="$DIST/RELEASE_NOTES.md"
if [[ ! -f "$NOTES" ]]; then
    cat > "$NOTES" <<NOTES
# FLACtastic ${VERSION}

**Release date:** $(date +%Y-%m-%d)
**Platforms:** Linux x86_64 (AppImage, .deb, .rpm)

## What's new
- _(fill in changes for this beta)_

## Known issues
- _(fill in known issues)_

## Installation
- **Debian/Ubuntu:** \`sudo apt install ./FLACtastic-${VERSION}-linux-amd64.deb\`
- **Fedora/openSUSE:** \`sudo dnf install ./FLACtastic-${VERSION}-linux-x86_64.rpm\`
- **Anything else:** \`chmod +x FLACtastic-${VERSION}-linux-x86_64.AppImage\` and run it.

Sync finds other devices over mDNS and listens on a random TCP port (as on
the Mac). If a firewall (ufw, firewalld) blocks incoming connections, allow
UDP 5353 and TCP from your local network, e.g. \`sudo ufw allow from 192.168.0.0/16\`.
NOTES
    echo "▶ Wrote release notes stub: $NOTES"
fi

echo
echo "═══════════════════════════════════════════════"
echo "  ✅ Release build complete: v$VERSION"
echo "═══════════════════════════════════════════════"
ls -la "$DIST"
