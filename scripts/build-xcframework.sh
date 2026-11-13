#!/usr/bin/env bash
#
# Build libotio for every Apple platform and package it as one XCFramework.
#
#   scripts/build-xcframework.sh [<out>]
#
# Writes <out>/libotio.xcframework (default: target/xcframework), holding the
# static library and the C headers for each of:
#
#   macos-arm64_x86_64             aarch64-apple-darwin + x86_64-apple-darwin
#   ios-arm64                      aarch64-apple-ios
#   ios-arm64-simulator            aarch64-apple-ios-sim
#   tvos-arm64                     aarch64-apple-tvos
#   tvos-arm64-simulator           aarch64-apple-tvos-sim
#
# Each library carries the extensions a release carries (otio-capi's default
# features, `ext-sync` and `ext-ori`); they do nothing until a program turns
# one on. The headers are `otio.h` and each extension's header, with no module
# map: the Swift package brings its own `COtio` module, and a module map here
# would declare a second one.
#
# The Swift package in sdk/swift links this XCFramework instead of a bare
# `libotio.a` whenever it finds it at sdk/swift/lib/libotio.xcframework, which
# is how an iOS or tvOS app gets the SDK:
#
#   scripts/build-xcframework.sh
#   cp -R target/xcframework/libotio.xcframework sdk/swift/lib/
#
# Needs a Mac with Xcode (`xcodebuild -create-xcframework` and `lipo`), and
# rustup. Every target here is tier 2, so stable Rust has a prebuilt standard
# library for each and no nightly is needed. Set OTIO_XCFRAMEWORK_TARGETS to a
# space-separated subset of the Rust targets above to build fewer (macOS is
# packaged only when both of its targets are in the set).

set -euo pipefail

out=${1:-target/xcframework}
targets=${OTIO_XCFRAMEWORK_TARGETS:-"aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-tvos aarch64-apple-tvos-sim"}

# The oldest OS each library is built for, which is also the oldest the Swift
# package declares. Rust raises the arm64 simulators to 14.0 on its own, the
# first release that ran them.
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-10.13}
export IPHONEOS_DEPLOYMENT_TARGET=${IPHONEOS_DEPLOYMENT_TARGET:-13.0}
export TVOS_DEPLOYMENT_TARGET=${TVOS_DEPLOYMENT_TARGET:-13.0}

# One LTO'd object instead of the whole standard library's set, as the
# release's other libraries are built. CI's check sets these to false and 16
# to build faster, since what it proves does not depend on them.
export CARGO_PROFILE_RELEASE_LTO=${CARGO_PROFILE_RELEASE_LTO:-true}
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-1}

has() { case " $targets " in *" $1 "*) return 0 ;; *) return 1 ;; esac }

rustup target add $targets

for target in $targets; do
    echo "== $target"
    # Only the static library. The crate also asks for a cdylib, which an
    # XCFramework of static libraries has no use for, and linking one for a
    # mobile target would need that platform's SDK for nothing.
    cargo rustc -p otio-capi --release --target "$target" --crate-type staticlib
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

headers="$work/include"
mkdir -p "$headers"
cp crates/otio-capi/include/otio.h "$headers/"
cp extensions/*/capi/include/*.h "$headers/"

args=()
add() { # <library>
    args+=(-library "$1" -headers "$headers")
}

if has aarch64-apple-darwin && has x86_64-apple-darwin; then
    mkdir -p "$work/macos"
    lipo -create \
        target/aarch64-apple-darwin/release/libotio.a \
        target/x86_64-apple-darwin/release/libotio.a \
        -output "$work/macos/libotio.a"
    add "$work/macos/libotio.a"
fi
for target in aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-tvos aarch64-apple-tvos-sim; do
    if has "$target"; then
        add "target/$target/release/libotio.a"
    fi
done

[ ${#args[@]} -gt 0 ] || { echo "no Apple targets to package" >&2; exit 1; }

mkdir -p "$out"
rm -rf "$out/libotio.xcframework"
xcodebuild -create-xcframework "${args[@]}" -output "$out/libotio.xcframework"
echo "wrote $out/libotio.xcframework:"
ls "$out/libotio.xcframework"
