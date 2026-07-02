#!/bin/bash

APP_NAME="PDOS"
BUILD_DIR="build"
APP_BUNDLE="$BUILD_DIR/$APP_NAME.app"
MACOS_DIR="$APP_BUNDLE/Contents/MacOS"
RESOURCES_DIR="$APP_BUNDLE/Contents/Resources"

set -e

# 1. Compile Rust daemon in release
echo "🔨 Compiling Rust Daemon (release)..."
cargo build --release --manifest-path ../../../cli/Cargo.toml 2>&1 | tail -3

# 2. Build app bundle
echo "📦 Packaging Mac App Bundle..."
rm -rf "$BUILD_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

# 3. Copy Rust binary into app
cp ../../../target/release/dos "$RESOURCES_DIR/dos"

# 4. Compile Swift sources
echo "⚙️  Compiling Swift UI..."
swiftc Sources/PDOS/*.swift \
    -o "$MACOS_DIR/$APP_NAME" \
    -framework Cocoa \
    -framework WebKit 2>&1

# 5. Create Info.plist
cat > "$APP_BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>com.pdos.mac</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>2.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
</dict>
</plist>
PLIST

echo "✅ Built at $APP_BUNDLE"
echo "🗜️ Zipping..."
cd "$BUILD_DIR"
rm -f PDOS_Mac_Release.zip
zip -r PDOS_Mac_Release.zip "$APP_NAME.app" > /dev/null
cp PDOS_Mac_Release.zip ~/Desktop/
echo "🎉 Bundle on Desktop: ~/Desktop/PDOS_Mac_Release.zip"
