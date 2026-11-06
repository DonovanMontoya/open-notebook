#!/bin/sh
# Builds "Open Notebook.app" from the Rune app: scripts/bundle.sh [out-dir]
# It keeps the Tauri app's bundle id, so it replaces that app on this Mac and opens the
# same data folder (~/Library/Application Support/com.donovanmontoya.opennotebook).
set -eu
cd "$(dirname "$0")/.."
out="${1:-target/bundle}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
cargo build --release -p open-notebook-mac
app="$out/Open Notebook.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/open-notebook "$app/Contents/MacOS/open-notebook"
cp app/assets/icon.icns "$app/Contents/Resources/icon.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Open Notebook</string>
  <key>CFBundleDisplayName</key><string>Open Notebook</string>
  <key>CFBundleIdentifier</key><string>com.donovanmontoya.opennotebook</string>
  <key>CFBundleExecutable</key><string>open-notebook</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Open Notebook uses the microphone for live voice conversations and only while one is running.</string>
</dict>
</plist>
PLIST
# Ad-hoc signature so Gatekeeper runs a local build.
codesign --force --sign - "$app" >/dev/null
echo "$app"
