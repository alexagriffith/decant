#!/usr/bin/env bash
# Assemble Decant.app from the SwiftPM release build — no Xcode project.
# Output: macos/build/Decant.app (LSUIElement menu bar app, ad-hoc signed).
set -euo pipefail
cd "$(dirname "$0")"

swift build -c release
BIN_DIR="$(swift build -c release --show-bin-path)"

APP=build/Decant.app
BIN="$BIN_DIR/DecantBar"
test -x "$BIN" || { echo "error: $BIN missing after build" >&2; exit 1; }

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/DecantBar"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>DecantBar</string>
  <key>CFBundleIdentifier</key><string>dev.decant.menubar</string>
  <key>CFBundleName</key><string>Decant</string>
  <key>CFBundleDisplayName</key><string>Decant</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Ad-hoc signature: required on Apple silicon; fine for a local personal app.
codesign --force --sign - "$APP"

echo "built $APP"
