# Continuous integration

One workflow, [`.github/workflows/ci.yml`](../.github/workflows/ci.yml), on
every pull request and every push to `main`. It is staged rather than flat:
a cheap gate first, then the core library, then the fan-out that depends on
it.

```
changes ──▶ lint ──┬──▶ library (ubuntu, macos) ──┬──▶ Go SDK
                   │                              ├──▶ Swift SDK
                   │                              ├──▶ Zig SDK
                   ├──▶ generated ────────────────┼──▶ C++ SDK
                   │                              ├──▶ C# SDK
                   │                              └──▶ Objective-C SDK
                   ├──▶ test (ubuntu, macos, windows)
                   ├──▶ C ABI (ubuntu, macos)
                   ├──▶ Python bindings (ubuntu, macos, windows)
                   ├──▶ Minimum supported Rust version
                   ├──▶ TypeScript SDK
                   └──▶ Documentation site
                                                   ──▶ ci
```

| Stage | Job | What it proves |
| --- | --- | --- |
| Select | `changes` | Which of the rest are needed at all. |
| Gate | `lint` | `cargo fmt`, `cargo clippy -D warnings`, rustdoc. |
| Core | `library` | `libotio` builds, static and shared, and is published as an artifact. |
| Core | `generated` | Every generated SDK is what the C ABI says it should be. |
| Fan-out | `test`, `c-abi`, `python`, `msrv` | The Rust workspace, on its three platforms. |
| Fan-out | `go`, `swift`, `zig`, `cpp`, `csharp`, `objc` | Each SDK's own toolchain, against the artifact. |
| Fan-out | `typescript` | The wasm package, in Node and in Chromium. |
| Fan-out | `site` | The documentation site: sample check, types, lint, tests, build. |
| Gate | `ci` | Everything that ran, passed. |

## Why it is staged

`lint` is a minute of one Linux runner and it fails for the reasons that
would fail every other job too — a formatting slip, a clippy denial, a
rustdoc warning. Putting it in front means those cost one job rather than
nineteen.

`library` publishes two artifacts rather than one. Everything that *links*
the core takes `libotio.a`; C# loads it by name at run time and takes the
shared build instead. They are separate artifacts because a `.so` sitting
beside the `.a` would change what `-lotio` resolves to for everyone else.

`library` exists because four jobs used to run `cargo build -p otio-capi
--release` themselves, eight identical builds across two platforms. It now
runs once per platform and hands `libotio.a` to the SDK jobs as an artifact,
which is also why those jobs install no Rust toolchain at all: a checkout,
their own language, and the library is everything they need.
`crates/otio-capi/include/otio.h` is committed, so it comes with the
checkout. The repository is public, so none of this is a billing question —
what it buys is runner concurrency, and the macOS runners are the ones that
queue. In the last full run before this change, the macOS Zig and C ABI jobs
each waited three quarters of a minute for a runner, while their Linux twins
had already finished.

`generated` is the drift check — `cargo run -p otio-sdk-gen -- --check`.
The SDK jobs are downstream of it because testing a generated package that
no longer matches the C ABI tells you nothing useful.

The TypeScript SDK hangs off `lint` rather than `library`: it compiles the
wasm crate itself, so the static library is no use to it.

The cost of staging is wall-clock — the fan-out starts roughly a minute in
rather than immediately, which was about 40 seconds on the first full run of
it, measured against the last comparable run before. The saving is
everything downstream of a failure, and the runner time four jobs no longer
spend rebuilding the same library.

## What runs, and what does not

[`.github/ci/select-jobs.sh`](../.github/ci/select-jobs.sh) reads the paths
a pull request or a push to `main` changed and decides. It has one rule
worth stating plainly:

> A path it does not recognise runs everything.

Only these are ever narrowed:

| Changed path | What runs |
| --- | --- |
| `README.md`, `LICENSE`, `docs/**`, `sdk/README.md`, `.github/*.md`, `crates/*/README.md` | Nothing. |
| `sdk/go/**` and the like — `swift`, `zig`, `cpp`, `csharp`, `objc` | The core library, the drift check, and that one SDK. |
| `crates/otio-wasm/ts/**` | The drift check and the TypeScript SDK. |
| `site/**` | The documentation site. |
| `site/content/samples/<id>/<language>.<ext>` | The drift check, the documentation site, and that language's job: `rust.rs` the Rust tests and MSRV, `python.py` the Python bindings, `c.c` the C ABI, `typescript.ts` the TypeScript SDK, and the other SDKs as above. |
| `site/scripts/compile-samples.mjs`, any other file under `site/content/samples/` | Everything. |
| Anything else | Everything. |

A pull request or a push whose diff cannot be read runs everything.

### A push to `main` is filtered too

A push to `main` reads what that push changed, from the commit `main` was on
before it (`github.event.before`) to the new one. It used to run everything
unconditionally, which is where most of the cost went: every merged pull
request had already been checked on its branch, and then paid for the whole
thirty-job matrix again on `main` even when it touched only a README or the
site. A merge that reaches the workspace, the C ABI or an SDK still runs
everything there, so `main` keeps its own verdict on every change that can
break it.

The fallbacks are the same fail-safe rule. A push that creates the branch has
no previous commit, and a force-push can leave one this clone never fetched;
both run everything.

### It is the pull request's whole diff, not the last commit

On a pull request the selector reads every path the branch changes against
its merge base, not the paths of the most recent push. A documentation
commit on top of a change under `crates/` therefore still runs everything;
only a pull request whose *entire* diff is prose skips anything.

That is the behaviour to want, because CI has to have a verdict on what
would be merged. Reading only the last commit would let a branch that
changed the C ABI in one commit and a README in the next skip the tests that
matter. It is also the answer to why a one-line docs fix, pushed to a
long-running branch, ran the Windows matrix again.

The site's rows are the same distinction as `sdk/<language>/README.md`
below, from the other side. Its prose and its components are read by nothing
but its own build. Its *samples* are compiled by each language's own job,
against that SDK — which is what stops a documentation page describing a
call the library stopped having — so a change to one has to reach the job
that compiles it. The language is written on the filename, so the selector
reads it there and runs that one job. A sample file named for a language it
does not know, or a change to the harness every job shares, runs everything.

Note what is *not* in the prose row. `sdk/<language>/README.md` is generated
from the C ABI's doc comments, so a hand edit to one has to reach the drift
check, and it does: it falls into that language's bucket instead. The same
goes for `sdk/api.json`, which is generated and is not a documentation file
whatever its neighbours look like.

### Adding a path

A new directory — another SDK target, a scripts folder — costs a full run
until it is classified, which is the safe way round. To narrow it, add a `case` arm to `select-jobs.sh` and a line to the
table at the bottom of that script; `./.github/ci/select-jobs.sh
--self-test` checks the table, and the `changes` job runs it on every CI
run, so a filter that stops agreeing with its own examples fails CI rather
than quietly skipping a job.

## Branch protection

Require **one** check: `CI complete` (the `ci` job).

Requiring the individual jobs would not survive the path filtering. A
required check that is skipped is never reported at all, so a pull request
waiting on `Test (windows-latest)` could never merge the moment somebody
fixed a typo in the README. The `ci` job is reported on every run whatever
ran, and passes only if nothing that ran failed; a skipped job is a pass
there, because a job that was not needed is not a result.

## Things that have bitten us

- **A skipped gate skips everything behind it.** A job whose `needs` include
  a skipped job is itself skipped unless its `if` says otherwise, because
  GitHub puts an implicit `success()` in front of every condition. `lint` is
  skipped for a change that reaches only the site, so until #61 the `site`
  job never ran for a site-only pull request, and `CI complete` passed with
  nothing checked. A job that can run when its gate did not has to say
  `!cancelled()` and check the gate's result itself, as `site` now does.

- **Build before you test, as two commands.** The C ABI test links whatever
  `libotio` is on disk when it runs, and cargo does not order that against
  building it, so a single `cargo test` can test the previous library. The
  `test` and `c-abi` jobs both keep the build as its own step.
- **A red run tells you nothing if no runner started.** Jobs that die within
  seconds with no runner name and logs that 404 are runner allocation, not a
  test result.
- **Every job stops after 30 minutes.** GitHub's own limit is six hours, so
  without `timeout-minutes` a hung test holds a runner that long. Each job
  here finishes in a few minutes; 30 leaves room for a cold cache.
- **A test that is quick on Linux can be slow on Windows.** Creating files is
  far slower there, so a test that wrote and extracted 65,536 files to check
  the zip64 entry count ran for over five minutes on the Windows runner and
  about a second elsewhere. It now builds the archive in memory. Keep tests
  that need many entries off the file system.
