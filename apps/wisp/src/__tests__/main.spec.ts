import { describe, expect, it, vi } from 'vitest'

vi.mock('../styles.css', () => ({}))

describe('viewFor', () => {
  it('picks the view from the query and falls back to the panel', async () => {
    document.body.innerHTML = '<div id="app"></div>'
    const { viewFor, views } = await import('../main')
    expect(viewFor('?view=timeline')).toBe(views.timeline)
    expect(viewFor('?view=nope')).toBe(views.panel)
    expect(viewFor('')).toBe(views.panel)
  })
})
