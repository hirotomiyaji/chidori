#!/bin/sh
# Builds Chidori.app (Metal, release) under target/, signed ad hoc, for this Mac.
#
# A bundle, rather than the bare binary, so macOS asks for the microphone and system audio in
# Chidori's own name (its Info.plist says why) and remembers the answer, instead of attributing
# them to the terminal that launched it. Packaging for others (a .dmg, updates) is issue 6 step 4.
set -eu
cd "$(dirname "$0")/.."
. scripts/macos-env.sh

cargo build --release -p chidori --features gpu-metal --locked

target="${CARGO_TARGET_DIR:-target}"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
app="$target/Chidori.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$target/release/chidori" "$app/Contents/MacOS/chidori"
# The app reads its resources beside the executable (host::Paths::system).
cp -R crates/app/resources "$app/Contents/MacOS/resources"
cp crates/app/icons/icon.icns "$app/Contents/Resources/icon.icns"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Chidori</string>
  <key>CFBundleDisplayName</key><string>Chidori</string>
  <key>CFBundleIdentifier</key><string>dev.yusai.chidori</string>
  <key>CFBundleExecutable</key><string>chidori</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>14.6</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key>
  <string>自分の声を文字起こしするためにマイクを使います。録音はこの Mac の外に送られません。 / Chidori transcribes your voice from the microphone. Nothing leaves this Mac.</string>
  <key>NSAudioCaptureUsageDescription</key>
  <string>相手の声（この Mac で鳴っている音）を文字起こしするために使います。録音はこの Mac の外に送られません。 / Chidori transcribes the sound playing on this Mac. Nothing leaves this Mac.</string>
  <key>NSScreenCaptureUsageDescription</key>
  <string>スクリーンショットを文字起こしに挟むために使います。 / Chidori puts screenshots into the transcript.</string>
</dict>
</plist>
EOF

codesign --force --deep --sign - "$app"
echo "Built $app"
