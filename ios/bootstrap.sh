#!/usr/bin/env bash
# Bootstrap the iOS app: build the Rust core xcframework, vendor the Swift
# binding, copy the brand assets, generate the Xcode project.
# Run after a fresh checkout and whenever the FFI surface or the icon changes.
set -euo pipefail
cd "$(dirname "$0")" # ios/
[ -x "$HOME/.cargo/bin/rustup" ] && export PATH="$HOME/.cargo/bin:$PATH"

echo "▸ 1/3 building the shared Rust core xcframework"
../core/build-ios-xcframework.sh

echo "▸ 2/3 vendoring the Swift binding + brand assets"
mkdir -p Generated Assets.xcassets
cp ../target/uniffi-swift/membox_core.swift Generated/membox_core.swift
rm -rf Assets.xcassets/AppIcon.appiconset Assets.xcassets/LaunchBackground.colorset Assets.xcassets/LaunchMark.imageset
cp -R ../brand/generated/ios/AppIcon.appiconset ../brand/generated/ios/LaunchBackground.colorset ../brand/generated/ios/LaunchMark.imageset Assets.xcassets/
[ -f Assets.xcassets/Contents.json ] || echo '{ "info" : { "author" : "membox", "version" : 1 } }' > Assets.xcassets/Contents.json

echo "▸ 3/3 generating the Xcode project"
xcodegen generate

echo "✓ bootstrapped — ./build.sh to build+run on the simulator, or open Membox.xcodeproj"
