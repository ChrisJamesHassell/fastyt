#!/usr/bin/env bash
# Builds fast.yt and installs it as ~/Applications/fast.yt.app (searchable in Spotlight).
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v cargo >/dev/null; then
    echo "Rust is required. Install it from https://rustup.rs" >&2
    exit 1
fi
for tool in mpv yt-dlp; do
    if ! command -v "$tool" >/dev/null; then
        echo "warning: $tool not found. Install with: brew install mpv yt-dlp deno" >&2
    fi
done
if ! command -v deno >/dev/null; then
    echo "note: deno not found. It is optional but helps yt-dlp avoid YouTube throttling: brew install deno" >&2
fi

cargo build --release

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
app="${FASTYT_APP_DIR:-$HOME/Applications}/fast.yt.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/fastyt "$app/Contents/MacOS/fastyt"
cp assets/fastyt.icns "$app/Contents/Resources/fastyt.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>fast.yt</string>
    <key>CFBundleDisplayName</key>
    <string>fast.yt</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.chrisjameshassell.fastyt</string>
    <key>CFBundleExecutable</key>
    <string>fastyt</string>
    <key>CFBundleIconFile</key>
    <string>fastyt</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${version}</string>
    <key>CFBundleVersion</key>
    <string>${version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

echo "Installed $app"
