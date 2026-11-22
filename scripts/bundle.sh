#!/usr/bin/env bash
# Construit dist/Dusk.app (release, signée) avec l'icône générée depuis assets/AppIcon.svg.
set -euo pipefail
cd "$(dirname "$0")/.."

APP=dist/Dusk.app
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

cargo build --release -p dusk-app

# Icône : SVG -> PNG 1024 transparent -> iconset -> icns
SRC="$TMP/AppIcon.png"
swift scripts/svg2png.swift assets/AppIcon.svg "$SRC" 1024
ICONSET="$TMP/AppIcon.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$SRC" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s*2)) $((s*2)) "$SRC" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/Dusk "$APP/Contents/MacOS/Dusk"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Dusk</string>
  <key>CFBundleDisplayName</key><string>Dusk</string>
  <key>CFBundleIdentifier</key><string>fr.pierrefardel.dusk</string>
  <key>CFBundleExecutable</key><string>Dusk</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Signature : identité stable si disponible (l'autorisation Accessibilité est
# alors conservée d'une version à l'autre), sinon signature ad hoc.
IDENTITY="${DUSK_SIGN_IDENTITY:-$(security find-identity -v -p codesigning | awk '/Apple Development/ {print $2; exit}')}"
if [ -n "$IDENTITY" ]; then
  codesign --force --deep --sign "$IDENTITY" "$APP"
  echo "Signé avec l'identité $IDENTITY"
else
  codesign --force --deep --sign - "$APP"
  echo "Signature ad hoc (l'autorisation Accessibilité sera à redonner après chaque build)"
fi
echo "OK : $APP"
