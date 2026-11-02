#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
ARCH="$(uname -m)"
APPIMAGETOOL_BIN="${APPIMAGETOOL:-appimagetool}"
APPDIR="$ROOT/target/appimage/TVnet.AppDir"
OUTPUT_DIR="$ROOT/dist"

case "$ARCH" in
    x86_64|aarch64) ;;
    *) echo "Architecture non prise en charge par ce script : $ARCH" >&2; exit 1 ;;
esac

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo est requis. Installez Rust stable via rustup, puis relancez ce script." >&2
    exit 1
fi
if ! command -v "$APPIMAGETOOL_BIN" >/dev/null 2>&1; then
    echo "appimagetool est requis (ou indiquez son chemin avec APPIMAGETOOL)." >&2
    exit 1
fi

cd "$ROOT"
cargo build --release --target "${ARCH}-unknown-linux-gnu"

rm -rf "$APPDIR"
mkdir -p \
    "$APPDIR/usr/bin" \
    "$APPDIR/usr/share/applications" \
    "$APPDIR/usr/share/icons/hicolor/256x256/apps" \
    "$OUTPUT_DIR"
install -m 0755 "target/${ARCH}-unknown-linux-gnu/release/tvnet" "$APPDIR/usr/bin/tvnet"
install -m 0755 packaging/appimage/AppRun "$APPDIR/AppRun"
install -m 0644 packaging/appimage/tvnet.desktop "$APPDIR/tvnet.desktop"
install -m 0644 packaging/appimage/tvnet.desktop "$APPDIR/usr/share/applications/tvnet.desktop"
install -m 0644 packaging/appimage/tvnet.svg "$APPDIR/tvnet.svg"
install -m 0644 packaging/appimage/tvnet.png "$APPDIR/tvnet.png"
install -m 0644 packaging/appimage/tvnet.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/tvnet.png"
install -m 0644 packaging/appimage/tvnet.png "$APPDIR/.DirIcon"

TEMP_OUTPUT="$OUTPUT_DIR/.TVnet-${ARCH}-build-$$.AppImage"
trap 'rm -f "$TEMP_OUTPUT"' EXIT HUP INT TERM
ARCH="$ARCH" "$APPIMAGETOOL_BIN" "$APPDIR" "$TEMP_OUTPUT"
chmod 0755 "$TEMP_OUTPUT"
mv -f "$TEMP_OUTPUT" "$OUTPUT_DIR/TVnet-${ARCH}.AppImage"
trap - EXIT HUP INT TERM
echo "AppImage créée : $OUTPUT_DIR/TVnet-${ARCH}.AppImage"
