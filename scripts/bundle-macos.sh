#!/bin/bash
# Build Surfterm.app macOS bundle
set -e

cd "$(dirname "$0")/.."

APP_NAME="Surfterm"
BUNDLE_DIR="target/release/${APP_NAME}.app"
CONTENTS="${BUNDLE_DIR}/Contents"
MACOS="${CONTENTS}/MacOS"
RESOURCES="${CONTENTS}/Resources"

echo "Building release..."
cargo build --release

echo "Creating app bundle..."
rm -rf "${BUNDLE_DIR}"
mkdir -p "${MACOS}" "${RESOURCES}"

cp "target/release/surfterm" "${MACOS}/surfterm"
cp "assets/AppIcon.icns" "${RESOURCES}/AppIcon.icns"

cat > "${CONTENTS}/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Surfterm</string>
    <key>CFBundleDisplayName</key>
    <string>Surfterm</string>
    <key>CFBundleIdentifier</key>
    <string>com.surfterm.app</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleExecutable</key>
    <string>surfterm</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
</dict>
</plist>
PLIST

echo "✓ Built ${BUNDLE_DIR}"
echo "  Run with: open ${BUNDLE_DIR}"
