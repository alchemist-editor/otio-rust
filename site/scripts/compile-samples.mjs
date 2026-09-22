#!/usr/bin/env node
// Compiles every documentation sample against the SDK in this checkout.
//
// The samples under content/samples are hand-written, because the commentary
// in them is the point and no generator would write it. That makes them the
// one kind of content on this site that can quietly stop being true: a
// backend changes shape, every SDK is regenerated, and the sample still sits
// there describing the old API. check-samples.mjs only proves a file exists.
//
// So each one is compiled by its own language's real toolchain, against the
// real SDK, the same way a reader would. A sample that no longer compiles
// fails the build that changed the API.
//
// Usage: node scripts/compile-samples.mjs <language> [...]
//
// Each language runs inside the CI job that already has its toolchain and a
// freshly built libotio, so this script assumes both and reports plainly when
// one is missing rather than skipping.

import { execFileSync } from 'node:child_process'
import { mkdirSync, readdirSync, rmSync, writeFileSync, copyFileSync, existsSync, symlinkSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const SITE = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const REPO = resolve(SITE, '..')
const SAMPLES = join(SITE, 'content', 'samples')
const SCRATCH = join(SITE, '.samples-build')

/**
 * Sample ids that have a file for this language, in a stable order. A sample
 * file is named after the language rather than the extension —
 * `typescript.ts` — so both are needed to find one.
 */
function samplesFor(language, extension) {
  return readdirSync(SAMPLES, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort()
    .map((id) => ({ id, file: join(SAMPLES, id, `${language}.${extension}`) }))
    .filter((sample) => existsSync(sample.file))
}

/** A sample id as a language identifier: `build-a-timeline` is not one. */
function identifier(id) {
  return id.replace(/[^A-Za-z0-9]/g, '_')
}

/** A fresh scratch directory for one language. */
function scratchFor(language) {
  const directory = join(SCRATCH, language)
  rmSync(directory, { recursive: true, force: true })
  mkdirSync(directory, { recursive: true })
  return directory
}

function run(command, args, options = {}) {
  execFileSync(command, args, { stdio: 'inherit', ...options })
}

/**
 * The static library every non-wasm SDK links against.
 *
 * CI builds it once and hands each SDK job the artifact, unpacked into that
 * SDK's own `lib/` — so a harness names its own first. Falling back to
 * `target/` is for a local checkout, and for the C ABI job, which builds
 * debug and whose samples link against the same library it proved.
 */
function requireLibotio(...preferred) {
  const candidates = [
    ...preferred,
    join(REPO, 'target', 'release'),
    join(REPO, 'target', 'debug'),
  ]
  for (const directory of candidates) {
    const library = join(directory, 'libotio.a')
    if (existsSync(library)) return library
  }
  throw new Error(
    `No libotio.a in any of:\n${candidates.map((d) => `    ${d}`).join('\n')}\n\n` +
    `Build it first:\n\n    cargo build -p otio-capi --release\n`,
  )
}

/** Where CI unpacks the library for one SDK's job. */
function sdkLib(name) {
  return join(REPO, 'sdk', name, 'lib')
}

const linkFlags = process.platform === 'darwin'
  ? ['-framework', 'CoreFoundation', '-framework', 'Security', '-liconv']
  : ['-lpthread', '-ldl', '-lm']

const harnesses = {
  // ---- C: the header the C ABI ships, and the library itself. -------------
  c: {
    extension: 'c',
    build(samples, scratch) {
      const library = requireLibotio()
      for (const sample of samples) {
        run(process.env.CC ?? 'cc', [
          '-std=c11', '-Wall', '-Wextra', '-Werror',
          '-I', join(REPO, 'crates', 'otio-capi', 'include'),
          sample.file,
          library,
          ...linkFlags,
          '-o', join(scratch, sample.id),
        ])
      }
    },
  },

  // ---- C++: header-only over the same library. ---------------------------
  cpp: {
    extension: 'cpp',
    build(samples, scratch) {
      const library = requireLibotio(sdkLib('cpp'))
      for (const sample of samples) {
        run(process.env.CXX ?? 'c++', [
          '-std=c++17', '-Wall', '-Wextra', '-Werror',
          '-I', join(REPO, 'sdk', 'cpp', 'include'),
          '-I', join(REPO, 'crates', 'otio-capi', 'include'),
          sample.file,
          library,
          ...linkFlags,
          '-o', join(scratch, sample.id),
        ])
      }
    },
  },

  // ---- Rust: a throwaway crate with one binary per sample. ---------------
  // `[workspace]` detaches it from the workspace it sits inside, so this
  // never joins `cargo build --workspace` by accident.
  rust: {
    extension: 'rs',
    build(samples, scratch) {
      const crates = ['opentime', 'otio-core', 'otio-adapter', 'otio-cmx3600']
      const manifest = [
        '[workspace]',
        '',
        '[package]',
        'name = "samples"',
        'version = "0.0.0"',
        'edition = "2021"',
        'publish = false',
        '',
        '[dependencies]',
        ...crates.map((name) => `${name} = { path = ${JSON.stringify(join(REPO, 'crates', name))} }`),
        '',
        ...samples.flatMap((sample) => [
          '[[bin]]',
          `name = ${JSON.stringify(sample.id)}`,
          `path = ${JSON.stringify(sample.file)}`,
          '',
        ]),
      ].join('\n')
      writeFileSync(join(scratch, 'Cargo.toml'), manifest)
      run('cargo', ['build', '--manifest-path', join(scratch, 'Cargo.toml')])
    },
  },

  // ---- Go: a throwaway module pointing at the SDK in this checkout. ------
  go: {
    extension: 'go',
    build(samples, scratch) {
      requireLibotio(sdkLib('go'))
      const name = 'github.com/alchemist-editor/otio-rust/site/samples'
      const sdk = 'github.com/alchemist-editor/otio-rust/sdk/go'
      writeFileSync(join(scratch, 'go.mod'), [
        `module ${name}`,
        '',
        'go 1.21',
        '',
        `require ${sdk} v0.0.0`,
        '',
        `replace ${sdk} => ${join(REPO, 'sdk', 'go')}`,
        '',
      ].join('\n'))
      // `go build` wants each `package main` in a directory of its own.
      for (const sample of samples) {
        const directory = join(scratch, sample.id)
        mkdirSync(directory, { recursive: true })
        copyFileSync(sample.file, join(directory, 'main.go'))
      }
      run('go', ['build', './...'], { cwd: scratch, env: { ...process.env, CGO_ENABLED: '1' } })
    },
  },

  // ---- Swift: a throwaway package with one executable per sample. --------
  // A sample is top-level code, which Swift only allows in `main.swift`. A
  // target name becomes a module name, so it has to be an identifier — the
  // sample ids have hyphens in them.
  swift: {
    extension: 'swift',
    build(samples, scratch) {
      const library = requireLibotio(sdkLib('swift'))
      // In its own directory, because SwiftPM takes a path dependency's
      // identity from its directory name: a package sitting in
      // `.samples-build/swift` and one at `sdk/swift` are both `swift`, and
      // the resolver calls that a cycle rather than a collision.
      const root = join(scratch, 'Samples')
      mkdirSync(root, { recursive: true })
      const targets = samples.map((sample) => ({ ...sample, target: identifier(sample.id) }))
      for (const sample of targets) {
        const directory = join(root, 'Sources', sample.target)
        mkdirSync(directory, { recursive: true })
        copyFileSync(sample.file, join(directory, 'main.swift'))
      }
      writeFileSync(join(root, 'Package.swift'), [
        '// swift-tools-version:5.9',
        'import PackageDescription',
        '',
        'let package = Package(',
        '    name: "Samples",',
        '    dependencies: [',
        // A path dependency's identity is its directory name, which is what
        // `.product(package:)` below has to name.
        `        .package(path: ${JSON.stringify(join(REPO, 'sdk', 'swift'))})`,
        '    ],',
        '    targets: [',
        targets.map((sample) => [
          '        .executableTarget(',
          `            name: ${JSON.stringify(sample.target)},`,
          '            dependencies: [.product(name: "OpenTimelineIO", package: "swift")]',
          '        )',
        ].join('\n')).join(',\n'),
        '    ]',
        ')',
        '',
      ].join('\n'))
      run('swift', ['build', '--package-path', root,
        '-Xlinker', `-L${dirname(library)}`])
    },
  },

  // ---- Zig: one `build-exe` per sample. ----------------------------------
  // Not `zig build`: that wants a package manifest for the samples
  // themselves, and there is no package here — just three files that have to
  // compile against the SDK's own `otio` module.
  zig: {
    extension: 'zig',
    build(samples, scratch) {
      const library = requireLibotio(sdkLib('zig'))
      const root = join(REPO, 'sdk', 'zig', 'src', 'root.zig')
      // What the SDK's own build.zig links, for the same reason: a Rust panic
      // unwinds, and the unwinder is not in libc.
      const platform = process.platform === 'darwin'
        ? ['-framework', 'CoreFoundation', '-framework', 'Security', '-liconv']
        : ['-lunwind']
      for (const sample of samples) {
        run('zig', [
          'build-exe',
          '--dep', 'otio',
          `-Mroot=${sample.file}`,
          `-Motio=${root}`,
          '-lc',
          '-L', dirname(library), '-lotio',
          ...platform,
          `-femit-bin=${join(scratch, sample.id)}`,
        ], { cwd: scratch })
      }
    },
  },

  // ---- TypeScript: typecheck against the package the wasm crate ships. ---
  // Resolved through `node_modules`, not a `paths` alias, because the package
  // reaches its types through conditional `exports` — a sample importing it
  // any other way would not be typechecking what a reader installs.
  typescript: {
    extension: 'ts',
    build(samples, scratch) {
      const sdk = join(REPO, 'crates', 'otio-wasm', 'ts')
      for (const [what, path] of [['built', join(sdk, 'dist')], ['installed', join(sdk, 'node_modules')]]) {
        if (!existsSync(path)) {
          throw new Error(
            `${path} is missing, so the package is not ${what}. Build it first:\n\n` +
            `    cd crates/otio-wasm/ts && npm install && npm run build\n`,
          )
        }
      }

      mkdirSync(join(scratch, 'node_modules', '@otio'), { recursive: true })
      symlinkSync(sdk, join(scratch, 'node_modules', '@otio', 'otio'), 'dir')
      // The samples use `node:fs` and `console`, whose types live beside the
      // package rather than in it.
      symlinkSync(join(sdk, 'node_modules', '@types'), join(scratch, 'node_modules', '@types'), 'dir')

      for (const sample of samples) {
        copyFileSync(sample.file, join(scratch, `${sample.id}.ts`))
      }
      // The samples are ES modules with top-level `await`, which is what they
      // would be in a project that installed this package.
      writeFileSync(join(scratch, 'package.json'), JSON.stringify({ type: 'module' }, null, 2))
      writeFileSync(join(scratch, 'tsconfig.json'), JSON.stringify({
        compilerOptions: {
          target: 'ES2022',
          module: 'NodeNext',
          moduleResolution: 'NodeNext',
          strict: true,
          noEmit: true,
          skipLibCheck: true,
        },
        include: ['*.ts'],
      }, null, 2))
      run(join(sdk, 'node_modules', '.bin', 'tsc'), ['--project', join(scratch, 'tsconfig.json')])
    },
  },

  // ---- Python: run them. -------------------------------------------------
  // Compiling a Python file proves it parses and nothing more, and the drift
  // this guards against is a call that no longer exists — which Python only
  // reports when the line runs. Both Python samples need no input; the one
  // that would is `.unavailable`.
  python: {
    extension: 'py',
    build(samples, scratch) {
      for (const sample of samples) {
        run(process.env.PYTHON ?? 'python3', [sample.file], { cwd: scratch })
      }
    },
  },
}

const requested = process.argv.slice(2)
if (requested.length === 0) {
  console.error(`usage: compile-samples.mjs <language> [...]\n\nlanguages: ${Object.keys(harnesses).join(', ')}`)
  process.exit(2)
}

let failed = false
for (const language of requested) {
  const harness = harnesses[language]
  if (harness === undefined) {
    console.error(`No harness for '${language}'. Known: ${Object.keys(harnesses).join(', ')}`)
    process.exit(2)
  }
  const samples = samplesFor(language, harness.extension)
  if (samples.length === 0) {
    console.error(`No ${language} samples found under content/samples.`)
    process.exit(2)
  }
  const names = samples.map((sample) => sample.id).join(', ')
  console.log(`\n== ${language}: ${samples.length} samples (${names})`)
  try {
    harness.build(samples, scratchFor(language))
    console.log(`   ${language} ok`)
  } catch (error) {
    console.error(`\n${language} failed: ${error.message}`)
    failed = true
  }
}

process.exit(failed ? 1 : 0)
