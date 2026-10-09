import { describe, expect, it } from 'vitest'
import { layoutOrbs } from '../lib/constellation'

describe('layoutOrbs', () => {
  const W = 320
  const H = 220

  it('puts the first peer left and the second right of the center', () => {
    const [a, b] = layoutOrbs(['a', 'b'], W, H)
    expect(a.x).toBeLessThan(W / 2)
    expect(b.x).toBeGreaterThan(W / 2)
    expect(Math.abs(a.y - H / 2)).toBeLessThan(1)
    expect(Math.abs(b.y - H / 2)).toBeLessThan(1)
  })

  it('keeps every orb inside the box and returns ids in order', () => {
    const ids = ['a', 'b', 'c', 'd', 'e', 'f']
    const placed = layoutOrbs(ids, W, H)
    expect(placed.map((p) => p.id)).toEqual(ids)
    for (const p of placed) {
      expect(p.x).toBeGreaterThanOrEqual(36)
      expect(p.x).toBeLessThanOrEqual(W - 36)
      expect(p.y).toBeGreaterThanOrEqual(36)
      expect(p.y).toBeLessThanOrEqual(H - 36)
    }
  })

  it('does not stack two orbs on the same spot', () => {
    const placed = layoutOrbs(['a', 'b', 'c', 'd'], W, H)
    for (let i = 0; i < placed.length; i++)
      for (let j = i + 1; j < placed.length; j++)
        expect(Math.hypot(placed[i].x - placed[j].x, placed[i].y - placed[j].y)).toBeGreaterThan(40)
  })

  it('handles no peers', () => {
    expect(layoutOrbs([], W, H)).toEqual([])
  })
})
