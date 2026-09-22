# Continuous integration

One workflow, [`.github/workflows/ci.yml`](../.github/workflows/ci.yml), on
every pull request and every push to `main`. It is staged rather than flat:
a cheap gate first, then the core library, then the fan-out that depends on
it.

```
changes ──▶ lint ──┬──▶ library (ubuntu, macos) ──┬──▶ Go SDK
                   │                              ├──▶ Swift SDK
                   ├──▶ generated ────────────────┼──▶ Zig SDK
                   │                              └──▶ C++ SDK
                   ├──▶ test (ubuntu, macos, windows)
                   ├──▶ C ABI (ubuntu, macos)
                   ├──▶ Python bindings (ubuntu, macos, windows)
                   ├──▶ Minimum supported Rust version
                   └──▶ TypeScript SDK
                                                   ──▶ ci
```

| Stage | Job | What it proves |
| --- | --- | --- |
| Select | `changes` | Which of the rest are needed at all. |
| Gate | `lint` | `cargo fmt`, `cargo clippy -D warnings`, rustdoc. |
| Core | `library` | `libotio.a` builds, and is published as an artifact. |
| Core | `generated` | Every generated SDK is what the C ABI says it should be. |
| Fan-out | `test`, `c-abi`, `python`, `msrv` | The Rust workspace, on its three platforms. |
| Fan-out | `go`, `swift`, `zig`, `cpp` | Each SDK's own toolchain, against the artifact. |
| Fan-out | `typescript` | The wasm package, in Node and in Chromium. |
| Gate | `ci` | Everything that ran, passed. |

## Why it is staged

`lint` is a minute of one Linux runner and it fails for the reasons that
would fail every other job too — a formatting slip, a clippy denial, a
rustdoc warning. Putting it in front means those cost one job rather than
nineteen.

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
a pull request changed and decides. It has one rule worth stating plainly:

> A path it does not recognise runs everything.

Only these are ever narrowed:

| Changed path | What runs |
| --- | --- |
| `README.md`, `LICENSE`, `docs/**`, `sdk/README.md`, `.github/*.md`, `crates/*/README.md` | Nothing. |
| `sdk/go/**` and the like | The core library, the drift check, and that one SDK. |
| `crates/otio-wasm/ts/**` | The drift check and the TypeScript SDK. |
| `site/**` | The documentation site. |
| `site/content/samples/**`, `site/scripts/compile-samples.mjs` | Everything. |
| Anything else | Everything. |

A push to `main` always runs everything, and so does a pull request whose
diff cannot be read.

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

The site's two rows are the same distinction as `sdk/<language>/README.md`
below, from the other side. Its prose and its components are read by nothing
but its own build. Its *samples* are compiled by each SDK's own job, against
that SDK — which is what stops a documentation page describing a call the
library stopped having — so a change to one has to reach every job that
might compile it. Which language a sample belongs to is written on its
filename, so that row could be narrowed per language; it is not, because
samples change rarely and one that runs a job too many costs less than one
that runs a job too few.

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

- **Build before you test, as two commands.** The C ABI test links whatever
  `libotio` is on disk when it runs, and cargo does not order that against
  building it, so a single `cargo test` can test the previous library. The
  `test` and `c-abi` jobs both keep the build as its own step.
- **A red run tells you nothing if no runner started.** Jobs that die within
  seconds with no runner name and logs that 404 are runner allocation, not a
  test result.
