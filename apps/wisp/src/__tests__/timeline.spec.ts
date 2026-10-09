import { describe, expect, it } from 'vitest'
import { historyQuery } from '../lib/timeline'

describe('historyQuery', () => {
  it('omits empty filters and keeps the 200 item limit', () => {
    expect(historyQuery({ text: '  ', source: 'all', kind: 'all' })).toEqual({ limit: 200 })
    expect(historyQuery({ text: ' invoice ', source: 'abc', kind: 'Image' })).toEqual({
      text: 'invoice',
      source: 'abc',
      kind: 'Image',
      limit: 200,
    })
  })
})
