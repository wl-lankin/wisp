import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const JSON_FILES = ['apps/wisp/package.json', 'apps/wisp/src-tauri/tauri.conf.json']
const TOML_FILES = ['apps/wisp/src-tauri/Cargo.toml', 'crates/wisp-core/Cargo.toml']
const LOCK_FILE = 'apps/wisp/package-lock.json'

const JSON_VERSION = /^(  "version"\s*:\s*")([^"]*)(")/m
const LOCK_ROOT_VERSION = /^(  "version"\s*:\s*")([^"]*)(")/m
const LOCK_PACKAGE_VERSION = /("packages"\s*:\s*\{\s*""\s*:\s*\{[\s\S]*?\n {6}"version"\s*:\s*")([^"]*)(")/
const SEMVER = /^\d+\.\d+\.\d+$/

function read(root, file) {
  return readFileSync(join(root, file), 'utf8')
}

function tomlPackageVersion(text) {
  let inPackage = false
  for (const line of text.split(/\r?\n/)) {
    const table = line.match(/^\s*\[([^\]]*)\]\s*$/)
    if (table) {
      inPackage = table[1].trim() === 'package'
      continue
    }
    const m = inPackage && line.match(/^\s*version\s*=\s*"([^"]*)"/)
    if (m) return m[1]
  }
  throw new Error('no version in [package]')
}

function setTomlPackageVersion(text, version) {
  let inPackage = false
  let done = false
  const out = text.split(/(\r?\n)/).map((part) => {
    if (done || /^\r?\n$/.test(part)) return part
    const table = part.match(/^\s*\[([^\]]*)\]\s*$/)
    if (table) {
      inPackage = table[1].trim() === 'package'
      return part
    }
    if (inPackage && /^\s*version\s*=\s*"[^"]*"/.test(part)) {
      done = true
      return part.replace(/^(\s*version\s*=\s*")[^"]*(")/, `$1${version}$2`)
    }
    return part
  })
  if (!done) throw new Error('no version in [package]')
  return out.join('')
}

export function readVersions(root) {
  const versions = {}
  for (const file of JSON_FILES) {
    const m = read(root, file).match(JSON_VERSION)
    if (!m) throw new Error(`no top-level version in ${file}`)
    versions[file] = m[2]
  }
  for (const file of TOML_FILES) {
    try {
      versions[file] = tomlPackageVersion(read(root, file))
    } catch (e) {
      throw new Error(`${e.message} in ${file}`)
    }
  }
  return versions
}

export function mismatches(versions) {
  const entries = Object.entries(versions)
  const counts = new Map()
  for (const [, v] of entries) counts.set(v, (counts.get(v) ?? 0) + 1)
  if (counts.size <= 1) return []
  // The expected version is the most common one; ties resolve to the first file listed.
  let expected = entries[0][1]
  for (const [v, n] of counts) if (n > counts.get(expected)) expected = v
  return entries
    .filter(([, v]) => v !== expected)
    .map(([file, v]) => `${file}: ${v} (expected ${expected})`)
}

export function setVersion(root, version) {
  if (!SEMVER.test(version)) throw new Error(`invalid version "${version}", expected MAJOR.MINOR.PATCH`)
  // Compute every new text first so a failure leaves all files untouched.
  const writes = []
  for (const file of JSON_FILES) {
    const text = read(root, file)
    if (!JSON_VERSION.test(text)) throw new Error(`no top-level version in ${file}`)
    writes.push([file, text.replace(JSON_VERSION, `$1${version}$3`)])
  }
  for (const file of TOML_FILES) {
    writes.push([file, setTomlPackageVersion(read(root, file), version)])
  }
  if (existsSync(join(root, LOCK_FILE))) {
    const text = read(root, LOCK_FILE)
    if (LOCK_ROOT_VERSION.test(text) && LOCK_PACKAGE_VERSION.test(text)) {
      writes.push([LOCK_FILE, text.replace(LOCK_ROOT_VERSION, `$1${version}$3`).replace(LOCK_PACKAGE_VERSION, `$1${version}$3`)])
    }
  }
  for (const [file, text] of writes) writeFileSync(join(root, file), text)
}

function repoRoot() {
  return resolve(dirname(fileURLToPath(import.meta.url)), '../../..')
}

function main(argv) {
  const [command, arg] = argv
  const root = repoRoot()
  if (command === 'check') {
    const versions = readVersions(root)
    const bad = mismatches(versions)
    if (bad.length) {
      console.error('Version mismatch:\n' + bad.map((l) => `  ${l}`).join('\n'))
      return 1
    }
    console.log(Object.values(versions)[0])
    return 0
  }
  if (command === 'set') {
    try {
      setVersion(root, arg ?? '')
    } catch (e) {
      console.error(e.message)
      return 1
    }
    console.log(`Set version ${arg}`)
    return 0
  }
  console.error('Usage: node scripts/versions.mjs check | set <MAJOR.MINOR.PATCH>')
  return 1
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2))
}
