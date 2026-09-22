# Releasing

What this repository publishes, and how a release goes out. Today that is one
package: the TypeScript SDK, published to npm as
[`@alchemist-edit/otio`](https://www.npmjs.com/package/@alchemist-edit/otio)
under the `alchemist-edit` npm organization.

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

No npm token is stored anywhere once the package exists. The workflow uses
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

Otherwise, bootstrap it with a token that lives for a day:

1. npmjs.com → your avatar → **Access Tokens → Generate New Token → Granular
   Access Token**. Packages and scopes: **Read and write**, limited to the
   `@alchemist-edit` scope. Expiry: the shortest offered. Tick *Bypass
   two-factor authentication* if your account requires 2FA to publish,
   because a workflow cannot answer a 2FA prompt.
2. GitHub → **Settings → Environments → npm → Environment secrets → Add
   secret**: name `NPM_TOKEN`, value the token.
3. Cut the release as below. The workflow uses the token only because there
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
authentication and disallow tokens*, delete the `NPM_TOKEN` secret from the
environment, and revoke the token on npmjs.com. From then on the workflow is
the only way a version reaches npm.

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
