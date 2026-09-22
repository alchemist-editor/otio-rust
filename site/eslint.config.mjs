import next from 'eslint-config-next/core-web-vitals'
import typescript from 'eslint-config-next/typescript'

/**
 * `eslint-config-next` 16 ships flat configs, so they are used directly. The
 * `FlatCompat` bridge is for the older shareable configs and is not needed
 * here.
 */
const config = [
  { ignores: ['.next/**', 'out/**', 'node_modules/**', 'content/**', '.samples-build/**'] },
  ...next,
  ...typescript,
]

export default config
