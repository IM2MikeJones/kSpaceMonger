#!/usr/bin/env bash
# Build kSpaceMonger release assets: AppImage + .deb (amd64).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG="$ROOT/packaging"
DIST="$ROOT/dist"
DESKTOP_ID=com.im2mikejones.kspacemonger
ICON_NAME=kspacemonger

cd "$ROOT"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [[ "$VERSION" == *-dev ]]; then
  echo "warning: version is $VERSION (dev). Prefer a release version like 0.3.0 before tagging." >&2
fi

echo "==> cargo build --release"
cargo build --release

BIN="$ROOT/target/release/kSpaceMonger"
test -x "$BIN"

rm -rf "$DIST"
mkdir -p "$DIST"

# --- shared desktop (Exec for system / AppImage binary name) ---
stage_icons() {
  local dest="$1"   # e.g. .../usr/share/icons or .../share/icons
  for sz in 32 48 256; do
    mkdir -p "$dest/hicolor/${sz}x${sz}/apps"
    cp "$PKG/icons/hicolor/${sz}x${sz}/apps/${ICON_NAME}.png" \
      "$dest/hicolor/${sz}x${sz}/apps/${ICON_NAME}.png"
  done
}

# ========== AppImage ==========
echo "==> AppImage"
APPDIR="$DIST/AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications"
cp "$BIN" "$APPDIR/usr/bin/kSpaceMonger"
chmod 755 "$APPDIR/usr/bin/kSpaceMonger"
cp "$PKG/${DESKTOP_ID}.desktop" "$APPDIR/usr/share/applications/${DESKTOP_ID}.desktop"
stage_icons "$APPDIR/usr/share/icons"

# AppDir root bits required by appimagetool
cp "$PKG/${DESKTOP_ID}.desktop" "$APPDIR/${DESKTOP_ID}.desktop"
# Prefer a short Icon basename at AppDir root
cp "$PKG/icons/hicolor/256x256/apps/${ICON_NAME}.png" "$APPDIR/${ICON_NAME}.png"
# AppRun: launch the bundled binary
cat > "$APPDIR/AppRun" << 'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/kSpaceMonger" "$@"
EOF
chmod 755 "$APPDIR/AppRun"

APPIMAGE_OUT="$DIST/kSpaceMonger-${VERSION}-x86_64.AppImage"
APPIMAGETOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
if [[ -z "$APPIMAGETOOL" && -x "$HOME/.local/bin/appimagetool" ]]; then
  APPIMAGETOOL="$HOME/.local/bin/appimagetool"
fi
if [[ -z "$APPIMAGETOOL" ]]; then
  echo "error: appimagetool not found (install to PATH or ~/.local/bin/appimagetool)" >&2
  exit 1
fi
ARCH=x86_64 "$APPIMAGETOOL" "$APPDIR" "$APPIMAGE_OUT"
chmod 755 "$APPIMAGE_OUT"

# ========== .deb ==========
echo "==> .deb"
DEB_ROOT="$DIST/deb"
rm -rf "$DEB_ROOT"
mkdir -p "$DEB_ROOT/DEBIAN" \
  "$DEB_ROOT/usr/bin" \
  "$DEB_ROOT/usr/share/applications" \
  "$DEB_ROOT/usr/share/doc/kspacemonger"

cp "$BIN" "$DEB_ROOT/usr/bin/kSpaceMonger"
chmod 755 "$DEB_ROOT/usr/bin/kSpaceMonger"
cp "$PKG/${DESKTOP_ID}.desktop" "$DEB_ROOT/usr/share/applications/${DESKTOP_ID}.desktop"
stage_icons "$DEB_ROOT/usr/share/icons"

# Debian package version: strip -dev for control if present still keep full for filename
DEB_VER="${VERSION}"
cat > "$DEB_ROOT/DEBIAN/control" << EOF
Package: kspacemonger
Version: ${DEB_VER}
Section: utils
Priority: optional
Architecture: amd64
Maintainer: Mike Jones <noreply@users.noreply.github.com>
Depends: libc6
Description: kSpaceMonger — disk and folder space map
 Fast classic-style treemap for exploring disk and folder usage on Linux.
Homepage: https://github.com/IM2MikeJones/kSpaceMonger
EOF

cat > "$DEB_ROOT/usr/share/doc/kspacemonger/copyright" << 'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: kSpaceMonger
Source: https://github.com/IM2MikeJones/kSpaceMonger

Files: *
Copyright: Mike Jones
License: Proprietary-or-as-stated-in-repo
 See the upstream repository for license terms.

Comment:
 kSpaceMonger is an independent Linux reimplementation inspired by the
 classic Windows SpaceMonger 1.x disk space map created by Sean Werkema
 (Copyright (C) 1998, 1999, and 2000). Original freeware docs:
 http://www.werkema.com/software/spacemonger.html
 Preserved source: https://github.com/seanofw/spacemonger1
 Not affiliated with Sean Werkema, Sixty-Five, Edgerunner, or Stardock.
EOF

gzip -9n -c "$ROOT/README.md" > "$DEB_ROOT/usr/share/doc/kspacemonger/README.gz" || true

# Installed size in KiB
INSTALLED_SIZE="$(du -sk "$DEB_ROOT" | awk '{print $1}')"
echo "Installed-Size: $INSTALLED_SIZE" >> "$DEB_ROOT/DEBIAN/control"

DEB_OUT="$DIST/kspacemonger_${DEB_VER}_amd64.deb"
dpkg-deb --root-owner-group --build "$DEB_ROOT" "$DEB_OUT"

echo
echo "Release assets:"
ls -lh "$APPIMAGE_OUT" "$DEB_OUT"
echo "Done."
