# The documentation site

The site at [`site/`](.) documents OpenTimelineIO itself and this port of it,
and shows every code sample in each language an SDK exists for, with a
switcher along the top of the sample.

```sh
cd site
npm install
npm run dev        # http://localhost:3000
npm run verify     # what CI runs: samples, types, lint, tests, build
```

## What it is built out of

| Piece | Version | What it does here |
| --- | --- | --- |
| Next.js | 16 | The App Router, in `output: 'export'` mode — every page is static |
| React | 19 | |
| Tailwind CSS | 4 | The design tokens live in `src/app/globals.css` |
| [Base UI](https://base-ui.com) | 1 | The unstyled primitives: tabs, `useRender` |
| [`@tanstack/markdown`](https://github.com/TanStack/markdown) | 0.0.x | Parses `content/docs/*.md` |
| [`@tanstack/highlight`](https://github.com/TanStack/highlight) | 0.1 | Highlights every fence and every sample, at build time |

Components under `src/components/ui` follow the shadcn/ui convention — a
`cva` recipe, variants as props, the source in this repository rather than in
a dependency — over Base UI primitives rather than Radix. That convention is
what lets a component from the shadcn or [ReUI](https://reui.io) registries
be dropped in beside them: `npx shadcn@latest add <url>` writes into the same
directory and uses the same tokens. Neither registry is reachable from CI's
network, so nothing here is fetched at build time.

## Where the content comes from

Three sources, and they are kept honest in different ways.

**The reference section is generated.** `/reference` reads
[`sdk/api.json`](../sdk/api.json) — the description of the C ABI that
`otio-sdk-model` writes out of the ABI's own Rust source, and that every SDK
is generated from. Each C declaration on those pages is lifted verbatim from
the committed [`otio.h`](../crates/otio-capi/include/otio.h) rather than
reassembled from parts. A page here cannot describe a call the library does
not have.

**The prose is written by hand**, in `content/docs/*.md`, because it is
explanation rather than interface. Frontmatter gives each page its title,
its sidebar group and its order.

**The samples are files**, one directory per sample under `content/samples/`,
one file per language:

```text
content/samples/read-an-edl/
├── rust.rs
├── python.unavailable    ← a note saying what is missing and why
├── typescript.ts
├── go.go
└── …
```

A page drops one in with a comment, which keeps the file valid Markdown that
GitHub still renders:

```md
<!-- ::sample id="read-an-edl" -->
```

`npm run check:samples` fails the build when a sample is missing a language
the switcher offers. A language that genuinely cannot do a thing yet answers
with a `.unavailable` file whose text becomes the tab's content — a reader
who came for Python should learn that the EDL adapter is not bound yet, not
silently get Go.

That check proves a file is there. What proves it is still true is
`scripts/compile-samples.mjs`, which compiles every sample with its own
language's real toolchain, against the SDK in this checkout:

```sh
node scripts/compile-samples.mjs rust go c cpp swift zig typescript python
```

Samples are the one kind of content here that a generator does not write, so
they are the one kind that can quietly stop being true: a backend changes
shape, every SDK is regenerated, and the sample still describes the old API.
Compiling it is the only thing that notices. Each language runs in the CI job
that already has its toolchain and its freshly built `libotio` — the Go
samples in the Go SDK job, the C ones in the C ABI job — so a change that
breaks a sample fails the same build that made the change, rather than
reaching a reader.

Python is run rather than compiled, because compiling a Python file proves
only that it parses, and the drift worth catching is a call that no longer
exists. TypeScript is typechecked against the built `@otio/otio` package
itself, resolved through `node_modules` the way a reader would install it,
rather than through a path alias.

## Languages the highlighter did not ship

`@tanstack/highlight` 0.1 ships thirty grammars, and Rust, Swift, Zig, C, C#
and Objective-C are not among them — which is most of what an OTIO SDK is
written in. They are defined here in `src/lib/languages/`, on a single-pass
scanner shared between them, and tested in `src/lib/languages/languages.test.ts`
to the bar the library sets for its own: valid-code fixtures, exact source
reconstruction, and a regression for each context-sensitive thing a grammar
gets wrong first.

Adding a language to the switcher is an entry in `src/lib/sdk-languages.ts`
plus a file per sample. If the highlighter has no grammar for it, one more
definition in `src/lib/languages/`.

## Deploying

The build writes a static `out/` directory with no server behind it, so it
can be hosted anywhere. Point a static host at `out/`; `trailingSlash` is on,
so every route is a real `index.html` and needs no rewrite rules.

[`vercel.json`](vercel.json) is set up for Vercel: import the repository, set
the root directory to `site`, and the rest is in that file.

It deliberately does **not** set `outputDirectory`. Vercel's Next.js builder
reads its own build directory — `.next`, where `routes-manifest.json` lives —
and serves the exported files from there; `output: 'export'` is something it
detects rather than something it is told. Pointing `outputDirectory` at `out`
instead makes the builder look for that manifest in the export, where it has
no reason to be, and the deployment fails with

```text
Error: The file "/vercel/path0/site/out/routes-manifest.json" couldn't be found.
```

If a deployment fails that way, check the project's own Output Directory
setting too: a value there does the same thing as the one in this file.
