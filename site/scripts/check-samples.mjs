#!/usr/bin/env node
/**
 * Fails when a code sample is missing a language the site advertises.
 *
 * The switcher along the top of every sample is a promise: pick a language
 * and the whole site is in it. A sample that only exists in four of the eight
 * breaks that promise quietly — the reader clicks Swift, gets Go, and learns
 * not to trust the tabs. So every sample has to answer for every shipping
 * language, either with a file or with a `.unavailable` note saying what is
 * missing and why. A note is a real answer; silence is not.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const SITE = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const SAMPLES = join(SITE, 'content/samples')

/**
 * The language list, read out of the TypeScript that defines it rather than
 * repeated here, so the two can never disagree.
 */
function shippingLanguages() {
  const source = readFileSync(join(SITE, 'src/lib/sdk-languages.ts'), 'utf8')
  const languages = []
  const entry = /id:\s*'([^']+)',[\s\S]*?extension:\s*'([^']+)',[\s\S]*?status:\s*'([^']+)'/g
  let match
  while ((match = entry.exec(source))) {
    const [, id, extension, status] = match
    if (status === 'shipping') languages.push({ id, extension })
  }
  if (languages.length === 0) {
    throw new Error('no shipping languages found in src/lib/sdk-languages.ts')
  }
  return languages
}

const languages = shippingLanguages()
const problems = []
let checked = 0

for (const id of readdirSync(SAMPLES).sort()) {
  const directory = join(SAMPLES, id)
  if (!statSync(directory).isDirectory()) continue
  const files = new Set(readdirSync(directory))

  for (const language of languages) {
    const sample = `${language.id}.${language.extension}`
    const note = `${language.id}.unavailable`
    if (files.has(sample)) {
      checked += 1
      continue
    }
    if (files.has(note)) {
      if (readFileSync(join(directory, note), 'utf8').trim().length === 0) {
        problems.push(`${id}/${note} is empty: say what is missing and why`)
      }
      continue
    }
    problems.push(`${id} has no ${sample} and no ${note}`)
  }

  for (const file of files) {
    const known = languages.some(
      (language) => file === `${language.id}.${language.extension}` || file === `${language.id}.unavailable`,
    )
    if (!known) problems.push(`${id}/${file} belongs to no language on the list`)
  }
}

if (problems.length > 0) {
  console.error('Samples are incomplete:\n')
  for (const problem of problems) console.error(`  ${problem}`)
  console.error(`\n${problems.length} problem${problems.length === 1 ? '' : 's'}.`)
  process.exit(1)
}

console.log(`${checked} samples across ${languages.length} languages, none missing.`)
