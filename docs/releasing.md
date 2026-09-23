# Releasing

What this repository publishes, and how a release goes out. There are two
releases, independent of each other:

- **The core library and the SDKs over its C ABI**, as a GitHub release: a
  `v<version>` tag, [`release-sdks.yml`](../.github/workflows/release-sdks.yml).
  See [The library and the SDKs](#the-library-and-the-sdks) below.
- **The TypeScript SDK**, published to npm as
  [`@alchemist-edit/otio`](https://www.npmjs.com/package/@alchemist-edit/otio)
  under the `alchemist-edit` npm organization: an `npm-v<version>` tag,
  `release-npm.yml`. Everything from here to that section is about this one.

```
tag npm-v0.2.0 on main
        │
        ▼
  build ─────────────────────────────────────────▶ publish  (environment: npm)
  version matches the tag, commit is on main       waits for approval, if the
  typecheck · build · test in Node                 environment requires it
  check:pack: tarball installed somewhere empty    uploads that same tarball,
  npm pack ──▶ artifact ─────────────────────────▶ OIDC, with provenance
```

| File | What it is |
| --- | --- |
| [`.github/workflows/release-npm.yml`](../.github/workflows/release-npm.yml) | The release: `build` makes and tests the tarball, `publish` uploads it |
| [`crates/otio-wasm/ts/package.json`](../crates/otio-wasm/ts/package.json) | The name, the version, and what goes in the tarball (`files`) |
| [`crates/otio-wasm/ts/README.md`](../crates/otio-wasm/ts/README.md) | The README npm shows on the package page |
| [`crates/otio-wasm/ts/scripts/prepack.mjs`](../crates/otio-wasm/ts/scripts/prepack.mjs) | Refuses to pack an unbuilt package, and brings in the root `LICENSE` |
| [`crates/otio-wasm/ts/scripts/check-pack.mjs`](../crates/otio-wasm/ts/scripts/check-pack.mjs) | Packs, installs into an empty project, and runs it there |

## How a release is authenticated

No npm token is needed once the package exists. The workflow uses
npm's **trusted publishing**: npmjs.com is told to trust one workflow file in
one repository, and the `publish` job proves it is that workflow with a
short-lived OpenID Connect token GitHub issues it. The same token signs a
**provenance** statement, so the npm page shows which commit and which
workflow run built each version, and anyone can check it with
`npm audit signatures`.

What was tested is what is published: `build` packs the tarball once, and
`publish` downloads that artifact and uploads it byte for byte. Nothing is
rebuilt in the job that holds the credentials.

## One-time setup

These need the owner's accounts, on npmjs.com and on GitHub.

### 1. The `npm` environment on GitHub

Repository **Settings → Environments → New environment**, named `npm`:

- **Required reviewers**: add yourself. Every release then waits in the
  Actions tab for an approval, whoever pushed the tag.
- **Deployment branches and tags**: *Selected branches and tags*, with a tag
  rule `npm-v*`.

Without this the environment is created on the first run with no protection,
which works, but means anyone who can push a tag can publish.

### 2. The first version

npm attaches a trusted publisher to a package that already exists, so the
very first version may have to go out another way. If the package's
settings page lets you add a trusted publisher before anything is published,
skip to step 3.

Otherwise, the workflow bootstraps it with a token. It reads
`NPM_PUBLISHING_TOKEN`, an organization secret on `alchemist-editor` that this
repository has access to (added 2026-09-23):

1. The token is an npm **granular access token** with **Read and write** on
   the `@alchemist-edit` scope. If your account requires 2FA to publish, it
   needs *Bypass two-factor authentication*, because a workflow cannot answer
   a 2FA prompt.
2. Cut the release as below. The workflow uses the token only because there
   is no trusted publisher yet.

### 3. The trusted publisher

npmjs.com → the `@alchemist-edit/otio` package → **Settings → Trusted
Publisher → GitHub Actions**:

| Field | Value |
| --- | --- |
| Organization or user | `alchemist-editor` |
| Repository | `otio-rust` |
| Workflow filename | `release-npm.yml` |
| Environment name | `npm` |

Then, on the same page, set **Publishing access** to *Require two-factor
authentication and disallow tokens*. Then either revoke the token on npmjs.com
and delete `NPM_PUBLISHING_TOKEN`, or, if other repositories publish with it,
remove `otio-rust` from the secret's repository access. From then on the
workflow is the only way a version of this package reaches npm.

## Cutting a release

1. Change the version in a pull request, from `crates/otio-wasm/ts`:

   ```sh
   npm version 0.2.0 --no-git-tag-version   # package.json and the lockfile
   ```

   Versions are the package's own, independent of the crates' versions, and
   follow semver; while it is `0.x`, a minor bump is the one that may break
   callers.

2. Merge it. The workflow refuses a tag on a commit that is not on `main`,
   so everything released has been through CI and review.

3. Tag the merge commit and push the tag:

   ```sh
   git fetch origin main
   git tag npm-v0.2.0 origin/main
   git push origin npm-v0.2.0
   ```

   The tag has to be `npm-v` followed by exactly the version in
   `package.json`, or the release stops before building anything.

4. Approve the `publish` job when the Actions tab asks.

A version with a pre-release part, such as `0.2.0-rc.1`, is published under
the `next` dist-tag, so a plain `npm install` never picks up a release
candidate; everything else goes to `latest`.

### Trying it without publishing

**Actions → Publish to npm → Run workflow** runs `build` alone: the same
checks, the same tarball, uploaded as an artifact you can download and
inspect, and no publish. Locally, `npm run check:pack` after `npm run build`
is the same tarball check.

### When a release is wrong

npm allows `npm unpublish` only within 72 hours and only when nothing depends
on the version, and a version number can never be used again even then. The
usual fix is a new patch version; to steer people off a bad one meanwhile,
`npm deprecate @alchemist-edit/otio@0.2.0 "use 0.2.1"`.

## The library and the SDKs

```
tag v0.2.0 on main
        │
        ▼
  plan ──▶ library ×7 ──────────▶ package ─────────▶ verify-zig ×6 ──▶ publish (environment: release)
  version matches  one runner per   the release       a program that      gh release create,
  the tag, commit  target: build,   assets, and       depends on the      the verified assets
  is on main       strip MSVC,      SHA256SUMS        Zig package builds  and nothing else
                   Zig SDK tests                      and runs
```

| File | What it is |
| --- | --- |
| [`.github/workflows/release-sdks.yml`](../.github/workflows/release-sdks.yml) | The release |
| [`scripts/package-release.sh`](../scripts/package-release.sh) | Turns the per-target libraries into the assets below |
| [`scripts/strip-msvc-builtins.sh`](../scripts/strip-msvc-builtins.sh) | Removes Rust's compiler-rt from MSVC static libraries, which Zig cannot link otherwise |
| [`.github/ci/zig-package-smoke`](../.github/ci/zig-package-smoke) | The program `verify-zig` builds against the unpacked Zig package |

### What a release contains

| Asset | What it is |
| --- | --- |
| `libotio-<version>-<target>.tar.gz` | `include/otio.h`, `lib/` (static, and shared where built) and the licence, for one target |
| `otio-zig-<version>.tar.gz` | The Zig package, with every target's static library under `lib/<target>/`; `zig fetch --save <url>` is all a user needs |
| `otio-<sdk>-<version>.tar.gz` | The Go, Swift, C++, C# and Objective-C SDKs' sources, laid out as in this repository, with an empty `lib/` for the library |
| `SHA256SUMS` | A checksum for each of the above |

The targets are `x86_64-linux-gnu`, `aarch64-linux-gnu`, `aarch64-macos`,
`x86_64-macos`, `x86_64-windows-msvc`, `aarch64-windows-msvc` and
`x86_64-windows-gnu` (built for `gnullvm`, whose unwinder is the one Zig
ships; static only). The target names are the Zig package's directory names.

Every library but `x86_64-macos` (cross-built on Apple silicon) and
`x86_64-windows-gnu` (static only) is built on a runner of its own platform,
and the Zig SDK's own tests run against it there. `verify-zig` then checks
the package as a user gets it, on every target a runner can run, the MinGW
one included. Windows MSVC libraries use the static C runtime (the one Zig
links) and are stripped of Rust's compiler-rt; the script says why.

### One-time setup

Repository **Settings → Environments → New environment**, named `release`,
with **Required reviewers**. Without it the environment is created on the
first run with no protection, and anyone who can push a `v*` tag can publish.

### Cutting a release

1. Change `version` under `[workspace.package]` in `Cargo.toml` in a pull
   request, and run `cargo run -p otio-sdk-gen` so the generated SDKs
   (`sdk/zig/build.zig.zon` among them) say the same.
2. Merge it.
3. Tag the merge commit and push the tag:

   ```sh
   git fetch origin main
   git tag v0.2.0 origin/main
   git push origin v0.2.0
   ```

   The tag has to be `v` followed by exactly the workspace version, and on
   `main`, or the release stops before building anything.
4. Approve the `publish` job when the Actions tab asks.

A version with a pre-release part, such as `0.2.0-rc.1`, is marked a
pre-release on GitHub.

### Trying it without publishing

**Actions → Release SDKs → Run workflow** runs everything but `publish` and
leaves the assets on the run as the `release` artifact. A pull request that
changes the workflow, the packaging script, the strip script or the smoke
program runs the same dry run on its own.
