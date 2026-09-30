#!/usr/bin/env bash
set -euo pipefail
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"
[[ "$(uname -s)" == Darwin ]] || { echo 'Run this script on macOS.' >&2; exit 1; }
cargo build --locked --release -p dc_app --bin diffcomp-studio
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
APP_BUNDLE="$PROJECT_ROOT/dist/macos/DiffComp Studio.app"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"
cp target/release/diffcomp-studio "$APP_BUNDLE/Contents/MacOS/diffcomp-studio.new"
mv -f "$APP_BUNDLE/Contents/MacOS/diffcomp-studio.new" "$APP_BUNDLE/Contents/MacOS/diffcomp-studio"
cat > "$APP_BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>DiffComp Studio</string>
<key>CFBundleIdentifier</key><string>com.diffcomp.studio</string>
<key>CFBundleVersion</key><string>$VERSION</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleExecutable</key><string>diffcomp-studio</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
if [[ -f assets/icon.icns ]]; then cp assets/icon.icns "$APP_BUNDLE/Contents/Resources/AppIcon.icns"; fi
codesign --force --sign "${DIFFCOMP_SIGN_IDENTITY:--}" "$APP_BUNDLE"
codesign --verify --strict "$APP_BUNDLE"
"$APP_BUNDLE/Contents/MacOS/diffcomp-studio" --version
printf 'Bundle ready: %s\n' "$APP_BUNDLE"
