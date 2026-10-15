#!/usr/bin/env bash
# Wrap a macOS build of MangaPoint in an app bundle.
#
#   tools/package-macos.sh <binary> <version> <output folder>
#
# Makes <output folder>/MangaPoint.app (with the cat as its icon) and copies
# the README, LICENSE and CHANGELOG beside it. The app is not signed or
# notarized, so macOS asks for confirmation the first time it is opened (see
# the README).
set -euo pipefail

binary="$1"
version="$2"
out="$3"
root="$(cd "$(dirname "$0")/.." && pwd)"

app="$out/MangaPoint.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/mangapoint"
chmod +x "$app/Contents/MacOS/mangapoint"

# The icon: the PNG at every size an .icns holds.
iconset="$(mktemp -d)/MangaPoint.iconset"
mkdir -p "$iconset"
source_png="$root/apps/desktop/assets/icon.png"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$source_png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$source_png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/MangaPoint.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>MangaPoint</string>
  <key>CFBundleDisplayName</key><string>MangaPoint</string>
  <key>CFBundleIdentifier</key><string>com.mangapoint.app</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleExecutable</key><string>mangapoint</string>
  <key>CFBundleIconFile</key><string>MangaPoint</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

cp "$root/README.md" "$root/LICENSE" "$root/CHANGELOG.md" "$out/"
