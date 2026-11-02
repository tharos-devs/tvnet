#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
APP_NAME="TVnet"
BUNDLE_ID="fr.tvnet.app"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)"
OUTPUT_DIR="$ROOT/dist"
APP_BUNDLE="$OUTPUT_DIR/$APP_NAME.app"
ICON_SOURCE="$ROOT/packaging/appimage/tvnet.png"

if [ "$(uname -s)" != "Darwin" ]; then
    echo "Ce script doit être exécuté sur macOS." >&2
    exit 1
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo est requis. Installez Rust stable via rustup, puis relancez ce script." >&2
    exit 1
fi
if ! command -v iconutil >/dev/null 2>&1 || ! command -v sips >/dev/null 2>&1; then
    echo "Les outils macOS iconutil et sips sont requis." >&2
    exit 1
fi
if [ -z "$VERSION" ]; then
    echo "Impossible de lire la version dans Cargo.toml." >&2
    exit 1
fi

cd "$ROOT"
cargo build --release

rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"
install -m 0755 "$ROOT/target/release/tvnet" "$APP_BUNDLE/Contents/MacOS/tvnet"
install -m 0644 "$ROOT/packaging/appimage/tvnet.ico" "$OUTPUT_DIR/TVnet.ico"

ICONSET="$(mktemp -d "${TMPDIR:-/tmp}/tvnet-icon.XXXXXX.iconset")"
trap 'rm -rf "$ICONSET"' EXIT HUP INT TERM
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$ICON_SOURCE" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double_size=$((size * 2))
    sips -z "$double_size" "$double_size" "$ICON_SOURCE" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP_BUNDLE/Contents/Resources/TVnet.icns"

cat > "$APP_BUNDLE/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>fr</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleExecutable</key>
    <string>tvnet</string>
    <key>CFBundleIconFile</key>
    <string>TVnet</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF
plutil -lint "$APP_BUNDLE/Contents/Info.plist" >/dev/null
touch "$APP_BUNDLE"
echo "Application créée : $APP_BUNDLE"
