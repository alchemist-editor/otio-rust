#!/usr/bin/env bash
#
# Assemble the release assets from the per-target library builds.
#
#   scripts/package-release.sh <version> <libraries> <out>
#
# <libraries> holds one directory per target, named the way the Zig SDK's
# build.zig names them (`aarch64-macos`, `x86_64-windows-msvc`, …), each
# with that target's static library and, where one was built, its shared
# library. The release workflow's `library` jobs produce exactly that.
#
# Written to <out>:
#
#   libotio-<version>-<target>.tar.gz   the C library for one target: include/,
#                                       lib/ and the licence. What the C++,
#                                       Go, Swift, C# and Objective-C SDKs
#                                       link, and what any C caller links.
#   otio-zig-<version>.tar.gz           the Zig package, with every target's
#                                       static library under lib/<target>/, so
#                                       `zig fetch` of this one URL is
#                                       enough on any of them
#   otio-<sdk>-<version>.tar.gz         the Go, Swift, C++, C# and Objective-C
#                                       SDKs' sources, laid out as in this
#                                       repository (they reach the header at
#                                       ../../crates/otio-capi/include), with
#                                       an empty lib/ to put a library in
#   SHA256SUMS                          every file above
#
# Run from the repository root. Sources come from `git archive HEAD`, so what
# is packaged is what is committed, whatever the working tree holds.

set -euo pipefail

if [ $# -ne 3 ]; then
    echo "usage: $0 <version> <libraries> <out>" >&2
    exit 2
fi
version=$1
libraries=$(cd "$2" && pwd)
mkdir -p "$3"
out=$(cd "$3" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Deterministic tarballs: sorted names, one owner, the commit's own time.
# Those flags are GNU tar's; set TAR to one where `tar` is BSD's (gtar on a
# Mac with Homebrew's gnu-tar).
mtime=$(git log -1 --format=%ct HEAD)
tar=${TAR:-tar}
pack() { # <archive> <directory to pack, relative to $work>
    "$tar" -C "$work" --sort=name --owner=0 --group=0 --numeric-owner \
        --mtime="@$mtime" -czf "$out/$1" "$2"
}

targets=()
for dir in "$libraries"/*/; do
    targets+=("$(basename "$dir")")
done
[ ${#targets[@]} -gt 0 ] || { echo "no libraries in $libraries" >&2; exit 1; }

# One C library archive per target.
for target in "${targets[@]}"; do
    name="libotio-$version-$target"
    mkdir -p "$work/$name/include" "$work/$name/lib"
    cp crates/otio-capi/include/otio.h "$work/$name/include/"
    cp "$libraries/$target"/* "$work/$name/lib/"
    cp LICENSE "$work/$name/"
    pack "$name.tar.gz" "$name"
done

# The Zig package: the SDK's committed files at the root, and the static
# library for each target beside them. Only the static one, because that is
# all the package links.
zig_name="otio-zig-$version"
mkdir -p "$work/$zig_name"
git archive HEAD sdk/zig | tar -x -C "$work/$zig_name" --strip-components=2
for target in "${targets[@]}"; do
    mkdir -p "$work/$zig_name/lib/$target"
    for static in libotio.a otio.lib; do
        [ -f "$libraries/$target/$static" ] && cp "$libraries/$target/$static" "$work/$zig_name/lib/$target/"
    done
done
cp LICENSE "$work/$zig_name/"
pack "$zig_name.tar.gz" "$zig_name"

# The SDKs that take the library from outside their own directory.
for sdk in go swift cpp csharp objc; do
    name="otio-$sdk-$version"
    mkdir -p "$work/$name"
    git archive HEAD "sdk/$sdk" crates/otio-capi/include LICENSE | tar -x -C "$work/$name"
    mkdir -p "$work/$name/sdk/$sdk/lib"
    pack "$name.tar.gz" "$name"
done

(cd "$out" && shasum -a 256 -- *.tar.gz > SHA256SUMS)
ls -l "$out"
