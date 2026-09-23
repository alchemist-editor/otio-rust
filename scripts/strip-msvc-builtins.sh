#!/usr/bin/env bash
#
# Remove Rust's `compiler_builtins` from an MSVC static library.
#
#   scripts/strip-msvc-builtins.sh target/x86_64-pc-windows-msvc/release/otio.lib
#
# Every Rust staticlib carries its own copy of compiler-rt (`__divti3`,
# `__multf3`, …) in the `compiler_builtins` members, and stable Rust cannot
# leave them out. Zig links its own compiler-rt into everything it builds,
# and both copies are COFF weak externals. lld-link outside MinGW mode
# rejects two weak definitions of one name as a duplicate symbol, so a Zig
# program cannot link an unstripped `otio.lib` for an MSVC target at all.
# Mach-O and the MinGW-mode link settle the pair themselves, which is why
# only MSVC archives go through this.
#
# What the rest of the library needs from those members is a handful of
# 128-bit integer and conversion routines (__divti3, __modti3, __udivti3,
# __fixdfti, __floattidf), all of which Zig's compiler-rt and LLVM's provide.
#
# Deletion goes through Zig's bundled llvm-ar (`zig ar`), which reads COFF
# libraries on any host. `ar d` removes one member per name and an archive
# can repeat a name, so this repeats until the listing is clean, and it
# deletes in batches to stay under Windows' 32K command-line limit: the
# members built from C are stored under their full build path.

set -euo pipefail

if [ $# -ne 1 ] || [ ! -f "$1" ]; then
    echo "usage: $0 <path to otio.lib>" >&2
    exit 2
fi
archive=$1
zig=${ZIG:-zig}
batch=40

removed=0
while :; do
    members=()
    while IFS= read -r name; do
        name=${name%$'\r'}
        case "$name" in
        *compiler_builtins*) members+=("$name") ;;
        esac
    done < <("$zig" ar t "$archive")
    [ ${#members[@]} -gt 0 ] || break
    for ((i = 0; i < ${#members[@]}; i += batch)); do
        "$zig" ar d "$archive" "${members[@]:i:batch}"
    done
    removed=$((removed + ${#members[@]}))
done

if [ "$removed" -eq 0 ]; then
    echo "$archive: no compiler_builtins members; is this a Rust staticlib?" >&2
    exit 1
fi
echo "$archive: removed $removed compiler_builtins members"
