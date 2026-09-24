#!/usr/bin/env bash
# Signed build → install → launch on the paired iPhone, over the air.
#
# `devicectl` talks to the phone over the network once it has been paired
# (Xcode › Devices, "Connect via network"), so nothing has to be plugged in —
# the phone shows up as <name>.coredevice.local. This is the only way membox
# reaches a device; there is no TestFlight lane, on purpose (§7).
#
#   ./deploy-device.sh                  # first paired iPhone
#   ./deploy-device.sh --xcframework    # rebuild the Rust core first (core changes)
#   DEVICE=<udid> ./deploy-device.sh
#
# Automatic signing with -allowProvisioningUpdates registers the App ID, the
# App Group and the iCloud container on the portal the first time.
set -euo pipefail
# basename before the cd: $0 is relative, and the cd would strand it.
SELF="$(basename "$0")"
cd "$(dirname "$0")"

DO_XCFRAMEWORK=0
while [ $# -gt 0 ]; do
  case "$1" in
    --xcframework) DO_XCFRAMEWORK=1 ;;
    -h|--help)     sed -n '2,14p' "$SELF"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
  shift
done

# A Rust change reaches the phone only through a fresh xcframework, and a
# project.yml change only through xcodegen — both silent no-ops otherwise.
[ "$DO_XCFRAMEWORK" = 1 ] && ./bootstrap.sh
command -v xcodegen >/dev/null 2>&1 && xcodegen generate >/dev/null

DEVICE="${DEVICE:-$(xcrun devicectl list devices 2>/dev/null | grep -i iphone | grep -oE '[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}' | head -1)}"
[ -n "$DEVICE" ] || { echo "no paired iPhone"; exit 1; }
LOG=$(mktemp)
if ! xcodebuild -project Membox.xcodeproj -scheme Membox -configuration Debug -destination "id=$DEVICE" \
  -derivedDataPath build-device -allowProvisioningUpdates build > "$LOG" 2>&1; then
  grep -E "error:" "$LOG" >&2 || tail -20 "$LOG" >&2; echo "✗ build failed ($LOG)" >&2; exit 1
fi
grep -E "BUILD SUCCEEDED" "$LOG"
xcrun devicectl device install app --device "$DEVICE" build-device/Build/Products/Debug-iphoneos/Membox.app | grep -iE "installed|error" || true
xcrun devicectl device process launch --device "$DEVICE" com.membox.Membox | grep -iE "Launched|error" || true
