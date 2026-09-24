#!/usr/bin/env bash
# Build for the simulator, install, launch.
set -euo pipefail
cd "$(dirname "$0")" # ios/

SIM_NAME="${SIM_NAME:-iPhone 17 Pro}"
UDID=$(xcrun simctl list devices available | grep -m1 "$SIM_NAME (" | grep -oE "[0-9A-F-]{36}")
[ -n "$UDID" ] || { echo "No '$SIM_NAME' simulator found"; exit 1; }

xcodebuild -project Membox.xcodeproj -scheme Membox -sdk iphonesimulator -configuration Debug \
  -destination "id=$UDID" -derivedDataPath build CODE_SIGNING_ALLOWED=NO \
  ARCHS=arm64 EXCLUDED_ARCHS=x86_64 build 2>&1 | grep -E "error:|warning: unused|BUILD|Compiling" | tail -20

xcrun simctl boot "$UDID" 2>/dev/null || true
xcrun simctl install "$UDID" build/Build/Products/Debug-iphonesimulator/Membox.app
xcrun simctl terminate "$UDID" com.membox.Membox 2>/dev/null || true
xcrun simctl launch "$UDID" com.membox.Membox
open -a Simulator
