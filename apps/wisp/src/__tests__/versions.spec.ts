import { afterEach, describe, expect, it } from 'vitest'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { mismatches, readVersions, setVersion } from '../../scripts/versions.mjs'

const dirs: string[] = []

function tree(versions: [string, string, string, string] = ['0.1.0', '0.1.0', '0.1.0', '0.1.0']): string {
  const root = mkdtempSync(join(tmpdir(), 'wisp-versions-'))
  dirs.push(root)
  mkdirSync(join(root, 'apps/wisp/src-tauri'), { recursive: true })
  mkdirSync(join(root, 'crates/wisp-core'), { recursive: true })
  writeFileSync(
    join(root, 'apps/wisp/package.json'),
    `{\n  "name": "wisp",\n  "version": "${versions[0]}",\n  "dependencies": { "vue": "^3.5.0" }\n}\n`,
  )
  writeFileSync(
    join(root, 'apps/wisp/src-tauri/Cargo.toml'),
    `[package]\nname = "wisp-app"\nversion = "${versions[1]}"\nedition = "2021"\n\n[dependencies]\nserde = "1"\ntauri = { version = "2", features = [] }\n`,
  )
  writeFileSync(
    join(root, 'apps/wisp/src-tauri/tauri.conf.json'),
    `{\n  "productName": "Wisp",\n  "version": "${versions[2]}",\n  "identifier": "x"\n}\n`,
  )
  writeFileSync(
    join(root, 'crates/wisp-core/Cargo.toml'),
    `[package]\r\nname = "wisp-core"\r\nversion = "${versions[3]}"\r\n\r\n[dependencies]\r\nserde = "1"\r\n`,
  )
  return root
}

afterEach(() => {
  while (dirs.length) rmSync(dirs.pop()!, { recursive: true, force: true })
})

describe('versions', () => {
  it('reads all four versions', () => {
    const v = readVersions(tree())
    expect(Object.keys(v)).toHaveLength(4)
    expect(Object.values(v)).toEqual(['0.1.0', '0.1.0', '0.1.0', '0.1.0'])
  })

  it('reports no mismatch when equal', () => {
    expect(mismatches(readVersions(tree()))).toEqual([])
  })

  it('names the odd file out', () => {
    const list = mismatches(readVersions(tree(['0.1.0', '0.1.0', '0.2.0', '0.1.0'])))
    expect(list).toHaveLength(1)
    expect(list[0]).toContain('tauri.conf.json')
    expect(list[0]).toContain('0.2.0')
  })

  it('sets all four, leaving dependencies and line endings alone', () => {
    const root = tree()
    setVersion(root, '0.2.0')
    expect(Object.values(readVersions(root))).toEqual(['0.2.0', '0.2.0', '0.2.0', '0.2.0'])
    const app = readFileSync(join(root, 'apps/wisp/src-tauri/Cargo.toml'), 'utf8')
    expect(app).toContain('serde = "1"')
    expect(app).toContain('tauri = { version = "2", features = [] }')
    const core = readFileSync(join(root, 'crates/wisp-core/Cargo.toml'), 'utf8')
    expect(core).toBe('[package]\r\nname = "wisp-core"\r\nversion = "0.2.0"\r\n\r\n[dependencies]\r\nserde = "1"\r\n')
    const pkg = readFileSync(join(root, 'apps/wisp/package.json'), 'utf8')
    expect(pkg).toBe('{\n  "name": "wisp",\n  "version": "0.2.0",\n  "dependencies": { "vue": "^3.5.0" }\n}\n')
  })

  it('follows the lockfile root versions but not dependency entries', () => {
    const root = tree()
    const lock = [
      '{',
      '  "name": "wisp",',
      '  "version": "0.1.0",',
      '  "packages": {',
      '    "": {',
      '      "name": "wisp",',
      '      "version": "0.1.0"',
      '    },',
      '    "node_modules/a": {',
      '      "version": "0.1.0"',
      '    }',
      '  }',
      '}',
      '',
    ].join('\n')
    writeFileSync(join(root, 'apps/wisp/package-lock.json'), lock)
    setVersion(root, '0.2.0')
    const after = readFileSync(join(root, 'apps/wisp/package-lock.json'), 'utf8')
    expect(after.match(/0\.2\.0/g)).toHaveLength(2)
    expect(after).toContain('"node_modules/a": {\n      "version": "0.1.0"')
  })

  it('rejects an invalid version', () => {
    const root = tree()
    expect(() => setVersion(root, '1.2')).toThrow()
    expect(() => setVersion(root, 'v1.2.3')).toThrow()
    expect(Object.values(readVersions(root))).toEqual(['0.1.0', '0.1.0', '0.1.0', '0.1.0'])
  })
})
