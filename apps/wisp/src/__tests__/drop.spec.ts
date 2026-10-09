import { describe, expect, it } from 'vitest'
import { peerIdAt, toCssPoint } from '../lib/drop'

describe('drop helpers', () => {
  it('finds the peer of the orb under the pointer', () => {
    document.body.innerHTML = '<div data-peer-id="abc"><span id="inner"></span></div><p id="other"></p>'
    expect(peerIdAt(document.getElementById('inner'))).toBe('abc')
    expect(peerIdAt(document.getElementById('other'))).toBeNull()
    expect(peerIdAt(null)).toBeNull()
  })
  it('converts physical drop positions to CSS pixels', () => {
    expect(toCssPoint({ x: 300, y: 150 }, 1.5)).toEqual({ x: 200, y: 100 })
  })
})
