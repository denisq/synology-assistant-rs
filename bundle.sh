#!/bin/sh
# Build "Synology Assistant.app" into ./dist (macOS only).
# usage: ./bundle.sh [--target aarch64-apple-darwin|x86_64-apple-darwin]
set -eu
cd "$(dirname "$0")"

APP="dist/Synology Assistant.app"
if [ "${1:-}" = "--target" ] && [ -n "${2:-}" ]; then
  cargo build --release --target "$2"
  BIN="target/$2/release/synology-assistant"
else
  cargo build --release
  BIN="target/release/synology-assistant"
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/SynologyAssistant"
if [ -f assets/AppIcon.icns ]; then
  cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
fi
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Synology Assistant</string>
  <key>CFBundleDisplayName</key><string>Synology Assistant</string>
  <key>CFBundleIdentifier</key><string>io.github.denisq.synology-assistant-rs</string>
  <key>CFBundleExecutable</key><string>SynologyAssistant</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSLocalNetworkUsageDescription</key>
  <string>Synology Assistant searches your local network for Synology NAS devices and sends Wake-on-LAN packets.</string>
</dict>
</plist>
EOF

codesign --force --deep --sign - "$APP"
echo "Built: $APP"
