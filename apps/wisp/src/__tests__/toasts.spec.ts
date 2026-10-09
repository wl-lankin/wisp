import { describe, expect, it } from 'vitest'
import { groupOffers, shouldHide, TOAST_LINGER_MS } from '../lib/toasts'
import type { Offer } from '../lib/types'

const o = (peer: string, itemId: string): Offer => ({
  peer,
  itemId,
  kind: 'Text',
  size: 1,
  preview: { kind: 'text', text: itemId, smart: { type: 'plain' } },
  ageMs: 0,
})

describe('groupOffers', () => {
  const names: Record<string, string> = { m: 'MacBook', l: 'Laptop', p: 'PC', x: 'Other' }
  const nameOf = (id: string) => names[id]

  it('stacks offers per device, newest device first', () => {
    const groups = groupOffers([o('m', '3'), o('l', '2'), o('m', '1')], nameOf)
    expect(groups.map((g) => g.name)).toEqual(['MacBook', 'Laptop'])
    expect(groups[0].offers.map((x) => x.itemId)).toEqual(['3', '1'])
  })

  it('shows at most three groups', () => {
    expect(groupOffers([o('m', '1'), o('l', '2'), o('p', '3'), o('x', '4')], nameOf)).toHaveLength(3)
  })
})

describe('shouldHide', () => {
  it('hides when empty, lingers 8 s, and never while hovered', () => {
    expect(shouldHide(0, 0, false, 0)).toBe(true)
    expect(shouldHide(TOAST_LINGER_MS - 1, 0, false, 1)).toBe(false)
    expect(shouldHide(TOAST_LINGER_MS, 0, false, 1)).toBe(true)
    expect(shouldHide(TOAST_LINGER_MS * 2, 0, true, 1)).toBe(false)
  })
})
