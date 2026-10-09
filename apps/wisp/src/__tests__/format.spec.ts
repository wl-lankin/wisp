import { describe, expect, it } from 'vitest'
import { formatAge, formatBytes, osLabel } from '../lib/format'

describe('format', () => {
  it('formats sizes', () => {
    expect(formatBytes(512)).toBe('512 B')
    expect(formatBytes(2048)).toBe('2 KB')
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB')
    expect(formatBytes(3 * 1024 ** 3)).toBe('3.0 GB')
  })
  it('formats ages', () => {
    expect(formatAge(800)).toBe('just now')
    expect(formatAge(2_000)).toBe('2s ago')
    expect(formatAge(3 * 60_000)).toBe('3 min ago')
    expect(formatAge(2 * 3_600_000)).toBe('2 h ago')
  })
  it('labels operating systems', () => {
    expect(osLabel('MacOs')).toBe('macOS')
    expect(osLabel('Windows')).toBe('Windows')
  })
})
