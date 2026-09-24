#!/usr/bin/env bash
# Build the membox-core xcframework (the iOS app links this) + the Swift binding.
# Same recipe as inbox2's core/build-ios-xcframework.sh.
#
# Prereq: rustup toolchain with the iOS targets:
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim
set -euo pipefail
cd "$(dirname "$0")" # core/
[ -x "$HOME/.cargo/bin/rustup" ] && export PATH="$HOME/.cargo/bin:$PATH"

FEAT="--features mobile-ffi"
T="../target"

echo "▸ building static libs for device + simulator"
cargo build --release $FEAT --target aarch64-apple-ios
cargo build --release $FEAT --target aarch64-apple-ios-sim

echo "▸ generating Swift bindings"
cargo build $FEAT
mkdir -p "$T/uniffi-swift"
cargo run $FEAT --bin uniffi-bindgen -- generate \
  --library "$T/debug/libmembox_core.dylib" \
  --language swift --out-dir "$T/uniffi-swift"

echo "▸ assembling the xcframework"
HDR="$T/uniffi-headers"
rm -rf "$HDR" && mkdir -p "$HDR"
cp "$T/uniffi-swift/membox_coreFFI.h" "$HDR/"
cp "$T/uniffi-swift/membox_coreFFI.modulemap" "$HDR/module.modulemap"

OUT="$T/MemboxCore.xcframework"
rm -rf "$OUT"
xcodebuild -create-xcframework \
  -library "$T/aarch64-apple-ios/release/libmembox_core.a" -headers "$HDR" \
  -library "$T/aarch64-apple-ios-sim/release/libmembox_core.a" -headers "$HDR" \
  -output "$OUT"

echo "✓ $OUT"
echo "✓ $T/uniffi-swift/membox_core.swift"
