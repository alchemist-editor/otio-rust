#!/usr/bin/env bash
#
# Which CI jobs a change needs.
#
# Reads the changed paths on standard input, one per line, and writes
# `name=true|false` lines for the `changes` job to publish as outputs.
#
# The rule is deliberately fail-safe: a path this does not recognise runs
# everything. Only the buckets listed below are ever narrowed, so a new
# directory — another SDK target, a docs site, a script — costs a full run
# until someone adds it here, rather than silently skipping a job that
# should have run.
#
# Run `select-jobs.sh --self-test` to check the table below still holds;
# the `changes` job does that on every run.

set -euo pipefail

# Everything runs. Also what an unrecognised path, an unreadable diff or an
# empty one falls back to.
everything() {
    cat <<'EOF'
any=true
rust=true
python=true
capi=true
library=true
go=true
swift=true
zig=true
cpp=true
csharp=true
objc=true
ts=true
site=true
EOF
}

select_jobs() {
    local path
    local any=false rust=false python=false capi=false go=false swift=false
    local zig=false cpp=false csharp=false objc=false ts=false site=false

    while IFS= read -r path; do
        [ -n "$path" ] || continue
        case "$path" in
        # Prose, and nothing but prose. None of these files is read by a
        # build, by a test, or by the SDK generator, so a change confined to
        # them needs no job at all. Everything the generator writes is
        # deliberately outside this list, `sdk/<language>/README.md`
        # included: those are generated, so a hand edit has to reach the
        # drift check.
        README.md | LICENSE | docs/* | sdk/README.md | .github/*.md | crates/*/README.md)
            continue
            ;;
        # One SDK's own package: its hand-written tests, its build files.
        # The generated files live here too, so the drift check runs on any
        # of these as well — see `any` below.
        sdk/go/*)
            any=true
            go=true
            ;;
        sdk/swift/*)
            any=true
            swift=true
            ;;
        sdk/zig/*)
            any=true
            zig=true
            ;;
        sdk/cpp/*)
            any=true
            cpp=true
            ;;
        sdk/csharp/*)
            any=true
            csharp=true
            ;;
        sdk/objc/*)
            any=true
            objc=true
            ;;
        # The TypeScript package. The wasm crate it is built from is Rust,
        # under `crates/otio-wasm/src`, and falls through to the catch-all.
        crates/otio-wasm/ts/*)
            any=true
            ts=true
            ;;
        # The documentation site's samples. Each is compiled in its own
        # language's job, against that SDK, and the language is written on
        # the filename (`site/content/samples/<id>/<language>.<ext>`), so a
        # sample reaches its own job and the site's, and no other. The
        # harness that compiles them is shared by every job, so it runs
        # everything — as does a sample file named for a language this
        # does not know.
        site/content/samples/*/*)
            any=true
            site=true
            case "${path##*/}" in
            rust.rs) rust=true ;;
            python.py) python=true ;;
            c.c) capi=true ;;
            go.go) go=true ;;
            swift.swift) swift=true ;;
            zig.zig) zig=true ;;
            cpp.cpp) cpp=true ;;
            csharp.cs) csharp=true ;;
            objectivec.m) objc=true ;;
            typescript.ts) ts=true ;;
            *)
                everything
                return 0
                ;;
            esac
            ;;
        site/content/samples/* | site/scripts/compile-samples.mjs)
            everything
            return 0
            ;;
        # The rest of the site: prose, components, the grammars. Read by
        # nothing outside its own build.
        site/*)
            site=true
            ;;
        # Anything else — the workspace, the C ABI, `sdk/api.json`, the
        # workflow itself, a path nobody has classified yet.
        *)
            everything
            return 0
            ;;
        esac
    done

    # The compiled SDKs link against `libotio.a`, so the core library is
    # built whenever one of them runs. TypeScript does not: it compiles the
    # wasm crate itself.
    local library=false
    if [ "$go" = true ] || [ "$swift" = true ] || [ "$zig" = true ] || [ "$cpp" = true ] ||
        [ "$csharp" = true ] || [ "$objc" = true ]; then
        library=true
    fi

    cat <<EOF
any=$any
rust=$rust
python=$python
capi=$capi
library=$library
go=$go
swift=$swift
zig=$zig
cpp=$cpp
csharp=$csharp
objc=$objc
ts=$ts
site=$site
EOF
}

# A case per line: the paths, then `|`, then the outputs that must be true.
# Everything not named must come out false.
self_test() {
    local failures=0
    while IFS='|' read -r paths expected; do
        [ -n "${paths// /}" ] || continue
        case "$paths" in \#*) continue ;; esac

        local got want
        got=$(printf '%s\n' $paths | select_jobs | { grep '=true$' || true; } | cut -d= -f1 | sort | tr '\n' ' ')
        want=$(printf '%s\n' $expected | tr ' ' '\n' | { grep -v '^$' || true; } | sort | tr '\n' ' ')
        if [ "$got" != "$want" ]; then
            echo "select-jobs: $paths"
            echo "  expected: ${want:-nothing}"
            echo "  got:      ${got:-nothing}"
            failures=$((failures + 1))
        fi
    done <<'CASES'
README.md                             |
README.md sdk/README.md               |
docs/adr/0003-sdk-generation.md       |
docs/ci.md LICENSE                    |
crates/otio-capi/README.md            |
sdk/go/otio_test.go                   | any go library
sdk/go/README.md                      | any go library
sdk/cpp/tests/tests.cpp               | any cpp library
sdk/swift/Package.swift               | any swift library
sdk/zig/build.zig                     | any zig library
sdk/csharp/tests/Program.cs           | any csharp library
sdk/objc/tests/tests.m                | any objc library
sdk/objc/Makefile sdk/csharp/README.md | any csharp objc library
sdk/cpp/tests/tests.cpp sdk/go/otio.go| any cpp go library
crates/otio-wasm/ts/src/browser.ts    | any ts
site/content/docs/index.md            | site
site/src/lib/sdk-languages.ts         | site
site/content/docs/index.md README.md  | site
site/content/samples/time-math/go.go  | any go library site
site/content/samples/time-math/rust.rs | any rust site
site/content/samples/read-an-edl/python.py | any python site
site/content/samples/build-a-timeline/c.c | any capi site
site/content/samples/time-math/swift.swift | any swift library site
site/content/samples/time-math/zig.zig | any zig library site
site/content/samples/time-math/cpp.cpp | any cpp library site
site/content/samples/time-math/csharp.cs | any csharp library site
site/content/samples/time-math/objectivec.m | any objc library site
site/content/samples/time-math/typescript.ts | any ts site
site/content/samples/a/go.go site/content/samples/b/zig.zig | any go zig library site
site/content/samples/time-math/kotlin.kt | any rust python capi library go swift zig cpp csharp objc ts site
site/content/samples/stray.md          | any rust python capi library go swift zig cpp csharp objc ts site
site/scripts/compile-samples.mjs      | any rust python capi library go swift zig cpp csharp objc ts site
site/package.json sdk/go/otio.go      | any go library site
crates/otio-wasm/ts/package.json README.md | any ts
crates/otio-capi/src/lib.rs           | any rust python capi library go swift zig cpp csharp objc ts site
crates/otio-core/src/lib.rs           | any rust python capi library go swift zig cpp csharp objc ts site
Cargo.toml                            | any rust python capi library go swift zig cpp csharp objc ts site
sdk/api.json                          | any rust python capi library go swift zig cpp csharp objc ts site
.github/workflows/ci.yml              | any rust python capi library go swift zig cpp csharp objc ts site
.github/ci/select-jobs.sh             | any rust python capi library go swift zig cpp csharp objc ts site
docs/ci.md crates/otio-capi/src/lib.rs| any rust python capi library go swift zig cpp csharp objc ts site
some-unclassified-directory/thing.txt | any rust python capi library go swift zig cpp csharp objc ts site
sdk/go/otio.go crates/opentime/src/lib.rs | any rust python capi library go swift zig cpp csharp objc ts site
CASES

    if [ "$failures" -gt 0 ]; then
        echo "select-jobs: $failures case(s) do not hold"
        return 1
    fi
    echo "select-jobs: every case holds"
}

case "${1:-}" in
--all) everything ;;
--self-test) self_test ;;
"") select_jobs ;;
*)
    echo "select-jobs.sh [--all | --self-test]" >&2
    exit 2
    ;;
esac
