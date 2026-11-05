#!/bin/sh
# Builds the window and wraps it into "Smart Shoebox.app", ready to double-click
# or to drag into /Applications. Usage: scripts/bundle-macos.sh
set -eu
cd "$(dirname "$0")/.."

cargo build-gui

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
app="target/release/Smart Shoebox.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp target/release/shoebox-gui "$app/Contents/MacOS/shoebox-gui"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Smart Shoebox</string>
  <key>CFBundleDisplayName</key><string>Smart Shoebox</string>
  <key>CFBundleIdentifier</key><string>io.github.dseaku.smart-shoebox</string>
  <key>CFBundleExecutable</key><string>shoebox-gui</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

echo "Built $app"
